use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The share of a typical day's energy made by each slot boundary of local
/// time, learned from recent complete days. Empty without enough history.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Curve {
    /// The local day this curve was built for, YYYY-MM-DD.
    pub date: String,
    #[ts(type = "number")]
    pub slot_seconds: i64,
    pub based_on_days: u32,
    /// One share per boundary from local midnight, one slot apart.
    pub median: Vec<f64>,
    /// The 25th percentile share at each boundary.
    pub lower: Vec<f64>,
    /// The 75th percentile share at each boundary.
    pub upper: Vec<f64>,
}
