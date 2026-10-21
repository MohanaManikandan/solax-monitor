use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The card a metric sits in. Its lines take the section's colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Section {
    Solar,
    Grid,
    Inverter,
}

/// Where a metric's number sits in the inverter's Data array. Each slot holds
/// a whole number from 0 to 65,535, which inverter documentation calls a
/// register.
///
/// `signed` marks a number that can go below zero. A slot has no room for a
/// minus sign, so the top half of its range stands for negatives: 65,535 is
/// -1. A number split across two slots follows the same rule at its own size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DataSlots {
    /// The number fits in one slot. Written as `index = 4`.
    One {
        #[ts(type = "number")]
        index: usize,
        signed: bool,
    },
    /// The number outgrows one slot, so the inverter splits it like an
    /// odometer with two wheels: the low slot counts to 65,535 and wraps to
    /// zero, and the high slot counts the wraps. The value is
    /// low + 65,536 × high. Written as `index = [low, high]`.
    Two {
        #[ts(type = "number")]
        low: usize,
        #[ts(type = "number")]
        high: usize,
        signed: bool,
    },
}

impl DataSlots {
    /// Highest Data index these slots read.
    pub fn highest_index(&self) -> usize {
        match *self {
            DataSlots::One { index, .. } => index,
            DataSlots::Two { low, high, .. } => low.max(high),
        }
    }
}

/// Which readings count toward a metric's daily peak.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Peak {
    /// Every reading of the day.
    AllDay,
    /// Only readings taken while the plant is producing. PV voltage needs it:
    /// idle panels sit at open circuit, which reads higher than any voltage
    /// seen while generating. The page also starts the chart's value axis
    /// from readings past 2% of the rating, so start-up readings run off the
    /// bottom edge.
    WhileProducing,
}

/// The chart a metric is drawn on. Metrics naming the same chart share it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Chart {
    pub name: String,
    /// Start with the line switched off. Its legend entry brings it in.
    pub hidden: bool,
}

/// A metric shown prominently on the hero card, under the meter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Feature {
    /// Wording on the hero card, where the metric's own label often reads as
    /// a fragment.
    pub label: String,
    /// Render quietly, for context beside the headline figure.
    pub subtle: bool,
}

/// One reading the page shows. The page receives exactly this shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Metric {
    pub key: String,
    pub label: String,
    pub unit: String,
    pub section: Section,
    pub slots: DataSlots,
    /// Size of one count in the metric's unit. The inverter sends whole
    /// numbers, so 2222 at a scale of 0.1 is 222.2 V, and the value can only
    /// move in steps of this size. Must be positive.
    pub scale: f64,
    /// Digits shown after the decimal point. Values are rounded to this, and
    /// digits beyond what the scale carries are always zero.
    pub decimals: u8,
    pub chart: Option<Chart>,
    pub peak: Option<Peak>,
    pub feature: Option<Feature>,
    /// Wording for the closing value in a past day's summary, such as
    /// "generated" or "lifetime".
    pub summary: Option<String>,
    pub note: Option<String>,
}

impl Metric {
    /// Raw value from the slots, scaled into the unit the UI shows. Rounded to
    /// the declared precision because 2321 * 0.1 is 232.10000000000002 in
    /// binary floating point, and that noise would otherwise reach the page.
    pub fn scaled(&self, raw: i64) -> f64 {
        let value = raw as f64 * self.scale;
        let factor = 10f64.powi(self.decimals as i32);
        (value * factor).round() / factor
    }
}

/// The metrics that play a role of their own, by key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Roles {
    /// Its value above zero means the plant is producing.
    pub hero: String,
    /// Holds the inverter's run mode, which drives the fault badge.
    pub run_mode: String,
    /// Counts today's yield; starts from zero each morning.
    pub daily_yield: String,
    /// Counts yield over the inverter's lifetime.
    pub lifetime_yield: String,
    /// Counts down the seconds before the inverter connects to the grid,
    /// each morning and after a fault; zero the rest of the time.
    #[serde(default)]
    #[ts(type = "string | null")]
    pub grid_countdown: Option<String>,
    /// The power the inverter delivers, which its output limit caps; the
    /// hero stands in when it is not named.
    #[serde(default)]
    #[ts(type = "string | null")]
    pub output: Option<String>,
}

/// Facts about the plant and the server that screens need.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Site {
    pub inverter_serial: String,
    pub rating_kw: f64,
    /// The most the inverter delivers, which can sit above its rating; the
    /// charts draw it as the inverter's limit. None from a server that
    /// predates it, where the rating stands in.
    #[serde(default)]
    pub max_output_kw: Option<f64>,
    /// IANA name, such as "Asia/Kolkata". Days and times are read in it.
    pub timezone: String,
    #[ts(type = "number")]
    pub poll_interval_seconds: u64,
    /// kg of CO2 per kWh of grid electricity displaced.
    pub co2_kg_per_kwh: f64,
    /// The panels' combined peak power, when configured. Specific yield
    /// divides by it, and by the inverter's rating without it.
    #[serde(default)]
    pub panel_kwp: Option<f64>,
}

/// Everything a screen needs to know before showing data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Registry {
    pub roles: Roles,
    pub metrics: Vec<Metric>,
    pub site: Site,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voltage() -> Metric {
        Metric {
            key: "ac_volts".into(),
            label: "Voltage".into(),
            unit: "V".into(),
            section: Section::Grid,
            slots: DataSlots::One {
                index: 0,
                signed: false,
            },
            scale: 0.1,
            decimals: 1,
            chart: Some(Chart {
                name: "voltage".into(),
                hidden: false,
            }),
            peak: Some(Peak::AllDay),
            feature: None,
            summary: None,
            note: None,
        }
    }

    #[test]
    fn data_slots_serialize_as_one_or_two() {
        let one = DataSlots::One { index: 4, signed: false };
        let two = DataSlots::Two { low: 19, high: 20, signed: true };
        assert_eq!(serde_json::to_string(&one).unwrap(), r#"{"one":{"index":4,"signed":false}}"#);
        assert_eq!(
            serde_json::to_string(&two).unwrap(),
            r#"{"two":{"low":19,"high":20,"signed":true}}"#
        );
    }

    #[test]
    fn a_metric_serializes_its_fields_in_declaration_order() {
        assert_eq!(
            serde_json::to_string(&voltage()).unwrap(),
            r#"{"key":"ac_volts","label":"Voltage","unit":"V","section":"grid","slots":{"one":{"index":0,"signed":false}},"scale":0.1,"decimals":1,"chart":{"name":"voltage","hidden":false},"peak":"all_day","feature":null,"summary":null,"note":null}"#
        );
    }

    #[test]
    fn a_metric_round_trips_through_json() {
        let text = serde_json::to_string(&voltage()).unwrap();
        assert_eq!(serde_json::from_str::<Metric>(&text).unwrap(), voltage());
    }

    #[test]
    fn a_site_from_a_server_that_predates_the_limit_has_none_rather_than_zero() {
        let older = r#"{"inverter_serial":"","rating_kw":3.0,"timezone":"Asia/Kolkata",
                        "poll_interval_seconds":5,"co2_kg_per_kwh":0.71}"#;
        let site: Site = serde_json::from_str(older).unwrap();
        assert_eq!(site.max_output_kw, None);
        assert_eq!(site.panel_kwp, None);
    }

    #[test]
    fn scaled_rounds_to_the_declared_decimals() {
        assert_eq!(voltage().scaled(2321), 232.1);
    }

    #[test]
    fn highest_index_reads_the_higher_slot_of_two() {
        assert_eq!(DataSlots::Two { low: 19, high: 20, signed: false }.highest_index(), 20);
        assert_eq!(DataSlots::One { index: 23, signed: false }.highest_index(), 23);
    }
}
