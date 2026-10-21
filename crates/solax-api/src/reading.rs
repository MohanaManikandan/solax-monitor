use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A metric's value: a number in its unit, or a label for the run mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(untagged)]
pub enum MetricValue {
    Number(f64),
    Label(String),
}

/// What the device said about itself on a reading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Device {
    /// Firmware version. Missing on readings imported from the old app.
    pub ver: Option<String>,
    /// The inverter type, which decides the whole Data layout.
    #[ts(type = "number | null")]
    pub inverter_type: Option<i64>,
}

/// How much the run mode should alarm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Ok,
    Info,
    Warn,
    Fault,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Ok => "ok",
            Severity::Info => "info",
            Severity::Warn => "warn",
            Severity::Fault => "fault",
        }
    }
}

/// One code from the inverter's Error object, with its name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FaultCode {
    pub code: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Fault {
    pub codes: Vec<FaultCode>,
}

/// The inverter's own account of its health on a reading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Health {
    pub run_mode: Option<String>,
    pub severity: Severity,
    /// Present when the reply carried an Error object.
    pub fault: Option<Fault>,
}

/// One stored reading, scaled into each metric's unit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Reading {
    /// Epoch seconds.
    #[ts(type = "number")]
    pub ts: i64,
    /// By metric key. `None` where the reading has nothing in that slot.
    pub values: BTreeMap<String, Option<MetricValue>>,
    pub device: Device,
    pub health: Health,
}
