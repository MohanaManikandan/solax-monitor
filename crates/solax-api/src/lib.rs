//! The types every client of the solax server sees on the wire. The server
//! serializes them, the app deserializes the same structs, and the screens
//! use TypeScript generated from them into `bindings/`.

#[cfg(test)]
mod bindings;
mod curve;
mod day;
mod now;
mod projection;
mod reading;
mod registry;

pub use curve::Curve;
pub use day::{Day, DaySummary, Stretches};
pub use now::{LiveEvent, Now, PollWindow, PollerState};
pub use projection::Projection;
pub use reading::{Device, Fault, FaultCode, Health, MetricValue, Reading, Severity};
pub use registry::{Chart, DataSlots, Feature, Metric, Peak, Registry, Roles, Section, Site};
