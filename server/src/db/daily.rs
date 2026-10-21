//! The per-day summary table, and the queries that summarise a day.

use rusqlite::{params, Connection, OptionalExtension};

use super::derating;

/// One local day's figures as raw register values, before scaling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DayRaw {
    pub samples: i64,
    pub yield_raw: Option<i64>,
    pub peak_raw: Option<i64>,
    pub peak_ts: Option<i64>,
    pub producing_from: Option<i64>,
    pub producing_until: Option<i64>,
    pub lifetime_raw: Option<i64>,
    pub fault_readings: i64,
    pub limit_seconds: Option<i64>,
    pub derate_count: Option<i64>,
    pub derate_seconds: Option<i64>,
}

/// The generated columns a day summary reads, by role.
pub struct RoleColumns {
    pub hero: String,
    pub daily_yield: String,
    pub lifetime_yield: String,
    /// The column measured against the inverter's limit, and the raw
    /// reading in it from which the inverter counts as at the limit, when
    /// one is known.
    pub limit_column: String,
    pub limit_raw: Option<i64>,
    pub derating: Option<DeratingColumns>,
}

/// What heat derating is read from: a column and its scale for each of the
/// output, the panel voltage and the temperature, and the level the
/// inverter steps down to.
pub struct DeratingColumns {
    pub output: (String, f64),
    pub panel_voltage: (String, f64),
    pub temperature: (String, f64),
    pub level_watts: f64,
}

/// Bumped whenever the way a day is summed up changes, so stored day rows
/// are worked out again; the fingerprint carries it.
pub const SUMMARY_VERSION: u32 = 1;

/// Longest step between two readings at the limit that still counts as
/// time spent there; a longer one is an outage, not time at the limit.
pub const LONGEST_STEP_AT_LIMIT: i64 = 600;

/// Columns added to the table after it was first made.
const LATER_COLUMNS: [&str; 3] = ["limit_seconds", "derate_count", "derate_seconds"];

/// Adds the columns a table made before them is missing.
pub fn add_missing_columns(connection: &Connection) -> rusqlite::Result<()> {
    for column in LATER_COLUMNS {
        let present: bool = connection.query_row(
            "SELECT COUNT(*) > 0 FROM pragma_table_info('daily') WHERE name = ?1",
            [column],
            |row| row.get(0),
        )?;
        if !present {
            connection.execute_batch(&format!("ALTER TABLE daily ADD COLUMN {column} INTEGER;"))?;
        }
    }
    Ok(())
}

/// Completed days only. Values are raw, so a change of scale or decimals
/// never makes a row stale; the fingerprint catches everything else a row
/// depends on.
pub const CREATE_TABLE: &str = "CREATE TABLE IF NOT EXISTS daily (
    date            TEXT PRIMARY KEY,
    fingerprint     TEXT NOT NULL,
    samples         INTEGER NOT NULL,
    yield_raw       INTEGER,
    peak_raw        INTEGER,
    peak_ts         INTEGER,
    producing_from  INTEGER,
    producing_until INTEGER,
    lifetime_raw    INTEGER,
    fault_readings  INTEGER NOT NULL,
    limit_seconds   INTEGER,
    derate_count    INTEGER,
    derate_seconds  INTEGER
);";

/// Summarises the readings in a window, and returns the stretches at the
/// limit and in heat derating that its figures for both add up. The day is
/// one contiguous ts range, so each statement is a rowid scan of that day
/// alone. The peak's time and the closing lifetime count are separate
/// statements because each needs its own ordering.
pub fn summarise(
    connection: &Connection,
    columns: &RoleColumns,
    from: i64,
    to: i64,
) -> rusqlite::Result<(DayRaw, Marks)> {
    let RoleColumns {
        hero,
        daily_yield,
        lifetime_yield,
        ..
    } = columns;
    let (samples, yield_raw, peak_raw, producing_from, producing_until, fault_readings) =
        connection.query_row(
            &format!(
                "SELECT COUNT(*), MAX({daily_yield}), MAX({hero}),
                        MIN(CASE WHEN {hero} > 0 THEN ts END),
                        MAX(CASE WHEN {hero} > 0 THEN ts END),
                        COALESCE(SUM(raw_error IS NOT NULL), 0)
                 FROM samples WHERE ts >= ?1 AND ts <= ?2"
            ),
            params![from, to],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )?;
    let peak_ts = match peak_raw {
        Some(peak) => connection
            .query_row(
                &format!(
                    "SELECT ts FROM samples WHERE ts >= ?1 AND ts <= ?2 AND {hero} = ?3
                     ORDER BY ts LIMIT 1"
                ),
                params![from, to, peak],
                |row| row.get(0),
            )
            .optional()?,
        None => None,
    };
    let lifetime_raw = connection
        .query_row(
            &format!(
                "SELECT {lifetime_yield} FROM samples
                 WHERE ts >= ?1 AND ts <= ?2 AND {lifetime_yield} IS NOT NULL
                 ORDER BY ts DESC LIMIT 1"
            ),
            params![from, to],
            |row| row.get(0),
        )
        .optional()?;
    let marks = marks(connection, columns, from, to)?;
    let length = |stretches: &Vec<[i64; 2]>| stretches.iter().map(|[start, end]| end - start).sum::<i64>();
    let raw = DayRaw {
        samples,
        yield_raw,
        peak_raw,
        peak_ts,
        producing_from,
        producing_until,
        lifetime_raw,
        fault_readings,
        // The runs at the limit add up to the time spent there.
        limit_seconds: marks.at_limit.as_ref().map(length),
        derate_count: marks.derating.as_ref().map(|derates| derates.len() as i64),
        derate_seconds: marks.derating.as_ref().map(length),
    };
    Ok((raw, marks))
}

/// What a window's readings say about the inverter's limit and heat
/// derating, each as first and last reading of every stretch, oldest first.
/// None where the limit is unknown or the derating metrics are not named.
#[derive(Debug, Default)]
pub struct Marks {
    pub at_limit: Option<Vec<[i64; 2]>>,
    pub derating: Option<Vec<[i64; 2]>>,
}

/// Reads the window's readings once for both kinds of stretch. Every column
/// read out of a stored reading costs as much as the scan itself, so one
/// pass over the few columns these need replaces a query for each.
///
/// A run at the limit ends at a reading below the limit, one without a
/// value, or a step longer than an outage allows; a lone reading at the
/// limit spans no time and is left out. Derating reads only readings that
/// carry all three of its values.
pub fn marks(connection: &Connection, columns: &RoleColumns, from: i64, to: i64) -> rusqlite::Result<Marks> {
    let RoleColumns { limit_column, limit_raw, derating, .. } = columns;
    if limit_raw.is_none() && derating.is_none() {
        return Ok(Marks::default());
    }
    let derating_columns = derating
        .as_ref()
        .map(|derating| format!(", {}, {}, {}", derating.output.0, derating.panel_voltage.0, derating.temperature.0))
        .unwrap_or_default();
    let mut statement = connection.prepare(&format!(
        "SELECT ts, {limit_column}{derating_columns} FROM samples WHERE ts >= ?1 AND ts <= ?2 ORDER BY ts"
    ))?;
    let mut rows = statement.query(params![from, to])?;

    let mut at_limit = Vec::new();
    let mut run: Option<[i64; 2]> = None;
    let close = |run: Option<[i64; 2]>, at_limit: &mut Vec<[i64; 2]>| {
        if let Some([start, end]) = run.filter(|[start, end]| end > start) {
            at_limit.push([start, end]);
        }
    };
    let mut readings = Vec::new();
    while let Some(row) = rows.next()? {
        let ts: i64 = row.get(0)?;
        if let Some(limit) = limit_raw {
            let value: Option<i64> = row.get(1)?;
            let here = value.is_some_and(|value| value >= *limit);
            run = match run {
                Some([start, end]) if here && ts - end <= LONGEST_STEP_AT_LIMIT => Some([start, ts]),
                finished => {
                    close(finished, &mut at_limit);
                    here.then_some([ts, ts])
                }
            };
        }
        if let Some(derating) = derating {
            let values: (Option<i64>, Option<i64>, Option<i64>) = (row.get(2)?, row.get(3)?, row.get(4)?);
            if let (Some(output), Some(volts), Some(celsius)) = values {
                readings.push(derating::Reading {
                    ts,
                    output_watts: output as f64 * derating.output.1,
                    panel_volts: volts as f64 * derating.panel_voltage.1,
                    celsius: celsius as f64 * derating.temperature.1,
                });
            }
        }
    }
    close(run, &mut at_limit);
    Ok(Marks {
        at_limit: limit_raw.map(|_| at_limit),
        derating: derating.as_ref().map(|derating| {
            derating::derates(&readings, derating.level_watts)
                .into_iter()
                .map(|(start, end)| [start, end])
                .collect()
        }),
    })
}

/// A stored row, if one exists for the date with this fingerprint.
pub fn cached(
    connection: &Connection,
    date: &str,
    fingerprint: &str,
) -> rusqlite::Result<Option<DayRaw>> {
    connection
        .query_row(
            "SELECT samples, yield_raw, peak_raw, peak_ts, producing_from, producing_until,
                    lifetime_raw, fault_readings, limit_seconds, derate_count, derate_seconds
             FROM daily WHERE date = ?1 AND fingerprint = ?2",
            params![date, fingerprint],
            |row| {
                Ok(DayRaw {
                    samples: row.get(0)?,
                    yield_raw: row.get(1)?,
                    peak_raw: row.get(2)?,
                    peak_ts: row.get(3)?,
                    producing_from: row.get(4)?,
                    producing_until: row.get(5)?,
                    lifetime_raw: row.get(6)?,
                    fault_readings: row.get(7)?,
                    limit_seconds: row.get(8)?,
                    derate_count: row.get(9)?,
                    derate_seconds: row.get(10)?,
                })
            },
        )
        .optional()
}

/// Stores a completed day, replacing any row the date had.
pub fn store(
    connection: &Connection,
    date: &str,
    fingerprint: &str,
    raw: &DayRaw,
) -> rusqlite::Result<()> {
    connection.execute(
        "INSERT OR REPLACE INTO daily
            (date, fingerprint, samples, yield_raw, peak_raw, peak_ts, producing_from,
             producing_until, lifetime_raw, fault_readings, limit_seconds, derate_count,
             derate_seconds)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            date,
            fingerprint,
            raw.samples,
            raw.yield_raw,
            raw.peak_raw,
            raw.peak_ts,
            raw.producing_from,
            raw.producing_until,
            raw.lifetime_raw,
            raw.fault_readings,
            raw.limit_seconds,
            raw.derate_count,
            raw.derate_seconds
        ],
    )?;
    Ok(())
}

/// The hero column's raw readings in a window, oldest first.
pub fn hero_series(
    connection: &Connection,
    hero: &str,
    from: i64,
    to: i64,
) -> rusqlite::Result<Vec<(i64, Option<i64>)>> {
    let mut statement = connection.prepare(&format!(
        "SELECT ts, {hero} FROM samples WHERE ts >= ?1 AND ts <= ?2 ORDER BY ts"
    ))?;
    let rows = statement.query_map(params![from, to], |row| Ok((row.get(0)?, row.get(1)?)))?;
    rows.collect()
}
