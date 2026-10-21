use chrono::{DateTime, Duration, NaiveDate, NaiveTime, TimeZone};
use chrono_tz::Tz;

/// Epoch seconds for local midnight opening `date` and the instant just
/// before the next local midnight. Days are always resolved in the configured
/// zone: on Asia/Kolkata a UTC day would start at 05:30 local and cut the
/// solar curve in the middle of the morning.
pub fn day_bounds(tz: Tz, date: NaiveDate) -> (i64, i64) {
    let start = local_midnight(tz, date);
    let end = local_midnight(tz, date + Duration::days(1)) - 1;
    (start, end)
}

fn local_midnight(tz: Tz, date: NaiveDate) -> i64 {
    let naive = date.and_hms_opt(0, 0, 0).expect("midnight is a valid time");
    tz.from_local_datetime(&naive)
        .earliest()
        // A zone can skip midnight during a DST jump. Walk forward until the
        // clock exists rather than failing the request.
        .or_else(|| {
            (1..=4).find_map(|hour| {
                let candidate = date.and_hms_opt(hour, 0, 0)?;
                tz.from_local_datetime(&candidate).earliest()
            })
        })
        .map(|moment| moment.timestamp())
        .unwrap_or(0)
}

pub fn today(tz: Tz) -> NaiveDate {
    chrono::Utc::now().with_timezone(&tz).date_naive()
}

/// The hours of the local day worth polling. Outside them the inverter has
/// powered down and the dongle stops answering, so every request is a failure
/// and a warning line for nothing.
#[derive(Debug, Clone, Copy)]
pub struct Window {
    pub from: NaiveTime,
    pub until: NaiveTime,
}

impl Window {
    pub fn contains(&self, at: DateTime<Tz>) -> bool {
        let now = at.time();
        if self.from <= self.until {
            // An ordinary daytime window: 05:00 to 19:00.
            now >= self.from && now < self.until
        } else {
            // A window written the other way round wraps midnight, so 22:00
            // to 06:00 means the night rather than never.
            now >= self.from || now < self.until
        }
    }

    /// When polling next resumes, as epoch seconds. Resolved in local time on
    /// every call so a daylight saving shift moves the boundary with the
    /// clock rather than drifting an hour away from it.
    pub fn next_start(&self, tz: Tz, after: DateTime<Tz>) -> i64 {
        let today = after.date_naive();
        for offset in 0..=2 {
            let date = today + Duration::days(offset);
            let Some(moment) = resolve_local(tz, date, self.from) else {
                continue;
            };
            if moment > after.timestamp() {
                return moment;
            }
        }
        // Unreachable for any real window, but a sane answer beats a panic.
        after.timestamp() + 3600
    }
}

/// Local wall clock to epoch seconds, stepping over a time that a daylight
/// saving jump skipped rather than failing.
fn resolve_local(tz: Tz, date: NaiveDate, time: NaiveTime) -> Option<i64> {
    let naive = date.and_time(time);
    if let Some(moment) = tz.from_local_datetime(&naive).earliest() {
        return Some(moment.timestamp());
    }
    (1..=120).find_map(|minutes| {
        let shifted = naive + Duration::minutes(minutes);
        tz.from_local_datetime(&shifted)
            .earliest()
            .map(|moment| moment.timestamp())
    })
}
