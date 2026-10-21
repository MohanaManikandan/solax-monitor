//! The versioned API. Within v1, fields and routes are only ever added.

use std::collections::BTreeMap;
use std::convert::Infallible;

use axum::extract::{Query, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::routing::get;
use axum::{Json, Router};
use chrono::NaiveDate;
use futures_util::stream::{self, Stream};
use futures_util::StreamExt;
use serde::Deserialize;
use solax_api::LiveEvent;
use tokio_stream::wrappers::BroadcastStream;

use crate::api::{ApiError, AppState};
use crate::db::Snapshot;
use crate::metrics::{Metric, Peak};
use crate::{analytics, clock, downsample, now, projection};

pub fn routes() -> Router<AppState> {
    let bulk = Router::new()
        .route("/registry", get(registry_handler))
        .route("/now", get(now_handler))
        .route("/day", get(day_handler))
        .route("/days", get(days_handler))
        .route("/curve", get(curve_handler))
        .route("/export", get(crate::api::export_handler))
        .layer(tower_http::compression::CompressionLayer::new());
    // Outside the compression layer: a compressor holds events back until it
    // has a block to flush, which turns a push feed into a stuttering one.
    let live = Router::new().route("/live", get(live_handler));
    bulk.merge(live)
}

/// Opens with the poller's state, then sends every update. A screen opened
/// mid-sleep learns why nothing is arriving without waiting for sunrise.
async fn live_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let updates = BroadcastStream::new(state.updates.subscribe());
    let opening = stream::once(async move {
        Ok(Event::default()
            .json_data(LiveEvent::Poller(now::poller_state(&state)))
            .unwrap_or_else(|_| Event::default().comment("poller state unavailable")))
    });
    let events = updates.filter_map(|update| async move {
        update
            .ok()
            .and_then(|event| Event::default().json_data(event.as_ref()).ok())
            .map(Ok)
    });
    Sse::new(opening.chain(events)).keep_alive(KeepAlive::default())
}

async fn registry_handler(State(state): State<AppState>) -> Json<solax_api::Registry> {
    Json(solax_api::Registry {
        roles: state.registry.roles(),
        metrics: state.registry.metrics().to_vec(),
        site: solax_api::Site {
            inverter_serial: state.config.inverter.inverter_serial.clone(),
            rating_kw: state.config.inverter.rating_kw,
            max_output_kw: Some(state.config.inverter.max_output_kw),
            timezone: state.config.storage.timezone.name().to_string(),
            poll_interval_seconds: state.config.inverter.poll_interval.as_secs(),
            co2_kg_per_kwh: state.config.site.co2_kg_per_kwh,
            panel_kwp: state.config.site.panel_kwp,
        },
    })
}

async fn now_handler(State(state): State<AppState>) -> Result<Json<solax_api::Now>, ApiError> {
    Ok(Json(now::build_now(&state).await?))
}

fn parse_date(value: &str, name: &str) -> Result<NaiveDate, ApiError> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| ApiError::BadRequest(format!("{name} {value:?} is not a date like 2026-09-20")))
}

#[derive(Deserialize)]
struct DayQuery {
    date: Option<String>,
    points: Option<usize>,
}

async fn day_handler(
    State(state): State<AppState>,
    Query(query): Query<DayQuery>,
) -> Result<Json<solax_api::Day>, ApiError> {
    let timezone = state.config.storage.timezone;
    let date = match query.date.as_deref() {
        Some(value) => parse_date(value, "date")?,
        None => clock::today(timezone),
    };
    if let Some(points) = query.points {
        if points < 2 {
            return Err(ApiError::BadRequest("points must be 2 or more".into()));
        }
    }
    let (from, to) = clock::day_bounds(timezone, date);
    let snapshots = state.store.history(from, to).await?;
    let summary = state.store.summary(from, to).await?;
    let registry = &state.registry;
    let charted: Vec<&Metric> = registry.charted().collect();

    let selection = query.points.and_then(|points| {
        let raw_of = |metric: &Metric| -> Vec<Option<i64>> {
            snapshots
                .iter()
                .map(|snapshot| snapshot.raw.get(&metric.key).copied())
                .collect()
        };
        let timestamps: Vec<i64> = snapshots.iter().map(|snapshot| snapshot.ts).collect();
        let hero = raw_of(registry.hero());
        // A peak counted while producing only looks at rows where the hero is
        // above zero, the same rule as the day's peak itself, so the row kept
        // is the one the marker points at.
        let peaked: Vec<Vec<Option<i64>>> = charted
            .iter()
            .filter(|metric| metric.peak.is_some())
            .map(|metric| {
                let values = raw_of(metric);
                if metric.peak != Some(Peak::WhileProducing) {
                    return values;
                }
                values
                    .into_iter()
                    .zip(&hero)
                    .map(|(value, hero_value)| value.filter(|_| hero_value.is_some_and(|power| power > 0)))
                    .collect()
            })
            .collect();
        // The buckets span the readings, not the whole day: the poller sleeps
        // overnight, and buckets laid over the dark hours would hold nothing,
        // leaving a phone about half the points it asked for.
        let span = match (timestamps.first(), timestamps.last()) {
            (Some(&first), Some(&last)) => (first, last),
            _ => (from, to),
        };
        downsample::select_rows(&timestamps, &hero, &peaked, span, points)
    });
    let kept: Vec<&Snapshot> = match &selection {
        Some(selection) => selection.rows.iter().map(|&row| &snapshots[row]).collect(),
        None => snapshots.iter().collect(),
    };

    let series = charted
        .iter()
        .map(|metric| {
            let column = kept
                .iter()
                .map(|snapshot| snapshot.raw.get(&metric.key).map(|&raw| metric.scaled(raw)))
                .collect();
            (metric.key.clone(), column)
        })
        .collect();
    let peaks = registry
        .peaked()
        .map(|metric| {
            let value = summary.peaks.get(&metric.key).map(|&raw| metric.scaled(raw));
            (metric.key.clone(), value)
        })
        .collect();
    let closing = match snapshots.last() {
        Some(snapshot) => now::values(registry, snapshot),
        None => BTreeMap::new(),
    };
    let stretches = state.store.stretches(from, to).await?;

    Ok(Json(solax_api::Day {
        date: date.to_string(),
        from,
        to,
        count: snapshots.len() as i64,
        bucket_seconds: selection.as_ref().map(|selection| selection.bucket_seconds),
        ts: kept.iter().map(|snapshot| snapshot.ts).collect(),
        stretches,
        series,
        peaks,
        closing,
    }))
}

#[derive(Deserialize)]
struct DaysQuery {
    from: Option<String>,
    to: Option<String>,
}

async fn days_handler(
    State(state): State<AppState>,
    Query(query): Query<DaysQuery>,
) -> Result<Json<Vec<solax_api::DaySummary>>, ApiError> {
    let (Some(from), Some(to)) = (query.from.as_deref(), query.to.as_deref()) else {
        return Err(ApiError::BadRequest("from and to are both required".into()));
    };
    let from = parse_date(from, "from")?;
    let to = parse_date(to, "to")?;
    if from > to {
        return Err(ApiError::BadRequest("from must not be after to".into()));
    }
    if (to - from).num_days() + 1 > analytics::MAX_DAYS_PER_REQUEST {
        return Err(ApiError::BadRequest(format!(
            "ask for at most {} days at a time",
            analytics::MAX_DAYS_PER_REQUEST
        )));
    }
    let today = clock::today(state.config.storage.timezone);
    Ok(Json(
        analytics::day_summaries(&state.store, &state.registry, from, to, today).await?,
    ))
}

/// Today's typical-day curve, which a screen fetches once a day to draw the
/// projected yield path.
async fn curve_handler(State(state): State<AppState>) -> Result<Json<solax_api::Curve>, ApiError> {
    let timezone = state.config.storage.timezone;
    let today = clock::today(timezone);
    let curve = state.curves.curve(&state.store, timezone, today).await?;
    Ok(Json(solax_api::Curve {
        date: today.to_string(),
        slot_seconds: projection::SLOT_SECONDS,
        based_on_days: curve.based_on_days as u32,
        median: curve.median.clone(),
        lower: curve.lower.clone(),
        upper: curve.upper.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use chrono::{Duration, NaiveDate, TimeZone};
    use solax_api::Projection;

    use crate::now::build_now_at;
    use crate::test_support::{self, get, SyntheticDay, TOKEN};

    #[tokio::test]
    async fn registry_names_the_roles_and_the_site() {
        let (_folder, state) = test_support::app_state();
        let (status, body) = get(&state, "/api/v1/registry", Some(TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
        let registry: solax_api::Registry = serde_json::from_slice(&body).unwrap();
        assert_eq!(registry.roles.daily_yield, "today_kwh");
        assert_eq!(registry.roles.lifetime_yield, "total_kwh");
        assert_eq!(registry.site.co2_kg_per_kwh, 0.71);
        assert_eq!(registry.site.panel_kwp, None);
        assert_eq!(registry.site.timezone, "Asia/Kolkata");
        assert_eq!(registry.site.inverter_serial, "XM-TEST");
        assert_eq!(registry.metrics.len(), 6);
    }

    #[tokio::test]
    async fn v1_routes_need_the_token() {
        let (_folder, state) = test_support::app_state();
        let (status, _) = get(&state, "/api/v1/registry", None).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn now_on_an_empty_store_has_no_reading_and_not_enough_history() {
        let (_folder, state) = test_support::app_state();
        let (status, body) = get(&state, "/api/v1/now", Some(TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
        let now: solax_api::Now = serde_json::from_slice(&body).unwrap();
        assert_eq!(now.reading, None);
        assert_eq!(now.today.samples, 0);
        assert_eq!(now.projection, Projection::NotEnoughHistory { days: 0 });
    }

    #[tokio::test]
    async fn now_at_noon_reports_today_and_the_projection() {
        let (_folder, state) = test_support::app_state();
        let today = NaiveDate::from_ymd_opt(2026, 9, 25).unwrap();
        for offset in 1..=3 {
            SyntheticDay::clear(today - Duration::days(offset)).insert(&state.store).await;
        }
        let mut morning = SyntheticDay::clear(today);
        morning.stops_at_hour = Some(12.0);
        morning.insert(&state.store).await;
        let full_day = SyntheticDay::clear(today).samples().last().unwrap().data[21] as f64 / 10.0;
        let noon = test_support::TIMEZONE
            .from_local_datetime(&today.and_hms_opt(12, 0, 0).unwrap())
            .unwrap();

        let now = build_now_at(&state, noon).await.unwrap();

        let made = now.today.yield_kwh.unwrap();
        assert_eq!(now.reading.unwrap().ts, noon.timestamp());
        let Projection::OnPace { expected_kwh, based_on_days, .. } = now.projection else {
            panic!("expected a projection on pace, got {:?}", now.projection);
        };
        assert_eq!(based_on_days, 3);
        assert!((expected_kwh - 2.0 * made).abs() <= 0.2, "{expected_kwh} vs {made}");
        assert!((expected_kwh - full_day).abs() <= 0.3, "{expected_kwh} vs {full_day}");
    }

    #[tokio::test]
    async fn the_projection_holds_at_the_last_reading_when_readings_stop() {
        let (_folder, state) = test_support::app_state();
        let today = NaiveDate::from_ymd_opt(2026, 9, 25).unwrap();
        for offset in 1..=3 {
            SyntheticDay::clear(today - Duration::days(offset)).insert(&state.store).await;
        }
        let mut morning = SyntheticDay::clear(today);
        morning.stops_at_hour = Some(12.0);
        morning.insert(&state.store).await;
        let at = |hour: u32| {
            test_support::TIMEZONE
                .from_local_datetime(&today.and_hms_opt(hour, 0, 0).unwrap())
                .unwrap()
        };

        let at_noon = build_now_at(&state, at(12)).await.unwrap();
        let three_hours_later = build_now_at(&state, at(15)).await.unwrap();

        assert!(matches!(at_noon.projection, Projection::OnPace { .. }));
        assert_eq!(three_hours_later.projection, at_noon.projection);
    }

    #[tokio::test]
    async fn a_day_and_today_carry_the_stretches_to_mark_on_the_power_chart() {
        let (_folder, mut state) = test_support::app_state();
        state.store = state.store.clone().with_output_limit(3000.0);
        let today = NaiveDate::from_ymd_opt(2026, 9, 20).unwrap();
        let clear = SyntheticDay::clear(today);
        clear.insert(&state.store).await;
        // A clear day passes 97% of the 3,000 W limit, 2,910 W, once around
        // noon, in readings five minutes apart: one stretch, from the first
        // reading that high to the last.
        let high: Vec<i64> = clear.samples().iter().filter(|sample| sample.data[13] >= 2910).map(|sample| sample.ts).collect();
        let expected = solax_api::Stretches { at_limit: vec![[high[0], *high.last().unwrap()]], derating: Vec::new() };
        assert!(high.len() > 2);

        let (_, body) = get(&state, "/api/v1/day?date=2026-09-20&points=60", Some(TOKEN)).await;
        let day: solax_api::Day = serde_json::from_slice(&body).unwrap();
        let evening = test_support::TIMEZONE
            .from_local_datetime(&today.and_hms_opt(20, 0, 0).unwrap())
            .unwrap();
        let now = build_now_at(&state, evening).await.unwrap();

        assert_eq!(day.stretches, expected, "a downsampled day keeps the full-resolution stretches");
        assert_eq!(now.stretches, expected);
    }

    async fn day_with_readings() -> (tempfile::TempDir, crate::api::AppState) {
        let (folder, state) = test_support::app_state();
        SyntheticDay::clear(NaiveDate::from_ymd_opt(2026, 9, 20).unwrap())
            .insert(&state.store)
            .await;
        (folder, state)
    }

    #[tokio::test]
    async fn day_without_points_is_full_resolution() {
        let (_folder, state) = day_with_readings().await;
        let (status, body) = get(&state, "/api/v1/day?date=2026-09-20", Some(TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
        let day: solax_api::Day = serde_json::from_slice(&body).unwrap();
        assert_eq!(day.count, 169);
        assert_eq!(day.ts.len(), 169);
        assert_eq!(day.bucket_seconds, None);
        assert_eq!(day.peaks["pv_power"], Some(3000.0));
        assert_eq!(
            day.series.keys().collect::<Vec<_>>(),
            ["ac_volts", "pv_power", "pv_volts", "today_kwh"]
        );
    }

    #[tokio::test]
    async fn day_downsamples_when_asked_and_keeps_the_peak() {
        let (_folder, state) = day_with_readings().await;
        let (_, body) = get(&state, "/api/v1/day?date=2026-09-20&points=20", Some(TOKEN)).await;
        let day: solax_api::Day = serde_json::from_slice(&body).unwrap();
        assert_eq!(day.count, 169);
        assert!(day.ts.len() <= 20 + 3, "{}", day.ts.len());
        // Ten buckets over the readings, 05:00 to 19:00, not the whole day.
        assert_eq!(day.bucket_seconds, Some(5041));
        assert!(day.series["pv_power"].contains(&Some(3000.0)));
        assert!(day.ts.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[tokio::test]
    async fn every_peak_survives_downsampling_even_when_idle_reads_higher() {
        let (_folder, state) = test_support::app_state();
        let mut day = SyntheticDay::clear(NaiveDate::from_ymd_opt(2026, 9, 20).unwrap());
        day.idle_pv_volts = 2700;
        day.pv_volts_spike_hour = Some(10.0);
        day.insert(&state.store).await;

        let (_, body) = get(&state, "/api/v1/day?date=2026-09-20&points=20", Some(TOKEN)).await;
        let downsampled: solax_api::Day = serde_json::from_slice(&body).unwrap();

        assert_eq!(downsampled.peaks["pv_volts"], Some(245.0));
        for (key, peak) in &downsampled.peaks {
            if let Some(series) = downsampled.series.get(key) {
                assert!(series.contains(peak), "{key}: {peak:?} missing from its series");
            }
        }
    }

    #[tokio::test]
    async fn points_below_two_are_rejected() {
        let (_folder, state) = day_with_readings().await;
        for points in [0, 1] {
            let path = format!("/api/v1/day?date=2026-09-20&points={points}");
            let (status, _) = get(&state, &path, Some(TOKEN)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "points={points}");
        }
    }

    #[tokio::test]
    async fn a_downsampled_day_spends_its_points_where_there_are_readings() {
        // Readings run from 05:00 to 19:00; the night holds none.
        let (_folder, state) = day_with_readings().await;
        let (_, body) = get(&state, "/api/v1/day?date=2026-09-20&points=60", Some(TOKEN)).await;
        let day: solax_api::Day = serde_json::from_slice(&body).unwrap();
        assert!(day.ts.len() >= 55, "only {} rows of the 60 asked for", day.ts.len());
        assert!(day.ts.len() <= 60 + 2, "{} rows", day.ts.len());
    }

    #[tokio::test]
    async fn points_above_the_days_rows_return_everything() {
        let (_folder, state) = day_with_readings().await;
        let (_, body) = get(&state, "/api/v1/day?date=2026-09-20&points=1000", Some(TOKEN)).await;
        let day: solax_api::Day = serde_json::from_slice(&body).unwrap();
        assert_eq!(day.ts.len(), 169);
        assert_eq!(day.bucket_seconds, None);
    }

    #[tokio::test]
    async fn a_malformed_date_is_rejected() {
        let (_folder, state) = test_support::app_state();
        let (status, _) = get(&state, "/api/v1/day?date=20-09-2026", Some(TOKEN)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn days_returns_rows_for_days_with_readings() {
        let (_folder, state) = day_with_readings().await;
        let (status, body) =
            get(&state, "/api/v1/days?from=2026-09-18&to=2026-09-22", Some(TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
        let rows: Vec<solax_api::DaySummary> = serde_json::from_slice(&body).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].date, "2026-09-20");
    }

    #[tokio::test]
    async fn days_rejects_a_reversed_or_too_long_range() {
        let (_folder, state) = test_support::app_state();
        for path in [
            "/api/v1/days?from=2026-09-22&to=2026-09-18",
            "/api/v1/days?from=2025-01-01&to=2026-09-18",
            "/api/v1/days?from=2026-09-18",
        ] {
            let (status, _) = get(&state, path, Some(TOKEN)).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
        }
    }

    #[tokio::test]
    async fn the_v1_stream_opens_with_the_poller_state() {
        use http_body_util::BodyExt;
        use tower::ServiceExt;

        let (_folder, state) = test_support::app_state();
        let request = axum::http::Request::builder()
            .uri("/api/v1/live")
            .header("x-auth-token", TOKEN)
            .body(axum::body::Body::empty())
            .unwrap();
        let response = crate::api::router(state).oneshot(request).await.unwrap();
        let mut body = response.into_body();
        let frame = body.frame().await.unwrap().unwrap();
        let text = String::from_utf8(frame.into_data().unwrap().to_vec()).unwrap();
        assert!(text.starts_with(r#"data: {"type":"poller""#), "{text}");
    }

    #[tokio::test]
    async fn export_lists_every_reading_with_its_header() {
        let (_folder, state) = day_with_readings().await;
        let (status, body) = get(&state, "/api/v1/export?date=2026-09-20", Some(TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
        let text = String::from_utf8(body).unwrap();
        assert!(text.starts_with("timestamp,"), "{}", &text[..text.len().min(80)]);
        assert_eq!(text.lines().count(), 170);
    }

    #[tokio::test]
    async fn the_old_routes_are_gone() {
        let (_folder, state) = test_support::app_state();
        for path in ["/api/metrics", "/api/latest", "/api/history", "/api/summary", "/api/days", "/api/status", "/api/live", "/api/export"] {
            let (status, _) = get(&state, path, Some(TOKEN)).await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        }
    }

    #[tokio::test]
    async fn the_curve_is_empty_without_history() {
        let (_folder, state) = test_support::app_state();
        let (status, body) = get(&state, "/api/v1/curve", Some(TOKEN)).await;
        assert_eq!(status, StatusCode::OK);
        let curve: solax_api::Curve = serde_json::from_slice(&body).unwrap();
        assert_eq!(curve.based_on_days, 0);
        assert!(curve.median.is_empty());
        assert_eq!(curve.slot_seconds, 300);
    }

    #[tokio::test]
    async fn the_curve_has_a_share_for_every_boundary_with_history() {
        let (_folder, state) = test_support::app_state();
        let today = crate::clock::today(test_support::TIMEZONE);
        for offset in 1..=3 {
            SyntheticDay::clear(today - Duration::days(offset)).insert(&state.store).await;
        }
        let (_, body) = get(&state, "/api/v1/curve", Some(TOKEN)).await;
        let curve: solax_api::Curve = serde_json::from_slice(&body).unwrap();
        assert_eq!(curve.date, today.to_string());
        assert_eq!(curve.based_on_days, 3);
        assert_eq!(curve.median.len(), 289);
        assert!(curve.median.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!((curve.median[288] - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn now_says_how_much_space_the_database_takes() {
        let (folder, state) = test_support::app_state();
        SyntheticDay::clear(NaiveDate::from_ymd_opt(2026, 9, 20).unwrap()).insert(&state.store).await;
        let on_disk: u64 = std::fs::read_dir(folder.path())
            .unwrap()
            .map(|entry| entry.unwrap().metadata().unwrap().len())
            .sum();
        let (_, body) = get(&state, "/api/v1/now", Some(TOKEN)).await;
        let now: solax_api::Now = serde_json::from_slice(&body).unwrap();
        assert!(now.storage_bytes > 0);
        assert_eq!(now.storage_bytes as u64, on_disk);
    }

    #[tokio::test]
    async fn now_says_how_much_history_is_stored() {
        let (_folder, state) = test_support::app_state();
        let day = SyntheticDay::clear(NaiveDate::from_ymd_opt(2026, 9, 20).unwrap());
        day.insert(&state.store).await;
        let (_, body) = get(&state, "/api/v1/now", Some(TOKEN)).await;
        let now: solax_api::Now = serde_json::from_slice(&body).unwrap();
        assert_eq!(now.stored, 169);
        assert_eq!(now.earliest, Some(day.samples()[0].ts));
    }
}
