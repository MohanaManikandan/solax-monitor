//! What a screen needs to show the present: the latest reading, today's
//! figures, the poller's state and the projection.

use std::collections::BTreeMap;

use anyhow::Result;
use chrono::DateTime;
use chrono_tz::Tz;
use solax_api::{
    Device, Fault, FaultCode, Health, MetricValue, Now, PollWindow, PollerState, Reading, Severity,
};

use crate::api::{error_codes, AppState};
use crate::db::Snapshot;
use crate::metrics::{fault_code_label, run_mode_label, run_mode_severity, Registry};
use crate::{analytics, clock, projection};

pub fn poller_state(state: &AppState) -> PollerState {
    let timezone = state.config.storage.timezone;
    poller_state_at(state, chrono::Utc::now().with_timezone(&timezone))
}

pub fn poller_state_at(state: &AppState, now: DateTime<Tz>) -> PollerState {
    let timezone = state.config.storage.timezone;
    let poll_window = state.config.inverter.poll_window;
    let (sleeping, resumes_at) = match poll_window {
        Some(window) if !window.contains(now) => (true, Some(window.next_start(timezone, now))),
        _ => (false, None),
    };
    PollerState {
        sleeping,
        resumes_at,
        window: poll_window.map(|window| PollWindow {
            from: window.from.format("%H:%M").to_string(),
            until: window.until.format("%H:%M").to_string(),
        }),
    }
}

/// Every metric's value on a reading, scaled, with the run mode as its label.
pub fn values(registry: &Registry, snapshot: &Snapshot) -> BTreeMap<String, Option<MetricValue>> {
    registry
        .metrics()
        .iter()
        .map(|metric| {
            let value = snapshot.raw.get(&metric.key).map(|&raw| {
                if registry.is_run_mode(metric) {
                    MetricValue::Label(run_mode_label(raw))
                } else {
                    MetricValue::Number(metric.scaled(raw))
                }
            });
            (metric.key.clone(), value)
        })
        .collect()
}

pub fn reading(registry: &Registry, snapshot: &Snapshot) -> Reading {
    let code = snapshot.raw.get(&registry.run_mode().key).copied();
    let fault = snapshot.error.as_deref().map(|raw| Fault {
        codes: error_codes(raw)
            .into_iter()
            .map(|code| FaultCode {
                label: fault_code_label(&code),
                code,
            })
            .collect(),
    });
    Reading {
        ts: snapshot.ts,
        values: values(registry, snapshot),
        device: Device {
            ver: snapshot.ver.clone(),
            inverter_type: snapshot.inverter_type,
        },
        health: Health {
            run_mode: code.map(run_mode_label),
            severity: code.map(run_mode_severity).unwrap_or(Severity::Info),
            fault,
        },
    }
}

pub async fn build_now(state: &AppState) -> Result<Now> {
    let timezone = state.config.storage.timezone;
    build_now_at(state, chrono::Utc::now().with_timezone(&timezone)).await
}

/// `Now` as of a given moment, which also decides which day is today.
pub async fn build_now_at(state: &AppState, now: DateTime<Tz>) -> Result<Now> {
    let registry = &state.registry;
    let timezone = state.config.storage.timezone;
    let today = now.date_naive();
    let (from, to) = clock::day_bounds(timezone, today);

    let reading = state
        .store
        .latest()
        .await?
        .map(|snapshot| reading(registry, &snapshot));
    let summary = state.store.summary(from, to).await?;
    let peaks = registry
        .peaked()
        .map(|metric| {
            let value = summary.peaks.get(&metric.key).map(|&raw| metric.scaled(raw));
            (metric.key.clone(), value)
        })
        .collect();
    // The day's figures and the stretches they add up come from one pass.
    let (today_raw, stretches) = state.store.day_raw(today).await?;
    let today_summary = analytics::day_summary(registry, today, &today_raw);
    let curve = state.curves.curve(&state.store, timezone, today).await?;
    // The yield so far is as of today's latest reading, so the share of the
    // day is measured at that moment too. When readings stop, the projection
    // holds instead of sliding with the clock.
    let measured_at = reading
        .as_ref()
        .map(|latest| latest.ts)
        .filter(|&timestamp| timestamp >= from && timestamp <= to)
        .unwrap_or_else(|| now.timestamp());
    let projection = projection::project(
        &curve,
        today_summary.yield_kwh,
        measured_at - from,
        registry.daily_yield().decimals,
    );

    Ok(Now {
        reading,
        peaks,
        today: today_summary,
        stretches,
        poller: poller_state_at(state, now),
        projection,
        stored: summary.stored,
        earliest: summary.earliest,
        storage_bytes: state.store.storage_bytes() as i64,
    })
}
