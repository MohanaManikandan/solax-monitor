use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::{DaySummary, Projection, Reading, Stretches};

/// The hours the poller is awake, local HH:MM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PollWindow {
    pub from: String,
    pub until: String,
}

/// Whether the poller is awake, and when it next will be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PollerState {
    pub sleeping: bool,
    #[ts(type = "number | null")]
    pub resumes_at: Option<i64>,
    /// `None` when polling runs around the clock.
    pub window: Option<PollWindow>,
}

/// Everything a screen needs to show the present, in one response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Now {
    /// The latest stored reading, which may be from an earlier day.
    pub reading: Option<Reading>,
    /// Today's highest value of each metric with a peak.
    pub peaks: BTreeMap<String, Option<f64>>,
    pub today: DaySummary,
    /// Today's stretches to mark on the power chart so far.
    #[serde(default)]
    pub stretches: Stretches,
    pub poller: PollerState,
    pub projection: Projection,
    /// Readings stored over all time.
    #[serde(default)]
    #[ts(type = "number")]
    pub stored: i64,
    /// Epoch seconds of the first stored reading.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub earliest: Option<i64>,
    /// Bytes the database takes on disk, its write-ahead log included.
    #[serde(default)]
    #[ts(type = "number")]
    pub storage_bytes: i64,
}

/// One message on the live stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LiveEvent {
    Reading(Now),
    Poller(PollerState),
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::{
        DaySummary, Device, Health, LiveEvent, MetricValue, Now, PollWindow, PollerState,
        Projection, Reading, Severity, Stretches,
    };

    fn empty_day() -> DaySummary {
        DaySummary {
            date: "2026-10-04".into(),
            samples: 0,
            yield_kwh: None,
            peak_power_w: None,
            peak_at: None,
            producing_from: None,
            producing_until: None,
            lifetime_kwh: None,
            fault_readings: 0,
            limit_seconds: None,
            derate_count: None,
            derate_seconds: None,
        }
    }

    #[test]
    fn a_poller_event_is_tagged_by_type() {
        let event = LiveEvent::Poller(PollerState {
            sleeping: true,
            resumes_at: Some(1_791_138_600),
            window: Some(PollWindow { from: "05:00".into(), until: "19:00".into() }),
        });
        assert_eq!(
            serde_json::to_string(&event).unwrap(),
            r#"{"type":"poller","sleeping":true,"resumes_at":1791138600,"window":{"from":"05:00","until":"19:00"}}"#
        );
    }

    #[test]
    fn a_projection_is_tagged_by_state() {
        let projection = Projection::OnPace {
            expected_kwh: 15.0,
            low_kwh: 13.6,
            high_kwh: 16.7,
            based_on_days: 14,
        };
        assert_eq!(
            serde_json::to_string(&projection).unwrap(),
            r#"{"state":"on_pace","expected_kwh":15.0,"low_kwh":13.6,"high_kwh":16.7,"based_on_days":14}"#
        );
        assert_eq!(
            serde_json::to_string(&Projection::TooEarly { from: Some("08:30".into()) }).unwrap(),
            r#"{"state":"too_early","from":"08:30"}"#
        );
    }

    #[test]
    fn a_metric_value_is_a_bare_number_or_label() {
        assert_eq!(serde_json::to_string(&MetricValue::Number(232.1)).unwrap(), "232.1");
        assert_eq!(serde_json::to_string(&MetricValue::Label("Normal".into())).unwrap(), r#""Normal""#);
        assert_eq!(serde_json::from_str::<MetricValue>("232.1").unwrap(), MetricValue::Number(232.1));
        assert_eq!(
            serde_json::from_str::<MetricValue>(r#""Normal""#).unwrap(),
            MetricValue::Label("Normal".into())
        );
    }

    #[test]
    fn a_reading_event_round_trips() {
        let mut values = BTreeMap::new();
        values.insert("ac_volts".to_string(), Some(MetricValue::Number(232.1)));
        values.insert("run_mode".to_string(), Some(MetricValue::Label("Normal".into())));
        values.insert("pv_volts".to_string(), None);
        let event = LiveEvent::Reading(Now {
            reading: Some(Reading {
                ts: 1_790_081_936,
                values,
                device: Device { ver: Some("3.016.02".into()), inverter_type: Some(22) },
                health: Health { run_mode: Some("Normal".into()), severity: Severity::Ok, fault: None },
            }),
            peaks: BTreeMap::new(),
            today: empty_day(),
            stretches: Stretches { at_limit: vec![[1_790_081_000, 1_790_081_900]], derating: Vec::new() },
            poller: PollerState { sleeping: false, resumes_at: None, window: None },
            projection: Projection::NotEnoughHistory { days: 2 },
            stored: 0,
            earliest: None,
            storage_bytes: 0,
        });
        let text = serde_json::to_string(&event).unwrap();
        assert!(text.starts_with(r#"{"type":"reading","reading":"#), "{text}");
        assert_eq!(serde_json::from_str::<LiveEvent>(&text).unwrap(), event);

        // A server from before the later fields sends none of them, and the
        // event still reads, with each one empty.
        let mut older: serde_json::Value = serde_json::from_str(&text).unwrap();
        for later in ["stretches", "stored", "earliest", "storage_bytes"] {
            older.as_object_mut().unwrap().remove(later);
        }
        let LiveEvent::Reading(now) = serde_json::from_value::<LiveEvent>(older).unwrap() else {
            panic!("expected a reading");
        };
        assert_eq!((now.stored, now.earliest, now.storage_bytes), (0, None, 0));
        assert_eq!(now.stretches, Stretches::default());
    }

    #[test]
    fn severity_reads_as_the_old_strings() {
        assert_eq!(Severity::Ok.as_str(), "ok");
        assert_eq!(Severity::Fault.as_str(), "fault");
        assert_eq!(serde_json::to_string(&Severity::Warn).unwrap(), r#""warn""#);
    }
}
