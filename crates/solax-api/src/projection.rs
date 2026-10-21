use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Where today's yield is heading, at today's pace so far.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Projection {
    /// Fewer than 3 completed days to learn a typical day from.
    NotEnoughHistory { days: u32 },
    /// A typical day has made too little by now for the numbers to mean
    /// anything. `from` is the local time, HH:MM, when that changes.
    TooEarly { from: Option<String> },
    OnPace {
        expected_kwh: f64,
        low_kwh: f64,
        high_kwh: f64,
        based_on_days: u32,
    },
    /// A typical day is done by now; the yield so far is the day's figure.
    Finished { yield_kwh: f64 },
}
