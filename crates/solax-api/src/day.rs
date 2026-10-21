use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::MetricValue;

/// One local day in figures. Times are epoch seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct DaySummary {
    /// The local day, YYYY-MM-DD.
    pub date: String,
    #[ts(type = "number")]
    pub samples: i64,
    /// The day's highest reading of the daily yield counter.
    pub yield_kwh: Option<f64>,
    /// The day's highest hero reading.
    pub peak_power_w: Option<f64>,
    #[ts(type = "number | null")]
    pub peak_at: Option<i64>,
    /// First reading with the hero above zero.
    #[ts(type = "number | null")]
    pub producing_from: Option<i64>,
    /// Last reading with the hero above zero.
    #[ts(type = "number | null")]
    pub producing_until: Option<i64>,
    /// The lifetime counter at the day's last reading.
    pub lifetime_kwh: Option<f64>,
    /// Readings that carried an Error object.
    #[ts(type = "number")]
    pub fault_readings: i64,
    /// Seconds the hero spent within 3% of the inverter's output limit,
    /// where the panels could give more than it delivers. None when no
    /// limit is configured.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub limit_seconds: Option<i64>,
    /// Times the inverter stepped its output down because it ran too hot.
    /// None when the metrics that show it are not named.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub derate_count: Option<i64>,
    /// Seconds spent stepped down for heat.
    #[serde(default)]
    #[ts(type = "number | null")]
    pub derate_seconds: Option<i64>,
}

/// The stretches of a day worth marking on its power chart, each as the
/// first and last reading in it, epoch seconds, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
pub struct Stretches {
    /// Runs of readings within 3% of the inverter's output limit, the same
    /// readings a day's limit_seconds adds up.
    #[ts(type = "Array<[number, number]>")]
    pub at_limit: Vec<[i64; 2]>,
    /// Times the inverter held its output down because it ran too hot.
    #[ts(type = "Array<[number, number]>")]
    pub derating: Vec<[i64; 2]>,
}

/// One local day of readings for the charts, on one shared time axis.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Day {
    pub date: String,
    #[ts(type = "number")]
    pub from: i64,
    #[ts(type = "number")]
    pub to: i64,
    /// Readings stored for the day, before any downsampling.
    #[ts(type = "number")]
    pub count: i64,
    /// Width of each downsampling bucket; `None` at full resolution.
    /// Neighbouring rows can sit up to twice this apart with no reading
    /// missing, so a step longer than twice this width is a gap.
    #[ts(type = "number | null")]
    pub bucket_seconds: Option<i64>,
    #[ts(type = "Array<number>")]
    pub ts: Vec<i64>,
    /// The stretches to mark on the power chart, from every reading of the
    /// day, downsampled or not.
    #[serde(default)]
    pub stretches: Stretches,
    /// Charted metrics by key, one value per entry of `ts`.
    pub series: BTreeMap<String, Vec<Option<f64>>>,
    /// The day's highest value of each metric with a peak.
    pub peaks: BTreeMap<String, Option<f64>>,
    /// Every metric's value at the day's last reading.
    pub closing: BTreeMap<String, Option<MetricValue>>,
}
