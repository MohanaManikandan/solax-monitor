//! The projected end-of-day yield: where today ends if the rest of it goes
//! the way a typical recent day does.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use chrono::{Duration, NaiveDate};
use chrono_tz::Tz;
use solax_api::Projection;

use crate::clock;
use crate::db::Store;

pub const SLOT_SECONDS: i64 = 300;
/// From midnight to the next midnight, in 5 minute steps.
const BOUNDARIES: usize = 289;
const HISTORY_DAYS: usize = 14;
const LOOKBACK_DAYS: i64 = 28;
const MINIMUM_DAYS: usize = 3;
const LONGEST_GAP_SECONDS: i64 = 600;
const LONGEST_IDLE_SECONDS: i64 = 1800;
const TOO_EARLY_SHARE: f64 = 0.10;
const FINISHED_SHARE: f64 = 0.99;

/// One completed day's hero readings as seconds since its local midnight and
/// the raw value, oldest first.
pub struct DayPower {
    pub readings: Vec<(i64, f64)>,
}

/// The share of a day's energy made by each 5 minute boundary across recent
/// days: the median, the 25th percentile (`lower`) and the 75th (`upper`).
#[derive(Debug, Clone, PartialEq)]
pub struct TypicalCurve {
    pub median: Vec<f64>,
    pub lower: Vec<f64>,
    pub upper: Vec<f64>,
    pub based_on_days: usize,
}

impl TypicalCurve {
    fn at(shares: &[f64], seconds: i64) -> f64 {
        let clamped = seconds.clamp(0, (BOUNDARIES as i64 - 1) * SLOT_SECONDS);
        let index = (clamped / SLOT_SECONDS) as usize;
        if index + 1 >= shares.len() {
            return shares[shares.len() - 1];
        }
        let fraction = (clamped % SLOT_SECONDS) as f64 / SLOT_SECONDS as f64;
        shares[index] + (shares[index + 1] - shares[index]) * fraction
    }

    /// The first local time, HH:MM, at which the lower share reaches 10%.
    fn ready_at(&self) -> Option<String> {
        let boundary = self.lower.iter().position(|&share| share >= TOO_EARLY_SHARE)?;
        let minutes = boundary as i64 * SLOT_SECONDS / 60;
        Some(format!("{:02}:{:02}", minutes / 60, minutes % 60))
    }
}

/// The share of the day's energy made by each boundary, or `None` for a day
/// that cannot teach a typical shape: no energy, a gap of more than 10
/// minutes while producing, more than 30 minutes at zero between producing
/// readings, which is a fault or a shutdown rather than weather, or no idle
/// reading before the first producing one or after the last, which means the
/// poller missed the day's start or end.
pub fn day_shares(day: &DayPower) -> Option<Vec<f64>> {
    let readings = &day.readings;
    let first_producing = readings.iter().position(|&(_, power)| power > 0.0)?;
    let last_producing = readings.iter().rposition(|&(_, power)| power > 0.0)?;
    if first_producing == 0 || last_producing + 1 >= readings.len() {
        return None;
    }
    let producing = &readings[first_producing - 1..=last_producing + 1];
    if producing
        .windows(2)
        .any(|pair| pair[1].0 - pair[0].0 > LONGEST_GAP_SECONDS)
    {
        return None;
    }
    let mut last_producing_time = readings[first_producing].0;
    for &(time, power) in &readings[first_producing..=last_producing] {
        if power > 0.0 {
            if time - last_producing_time > LONGEST_IDLE_SECONDS {
                return None;
            }
            last_producing_time = time;
        }
    }
    let mut cumulative = Vec::with_capacity(producing.len());
    let mut energy = 0.0;
    cumulative.push((producing[0].0, 0.0));
    for pair in producing.windows(2) {
        energy += (pair[0].1 + pair[1].1) / 2.0 * (pair[1].0 - pair[0].0) as f64;
        cumulative.push((pair[1].0, energy));
    }
    if energy <= 0.0 {
        return None;
    }
    Some(
        (0..BOUNDARIES)
            .map(|boundary| interpolate(&cumulative, boundary as i64 * SLOT_SECONDS) / energy)
            .collect(),
    )
}

fn interpolate(points: &[(i64, f64)], at: i64) -> f64 {
    let (first_time, first_value) = points[0];
    let (last_time, last_value) = points[points.len() - 1];
    if at <= first_time {
        return first_value;
    }
    if at >= last_time {
        return last_value;
    }
    let after = points.partition_point(|&(time, _)| time < at);
    let (before_time, before_value) = points[after - 1];
    let (after_time, after_value) = points[after];
    before_value
        + (after_value - before_value) * (at - before_time) as f64
            / (after_time - before_time) as f64
}

fn percentile(sorted: &[f64], quantile: f64) -> f64 {
    let position = quantile * (sorted.len() - 1) as f64;
    let lower_index = position.floor() as usize;
    let upper_index = position.ceil() as usize;
    sorted[lower_index] + (sorted[upper_index] - sorted[lower_index]) * (position - lower_index as f64)
}

/// The typical curve from completed days given oldest first, using the 14
/// most recent days that pass the checks for a usable day.
pub fn typical_curve(days: &[DayPower]) -> TypicalCurve {
    let usable: Vec<Vec<f64>> = days
        .iter()
        .rev()
        .filter_map(day_shares)
        .take(HISTORY_DAYS)
        .collect();
    let mut curve = TypicalCurve {
        median: Vec::new(),
        lower: Vec::new(),
        upper: Vec::new(),
        based_on_days: usable.len(),
    };
    if usable.is_empty() {
        return curve;
    }
    for boundary in 0..BOUNDARIES {
        let mut shares: Vec<f64> = usable.iter().map(|day| day[boundary]).collect();
        shares.sort_by(f64::total_cmp);
        curve.median.push(percentile(&shares, 0.5));
        curve.lower.push(percentile(&shares, 0.25));
        curve.upper.push(percentile(&shares, 0.75));
    }
    curve
}

fn round(value: f64, decimals: u8) -> f64 {
    let factor = 10f64.powi(decimals as i32);
    (value * factor).round() / factor
}

/// Where today ends at today's pace. A missing yield counts as nothing made
/// yet. The range comes from the 25th and 75th percentile shares, and none
/// of the three figures is ever below the yield already made.
pub fn project(
    curve: &TypicalCurve,
    yield_so_far: Option<f64>,
    seconds_since_midnight: i64,
    decimals: u8,
) -> Projection {
    if curve.based_on_days < MINIMUM_DAYS {
        return Projection::NotEnoughHistory {
            days: curve.based_on_days as u32,
        };
    }
    let made = yield_so_far.unwrap_or(0.0);
    let median = TypicalCurve::at(&curve.median, seconds_since_midnight);
    if median >= FINISHED_SHARE {
        return Projection::Finished {
            yield_kwh: round(made, decimals),
        };
    }
    // Gating on the lower share keeps every divisor at 10% or more.
    let lower = TypicalCurve::at(&curve.lower, seconds_since_midnight);
    if lower < TOO_EARLY_SHARE {
        return Projection::TooEarly {
            from: curve.ready_at(),
        };
    }
    let upper = TypicalCurve::at(&curve.upper, seconds_since_midnight);
    let at_least_made = |value: f64| round(value.max(made), decimals);
    Projection::OnPace {
        expected_kwh: at_least_made(made / median),
        low_kwh: at_least_made(made / upper),
        high_kwh: at_least_made(made / lower),
        based_on_days: curve.based_on_days as u32,
    }
}

/// The typical curve, rebuilt the first time it is needed on each local day.
#[derive(Clone, Default)]
pub struct CurveCache {
    built: Arc<Mutex<Option<(NaiveDate, Arc<TypicalCurve>)>>>,
}

impl CurveCache {
    pub async fn curve(
        &self,
        store: &Store,
        timezone: Tz,
        today: NaiveDate,
    ) -> Result<Arc<TypicalCurve>> {
        if let Some((built_for, curve)) = self.built.lock().unwrap().as_ref() {
            if *built_for == today {
                return Ok(curve.clone());
            }
        }
        let mut days = Vec::new();
        for offset in (1..=LOOKBACK_DAYS).rev() {
            let date = today - Duration::days(offset);
            let (from, to) = clock::day_bounds(timezone, date);
            let readings = store
                .hero_series(from, to)
                .await?
                .into_iter()
                .filter_map(|(timestamp, value)| value.map(|value| (timestamp - from, value as f64)))
                .collect();
            days.push(DayPower { readings });
        }
        let curve = Arc::new(typical_curve(&days));
        *self.built.lock().unwrap() = Some((today, curve.clone()));
        Ok(curve)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, NaiveDate};

    use super::*;
    use crate::test_support::{self, SyntheticDay};

    fn date(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    fn power_of(day: &SyntheticDay) -> DayPower {
        let opening = test_support::midnight(day.date);
        DayPower {
            readings: day
                .samples()
                .iter()
                .map(|sample| (sample.ts - opening, sample.data[13] as f64))
                .collect(),
        }
    }

    fn clear_days(count: i64) -> Vec<DayPower> {
        (0..count)
            .map(|offset| power_of(&SyntheticDay::clear(date("2026-09-01") + Duration::days(offset))))
            .collect()
    }

    const NOON: i64 = 12 * 3600;

    #[test]
    fn identical_days_project_double_at_half_way() {
        let curve = typical_curve(&clear_days(3));
        assert_eq!(
            project(&curve, Some(10.0), NOON, 1),
            Projection::OnPace {
                expected_kwh: 20.0,
                low_kwh: 20.0,
                high_kwh: 20.0,
                based_on_days: 3,
            }
        );
    }

    #[test]
    fn fewer_than_three_usable_days_is_not_enough_history() {
        let curve = typical_curve(&clear_days(2));
        assert_eq!(project(&curve, Some(10.0), NOON, 1), Projection::NotEnoughHistory { days: 2 });
    }

    #[test]
    fn early_morning_is_too_early_and_says_when_it_will_be_ready() {
        let curve = typical_curve(&clear_days(3));
        assert_eq!(
            project(&curve, Some(0.1), 6 * 3600 + 1800, 1),
            Projection::TooEarly { from: Some("08:30".into()) }
        );
    }

    #[test]
    fn evening_is_finished_with_the_yield_so_far() {
        let curve = typical_curve(&clear_days(3));
        assert_eq!(
            project(&curve, Some(22.9), 18 * 3600 + 1800, 1),
            Projection::Finished { yield_kwh: 22.9 }
        );
    }

    #[test]
    fn a_day_without_readings_projects_zero() {
        let curve = typical_curve(&clear_days(3));
        assert_eq!(
            project(&curve, None, NOON, 1),
            Projection::OnPace {
                expected_kwh: 0.0,
                low_kwh: 0.0,
                high_kwh: 0.0,
                based_on_days: 3,
            }
        );
    }

    #[test]
    fn a_day_with_a_long_gap_is_left_out() {
        let mut gapped = SyntheticDay::clear(date("2026-09-10"));
        gapped.gap_hours = Some((11.0, 11.5));
        let mut days = clear_days(2);
        days.push(power_of(&gapped));
        assert_eq!(typical_curve(&days).based_on_days, 2);
    }

    #[test]
    fn a_day_the_poller_stopped_early_is_left_out() {
        let mut truncated = SyntheticDay::clear(date("2026-09-10"));
        truncated.stops_at_hour = Some(15.0);
        let mut days = clear_days(2);
        days.push(power_of(&truncated));
        assert_eq!(typical_curve(&days).based_on_days, 2);
    }

    #[test]
    fn a_day_with_a_long_stretch_at_zero_is_left_out() {
        let mut fault = SyntheticDay::clear(date("2026-09-10"));
        fault.idle_hours = Some((9.0, 14.0));
        let mut days = clear_days(2);
        days.push(power_of(&fault));
        assert_eq!(typical_curve(&days).based_on_days, 2);
    }

    #[test]
    fn a_short_dip_to_zero_still_counts() {
        let mut dip = SyntheticDay::clear(date("2026-09-10"));
        dip.idle_hours = Some((10.0, 10.2));
        let mut days = clear_days(2);
        days.push(power_of(&dip));
        assert_eq!(typical_curve(&days).based_on_days, 3);
    }

    #[test]
    fn days_of_different_size_share_one_shape() {
        let mut small = SyntheticDay::clear(date("2026-09-10"));
        small.peak_watts = 1500.0;
        let mut days = clear_days(2);
        days.push(power_of(&small));
        let Projection::OnPace { low_kwh, high_kwh, .. } =
            project(&typical_curve(&days), Some(10.0), NOON, 1)
        else {
            panic!("expected a projection on pace");
        };
        assert_eq!(low_kwh, high_kwh);
    }

    #[test]
    fn a_spread_of_days_gives_an_ordered_range() {
        let mut days = Vec::new();
        for (offset, (sunrise, sunset)) in [(6.0, 18.0), (6.5, 17.5), (7.0, 18.5), (5.5, 17.0)]
            .into_iter()
            .enumerate()
        {
            let mut day = SyntheticDay::clear(date("2026-09-01") + Duration::days(offset as i64));
            day.sunrise_hour = sunrise;
            day.sunset_hour = sunset;
            days.push(power_of(&day));
        }
        let Projection::OnPace { expected_kwh, low_kwh, high_kwh, .. } =
            project(&typical_curve(&days), Some(10.0), NOON, 1)
        else {
            panic!("expected a projection on pace");
        };
        assert!(low_kwh <= expected_kwh && expected_kwh <= high_kwh);
        assert!(low_kwh < high_kwh);
    }

    #[test]
    fn only_the_latest_fourteen_usable_days_count() {
        assert_eq!(typical_curve(&clear_days(20)).based_on_days, 14);
    }

    #[tokio::test]
    async fn the_curve_is_built_once_per_day() {
        let (_folder, store) = test_support::store();
        let today = date("2026-10-04");
        for offset in 1..=3 {
            SyntheticDay::clear(today - Duration::days(offset)).insert(&store).await;
        }
        let cache = CurveCache::default();

        let first = cache.curve(&store, test_support::TIMEZONE, today).await.unwrap();
        SyntheticDay::clear(today - Duration::days(4)).insert(&store).await;
        let same_day = cache.curve(&store, test_support::TIMEZONE, today).await.unwrap();
        let next_day = cache
            .curve(&store, test_support::TIMEZONE, today + Duration::days(1))
            .await
            .unwrap();

        assert_eq!(first.based_on_days, 3);
        assert_eq!(same_day.based_on_days, 3);
        assert_eq!(next_day.based_on_days, 4);
    }
}
