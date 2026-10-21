//! Day rows for the API, scaled from the raw figures the store keeps.

use anyhow::Result;
use chrono::NaiveDate;
use solax_api::DaySummary;

use crate::db::{DayRaw, Store};
use crate::metrics::Registry;

/// Longest range one request may ask for, a little over a year.
pub const MAX_DAYS_PER_REQUEST: i64 = 400;

/// Scales one day's raw figures with the registry as it is now.
pub fn day_summary(registry: &Registry, date: NaiveDate, raw: &DayRaw) -> DaySummary {
    let hero = registry.hero();
    let daily_yield = registry.daily_yield();
    let lifetime_yield = registry.lifetime_yield();
    DaySummary {
        date: date.to_string(),
        samples: raw.samples,
        yield_kwh: raw.yield_raw.map(|value| daily_yield.scaled(value)),
        peak_power_w: raw.peak_raw.map(|value| hero.scaled(value)),
        peak_at: raw.peak_ts,
        producing_from: raw.producing_from,
        producing_until: raw.producing_until,
        lifetime_kwh: raw.lifetime_raw.map(|value| lifetime_yield.scaled(value)),
        fault_readings: raw.fault_readings,
        limit_seconds: raw.limit_seconds,
        derate_count: raw.derate_count,
        derate_seconds: raw.derate_seconds,
    }
}

/// One row per local day from `from` to `to` inclusive, leaving out days
/// with no readings.
pub async fn day_summaries(
    store: &Store,
    registry: &Registry,
    from: NaiveDate,
    to: NaiveDate,
    today: NaiveDate,
) -> Result<Vec<DaySummary>> {
    let dates: Vec<NaiveDate> = from.iter_days().take_while(|date| *date <= to).collect();
    let raws = store.day_raws(dates, today).await?;
    Ok(raws
        .into_iter()
        .filter(|(_, raw)| raw.samples > 0)
        .map(|(date, raw)| day_summary(registry, date, &raw))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use chrono::NaiveDate;

    use super::*;
    use crate::db::Store;
    use crate::test_support::{self, midnight, SyntheticDay};

    fn date(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    const TODAY: &str = "2026-10-04";

    #[tokio::test]
    async fn a_clear_day_summarises_to_its_known_figures() {
        let (_folder, store) = test_support::store();
        let day = SyntheticDay::clear(date("2026-09-20"));
        day.insert(&store).await;
        let last = day.samples().last().unwrap().clone();

        let rows = day_summaries(&store, &test_support::registry(), day.date, day.date, date(TODAY))
            .await
            .unwrap();

        let opening = midnight(day.date);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.date, "2026-09-20");
        assert_eq!(row.samples, 169);
        assert_eq!(row.yield_kwh, Some(last.data[21] as f64 / 10.0));
        assert_eq!(row.peak_power_w, Some(3000.0));
        assert_eq!(row.peak_at, Some(opening + 12 * 3600));
        assert_eq!(row.producing_from, Some(opening + 6 * 3600 + 300));
        assert_eq!(row.producing_until, Some(opening + 18 * 3600 - 300));
        assert_eq!(row.lifetime_kwh, Some((18_000 + last.data[21]) as f64 / 10.0));
        assert_eq!(row.fault_readings, 0);
    }

    #[tokio::test]
    async fn day_rows_are_answered_from_a_database_that_cannot_be_written() {
        let (folder, store) = test_support::store();
        let day = SyntheticDay::clear(date("2026-09-20"));
        day.insert(&store).await;
        drop(store);
        if !test_support::make_read_only(folder.path()) {
            return;
        }
        let registry = test_support::registry();
        let read_only = Store::open(&folder.path().join("solax.db"), registry.clone(), test_support::TIMEZONE).unwrap();

        let rows = day_summaries(&read_only, &registry, day.date, day.date, date(TODAY))
            .await
            .unwrap();

        assert_eq!(rows[0].samples, 169);
    }

    #[tokio::test]
    async fn time_at_the_limit_adds_up_the_stretches_spent_within_three_percent_of_it() {
        let (_folder, store) = test_support::store();
        let store = store.with_output_limit(3000.0);
        let day = SyntheticDay::clear(date("2026-09-20"));
        day.insert(&store).await;
        let near_limit = |watts: i64| watts as f64 >= 0.97 * 3000.0;
        let samples = day.samples();
        let expected: i64 = samples
            .windows(2)
            .filter(|pair| near_limit(pair[0].data[13]) && near_limit(pair[1].data[13]))
            .map(|pair| pair[1].ts - pair[0].ts)
            .sum();
        assert!(expected > 0);

        let rows = day_summaries(&store, &test_support::registry(), day.date, day.date, date(TODAY))
            .await
            .unwrap();

        assert_eq!(rows[0].limit_seconds, Some(expected));
    }

    #[tokio::test]
    async fn without_a_limit_there_is_no_time_at_it() {
        let (_folder, store) = test_support::store();
        let day = SyntheticDay::clear(date("2026-09-20"));
        day.insert(&store).await;

        let rows = day_summaries(&store, &test_support::registry(), day.date, day.date, date(TODAY))
            .await
            .unwrap();

        assert_eq!(rows[0].limit_seconds, None);
    }

    /// The test registry with the metrics heat derating reads named.
    fn derating_registry() -> std::sync::Arc<Registry> {
        let text = format!(
            "{}\n{}",
            test_support::REGISTRY,
            r#"
[[inverter]]
key = "ac_power"
label = "Output"
unit = "W"
index = 3
signed = true

[[inverter]]
key = "inverter_temperature"
label = "Temperature"
unit = "C"
index = 23

[derating]
output = "ac_power"
panel_voltage = "pv_volts"
temperature = "inverter_temperature"
"#
        );
        std::sync::Arc::new(crate::metrics::parse(&text).unwrap())
    }

    /// Readings every five seconds from 11:00: two minutes at the limit,
    /// ten held at the derated level with the panel voltage pushed up, then
    /// two back at the limit as the inverter cools.
    fn derated_samples(date: NaiveDate) -> Vec<crate::inverter::Sample> {
        let opening = midnight(date) + 11 * 3600;
        let segments = [(120, 3300, 2200, 85), (600, 2475, 2450, 84), (120, 3300, 2200, 82)];
        let mut samples = Vec::new();
        let mut second = 0;
        for (seconds, watts, deci_volts, celsius) in segments {
            for _ in 0..seconds / 5 {
                let mut data = vec![0_i64; 25];
                data[3] = watts;
                data[4] = deci_volts;
                data[10] = 2;
                data[13] = watts + 40;
                data[23] = celsius;
                samples.push(crate::inverter::Sample {
                    ts: opening + second,
                    sn: "TEST".into(),
                    ver: "3.0".into(),
                    inverter_type: 22,
                    data,
                    information: serde_json::json!(null),
                    error: None,
                });
                second += 5;
            }
        }
        samples
    }

    #[tokio::test]
    async fn heat_derating_counts_each_hold_at_the_derated_level_and_its_length() {
        let folder = tempfile::tempdir().unwrap();
        let registry = derating_registry();
        let store = Store::open(&folder.path().join("solax.db"), registry.clone(), test_support::TIMEZONE)
            .unwrap()
            .with_derated_output(2475.0);
        let day = date("2026-09-26");
        for sample in derated_samples(day) {
            store.insert_sample(sample).await.unwrap();
        }

        let rows = day_summaries(&store, &registry, day, day, date(TODAY)).await.unwrap();

        assert_eq!((rows[0].derate_count, rows[0].derate_seconds), (Some(1), Some(595)));
    }

    /// Readings every five seconds from 11:00, each segment held at the
    /// power delivered and the panels' power.
    fn power_samples(date: NaiveDate, segments: &[(i64, i64, i64)]) -> Vec<crate::inverter::Sample> {
        let opening = midnight(date) + 11 * 3600;
        let mut samples = Vec::new();
        let mut second = 0;
        for &(seconds, delivered, panels) in segments {
            for _ in 0..seconds / 5 {
                let mut data = vec![0_i64; 25];
                data[3] = delivered;
                data[4] = 2200;
                data[10] = 2;
                data[13] = panels;
                samples.push(crate::inverter::Sample {
                    ts: opening + second,
                    sn: "TEST".into(),
                    ver: "3.0".into(),
                    inverter_type: 22,
                    data,
                    information: serde_json::json!(null),
                    error: None,
                });
                second += 5;
            }
        }
        samples
    }

    #[tokio::test]
    async fn time_at_the_limit_measures_the_power_delivered_once_the_output_role_names_it() {
        // Ten minutes where the panels give 3,250 W but the inverter delivers
        // 3,176 W, under its 3,300 W limit, then ten pinned at the limit.
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("solax.db");
        let day = date("2026-09-26");
        let without_role = test_support::registry();
        let store = Store::open(&path, without_role.clone(), test_support::TIMEZONE).unwrap().with_output_limit(3300.0);
        for sample in power_samples(day, &[(600, 3176, 3250), (600, 3300, 3380)]) {
            store.insert_sample(sample).await.unwrap();
        }
        let by_panels = day_summaries(&store, &without_role, day, day, date(TODAY)).await.unwrap();
        drop(store);

        let text = format!(
            "output = \"ac_power\"\n{}\n[[grid]]\nkey = \"ac_power\"\nlabel = \"Power\"\nunit = \"W\"\nindex = 3\nsigned = true\n",
            test_support::REGISTRY
        );
        let with_role = std::sync::Arc::new(crate::metrics::parse(&text).unwrap());
        let store = Store::open(&path, with_role.clone(), test_support::TIMEZONE).unwrap().with_output_limit(3300.0);
        let by_output = day_summaries(&store, &with_role, day, day, date(TODAY)).await.unwrap();

        assert_eq!(by_panels[0].limit_seconds, Some(1195), "the panels' power alone passes 97% of the limit");
        assert_eq!(by_output[0].limit_seconds, Some(595), "a day cached before the role was named is worked out again");
    }

    #[tokio::test]
    async fn the_stretches_at_the_limit_add_up_to_the_time_spent_there() {
        let (_folder, store) = test_support::store();
        let store = store.with_output_limit(3000.0);
        let day = SyntheticDay::clear(date("2026-09-20"));
        day.insert(&store).await;
        let (from, to) = crate::clock::day_bounds(test_support::TIMEZONE, day.date);

        let stretches = store.stretches(from, to).await.unwrap();
        let rows = day_summaries(&store, &test_support::registry(), day.date, day.date, date(TODAY))
            .await
            .unwrap();

        assert!(!stretches.at_limit.is_empty());
        let seconds: i64 = stretches.at_limit.iter().map(|[start, end]| end - start).sum();
        assert_eq!(Some(seconds), rows[0].limit_seconds);
        assert!(stretches.derating.is_empty());
    }

    #[tokio::test]
    async fn the_derating_stretches_are_the_holds_at_the_derated_level() {
        let folder = tempfile::tempdir().unwrap();
        let store = Store::open(&folder.path().join("solax.db"), derating_registry(), test_support::TIMEZONE)
            .unwrap()
            .with_derated_output(2475.0);
        let day = date("2026-09-26");
        for sample in derated_samples(day) {
            store.insert_sample(sample).await.unwrap();
        }
        let (from, to) = crate::clock::day_bounds(test_support::TIMEZONE, day);

        let stretches = store.stretches(from, to).await.unwrap();

        let opening = midnight(day) + 11 * 3600;
        assert_eq!(stretches.derating, vec![[opening + 120, opening + 715]]);
        assert!(stretches.at_limit.is_empty(), "no limit is configured");
    }

    #[tokio::test]
    async fn without_the_derating_metrics_named_there_are_no_derating_figures() {
        let (_folder, store) = test_support::store();
        let store = store.with_derated_output(2475.0);
        let day = date("2026-09-26");
        for sample in derated_samples(day) {
            store.insert_sample(sample).await.unwrap();
        }

        let rows = day_summaries(&store, &test_support::registry(), day, day, date(TODAY))
            .await
            .unwrap();

        assert_eq!((rows[0].derate_count, rows[0].derate_seconds), (None, None));
    }

    #[tokio::test]
    async fn a_reading_written_into_a_day_after_its_figures_were_stored_is_counted() {
        let (_folder, store) = test_support::store();
        let day = SyntheticDay::clear(date("2026-09-20"));
        day.insert(&store).await;
        let first = day_summaries(&store, &test_support::registry(), day.date, day.date, date(TODAY)).await.unwrap();
        assert_eq!(first[0].samples, 169);

        // A reading between two of the day's, such as one merged in later.
        let mut late = day.samples()[100].clone();
        late.ts += 150;
        store.insert_sample(late).await.unwrap();
        let again = day_summaries(&store, &test_support::registry(), day.date, day.date, date(TODAY)).await.unwrap();

        assert_eq!(again[0].samples, 170);
    }

    #[tokio::test]
    async fn fault_readings_counts_readings_with_an_error() {
        let (_folder, store) = test_support::store();
        let mut day = SyntheticDay::clear(date("2026-09-20"));
        day.fault_hour = Some(10.0);
        day.insert(&store).await;

        let rows = day_summaries(&store, &test_support::registry(), day.date, day.date, date(TODAY))
            .await
            .unwrap();

        assert_eq!(rows[0].fault_readings, 1);
    }

    #[tokio::test]
    async fn completed_days_are_served_from_the_daily_table() {
        let (folder, store) = test_support::store();
        let day = SyntheticDay::clear(date("2026-09-20"));
        day.insert(&store).await;
        let registry = test_support::registry();
        day_summaries(&store, &registry, day.date, day.date, date(TODAY)).await.unwrap();

        // A reading changed behind the store's back, which only working the
        // day out again would see.
        let noon = day.samples()[84].ts;
        rusqlite::Connection::open(folder.path().join("solax.db"))
            .unwrap()
            .execute("UPDATE samples SET raw_data = json_set(raw_data, '$[13]', 9999) WHERE ts = ?1", [noon])
            .unwrap();
        let rows = day_summaries(&store, &registry, day.date, day.date, date(TODAY))
            .await
            .unwrap();

        assert_eq!(rows[0].peak_power_w, Some(3000.0));
    }

    #[tokio::test]
    async fn a_changed_timezone_recomputes_cached_rows() {
        let (folder, store) = test_support::store();
        let day = SyntheticDay::clear(date("2026-09-20"));
        day.insert(&store).await;
        let registry = test_support::registry();
        let first = day_summaries(&store, &registry, day.date, day.date, date(TODAY))
            .await
            .unwrap();

        let in_utc = Store::open(&folder.path().join("solax.db"), registry.clone(), chrono_tz::UTC)
            .unwrap();
        let second = day_summaries(&in_utc, &registry, day.date, day.date, date(TODAY))
            .await
            .unwrap();

        // In UTC the date covers 05:30 to 05:30 local, so it holds fewer of
        // the day's readings. A served cached row would still say 169.
        assert_eq!(first[0].samples, 169);
        assert_eq!(second[0].samples, 163);
    }

    #[tokio::test]
    async fn today_and_future_dates_are_never_stored() {
        let (_folder, store) = test_support::store();
        let registry = test_support::registry();
        let today = date(TODAY);
        SyntheticDay::clear(today - chrono::Duration::days(1)).insert(&store).await;
        SyntheticDay::clear(today).insert(&store).await;

        let rows = day_summaries(
            &store,
            &registry,
            today - chrono::Duration::days(1),
            today + chrono::Duration::days(2),
            today,
        )
        .await
        .unwrap();

        let dates: Vec<&str> = rows.iter().map(|row| row.date.as_str()).collect();
        assert_eq!(dates, ["2026-10-03", "2026-10-04"]);
        assert_eq!(store.stored_days(), ["2026-10-03"]);
    }

    #[tokio::test]
    async fn days_without_readings_are_left_out() {
        let (_folder, store) = test_support::store();
        SyntheticDay::clear(date("2026-09-20")).insert(&store).await;
        SyntheticDay::clear(date("2026-09-22")).insert(&store).await;

        let rows = day_summaries(
            &store,
            &test_support::registry(),
            date("2026-09-19"),
            date("2026-09-23"),
            date(TODAY),
        )
        .await
        .unwrap();

        let dates: Vec<&str> = rows.iter().map(|row| row.date.as_str()).collect();
        assert_eq!(dates, ["2026-09-20", "2026-09-22"]);
    }

    /// A month at the real 5 second rate answers from the daily table once
    /// its days are summarised. Slow to set up, so run it explicitly:
    /// `cargo test -p solax-server a_cached_month -- --ignored --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn a_cached_month_answers_quickly() {
        let (_folder, store) = test_support::store();
        let registry = test_support::registry();
        let first_day = date("2026-08-01");
        for offset in 0..30 {
            let mut day = SyntheticDay::clear(first_day + chrono::Duration::days(offset));
            day.step_seconds = 5;
            store.insert_samples_in_one_transaction(&day.samples());
        }
        let last_day = first_day + chrono::Duration::days(29);

        let started = Instant::now();
        day_summaries(&store, &registry, first_day, last_day, date(TODAY)).await.unwrap();
        let first_visit = started.elapsed();
        let started = Instant::now();
        let rows = day_summaries(&store, &registry, first_day, last_day, date(TODAY)).await.unwrap();
        let second_visit = started.elapsed();

        println!("first visit {first_visit:?}, second visit {second_visit:?}");
        assert_eq!(rows.len(), 30);
        assert!(second_visit < Duration::from_millis(100), "{second_visit:?}");
    }
}
