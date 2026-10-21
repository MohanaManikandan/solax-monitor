//! Heat derating: the inverter stepping its output down to a fixed level
//! when it runs too hot. The device never reports it. It shows in the
//! readings as output held flat at that level while hot, with the panel
//! voltage pushed up as the inverter moves the panels off their best
//! working point.

use std::collections::VecDeque;

/// One reading in the units the rule works in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Reading {
    pub ts: i64,
    pub output_watts: f64,
    pub panel_volts: f64,
    pub celsius: f64,
}

/// Bumped whenever the rule changes, so stored day rows are worked out again.
pub const RULE_VERSION: u32 = 2;

/// From this temperature a hold at the level can be a derate. The inverter
/// steps down at about 85 C.
const HOT_CELSIUS: f64 = 80.0;
/// How far from the level a reading may sit and still be on it.
const LEVEL_BAND_WATTS: f64 = 10.0;
/// How long output holds the level before the hold counts as a derate.
/// Output passing through the level on its way elsewhere stays far shorter.
const HOLD_SECONDS: i64 = 30;
/// Longest step between readings while a hold is being confirmed.
const LONGEST_HOLD_STEP: i64 = 30;
/// How far the panel voltage rises on the step, over its lowest in the
/// minute before. A plateau that merely lands on the level has no such rise.
const VOLTAGE_RISE: f64 = 8.0;
const VOLTAGE_LOOKBACK_SECONDS: i64 = 60;
/// Output above the level for this long ends a derate. A single reading
/// above it is the inverter overshooting as a cloud clears.
const RELEASE_SECONDS: i64 = 20;
/// A derate lasts through a cloud holding output below the level for up to
/// this long, and a hold through readings missing for as long.
const BRIDGE_SECONDS: i64 = 120;
/// Readings missing for up to this long inside a derate, with the first one
/// back still on the level and hot, leave it one derate: a derate lasts
/// about ten minutes, so a shorter gap almost surely sat inside it. After a
/// longer gap the inverter may have recovered and stepped down again.
const LONGEST_GAP_IN_A_DERATE: i64 = 600;

enum State {
    Idle,
    /// On the level, not yet for long enough.
    Holding { start: i64, last_on: i64 },
    Derated {
        start: i64,
        last_on: i64,
        above_since: Option<i64>,
    },
}

/// The stretches of a day spent derated, as first and last reading on the
/// level, oldest first. The readings are one day's, oldest first.
pub fn derates(readings: &[Reading], level_watts: f64) -> Vec<(i64, i64)> {
    let on_level = |reading: &Reading| (reading.output_watts - level_watts).abs() <= LEVEL_BAND_WATTS;
    let held = |reading: &Reading| on_level(reading) && reading.celsius >= HOT_CELSIUS;
    let mut found = Vec::new();
    let mut state = State::Idle;
    let mut recent_volts: VecDeque<(i64, f64)> = VecDeque::new();
    let mut previous: Option<i64> = None;
    for reading in readings {
        let ts = reading.ts;
        // Readings went missing before this one, as when the dongle stops
        // answering, rather than arriving off the level.
        let after_a_gap = previous.is_some_and(|before| ts - before > LONGEST_HOLD_STEP);
        previous = Some(ts);
        state = match state {
            State::Derated { start, last_on, above_since } => {
                let resumed = after_a_gap && held(reading) && ts - last_on <= LONGEST_GAP_IN_A_DERATE;
                if (on_level(reading) && ts - last_on <= BRIDGE_SECONDS) || resumed {
                    State::Derated { start, last_on: ts, above_since: None }
                } else {
                    let above_since = if reading.output_watts > level_watts + LEVEL_BAND_WATTS {
                        above_since.or(Some(ts))
                    } else {
                        None
                    };
                    let released = above_since.is_some_and(|since| ts - since >= RELEASE_SECONDS);
                    if ts - last_on <= BRIDGE_SECONDS && !released {
                        State::Derated { start, last_on, above_since }
                    } else {
                        found.push((start, last_on));
                        State::Idle
                    }
                }
            }
            State::Holding { start, last_on } => {
                let step = if after_a_gap { BRIDGE_SECONDS } else { LONGEST_HOLD_STEP };
                if on_level(reading) && ts - last_on <= step {
                    if ts - start >= HOLD_SECONDS {
                        State::Derated { start, last_on: ts, above_since: None }
                    } else {
                        State::Holding { start, last_on: ts }
                    }
                } else {
                    State::Idle
                }
            }
            State::Idle => State::Idle,
        };
        if let State::Idle = state {
            while recent_volts.front().is_some_and(|&(seen, _)| seen < ts - VOLTAGE_LOOKBACK_SECONDS) {
                recent_volts.pop_front();
            }
            // With no reading in the minute before, the step happened while
            // readings were missing and cannot be checked, so it is not asked
            // for; the hold still is.
            let stepped = match recent_volts.iter().map(|&(_, volts)| volts).reduce(f64::min) {
                Some(lowest) => reading.panel_volts >= lowest + VOLTAGE_RISE,
                None => after_a_gap,
            };
            if held(reading) && stepped {
                state = State::Holding { start: ts, last_on: ts };
            }
        }
        recent_volts.push_back((ts, reading.panel_volts));
    }
    if let State::Derated { start, last_on, .. } = state {
        found.push((start, last_on));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const LEVEL: f64 = 2475.0;

    /// Readings every five seconds, one segment after another, each held
    /// at its output, panel voltage and temperature.
    fn day(segments: &[(i64, f64, f64, f64)]) -> Vec<Reading> {
        let mut readings = Vec::new();
        let mut ts = 0;
        for &(seconds, output_watts, panel_volts, celsius) in segments {
            for _ in 0..seconds / 5 {
                readings.push(Reading { ts, output_watts, panel_volts, celsius });
                ts += 5;
            }
        }
        readings
    }

    const FULL: (f64, f64) = (3300.0, 220.0);
    const DERATED: (f64, f64) = (LEVEL, 245.0);

    #[test]
    fn a_hot_step_to_the_level_with_the_voltage_pushed_up_is_one_derate() {
        let readings = day(&[
            (120, FULL.0, FULL.1, 85.0),
            (600, DERATED.0, DERATED.1, 84.0),
            (120, FULL.0, FULL.1, 82.0),
        ]);
        assert_eq!(derates(&readings, LEVEL), vec![(120, 715)]);
    }

    /// The readings with those between `from` and `to` missing, as when the
    /// dongle stops answering for a while.
    fn without(readings: Vec<Reading>, from: i64, to: i64) -> Vec<Reading> {
        readings.into_iter().filter(|reading| reading.ts < from || reading.ts > to).collect()
    }

    #[test]
    fn readings_missing_for_a_few_minutes_mid_derate_leave_it_whole() {
        let readings = day(&[
            (120, FULL.0, FULL.1, 85.0),
            (600, DERATED.0, DERATED.1, 84.0),
            (120, FULL.0, FULL.1, 82.0),
        ]);
        assert_eq!(derates(&without(readings, 300, 450), LEVEL), vec![(120, 715)]);
    }

    #[test]
    fn readings_missing_during_the_first_hold_do_not_lose_the_derate() {
        let readings = day(&[
            (120, FULL.0, FULL.1, 85.0),
            (600, DERATED.0, DERATED.1, 84.0),
            (120, FULL.0, FULL.1, 82.0),
        ]);
        assert_eq!(derates(&without(readings, 145, 185), LEVEL), vec![(120, 715)]);
    }

    #[test]
    fn a_derate_found_still_on_after_a_long_gap_counts_again_without_the_voltage_step() {
        let readings = day(&[
            (120, FULL.0, FULL.1, 85.0),
            (300, DERATED.0, DERATED.1, 85.0),
            (1200, DERATED.0, DERATED.1, 85.0),
            (300, DERATED.0, DERATED.1, 85.0),
            (120, FULL.0, FULL.1, 82.0),
        ]);
        assert_eq!(derates(&without(readings, 420, 1615), LEVEL), vec![(120, 415), (1620, 1915)]);
    }

    #[test]
    fn a_voltage_rise_more_than_a_minute_before_reaching_the_level_is_not_the_step() {
        let readings = day(&[
            (120, FULL.0, FULL.1, 85.0),
            (70, FULL.0, DERATED.1, 85.0),
            (600, DERATED.0, DERATED.1, 85.0),
        ]);
        assert_eq!(derates(&readings, LEVEL), vec![]);
    }

    #[test]
    fn a_derate_still_going_at_the_latest_reading_is_counted_up_to_it() {
        let readings = day(&[(120, FULL.0, FULL.1, 85.0), (300, DERATED.0, DERATED.1, 85.0)]);
        assert_eq!(derates(&readings, LEVEL), vec![(120, 415)]);
    }

    #[test]
    fn output_passing_through_the_level_without_holding_it_is_not_a_derate() {
        let readings = day(&[
            (120, FULL.0, FULL.1, 85.0),
            (20, DERATED.0, DERATED.1, 85.0),
            (120, FULL.0, FULL.1, 85.0),
        ]);
        assert_eq!(derates(&readings, LEVEL), vec![]);
    }

    #[test]
    fn holding_the_level_while_cool_is_not_a_derate() {
        let readings = day(&[
            (120, FULL.0, FULL.1, 60.0),
            (600, DERATED.0, DERATED.1, 60.0),
            (120, FULL.0, FULL.1, 60.0),
        ]);
        assert_eq!(derates(&readings, LEVEL), vec![]);
    }

    #[test]
    fn holding_the_level_at_the_usual_panel_voltage_is_not_a_derate() {
        let readings = day(&[
            (120, FULL.0, FULL.1, 85.0),
            (600, DERATED.0, FULL.1, 85.0),
            (120, FULL.0, FULL.1, 85.0),
        ]);
        assert_eq!(derates(&readings, LEVEL), vec![]);
    }

    #[test]
    fn a_cloud_dip_and_a_one_reading_overshoot_leave_a_derate_whole() {
        let readings = day(&[
            (120, FULL.0, FULL.1, 85.0),
            (240, DERATED.0, DERATED.1, 84.0),
            (90, 1500.0, 215.0, 84.0),
            (5, 2513.0, 240.0, 84.0),
            (240, DERATED.0, DERATED.1, 83.0),
            (120, FULL.0, FULL.1, 82.0),
        ]);
        assert_eq!(derates(&readings, LEVEL), vec![(120, 690)]);
    }

    #[test]
    fn output_staying_above_the_level_ends_a_derate_and_the_next_step_is_another() {
        let readings = day(&[
            (120, FULL.0, FULL.1, 85.0),
            (180, DERATED.0, DERATED.1, 85.0),
            (120, FULL.0, FULL.1, 82.0),
            (180, DERATED.0, DERATED.1, 85.0),
            (60, FULL.0, FULL.1, 82.0),
        ]);
        assert_eq!(derates(&readings, LEVEL), vec![(120, 295), (420, 595)]);
    }
}
