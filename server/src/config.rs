use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use chrono_tz::Tz;
use serde::Deserialize;

use crate::clock::Window;
use crate::inverter::{split_endpoint, Endpoint};

/// Settings with every value already parsed into the type the app works with,
/// so nothing past loading parses or checks them again.
#[derive(Debug, Clone)]
pub struct Config {
    pub inverter: Inverter,
    pub server: Server,
    pub storage: Storage,
    pub site: Site,
    /// Keys an older solax-rs config carries that do nothing here, by
    /// section and name, so start-up can say they can go.
    pub leftover_keys: Vec<&'static str>,
}

#[derive(Debug, Clone)]
pub struct Inverter {
    pub endpoint: Endpoint,
    pub dongle_serial: String,
    pub inverter_serial: String,
    pub rating_kw: f64,
    /// The most the inverter delivers, which the charts draw as its limit.
    pub max_output_kw: f64,
    /// True when the file sets no limit and the rating stands in for it,
    /// which startup warns about: the limit, the time at it and the derated
    /// level all follow from it.
    pub limit_is_the_rating: bool,
    /// The output the inverter steps down to when it runs too hot.
    pub derated_output_kw: f64,
    pub poll_interval: Duration,
    pub allow_public_address: bool,
    /// Hours worth polling. None polls around the clock.
    pub poll_window: Option<Window>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
    #[serde(default = "default_bind")]
    pub bind: String,
    pub token: String,
}

#[derive(Debug, Clone)]
pub struct Storage {
    pub database_path: PathBuf,
    pub timezone: Tz,
}

#[derive(Debug, Clone)]
pub struct Site {
    /// kg of CO2 per kWh of grid electricity displaced.
    pub co2_kg_per_kwh: f64,
    /// The panels' combined peak power, when known.
    pub panel_kwp: Option<f64>,
}

/// The file exactly as written, before any value is parsed. Every table
/// refuses a key it does not know, so a line written under the wrong section
/// stops the server with its name rather than being quietly ignored.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    inverter: InverterTable,
    server: Server,
    storage: StorageTable,
    #[serde(default)]
    site: SiteTable,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InverterTable {
    address: String,
    dongle_serial: String,
    #[serde(default)]
    inverter_serial: String,
    #[serde(default = "default_rating_kw")]
    rating_kw: f64,
    /// The most the inverter delivers, which can sit above its rating.
    /// Defaults to the rating.
    #[serde(default)]
    max_output_kw: Option<f64>,
    /// The output the inverter steps down to when it runs too hot. Defaults
    /// to three quarters of the limit, which is what the X1-Mini G4 does.
    #[serde(default)]
    derated_output_kw: Option<f64>,
    #[serde(default = "default_interval")]
    poll_interval_seconds: u64,
    #[serde(default)]
    allow_public_address: bool,
    /// Local times bounding the hours worth polling, as "HH:MM". Leave both
    /// out to poll around the clock. Set both or neither.
    #[serde(default)]
    poll_from: Option<String>,
    #[serde(default)]
    poll_until: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StorageTable {
    #[serde(default = "default_database_path")]
    database_path: PathBuf,
    #[serde(default = "default_timezone")]
    timezone: String,
    /// Left over from an early solax-rs that imported CSV files. Accepted so
    /// a config copied over at switch-over still loads; it does nothing.
    #[serde(default)]
    import_csv_dir: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SiteTable {
    #[serde(default = "default_co2_kg_per_kwh")]
    co2_kg_per_kwh: f64,
    /// The panels' combined peak power, which specific yield divides by.
    #[serde(default)]
    panel_kwp: Option<f64>,
}

impl Default for SiteTable {
    fn default() -> Self {
        Self {
            co2_kg_per_kwh: default_co2_kg_per_kwh(),
            panel_kwp: None,
        }
    }
}

fn default_co2_kg_per_kwh() -> f64 {
    0.71
}

fn default_rating_kw() -> f64 {
    3.0
}

fn default_interval() -> u64 {
    10
}

fn default_bind() -> String {
    "0.0.0.0:8080".to_string()
}

fn default_database_path() -> PathBuf {
    PathBuf::from("/app/solax-data/solax.db")
}

fn default_timezone() -> String {
    "Asia/Kolkata".to_string()
}

fn parse_time(value: &str, field: &str) -> Result<chrono::NaiveTime> {
    chrono::NaiveTime::parse_from_str(value.trim(), "%H:%M")
        .or_else(|_| chrono::NaiveTime::parse_from_str(value.trim(), "%H:%M:%S"))
        .map_err(|_| anyhow::anyhow!("{field} {value:?} is not a time like \"05:00\""))
}

pub fn path() -> PathBuf {
    std::env::var_os("SOLAX_CONFIG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("config.toml"))
}

pub fn load() -> Result<Config> {
    let path = path();
    let text = std::fs::read_to_string(&path).with_context(|| {
        // Mode 0600 plus a container running as a different uid is the usual
        // cause, and the bare OS error gives no hint of that.
        format!(
            "reading {}. If this is a container, check that it runs as the user \
             that owns the file: the file is deliberately readable by its owner only",
            path.display()
        )
    })?;
    parse(&text).with_context(|| format!("parsing {}", path.display()))
}

pub fn parse(text: &str) -> Result<Config> {
    let file: ConfigFile = toml::from_str(text)?;
    let config = file.parse()?;
    config.validate()?;
    Ok(config)
}

impl ConfigFile {
    fn parse(self) -> Result<Config> {
        let mut leftover_keys = Vec::new();
        if self.storage.import_csv_dir.is_some() {
            leftover_keys.push("storage.import_csv_dir");
        }
        Ok(Config {
            inverter: self.inverter.parse()?,
            server: self.server,
            storage: self.storage.parse()?,
            site: Site {
                co2_kg_per_kwh: self.site.co2_kg_per_kwh,
                panel_kwp: self.site.panel_kwp,
            },
            leftover_keys,
        })
    }
}

impl InverterTable {
    fn parse(self) -> Result<Inverter> {
        let endpoint = split_endpoint(&self.address)?;
        let poll_window = self.poll_window()?;
        let max_output_kw = self.max_output_kw.unwrap_or(self.rating_kw);
        Ok(Inverter {
            endpoint,
            dongle_serial: self.dongle_serial,
            inverter_serial: self.inverter_serial,
            rating_kw: self.rating_kw,
            max_output_kw,
            limit_is_the_rating: self.max_output_kw.is_none(),
            // In whole watts: three quarters of 3.3 kW is otherwise
            // 2.4749999999999996.
            derated_output_kw: self
                .derated_output_kw
                .unwrap_or((0.75 * max_output_kw * 1000.0).round() / 1000.0),
            poll_interval: Duration::from_secs(self.poll_interval_seconds),
            allow_public_address: self.allow_public_address,
            poll_window,
        })
    }

    /// Both ends are required together: one alone has no defensible meaning,
    /// and silently ignoring it would leave the inverter polled all night
    /// while the config says otherwise.
    fn poll_window(&self) -> Result<Option<Window>> {
        match (&self.poll_from, &self.poll_until) {
            (None, None) => Ok(None),
            (Some(from), Some(until)) => Ok(Some(Window {
                from: parse_time(from, "inverter.poll_from")?,
                until: parse_time(until, "inverter.poll_until")?,
            })),
            _ => bail!("set both inverter.poll_from and inverter.poll_until, or neither"),
        }
    }
}

impl StorageTable {
    fn parse(self) -> Result<Storage> {
        let timezone = self
            .timezone
            .parse::<Tz>()
            .map_err(|error| anyhow::anyhow!("storage.timezone {:?}: {}", self.timezone, error))?;
        Ok(Storage {
            database_path: self.database_path,
            timezone,
        })
    }
}

impl Config {
    fn validate(&self) -> Result<()> {
        if self.inverter.dongle_serial.trim().is_empty() {
            bail!("inverter.dongle_serial is empty; it is sent as pwd= and checked against the reply");
        }
        let interval_seconds = self.inverter.poll_interval.as_secs();
        if interval_seconds < 1 || interval_seconds > 600 {
            bail!("inverter.poll_interval_seconds must be between 1 and 600");
        }
        if self.inverter.rating_kw <= 0.0 {
            bail!("inverter.rating_kw must be positive");
        }
        if self.inverter.max_output_kw < self.inverter.rating_kw {
            bail!("inverter.max_output_kw cannot be below inverter.rating_kw");
        }
        let derated = self.inverter.derated_output_kw;
        if !(derated > 0.0 && derated < self.inverter.max_output_kw) {
            bail!("inverter.derated_output_kw must be above zero and below inverter.max_output_kw");
        }
        if self.server.token.trim().is_empty() {
            bail!("server.token is empty; set one or nobody can reach the page");
        }
        // The token is planted verbatim into a Set-Cookie header, where a
        // semicolon ends the value and a newline is not a legal header byte
        // at all. A hex or base64 token can contain neither, but a hand
        // written one can, and the failure is silent: the visit authenticates
        // and sets the cookie, then every later request is refused because
        // the browser hands back only the part before the semicolon.
        if let Some(bad) = self
            .server
            .token
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || "-._~+/=".contains(*c)))
        {
            bail!(
                "server.token contains {bad:?}, which cannot travel in a cookie. \
                 Use letters, digits and any of - . _ ~ + / = \
                 (openssl rand -hex 16 gives a good one)"
            );
        }
        // Refuse to expose an unprotected page to the network. Loopback is
        // allowed to keep local experiments frictionless.
        let placeholder = self.server.token.trim() == "change-me";
        let loopback_only = self.server.bind.starts_with("127.")
            || self.server.bind.starts_with("localhost")
            || self.server.bind.starts_with("[::1]");
        if placeholder && !loopback_only {
            bail!(
                "server.token is still \"change-me\" while binding {}. \
                 Generate one with: openssl rand -hex 16",
                self.server.bind
            );
        }
        let co2 = self.site.co2_kg_per_kwh;
        if !(co2 >= 0.0) || !co2.is_finite() {
            bail!("site.co2_kg_per_kwh must be a number of zero or more");
        }
        if let Some(kwp) = self.site.panel_kwp {
            if !(kwp > 0.0) || !kwp.is_finite() {
                bail!("site.panel_kwp must be a number above zero");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
[inverter]
address = "127.0.0.1:9"
dongle_serial = "TEST"

[server]
bind = "127.0.0.1:0"
token = "test-token"

[storage]
database_path = "solax.db"
"#;

    #[test]
    fn co2_defaults_to_the_indian_grid_factor() {
        assert_eq!(parse(MINIMAL).unwrap().site.co2_kg_per_kwh, 0.71);
    }

    #[test]
    fn co2_can_be_set_per_site() {
        let text = format!("{MINIMAL}\n[site]\nco2_kg_per_kwh = 0.5\n");
        assert_eq!(parse(&text).unwrap().site.co2_kg_per_kwh, 0.5);
    }

    #[test]
    fn the_panel_capacity_is_optional_and_must_be_positive() {
        assert_eq!(parse(MINIMAL).unwrap().site.panel_kwp, None);
        let set = format!("{MINIMAL}\n[site]\npanel_kwp = 3.665\n");
        assert_eq!(parse(&set).unwrap().site.panel_kwp, Some(3.665));
        let zero = format!("{MINIMAL}\n[site]\npanel_kwp = 0.0\n");
        let error = parse(&zero).unwrap_err();
        assert!(format!("{error:#}").contains("site.panel_kwp"), "{error:#}");
    }

    #[test]
    fn the_output_limit_defaults_to_the_rating_and_says_so() {
        let config = parse(MINIMAL).unwrap();
        assert_eq!(config.inverter.max_output_kw, config.inverter.rating_kw);
        assert!(config.inverter.limit_is_the_rating);
        let set = MINIMAL.replace("[server]", "max_output_kw = 3.3\n\n[server]");
        assert!(!parse(&set).unwrap().inverter.limit_is_the_rating);
    }

    #[test]
    fn a_solax_rs_config_with_its_leftover_import_key_still_loads_and_names_it() {
        let text = MINIMAL.replace("[storage]", "[storage]\nimport_csv_dir = \"/app/solax-data\"");
        let config = parse(&text).unwrap();
        assert_eq!(config.leftover_keys, vec!["storage.import_csv_dir"]);
        assert!(parse(MINIMAL).unwrap().leftover_keys.is_empty());
    }

    #[test]
    fn a_key_in_the_wrong_section_is_refused_by_name() {
        let text = MINIMAL.replace("[storage]", "max_output_kw = 3.3\n\n[storage]");
        let error = parse(&text).unwrap_err();
        assert!(format!("{error:#}").contains("max_output_kw"), "{error:#}");
    }

    #[test]
    fn the_output_limit_can_sit_above_the_rating() {
        let text = MINIMAL.replace("[server]", "max_output_kw = 3.3\n\n[server]");
        assert_eq!(parse(&text).unwrap().inverter.max_output_kw, 3.3);
    }

    #[test]
    fn an_output_limit_below_the_rating_is_rejected() {
        let text = MINIMAL.replace("[server]", "max_output_kw = 2.0\n\n[server]");
        let error = parse(&text).unwrap_err();
        assert!(format!("{error:#}").contains("inverter.max_output_kw"), "{error:#}");
    }

    #[test]
    fn the_derated_output_defaults_to_three_quarters_of_the_limit() {
        let text = MINIMAL.replace("[server]", "max_output_kw = 3.3\n\n[server]");
        // Whole watts, so it logs and fingerprints as 2.475 rather than
        // 2.4749999999999996.
        assert_eq!(parse(&text).unwrap().inverter.derated_output_kw, 2.475);
    }

    #[test]
    fn the_derated_output_can_be_set_and_must_sit_below_the_limit() {
        let set = MINIMAL.replace("[server]", "derated_output_kw = 1.5\n\n[server]");
        assert_eq!(parse(&set).unwrap().inverter.derated_output_kw, 1.5);
        let too_high = MINIMAL.replace("[server]", "derated_output_kw = 3.0\n\n[server]");
        let error = parse(&too_high).unwrap_err();
        assert!(format!("{error:#}").contains("inverter.derated_output_kw"), "{error:#}");
    }

    #[test]
    fn a_negative_co2_factor_is_rejected() {
        let text = format!("{MINIMAL}\n[site]\nco2_kg_per_kwh = -1.0\n");
        let error = parse(&text).unwrap_err();
        assert!(format!("{error:#}").contains("site.co2_kg_per_kwh"), "{error:#}");
    }
}
