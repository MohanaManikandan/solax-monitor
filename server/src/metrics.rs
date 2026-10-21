use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

pub use solax_api::{Chart, DataSlots, Feature, Metric, Peak, Section};

/// Every metric in page order, plus the ones that play a role of their own.
#[derive(Debug, Clone)]
pub struct Registry {
    hero: String,
    run_mode: String,
    daily_yield: String,
    lifetime_yield: String,
    grid_countdown: Option<String>,
    output: Option<String>,
    derating: Option<DeratingRoles>,
    metrics: Vec<Metric>,
}

/// The metrics that show the inverter stepping its output down when it
/// runs too hot, by key.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeratingRoles {
    output: String,
    panel_voltage: String,
    temperature: String,
}

/// The derating metrics themselves.
pub struct DeratingMetrics<'registry> {
    /// The power the inverter delivers, in W.
    pub output: &'registry Metric,
    /// The panels' voltage, in V.
    pub panel_voltage: &'registry Metric,
    /// The inverter's temperature, in C.
    pub temperature: &'registry Metric,
}

impl Registry {
    pub fn metrics(&self) -> &[Metric] {
        &self.metrics
    }

    /// The metric whose value above zero means the plant is producing.
    pub fn hero(&self) -> &Metric {
        self.named(&self.hero)
    }

    /// The metric holding the inverter's run mode, which drives the fault
    /// badge.
    pub fn run_mode(&self) -> &Metric {
        self.named(&self.run_mode)
    }

    /// The metric counting today's yield, which resets each morning.
    pub fn daily_yield(&self) -> &Metric {
        self.named(&self.daily_yield)
    }

    /// The metric counting yield over the inverter's lifetime.
    pub fn lifetime_yield(&self) -> &Metric {
        self.named(&self.lifetime_yield)
    }

    /// The metric measured against the inverter's output limit: the power
    /// it delivers when the file names it, the hero otherwise. The limit caps
    /// what the inverter delivers, so the panels' own power can sit at the
    /// limit while the output is still under it.
    pub fn limit_metric(&self) -> &Metric {
        self.named(self.output.as_deref().unwrap_or(&self.hero))
    }

    /// The metrics heat derating is read from, when the file names them.
    pub fn derating(&self) -> Option<DeratingMetrics<'_>> {
        let roles = self.derating.as_ref()?;
        Some(DeratingMetrics {
            output: self.named(&roles.output),
            panel_voltage: self.named(&roles.panel_voltage),
            temperature: self.named(&roles.temperature),
        })
    }

    pub fn roles(&self) -> solax_api::Roles {
        solax_api::Roles {
            hero: self.hero.clone(),
            run_mode: self.run_mode.clone(),
            daily_yield: self.daily_yield.clone(),
            lifetime_yield: self.lifetime_yield.clone(),
            grid_countdown: self.grid_countdown.clone(),
            output: self.output.clone(),
        }
    }

    pub fn is_run_mode(&self, metric: &Metric) -> bool {
        metric.key == self.run_mode
    }

    pub fn charted(&self) -> impl Iterator<Item = &Metric> {
        self.metrics.iter().filter(|metric| metric.chart.is_some())
    }

    pub fn peaked(&self) -> impl Iterator<Item = &Metric> {
        self.metrics.iter().filter(|metric| metric.peak.is_some())
    }

    /// Highest Data index any metric reads, used to sanity check replies.
    pub fn highest_index(&self) -> usize {
        self.metrics
            .iter()
            .map(|metric| metric.slots.highest_index())
            .max()
            .unwrap_or(0)
    }

    fn find(&self, key: &str) -> Option<&Metric> {
        self.metrics.iter().find(|metric| metric.key == key)
    }

    fn named(&self, key: &str) -> &Metric {
        self.find(key)
            .expect("a registry checks that the metrics it names exist when it loads")
    }

    fn validate(&self) -> Result<()> {
        if self.metrics.is_empty() {
            bail!("the metric registry is empty");
        }
        let mut seen = HashSet::new();
        for metric in &self.metrics {
            if metric.key.is_empty()
                || !metric
                    .key
                    .chars()
                    .all(|character| {
                        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
                    })
            {
                bail!(
                    "metric key {:?} must be lowercase letters, digits and underscores; \
                     it becomes a column name",
                    metric.key
                );
            }
            if !seen.insert(metric.key.as_str()) {
                bail!("duplicate metric key {:?}", metric.key);
            }
            // Peaks take MAX over the raw value and scale afterwards, which
            // only agrees with the scaled extreme when the scale is positive.
            // A negative one would quietly report the day's minimum as its
            // peak, so it is rejected rather than half supported.
            //
            // Written as a negated greater-than on purpose: it also rejects
            // NaN, which `<= 0.0` would let through.
            if !(metric.scale > 0.0) {
                bail!(
                    "metric {:?} has scale {}; it must be greater than zero",
                    metric.key,
                    metric.scale
                );
            }
            // The name becomes the chart's title and a switcher button.
            if let Some(chart) = &metric.chart {
                if chart.name.is_empty()
                    || !chart
                        .name
                        .chars()
                        .all(|character| character.is_ascii_lowercase() || character == '_')
                {
                    bail!(
                        "metric {:?} names chart {:?}; a chart name must be lowercase \
                         letters and underscores",
                        metric.key,
                        chart.name
                    );
                }
                // The page's switcher keeps "off" for the button that hides
                // the second chart, so a chart by that name could never show.
                if chart.name == "off" {
                    bail!(
                        "metric {:?} names chart \"off\", which the page keeps for its Off button",
                        metric.key
                    );
                }
            }
        }

        // The page reads the hero's line to tell when the plant is producing:
        // where the chart's window opens, which readings fit the voltage
        // axis, and which count toward a gated peak marker. History only
        // carries charted metrics, so an uncharted hero would quietly switch
        // all of that off while the cards kept applying the rule.
        let Some(hero) = self.find(&self.hero) else {
            bail!("hero = {:?} names no metric", self.hero);
        };
        if hero.chart.is_none() {
            bail!(
                "hero metric {:?} must name a chart; the page reads its line to tell \
                 when the plant is producing",
                hero.key
            );
        }
        for (role, key) in [
            ("run_mode", &self.run_mode),
            ("daily_yield", &self.daily_yield),
            ("lifetime_yield", &self.lifetime_yield),
        ] {
            if self.find(key).is_none() {
                bail!("{role} = {key:?} names no metric");
            }
        }
        if let Some(key) = &self.grid_countdown {
            if self.find(key).is_none() {
                bail!("grid_countdown = {key:?} names no metric");
            }
        }
        // The output is measured against a limit in watts.
        if let Some(key) = &self.output {
            let Some(metric) = self.find(key) else {
                bail!("output = {key:?} names no metric");
            };
            if metric.unit != "W" {
                bail!("output = {key:?} is in {:?}; it is measured against a limit in \"W\"", metric.unit);
            }
        }
        // The derating rule works in watts, volts and degrees, so each of
        // its metrics has to be in that unit.
        if let Some(roles) = &self.derating {
            for (role, key, unit) in [
                ("output", &roles.output, "W"),
                ("panel_voltage", &roles.panel_voltage, "V"),
                ("temperature", &roles.temperature, "C"),
            ] {
                let Some(metric) = self.find(key) else {
                    bail!("derating.{role} = {key:?} names no metric");
                };
                if metric.unit != unit {
                    bail!(
                        "derating.{role} = {key:?} is in {:?}; derating reads it in {unit:?}",
                        metric.unit
                    );
                }
            }
        }

        // A chart has one axis on each side, so it can carry two units. A
        // third would have no axis to be read against.
        let mut units_by_chart: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
        for metric in &self.metrics {
            if let Some(chart) = &metric.chart {
                let units = units_by_chart.entry(chart.name.as_str()).or_default();
                if !units.contains(&metric.unit.as_str()) {
                    units.push(metric.unit.as_str());
                }
            }
        }
        if let Some((chart, units)) = units_by_chart.iter().find(|(_, units)| units.len() > 2) {
            bail!(
                "chart {:?} mixes {} units ({}); it has one axis each side, so two at most",
                chart,
                units.len(),
                units.join(", ")
            );
        }
        Ok(())
    }
}

/// The file exactly as written, one list of metrics per section.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RegistryFile {
    hero: String,
    run_mode: String,
    daily_yield: String,
    lifetime_yield: String,
    #[serde(default)]
    grid_countdown: Option<String>,
    #[serde(default)]
    output: Option<String>,
    #[serde(default)]
    derating: Option<DeratingRoles>,
    #[serde(default)]
    solar: Vec<MetricEntry>,
    #[serde(default)]
    grid: Vec<MetricEntry>,
    #[serde(default)]
    inverter: Vec<MetricEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MetricEntry {
    key: String,
    label: String,
    #[serde(default)]
    unit: String,
    index: IndexEntry,
    #[serde(default)]
    signed: bool,
    #[serde(default = "unit_scale")]
    scale: f64,
    #[serde(default)]
    decimals: u8,
    chart: Option<ChartEntry>,
    peak: Option<Peak>,
    feature: Option<FeatureEntry>,
    summary: Option<String>,
    note: Option<String>,
}

fn unit_scale() -> f64 {
    1.0
}

/// One slot as `index = 4`, or two as `index = [19, 20]`, low slot first.
#[derive(Deserialize)]
#[serde(untagged)]
enum IndexEntry {
    One(usize),
    Two([usize; 2]),
}

/// `chart = "power"`, or a table when the line starts hidden.
#[derive(Deserialize)]
#[serde(untagged)]
enum ChartEntry {
    Name(String),
    Table(ChartTable),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChartTable {
    name: String,
    #[serde(default)]
    hidden: bool,
}

/// `feature = "Generated today"`, or a table when it renders quietly.
#[derive(Deserialize)]
#[serde(untagged)]
enum FeatureEntry {
    Label(String),
    Table(FeatureTable),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FeatureTable {
    label: String,
    #[serde(default)]
    subtle: bool,
}

impl From<RegistryFile> for Registry {
    /// Sections go in page order, solar then grid then inverter, and each
    /// metric keeps its place within its section.
    fn from(file: RegistryFile) -> Self {
        let RegistryFile {
            hero,
            run_mode,
            daily_yield,
            lifetime_yield,
            grid_countdown,
            output,
            derating,
            solar,
            grid,
            inverter,
        } = file;
        let metrics = [
            (Section::Solar, solar),
            (Section::Grid, grid),
            (Section::Inverter, inverter),
        ]
        .into_iter()
        .flat_map(|(section, entries)| {
            entries
                .into_iter()
                .map(move |entry| entry.into_metric(section))
        })
        .collect();
        Registry {
            hero,
            run_mode,
            daily_yield,
            lifetime_yield,
            grid_countdown,
            output,
            derating,
            metrics,
        }
    }
}

impl MetricEntry {
    fn into_metric(self, section: Section) -> Metric {
        let slots = match self.index {
            IndexEntry::One(index) => DataSlots::One {
                index,
                signed: self.signed,
            },
            IndexEntry::Two([low, high]) => DataSlots::Two {
                low,
                high,
                signed: self.signed,
            },
        };
        Metric {
            key: self.key,
            label: self.label,
            unit: self.unit,
            section,
            slots,
            scale: self.scale,
            decimals: self.decimals,
            chart: self.chart.map(Chart::from),
            peak: self.peak,
            feature: self.feature.map(Feature::from),
            summary: self.summary,
            note: self.note,
        }
    }
}

impl From<ChartEntry> for Chart {
    fn from(entry: ChartEntry) -> Self {
        match entry {
            ChartEntry::Name(name) => Chart {
                name,
                hidden: false,
            },
            ChartEntry::Table(table) => Chart {
                name: table.name,
                hidden: table.hidden,
            },
        }
    }
}

impl From<FeatureEntry> for Feature {
    fn from(entry: FeatureEntry) -> Self {
        match entry {
            FeatureEntry::Label(label) => Feature {
                label,
                subtle: false,
            },
            FeatureEntry::Table(table) => Feature {
                label: table.label,
                subtle: table.subtle,
            },
        }
    }
}

/// Run mode labels. Information.txt documents no table for inverter type 22,
/// so this borrows the type 18 set: both are X1 non-hybrid, and the observed
/// value of 2 is "Normal" in every documented group. Unknown codes are
/// rendered as "Unknown (N)" rather than blank.
pub fn run_mode_label(code: i64) -> String {
    let name = match code {
        0 => "Waiting",
        1 => "Checking",
        2 => "Normal",
        3 => "Fault",
        4 => "Permanent Fault",
        5 => "Upgrading",
        6 => "EPS Checking",
        7 => "EPS",
        _ => return format!("Unknown ({code})"),
    };
    name.to_string()
}

/// How much a run mode should alarm. Normal is silent; a fault earns a badge
/// at the top of the page.
pub fn run_mode_severity(code: i64) -> solax_api::Severity {
    match code {
        2 => solax_api::Severity::Ok,
        3 | 4 => solax_api::Severity::Fault,
        // Running from batteries rather than the grid. Not broken, but not
        // the usual state either.
        7 => solax_api::Severity::Warn,
        _ => solax_api::Severity::Info,
    }
}

/// Names for the codes in the Error object's ErrorCode list.
///
/// Taken from what this inverter reported for a real fault, not from the IE
/// numbering in the manual: the two do not line up. Codes beyond these are
/// shown as bare numbers rather than guesses, because a wrong fault name is
/// worse than an honest number when diagnosing one.
pub fn fault_code_label(code: &str) -> String {
    match code.trim() {
        "1" => "Grid Lost Fault".to_string(),
        "2" => "Grid Volt Fault".to_string(),
        "3" => "Grid Frequency Fault".to_string(),
        other if other.is_empty() => "Unspecified fault".to_string(),
        other => format!("Fault code {other}"),
    }
}

pub fn path() -> PathBuf {
    std::env::var_os("SOLAX_METRICS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("metrics.toml"))
}

/// Registry compiled into the binary, used when no metrics.toml is present.
const EMBEDDED: &str = include_str!("../metrics.toml");

pub fn load() -> Result<Registry> {
    let path = path();
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => {
            tracing::info!(path = %path.display(), "loaded metric registry");
            text
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::info!("no metrics.toml on disk, using the embedded registry");
            EMBEDDED.to_string()
        }
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    parse(&text).with_context(|| format!("parsing {}", path.display()))
}

pub(crate) fn parse(text: &str) -> Result<Registry> {
    let file: RegistryFile = toml::from_str(text)?;
    let registry = Registry::from(file);
    registry.validate()?;
    Ok(registry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_registry_parses_and_validates() {
        parse(EMBEDDED).unwrap();
    }

    #[test]
    fn reads_short_and_long_forms_in_section_order() {
        let registry = parse(
            r#"
hero = "power"
run_mode = "mode"
daily_yield = "today"
lifetime_yield = "today"

[[inverter]]
key = "mode"
label = "Run mode"
index = 10

[[grid]]
key = "current"
label = "Current"
unit = "A"
index = 1
signed = true
scale = 0.1
decimals = 1
feature = { label = "Grid current", subtle = true }

[[solar]]
key = "power"
label = "Power"
unit = "W"
index = 13
chart = "power"
peak = "while_producing"

[[solar]]
key = "today"
label = "Today"
unit = "kWh"
index = [19, 20]
chart = { name = "power", hidden = true }
feature = "Generated today"
"#,
        )
        .unwrap();

        let keys: Vec<&str> = registry.metrics().iter().map(|metric| metric.key.as_str()).collect();
        assert_eq!(keys, ["power", "today", "current", "mode"]);

        let power = &registry.metrics()[0];
        assert_eq!(power.section, Section::Solar);
        assert_eq!(power.slots, DataSlots::One { index: 13, signed: false });
        assert_eq!(power.chart, Some(Chart { name: "power".into(), hidden: false }));
        assert_eq!(power.peak, Some(Peak::WhileProducing));
        assert_eq!((power.scale, power.decimals), (1.0, 0));

        let today = &registry.metrics()[1];
        assert_eq!(today.slots, DataSlots::Two { low: 19, high: 20, signed: false });
        assert_eq!(today.chart, Some(Chart { name: "power".into(), hidden: true }));
        assert_eq!(
            today.feature,
            Some(Feature { label: "Generated today".into(), subtle: false })
        );

        let current = &registry.metrics()[2];
        assert_eq!(current.section, Section::Grid);
        assert_eq!(current.slots, DataSlots::One { index: 1, signed: true });
        assert_eq!(
            current.feature,
            Some(Feature { label: "Grid current".into(), subtle: true })
        );

        assert_eq!(registry.hero().key, "power");
        assert_eq!(registry.run_mode().key, "mode");
        assert_eq!(registry.daily_yield().key, "today");
        assert_eq!(registry.lifetime_yield().key, "today");
    }

    #[test]
    fn a_grid_countdown_role_is_optional_and_must_name_a_metric() {
        let file = |countdown: &str| {
            format!(
                r#"
hero = "power"
run_mode = "power"
daily_yield = "power"
lifetime_yield = "power"
{countdown}

[[solar]]
key = "power"
label = "Power"
index = 13
chart = "power"
"#
            )
        };
        assert_eq!(parse(&file("")).unwrap().roles().grid_countdown, None);
        let named = parse(&file(r#"grid_countdown = "power""#)).unwrap();
        assert_eq!(named.roles().grid_countdown.as_deref(), Some("power"));
        let error = parse(&file(r#"grid_countdown = "nothing""#)).unwrap_err();
        assert!(format!("{error:#}").contains("grid_countdown"), "{error:#}");
    }

    #[test]
    fn the_output_role_is_optional_and_must_name_a_metric_in_watts() {
        let file = |output: &str| {
            format!(
                r#"
hero = "power"
run_mode = "power"
daily_yield = "power"
lifetime_yield = "power"
{output}

[[solar]]
key = "power"
label = "Power"
unit = "W"
index = 13
chart = "power"

[[grid]]
key = "delivered"
label = "Power"
unit = "W"
index = 3

[[grid]]
key = "volts"
label = "Voltage"
unit = "V"
index = 0
"#
            )
        };
        assert_eq!(parse(&file("")).unwrap().limit_metric().key, "power", "the hero stands in");
        let named = parse(&file(r#"output = "delivered""#)).unwrap();
        assert_eq!(named.limit_metric().key, "delivered");
        assert_eq!(named.roles().output.as_deref(), Some("delivered"));
        let missing = parse(&file(r#"output = "nothing""#)).unwrap_err();
        assert!(format!("{missing:#}").contains("output"), "{missing:#}");
        let volts = parse(&file(r#"output = "volts""#)).unwrap_err();
        assert!(format!("{volts:#}").contains("output"), "{volts:#}");
    }

    #[test]
    fn the_derating_metrics_are_optional_named_together_and_in_their_units() {
        let file = |derating: &str| {
            format!(
                r#"
hero = "power"
run_mode = "power"
daily_yield = "power"
lifetime_yield = "power"
{derating}

[[solar]]
key = "power"
label = "Power"
unit = "W"
index = 13
chart = "power"

[[solar]]
key = "volts"
label = "Voltage"
unit = "V"
index = 4
scale = 0.1

[[inverter]]
key = "heat"
label = "Temperature"
unit = "C"
index = 23
"#
            )
        };
        assert!(parse(&file("")).unwrap().derating().is_none());

        let named = parse(&file(
            "[derating]\noutput = \"power\"\npanel_voltage = \"volts\"\ntemperature = \"heat\"",
        ))
        .unwrap();
        let derating = named.derating().unwrap();
        assert_eq!(
            (derating.output.key.as_str(), derating.panel_voltage.key.as_str(), derating.temperature.key.as_str()),
            ("power", "volts", "heat")
        );

        let missing = parse(&file(
            "[derating]\noutput = \"power\"\npanel_voltage = \"nothing\"\ntemperature = \"heat\"",
        ))
        .unwrap_err();
        assert!(format!("{missing:#}").contains("derating.panel_voltage"), "{missing:#}");

        let wrong_unit = parse(&file(
            "[derating]\noutput = \"volts\"\npanel_voltage = \"volts\"\ntemperature = \"heat\"",
        ))
        .unwrap_err();
        assert!(format!("{wrong_unit:#}").contains("derating.output"), "{wrong_unit:#}");
    }

    #[test]
    fn rejects_a_key_it_does_not_know() {
        let error = parse(
            r#"
hero = "power"
run_mode = "power"
daily_yield = "power"
lifetime_yield = "power"

[[solar]]
key = "power"
label = "Power"
index = 13
chart = "power"
peak_when_producing = true
"#,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("peak_when_producing"), "{error:#}");
    }

    #[test]
    fn rejects_a_hero_that_names_no_metric() {
        let error = parse(
            r#"
hero = "missing"
run_mode = "power"
daily_yield = "power"
lifetime_yield = "power"

[[solar]]
key = "power"
label = "Power"
index = 13
chart = "power"
"#,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("\"missing\""), "{error:#}");
    }

    #[test]
    fn rejects_a_daily_yield_that_names_no_metric() {
        let error = parse(
            r#"
hero = "power"
run_mode = "power"
daily_yield = "missing"
lifetime_yield = "power"

[[solar]]
key = "power"
label = "Power"
index = 13
chart = "power"
"#,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("daily_yield = \"missing\""), "{error:#}");
    }

    #[test]
    fn requires_both_yield_roles() {
        let error = parse(
            r#"
hero = "power"
run_mode = "power"

[[solar]]
key = "power"
label = "Power"
index = 13
chart = "power"
"#,
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("daily_yield"), "{error:#}");
    }
}
