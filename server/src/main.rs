mod api;
mod clock;
mod config;
mod db;
mod inverter;
mod metrics;

mod analytics;
mod api_v1;
mod broadcast;
mod downsample;
mod now;
mod projection;
#[cfg(test)]
mod test_support;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};

/// Longest wait before trying again after a failed poll. Beyond a minute there
/// is nothing to gain from waiting further. A poll interval longer than this
/// is kept as it is, so a failure never makes the poller ask sooner.
const MAX_WAIT_AFTER_FAILED_POLL: Duration = Duration::from_secs(60);

/// Longest single sleep while outside polling hours. Waking periodically
/// means a suspended machine or a corrected clock cannot overshoot sunrise by
/// more than this.
const MAX_SLEEP_SECONDS_OUTSIDE_POLL_HOURS: u64 = 900;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "solax_server=info,tower_http=warn".into()),
        )
        .init();

    let config = Arc::new(config::load()?);
    let timezone = config.storage.timezone;
    let registry = Arc::new(metrics::load()?);

    match config.inverter.poll_window {
        Some(window) => tracing::info!(
            from = %window.from.format("%H:%M"),
            until = %window.until.format("%H:%M"),
            zone = %timezone.name(),
            "polling only within these hours"
        ),
        None => tracing::info!("polling around the clock"),
    }

    tracing::info!(
        metrics = registry.metrics().len(),
        highest_index = registry.highest_index(),
        "registry loaded"
    );

    // The limit decides the time counted at it, the chart's limit line and
    // the level heat derating is looked for at, so a wrong one skews all
    // three without failing anything. Say what is in use.
    tracing::info!(
        limit_kw = config.inverter.max_output_kw,
        derated_kw = config.inverter.derated_output_kw,
        panel_kwp = ?config.site.panel_kwp,
        "inverter limits"
    );
    for key in &config.leftover_keys {
        tracing::warn!(key, "left over from solax-rs and has no effect here; it can be removed");
    }
    if config.inverter.limit_is_the_rating {
        tracing::warn!(
            rating_kw = config.inverter.rating_kw,
            "inverter.max_output_kw is not set, so the rating stands in as the inverter's limit; \
             set it if the inverter delivers more, or time at the limit and heat derating will be wrong"
        );
    }

    let store = db::Store::open(&config.storage.database_path, registry.clone(), timezone)?
        .with_output_limit(config.inverter.max_output_kw * 1000.0)
        .with_derated_output(config.inverter.derated_output_kw * 1000.0);

    let (updates, _) = tokio::sync::broadcast::channel(64);

    let state = api::AppState {
        config: config.clone(),
        registry: registry.clone(),
        store: store.clone(),
        updates: updates.clone(),
        curves: projection::CurveCache::default(),
    };

    tokio::spawn(poll_forever(state.clone()));

    let listener = tokio::net::TcpListener::bind(&config.server.bind)
        .await
        .with_context(|| format!("binding {}", config.server.bind))?;
    tracing::info!(bind = %config.server.bind, "serving");

    axum::serve(listener, api::router(state))
        .await
        .context("running the server")
}

/// Poll the inverter for as long as the process lives. The browser is a
/// viewer of what this writes, never the thing that drives it.
async fn poll_forever(state: api::AppState) {
    let interval = state.config.inverter.poll_interval;
    let timezone = state.config.storage.timezone;
    let poll_window = state.config.inverter.poll_window;
    let mut client: Option<inverter::Client> = None;
    let mut failures: u32 = 0;
    let mut asleep = false;

    loop {
        // Outside the configured hours the inverter has powered down and the
        // dongle stops answering, so polling would only manufacture failures.
        // Sleep to the boundary rather than waking every interval to find the
        // same thing, but in capped steps so a suspend or a clock correction
        // cannot leave this asleep past sunrise.
        if let Some(window) = poll_window {
            let now = chrono::Utc::now().with_timezone(&timezone);
            if !window.contains(now) {
                let resumes = window.next_start(timezone, now);
                let remaining = (resumes - now.timestamp()).max(0) as u64;
                if !asleep {
                    asleep = true;
                    // Drop the connection so the next poll re-resolves the
                    // address: a night is long enough for a DHCP lease to move.
                    client = None;
                    failures = 0;
                    tracing::info!(resumes_in_seconds = remaining, "outside polling hours, sleeping");
                    broadcast::publish_poller_state(&state);
                }
                tokio::time::sleep(Duration::from_secs(
                    remaining.min(MAX_SLEEP_SECONDS_OUTSIDE_POLL_HOURS).max(1),
                ))
                .await;
                continue;
            }
            if asleep {
                asleep = false;
                tracing::info!("inside polling hours, resuming");
                broadcast::publish_poller_state(&state);
            }
        }

        let started = tokio::time::Instant::now();

        let result = async {
            if client.is_none() {
                client = Some(
                    inverter::Client::connect(
                        &state.config.inverter.endpoint,
                        &state.config.inverter.dongle_serial,
                        state.config.inverter.allow_public_address,
                        state.registry.highest_index() + 1,
                    )
                    .await?,
                );
            }
            client.as_ref().expect("client was just built").read().await
        }
        .await;

        match result {
            Ok(sample) => {
                failures = 0;
                let ts = sample.ts;

                // A fault belongs in the log as well as on the page: the page
                // only shows the current state, and by the time anyone looks
                // the inverter may have recovered.
                if let Some(error) = &sample.error {
                    let codes = error
                        .get("ErrorCode")
                        .and_then(|value| value.as_str())
                        .unwrap_or("unspecified");
                    tracing::warn!(codes, detail = %error, "the inverter reported a fault");
                }
                match state.store.insert_sample(sample).await {
                    Ok(true) => {
                        broadcast::publish_reading(&state).await;
                    }
                    Ok(false) => tracing::debug!(ts, "a sample for this second already existed"),
                    Err(error) => tracing::error!(%error, "storing the sample failed"),
                }
            }
            Err(error) => {
                failures = failures.saturating_add(1);
                tracing::warn!(%error, failures, "reading the inverter failed");
                // Rebuild the client so a changed address or a stale DNS
                // answer is picked up on the next attempt.
                client = None;
            }
        }

        let wait = if failures == 0 {
            interval
        } else {
            let factor = 1u32 << failures.min(5);
            interval
                .saturating_mul(factor)
                .min(MAX_WAIT_AFTER_FAILED_POLL)
                .max(interval)
        };
        let elapsed = started.elapsed();
        tokio::time::sleep(wait.saturating_sub(elapsed)).await;
    }
}
