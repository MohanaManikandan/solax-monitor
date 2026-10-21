//! Fixtures for tests: a small registry, a store in a temporary folder, and
//! synthetic days whose answers are known.

use std::sync::Arc;

use chrono::{NaiveDate, TimeZone};
use chrono_tz::Tz;
use serde_json::json;

use crate::db::Store;
use crate::inverter::Sample;
use crate::metrics::{self, Registry};

pub const TIMEZONE: Tz = chrono_tz::Asia::Kolkata;

/// The production registry in miniature: the hero, the two yield counters,
/// one peaked line per producing rule, and the run mode.
pub const REGISTRY: &str = r#"
hero = "pv_power"
run_mode = "run_mode"
daily_yield = "today_kwh"
lifetime_yield = "total_kwh"

[[solar]]
key = "pv_volts"
label = "Voltage"
unit = "V"
index = 4
scale = 0.1
decimals = 1
chart = "voltage"
peak = "while_producing"

[[solar]]
key = "pv_power"
label = "Power"
unit = "W"
index = 13
chart = "power"
peak = "all_day"

[[solar]]
key = "today_kwh"
label = "Today"
unit = "kWh"
index = 21
scale = 0.1
decimals = 1
chart = { name = "power", hidden = true }
summary = "generated"

[[solar]]
key = "total_kwh"
label = "All time"
unit = "kWh"
index = [19, 20]
scale = 0.1
decimals = 1
summary = "lifetime"

[[grid]]
key = "ac_volts"
label = "Voltage"
unit = "V"
index = 0
scale = 0.1
decimals = 1
chart = "voltage"
peak = "all_day"

[[inverter]]
key = "run_mode"
label = "Run mode"
index = 10
"#;

pub fn registry() -> Arc<Registry> {
    Arc::new(metrics::parse(REGISTRY).expect("the test registry is valid"))
}

/// A store in a fresh temporary folder. Keep the folder alive as long as the
/// store is used.
pub fn store() -> (tempfile::TempDir, Store) {
    let folder = tempfile::tempdir().expect("a temporary folder");
    let store = Store::open(&folder.path().join("solax.db"), registry(), TIMEZONE)
        .expect("the store opens");
    (folder, store)
}

/// Epoch seconds of the local midnight opening `date`.
pub fn midnight(date: NaiveDate) -> i64 {
    TIMEZONE
        .from_local_datetime(&date.and_hms_opt(0, 0, 0).expect("midnight exists"))
        .earliest()
        .expect("midnight resolves")
        .timestamp()
}

/// A day whose power rises and falls as a sine between sunrise and sunset,
/// polled from 05:00 to 19:00 local.
pub struct SyntheticDay {
    pub date: NaiveDate,
    pub peak_watts: f64,
    pub sunrise_hour: f64,
    pub sunset_hour: f64,
    pub step_seconds: i64,
    /// The lifetime counter at the day's first reading, in 0.1 kWh counts.
    pub lifetime_start: i64,
    /// Local hour of the one reading that carries an Error object.
    pub fault_hour: Option<f64>,
    /// Local hour of the last reading, for a day the poller stopped early.
    pub stops_at_hour: Option<f64>,
    /// Local hours between which no readings exist.
    pub gap_hours: Option<(f64, f64)>,
    /// Local hours between which the plant makes nothing, as during a fault.
    pub idle_hours: Option<(f64, f64)>,
    /// PV voltage while idle, in 0.1 V counts. Real panels read higher idle
    /// than producing, because nothing draws current.
    pub idle_pv_volts: i64,
    /// Local hour of the one producing reading whose PV voltage is 245.0 V,
    /// above the usual 230.0 V.
    pub pv_volts_spike_hour: Option<f64>,
}

impl SyntheticDay {
    pub fn clear(date: NaiveDate) -> Self {
        Self {
            date,
            peak_watts: 3000.0,
            sunrise_hour: 6.0,
            sunset_hour: 18.0,
            step_seconds: 300,
            lifetime_start: 18_000,
            fault_hour: None,
            stops_at_hour: None,
            gap_hours: None,
            idle_hours: None,
            idle_pv_volts: 400,
            pv_volts_spike_hour: None,
        }
    }

    pub fn power_at(&self, hour: f64) -> f64 {
        if hour <= self.sunrise_hour || hour >= self.sunset_hour {
            return 0.0;
        }
        if self
            .idle_hours
            .is_some_and(|(idle_from, idle_until)| hour > idle_from && hour < idle_until)
        {
            return 0.0;
        }
        let progress = (hour - self.sunrise_hour) / (self.sunset_hour - self.sunrise_hour);
        (self.peak_watts * (std::f64::consts::PI * progress).sin()).round()
    }

    pub fn samples(&self) -> Vec<Sample> {
        let opening = midnight(self.date);
        let last_second = self
            .stops_at_hour
            .map(|hour| (hour * 3600.0) as i64)
            .unwrap_or(19 * 3600);
        let mut energy_watt_seconds = 0.0;
        let mut previous: Option<(i64, f64)> = None;
        let mut samples = Vec::new();
        let mut second = 5 * 3600;
        while second <= last_second {
            let hour = second as f64 / 3600.0;
            let skipped = self
                .gap_hours
                .is_some_and(|(gap_from, gap_until)| hour > gap_from && hour < gap_until);
            let watts = self.power_at(hour);
            if let Some((previous_second, previous_watts)) = previous {
                energy_watt_seconds +=
                    (watts + previous_watts) / 2.0 * (second - previous_second) as f64;
            }
            previous = Some((second, watts));
            if !skipped {
                // 0.1 kWh is 360,000 watt seconds.
                let today_counts = (energy_watt_seconds / 360_000.0).floor() as i64;
                let lifetime = self.lifetime_start + today_counts;
                let mut data = vec![0_i64; 25];
                data[0] = 2300;
                let spike = self
                    .pv_volts_spike_hour
                    .is_some_and(|spike_hour| (spike_hour * 3600.0) as i64 == second);
                data[4] = match (watts > 0.0, spike) {
                    (true, true) => 2450,
                    (true, false) => 2300,
                    (false, _) => self.idle_pv_volts,
                };
                data[10] = 2;
                data[13] = watts as i64;
                data[19] = lifetime % 65_536;
                data[20] = lifetime / 65_536;
                data[21] = today_counts;
                let error = self
                    .fault_hour
                    .filter(|fault_hour| (fault_hour * 3600.0) as i64 == second)
                    .map(|_| json!({ "ErrorCode": "1" }));
                samples.push(Sample {
                    ts: opening + second,
                    sn: "TEST".into(),
                    ver: "3.0".into(),
                    inverter_type: 22,
                    data,
                    information: json!(null),
                    error,
                });
            }
            second += self.step_seconds;
        }
        samples
    }

    pub async fn insert(&self, store: &Store) {
        for sample in self.samples() {
            store.insert_sample(sample).await.expect("the sample is stored");
        }
    }
}

/// Makes every file in a folder read-only, and says whether that stops this
/// process writing them. Root writes read-only files regardless, so a test
/// about an unwritable database cannot run as root and says so instead.
pub fn make_read_only(folder: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    let mut files = Vec::new();
    for entry in std::fs::read_dir(folder).expect("the folder exists") {
        let path = entry.expect("an entry").path();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).expect("permissions change");
        files.push(path);
    }
    let unwritable = files
        .iter()
        .all(|path| std::fs::OpenOptions::new().append(true).open(path).is_err());
    if !unwritable {
        eprintln!("skipped: this process can write read-only files, as root can");
    }
    unwritable
}

pub const TOKEN: &str = "test-token";

pub fn config() -> Arc<crate::config::Config> {
    Arc::new(
        crate::config::parse(&format!(
            r#"
[inverter]
address = "127.0.0.1:9"
dongle_serial = "TEST"
inverter_serial = "XM-TEST"

[server]
bind = "127.0.0.1:0"
token = "{TOKEN}"

[storage]
database_path = "unused.db"
timezone = "Asia/Kolkata"
"#
        ))
        .expect("the test config is valid"),
    )
}

/// The whole server's state over a store in a fresh temporary folder.
pub fn app_state() -> (tempfile::TempDir, crate::api::AppState) {
    let (folder, store) = store();
    let (updates, _) = tokio::sync::broadcast::channel(64);
    let state = crate::api::AppState {
        config: config(),
        registry: registry(),
        store,
        updates,
        curves: crate::projection::CurveCache::default(),
    };
    (folder, state)
}

/// Sends a GET through the full router, auth included, and returns the
/// status and body.
pub async fn get(
    state: &crate::api::AppState,
    path: &str,
    token: Option<&str>,
) -> (axum::http::StatusCode, Vec<u8>) {
    use tower::ServiceExt;

    let mut request = axum::http::Request::builder().uri(path);
    if let Some(token) = token {
        request = request.header("x-auth-token", token);
    }
    let response = crate::api::router(state.clone())
        .oneshot(request.body(axum::body::Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, body.to_vec())
}
