use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use chrono::NaiveDate;
use chrono_tz::Tz;
use rusqlite::{params, Connection, OptionalExtension};

use crate::inverter::Sample;
use crate::metrics::{DataSlots, Metric, Peak, Registry};

mod daily;
mod derating;

pub use daily::DayRaw;

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub ts: i64,
    /// Firmware string and inverter type as the device reported them for this
    /// sample. Null on rows imported from CSV, which predate the database
    /// knowing about them. The stored `sn` is not read back: it doubles as the
    /// local API password, so it stays out of anything the server hands out.
    pub ver: Option<String>,
    pub inverter_type: Option<i64>,
    /// The Error object exactly as the dongle sent it, present only on a
    /// faulted reading. It sits outside Data, so no slot and no metric can
    /// describe it.
    pub error: Option<String>,
    /// Raw values keyed by metric key. Missing when the sample has nothing in
    /// that slot, which is the case for imported CSV rows.
    pub raw: BTreeMap<String, i64>,
}

#[derive(Clone)]
pub struct Store {
    connection: Arc<Mutex<Connection>>,
    registry: Arc<Registry>,
    timezone: Tz,
    path: Arc<std::path::PathBuf>,
    /// The most the inverter delivers, in watts, when known.
    output_limit: Option<f64>,
    /// The output the inverter steps down to when it runs too hot, in
    /// watts, when known.
    derated_output: Option<f64>,
    /// The information block last stored. A reading stores its own only when
    /// it differs, since the inverter repeats the same one on every reading.
    last_information: Arc<Mutex<Option<String>>>,
    /// A second connection, for working out ranges of days. A year worked
    /// out for the first time takes seconds; on its own connection it reads
    /// alongside new readings being written instead of holding them up.
    ranges: Arc<Mutex<Option<Connection>>>,
}

/// Whether a file with this much free space is worth compacting. Readings
/// are never deleted, so space only comes free when a cleanup clears a lot
/// at once, such as an older server's indexes and repeated blocks.
fn worth_compacting(total_pages: i64, free_pages: i64) -> bool {
    total_pages > 0 && free_pages * 5 >= total_pages
}

/// The stretches for the wire, a kind that is not measured having none.
fn stretches_of(marks: daily::Marks) -> solax_api::Stretches {
    solax_api::Stretches {
        at_limit: marks.at_limit.unwrap_or_default(),
        derating: marks.derating.unwrap_or_default(),
    }
}

/// SQLite reports a database this process may not write as a terse
/// "readonly database"; this says what that means here and how to fix it.
fn explain_read_only(error: anyhow::Error, path: &Path) -> anyhow::Error {
    let read_only = error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<rusqlite::Error>(),
            Some(rusqlite::Error::SqliteFailure(failure, _)) if failure.code == rusqlite::ErrorCode::ReadOnly
        )
    });
    if !read_only {
        return error;
    }
    error.context(format!(
        "{} cannot be written by the user this server runs as. It stores every reading \
         there and adds a column for each new metric, so make that user the owner of the \
         folder and its files, for example with chown -R 1000:1000 on the folder",
        path.display()
    ))
}

/// The generated column holding a metric's raw value.
fn column_name(metric: &Metric) -> String {
    format!("m_{}", metric.key)
}

/// SQL reading a metric's number out of its slots in the stored Data array.
/// It yields the raw value, not the scaled one: MAX over raw equals MAX over
/// scaled for a positive scale, so peak queries stay correct and scaling
/// stays in Rust.
///
/// The expression becomes part of the schema, so it stays plain SQLite that
/// any tool opening the database can evaluate. Any change to its text, even
/// whitespace, rebuilds every generated column on the next start.
fn slots_sql(slots: &DataSlots) -> String {
    match *slots {
        DataSlots::One {
            index,
            signed: false,
        } => slot_sql(index),
        DataSlots::One {
            index,
            signed: true,
        } => signed_slot_sql(&slot_sql(index)),
        DataSlots::Two {
            low,
            high,
            signed: false,
        } => format!("({} + 65536 * {})", slot_sql(low), slot_sql(high)),
        DataSlots::Two {
            low,
            high,
            signed: true,
        } => {
            let (low, high) = (slot_sql(low), slot_sql(high));
            format!(
                "(CASE WHEN {high} < 32768 THEN {low} + 65536 * {high} \
                  ELSE {low} + 65536 * {high} - 4294967296 END)"
            )
        }
    }
}

/// One slot of the stored Data array.
fn slot_sql(index: usize) -> String {
    format!("json_extract(raw_data,'$[{index}]')")
}

/// A one slot number read as signed, where the top half of the range stands
/// for negatives.
fn signed_slot_sql(slot: &str) -> String {
    format!("(CASE WHEN {slot} < 32768 THEN {slot} ELSE {slot} - 65536 END)")
}

impl Store {
    pub fn open(path: &Path, registry: Arc<Registry>, timezone: Tz) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!(
                    "creating the database directory {}. A relative storage.database_path \
                     resolves against the working directory, which in a container is /app \
                     and is not writable; the mounted volume is the intended location",
                    parent.display()
                )
            })?;
        }

        let connection = Connection::open(path)
            .with_context(|| format!("opening {}", path.display()))?;

        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;
             CREATE TABLE IF NOT EXISTS samples (
                ts              INTEGER PRIMARY KEY,
                sn              TEXT,
                ver             TEXT,
                type            INTEGER,
                raw_data        TEXT NOT NULL,
                raw_information TEXT,
                raw_error       TEXT,
                source          TEXT NOT NULL DEFAULT 'poll'
             );",
        )?;
        connection.execute_batch(daily::CREATE_TABLE)?;
        daily::add_missing_columns(&connection).map_err(|error| explain_read_only(error.into(), path))?;

        let store = Self {
            connection: Arc::new(Mutex::new(connection)),
            registry,
            timezone,
            path: Arc::new(path.to_path_buf()),
            output_limit: None,
            derated_output: None,
            last_information: Arc::new(Mutex::new(None)),
            ranges: Arc::new(Mutex::new(None)),
        };
        store
            .reconcile_columns()
            .map_err(|error| explain_read_only(error, path))?;
        store.settle_information()?;
        store.tidy();
        // Opened last: compacting needs the file to itself.
        let ranges = Connection::open(path).with_context(|| format!("opening {} again", path.display()))?;
        ranges.execute_batch("PRAGMA synchronous = NORMAL; PRAGMA busy_timeout = 5000;")?;
        *store.ranges.lock().unwrap() = Some(ranges);
        Ok(store)
    }

    /// SQLite keeps the space a cleanup frees inside the file, and the
    /// write-ahead log at the largest size the work needed. At start the file
    /// is compacted when a fifth of it is free, and the log is emptied, so
    /// the size on disk reflects the readings. Both are only savings, so a
    /// database that cannot be written is still served.
    fn tidy(&self) {
        let connection = self.connection.lock().unwrap();
        let pages = |pragma: &str| connection.query_row(&format!("PRAGMA {pragma}"), [], |row| row.get::<_, i64>(0));
        if let (Ok(total), Ok(free)) = (pages("page_count"), pages("freelist_count")) {
            if worth_compacting(total, free) {
                match connection.execute_batch("VACUUM;") {
                    Ok(()) => tracing::info!(freed_pages = free, "compacted the database"),
                    Err(error) => tracing::warn!(%error, "could not compact the database"),
                }
            }
        }
        if let Err(error) = connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);") {
            tracing::warn!(%error, "could not empty the write-ahead log");
        }
    }

    /// Clears the information blocks an older server stored on every
    /// reading, keeping each one that differs from the block stored before
    /// it, then remembers the last one. Clearing is only a saving, so a
    /// database that cannot be written keeps its repeats and is still served.
    fn settle_information(&self) -> Result<()> {
        let connection = self.connection.lock().unwrap();
        // Each stored block against the stored block before it, skipping
        // the readings that store none.
        const REPEATS: &str = "SELECT ts FROM (
                SELECT ts, raw_information AS block, LAG(raw_information) OVER (ORDER BY ts) AS before
                FROM samples WHERE raw_information IS NOT NULL
             ) WHERE block = before";
        let repeats: i64 = connection.query_row(&format!("SELECT COUNT(*) FROM ({REPEATS})"), [], |row| row.get(0))?;
        if repeats > 0 {
            match connection.execute(&format!("UPDATE samples SET raw_information = NULL WHERE ts IN ({REPEATS})"), []) {
                Ok(cleared) => tracing::info!(cleared, "cleared information blocks repeated on every reading"),
                Err(error) => tracing::warn!(%error, "could not clear the repeated information blocks"),
            }
        }
        let last: Option<String> = connection
            .query_row(
                "SELECT raw_information FROM samples WHERE raw_information IS NOT NULL ORDER BY ts DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        *self.last_information.lock().unwrap() = last;
        Ok(())
    }

    /// The most the inverter delivers, in watts, so day summaries can count
    /// the time spent at that limit.
    pub fn with_output_limit(mut self, watts: f64) -> Self {
        self.output_limit = Some(watts);
        self
    }

    /// The output the inverter steps down to when it runs too hot, in
    /// watts, so day summaries can count those steps.
    pub fn with_derated_output(mut self, watts: f64) -> Self {
        self.derated_output = Some(watts);
        self
    }

    /// Bytes the database takes on disk: the file itself and the
    /// write-ahead log and index SQLite keeps beside it.
    pub fn storage_bytes(&self) -> u64 {
        let base = self.path.as_os_str().to_owned();
        ["", "-wal", "-shm"]
            .iter()
            .map(|suffix| {
                let mut name = base.clone();
                name.push(suffix);
                std::fs::metadata(&name).map(|metadata| metadata.len()).unwrap_or(0)
            })
            .sum()
    }

    /// Bring the generated columns in line with the registry. Adding a column
    /// is instant and applies to rows written before it existed; a changed
    /// expression is repaired by dropping and re-adding, so the TOML stays
    /// the single source of truth.
    fn reconcile_columns(&self) -> Result<()> {
        let connection = self.connection.lock().unwrap();

        let table_sql: String = connection.query_row(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'samples'",
            [],
            |row| row.get(0),
        )?;

        let mut existing = HashSet::new();
        {
            // table_xinfo, not table_info: the plain form hides generated
            // columns, so every restart would try to add them again.
            let mut statement = connection.prepare("PRAGMA table_xinfo(samples)")?;
            let rows = statement.query_map([], |row| row.get::<_, String>(1))?;
            for name in rows {
                existing.insert(name?);
            }
        }

        // Plain columns added after a database was first created. Adding one
        // is cheap and applies to a table already holding rows, but it has to
        // be asked for: CREATE TABLE IF NOT EXISTS does nothing to a table
        // that already exists.
        for (column, definition) in [("raw_error", "raw_error TEXT")] {
            if !existing.contains(column) {
                connection
                    .execute_batch(&format!("ALTER TABLE samples ADD COLUMN {definition};"))
                    .with_context(|| format!("adding the {column} column"))?;
                existing.insert(column.to_string());
                tracing::info!(column, "added a missing column");
            }
        }

        for metric in self.registry.metrics() {
            let column = column_name(metric);
            let expression = slots_sql(&metric.slots);
            let definition =
                format!("{column} INTEGER GENERATED ALWAYS AS ({expression}) VIRTUAL");

            if existing.contains(&column) {
                if table_sql.contains(&definition) {
                    continue;
                }
                tracing::warn!(
                    metric = %metric.key,
                    "registry definition changed, rebuilding the generated column"
                );
                connection.execute_batch(&format!(
                    "DROP INDEX IF EXISTS idx_{column};
                     ALTER TABLE samples DROP COLUMN {column};"
                ))?;
            }

            connection
                .execute_batch(&format!("ALTER TABLE samples ADD COLUMN {definition};"))
                .with_context(|| format!("adding the generated column for {}", metric.key))?;
            tracing::info!(metric = %metric.key, slots = ?metric.slots, "generated column ready");
        }

        // A metric deleted from the registry leaves its column behind, which
        // would drift the schema away from the TOML. Dropping it loses no
        // data: the values live in raw_data and come back if the block does.
        let wanted: HashSet<String> = self.registry.metrics().iter().map(column_name).collect();
        for column in existing.iter().filter(|name| name.starts_with("m_")) {
            if wanted.contains(column) {
                continue;
            }
            tracing::info!(%column, "metric left the registry, dropping its column");
            connection.execute_batch(&format!(
                "DROP INDEX IF EXISTS idx_{column};
                 ALTER TABLE samples DROP COLUMN {column};"
            ))?;
        }

        // Every chart, day figure and peak reads a stretch of time, which the
        // table's own order already serves. Only finding when the hero peaked
        // searches by value, so its column is the one index worth its upkeep
        // on every reading; it also carries the count of stored readings.
        let hero = column_name(self.registry.hero());
        connection.execute_batch(&format!("CREATE INDEX IF NOT EXISTS idx_{hero} ON samples({hero});"))?;
        // Indexes an older server made on other columns cost a quarter of
        // the file and are never read. Dropping them is only a saving, so a
        // database that cannot be written keeps them and is still served.
        let unneeded: Vec<String> = connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND name GLOB 'idx_m_*'")?
            .query_map([], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<String>>>()?
            .into_iter()
            .filter(|name| *name != format!("idx_{hero}"))
            .collect();
        for name in unneeded {
            if let Err(error) = connection.execute_batch(&format!("DROP INDEX IF EXISTS \"{name}\";")) {
                tracing::warn!(%error, index = %name, "could not drop an unused index");
                break;
            }
            tracing::info!(index = %name, "dropped an index nothing reads");
        }

        Ok(())
    }

    /// Fixed columns first, then one per metric. The two halves are read back
    /// by the same offsets in `row_to_snapshot`, so they change together.
    const FIXED_COLUMNS: [&'static str; 4] = ["ts", "ver", "type", "raw_error"];

    fn select_columns(&self) -> String {
        let mut parts: Vec<String> = Self::FIXED_COLUMNS.iter().map(|c| c.to_string()).collect();
        parts.extend(self.registry.metrics().iter().map(column_name));
        parts.join(", ")
    }

    fn row_to_snapshot(&self, row: &rusqlite::Row<'_>) -> rusqlite::Result<Snapshot> {
        let offset = Self::FIXED_COLUMNS.len();
        let mut raw = BTreeMap::new();
        for (position, metric) in self.registry.metrics().iter().enumerate() {
            if let Some(value) = row.get::<_, Option<i64>>(position + offset)? {
                raw.insert(metric.key.clone(), value);
            }
        }
        Ok(Snapshot {
            ts: row.get(0)?,
            ver: row.get(1)?,
            inverter_type: row.get(2)?,
            error: row.get(3)?,
            raw,
        })
    }

    pub async fn insert_sample(&self, sample: Sample) -> Result<bool> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let connection = store.connection.lock().unwrap();
            let mut last_information = store.last_information.lock().unwrap();
            let information = serde_json::to_string(&sample.information)?;
            // The block is the same on almost every reading; a reading
            // without one has the block stored last before it.
            let repeated = last_information.as_deref() == Some(information.as_str());
            let changed = connection.execute(
                "INSERT INTO samples
                    (ts, sn, ver, type, raw_data, raw_information, raw_error, source)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'poll')
                 ON CONFLICT(ts) DO NOTHING",
                params![
                    sample.ts,
                    sample.sn,
                    sample.ver,
                    sample.inverter_type,
                    serde_json::to_string(&sample.data)?,
                    (!repeated).then_some(&information),
                    // Only present on a faulted reply, and the one thing you
                    // want after the fact when working out what happened.
                    sample
                        .error
                        .as_ref()
                        .map(serde_json::to_string)
                        .transpose()?,
                ],
            )?;
            if changed > 0 && !repeated {
                *last_information = Some(information);
            }
            // A finished day's stored figures came from the readings it had
            // then, so one written into it later, such as a merged import,
            // means working them out again.
            let date = chrono::DateTime::from_timestamp(sample.ts, 0)
                .map(|moment| moment.with_timezone(&store.timezone).date_naive());
            if let Some(date) = date.filter(|date| changed > 0 && *date < crate::clock::today(store.timezone)) {
                connection.execute("DELETE FROM daily WHERE date = ?1", [date.to_string()])?;
            }
            Ok(changed > 0)
        })
        .await?
    }

    pub async fn latest(&self) -> Result<Option<Snapshot>> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let connection = store.connection.lock().unwrap();
            let sql = format!(
                "SELECT {} FROM samples ORDER BY ts DESC LIMIT 1",
                store.select_columns()
            );
            let mut statement = connection.prepare(&sql)?;
            let mut rows = statement.query_map([], |row| store.row_to_snapshot(row))?;
            match rows.next() {
                Some(row) => Ok(Some(row?)),
                None => Ok(None),
            }
        })
        .await?
    }

    pub async fn history(&self, from: i64, to: i64) -> Result<Vec<Snapshot>> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let connection = store.connection.lock().unwrap();
            let sql = format!(
                "SELECT {} FROM samples WHERE ts >= ?1 AND ts <= ?2 ORDER BY ts",
                store.select_columns()
            );
            let mut statement = connection.prepare(&sql)?;
            let rows = statement.query_map(params![from, to], |row| store.row_to_snapshot(row))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
        .await?
    }

    /// One local day's raw figures, worked out from its readings now, with
    /// the stretches to mark on its power chart from the same pass.
    pub async fn day_raw(&self, date: NaiveDate) -> Result<(DayRaw, solax_api::Stretches)> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let connection = store.connection.lock().unwrap();
            let (from, to) = crate::clock::day_bounds(store.timezone, date);
            let (raw, marks) = daily::summarise(&connection, &store.role_columns(), from, to)?;
            Ok((raw, stretches_of(marks)))
        })
        .await?
    }

    /// Raw figures for each date, in the order given. A completed day, one
    /// before `today`, comes from the daily table while its fingerprint still
    /// matches, and is summarised and stored otherwise. Today and later
    /// dates are always worked out live and never stored.
    pub async fn day_raws(
        &self,
        dates: Vec<NaiveDate>,
        today: NaiveDate,
    ) -> Result<Vec<(NaiveDate, DayRaw)>> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let ranges = store.ranges.lock().unwrap();
            let connection = ranges.as_ref().expect("a store opens its range connection before it is used");
            let columns = store.role_columns();
            let fingerprint = store.daily_fingerprint();
            let mut summaries = Vec::with_capacity(dates.len());
            let mut storing_failed = false;
            for date in dates {
                let key = date.to_string();
                let completed = date < today;
                if completed {
                    if let Some(raw) = daily::cached(&connection, &key, &fingerprint)? {
                        summaries.push((date, raw));
                        continue;
                    }
                }
                let (from, to) = crate::clock::day_bounds(store.timezone, date);
                let (raw, _) = daily::summarise(&connection, &columns, from, to)?;
                // Storing is only a cache. A database this process cannot
                // write, such as a copy owned by another user, still answers;
                // the day is just worked out again next time.
                if completed && !storing_failed {
                    if let Err(error) = daily::store(&connection, &key, &fingerprint, &raw) {
                        tracing::warn!(%error, "could not store day summaries; working them out each time");
                        storing_failed = true;
                    }
                }
                summaries.push((date, raw));
            }
            Ok(summaries)
        })
        .await?
    }

    /// The stretches of a window to mark on its power chart: at the limit,
    /// when one is known, and derated for heat, when its metrics are named.
    pub async fn stretches(&self, from: i64, to: i64) -> Result<solax_api::Stretches> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let connection = store.connection.lock().unwrap();
            Ok(stretches_of(daily::marks(&connection, &store.role_columns(), from, to)?))
        })
        .await?
    }

    /// The hero metric's raw readings in a window, oldest first.
    pub async fn hero_series(&self, from: i64, to: i64) -> Result<Vec<(i64, Option<i64>)>> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let connection = store.connection.lock().unwrap();
            let hero = column_name(store.registry.hero());
            Ok(daily::hero_series(&connection, &hero, from, to)?)
        })
        .await?
    }

    fn role_columns(&self) -> daily::RoleColumns {
        daily::RoleColumns {
            hero: column_name(self.registry.hero()),
            daily_yield: column_name(self.registry.daily_yield()),
            lifetime_yield: column_name(self.registry.lifetime_yield()),
            limit_column: column_name(self.registry.limit_metric()),
            limit_raw: self.limit_raw(),
            derating: self.derating_columns(),
        }
    }

    /// Heat derating is counted only when the registry names its metrics and
    /// the level it steps down to is known.
    fn derating_columns(&self) -> Option<daily::DeratingColumns> {
        let metrics = self.registry.derating()?;
        let column = |metric: &Metric| (column_name(metric), metric.scale);
        Some(daily::DeratingColumns {
            output: column(metrics.output),
            panel_voltage: column(metrics.panel_voltage),
            temperature: column(metrics.temperature),
            level_watts: self.derated_output?,
        })
    }

    /// The raw reading of the limit metric from which the inverter counts as
    /// at its limit: within 3% of it, the same rule as the hero card's
    /// caption. Only a metric measured in watts can be compared with it.
    fn limit_raw(&self) -> Option<i64> {
        let metric = self.registry.limit_metric();
        let watts = self.output_limit?;
        if metric.unit != "W" || metric.scale <= 0.0 {
            return None;
        }
        Some((0.97 * watts / metric.scale).ceil() as i64)
    }

    /// Everything a stored day row depends on besides the readings: the
    /// timezone that decides which hours a date covers, the key and data
    /// slots of each role metric, the metric measured against the inverter's
    /// limit with the reading that counts as at it and the longest step that
    /// still counts as time there, and what heat derating is read from and
    /// by which version of its rule.
    fn daily_fingerprint(&self) -> String {
        let role = |metric: &Metric| serde_json::json!({ "key": metric.key, "slots": metric.slots });
        // Derating reads scaled values, so unlike the raw figures above it
        // depends on each metric's scale too.
        let scaled = |metric: &Metric| serde_json::json!({ "key": metric.key, "slots": metric.slots, "scale": metric.scale });
        let derating = self.derating_columns().map(|columns| {
            let metrics = self.registry.derating().expect("derating columns come from its metrics");
            serde_json::json!({
                "output": scaled(metrics.output),
                "panel_voltage": scaled(metrics.panel_voltage),
                "temperature": scaled(metrics.temperature),
                "level_watts": columns.level_watts,
                "rule": derating::RULE_VERSION,
            })
        });
        serde_json::json!({
            "summary": daily::SUMMARY_VERSION,
            "timezone": self.timezone.name(),
            "hero": role(self.registry.hero()),
            "daily_yield": role(self.registry.daily_yield()),
            "lifetime_yield": role(self.registry.lifetime_yield()),
            "limit_metric": role(self.registry.limit_metric()),
            "limit_raw": self.limit_raw(),
            "limit_step": daily::LONGEST_STEP_AT_LIMIT,
            "derating": derating,
        })
        .to_string()
    }

    /// Dates held in the daily table, oldest first.
    #[cfg(test)]
    pub fn stored_days(&self) -> Vec<String> {
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare("SELECT date FROM daily ORDER BY date").unwrap();
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap();
        rows.map(|row| row.unwrap()).collect()
    }

    /// Inserts many samples in one transaction, for tests that need a month
    /// of readings without a month of commits.
    #[cfg(test)]
    pub fn insert_samples_in_one_transaction(&self, samples: &[Sample]) {
        let mut connection = self.connection.lock().unwrap();
        let transaction = connection.transaction().unwrap();
        {
            let mut statement = transaction
                .prepare(
                    "INSERT INTO samples (ts, sn, ver, type, raw_data, raw_information, raw_error, source)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'poll') ON CONFLICT(ts) DO NOTHING",
                )
                .unwrap();
            for sample in samples {
                statement
                    .execute(params![
                        sample.ts,
                        sample.sn,
                        sample.ver,
                        sample.inverter_type,
                        serde_json::to_string(&sample.data).unwrap(),
                        serde_json::to_string(&sample.information).unwrap(),
                        sample.error.as_ref().map(|error| error.to_string()),
                    ])
                    .unwrap();
            }
        }
        transaction.commit().unwrap();
    }

    /// Peak raw value per metric marked `peak` over a window, plus the last
    /// value of every metric in that window.
    pub async fn summary(&self, from: i64, to: i64) -> Result<Summary> {
        let store = self.clone();
        tokio::task::spawn_blocking(move || {
            let connection = store.connection.lock().unwrap();

            let peaks_metrics: Vec<&Metric> = store.registry.peaked().collect();
            let mut peaks = BTreeMap::new();
            if !peaks_metrics.is_empty() {
                // Gated metrics count only rows where the plant was producing.
                // Kept inside one statement so the day is still scanned once.
                let producing = column_name(store.registry.hero());
                let selected = peaks_metrics
                    .iter()
                    .map(|metric| {
                        let column = column_name(metric);
                        match metric.peak {
                            Some(Peak::WhileProducing) => {
                                format!("MAX(CASE WHEN {producing} > 0 THEN {column} END)")
                            }
                            _ => format!("MAX({column})"),
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let sql =
                    format!("SELECT {selected} FROM samples WHERE ts >= ?1 AND ts <= ?2");
                let mut statement = connection.prepare(&sql)?;
                let values = statement.query_row(params![from, to], |row| {
                    let mut collected = Vec::new();
                    for offset in 0..peaks_metrics.len() {
                        collected.push(row.get::<_, Option<i64>>(offset)?);
                    }
                    Ok(collected)
                })?;
                for (metric, value) in peaks_metrics.iter().zip(values) {
                    if let Some(value) = value {
                        peaks.insert(metric.key.clone(), value);
                    }
                }
            }

            // How much is banked overall, and how far back it reaches.
            //
            // Two statements on purpose. SQLite turns a lone aggregate into an
            // index seek, but asking for two in one statement makes it scan
            // the table instead, because a single scan can only serve one
            // index. Measured at 300k rows: combined 10.8ms and growing with
            // the table, split 0.5ms and 0.00ms with MIN flat forever.
            //
            // These are the only whole-table queries here. Everything else is
            // bounded to a day and does not grow.
            let stored: i64 =
                connection.query_row("SELECT COUNT(*) FROM samples", [], |row| row.get(0))?;
            let earliest: Option<i64> =
                connection.query_row("SELECT MIN(ts) FROM samples", [], |row| row.get(0))?;

            Ok(Summary {
                peaks,
                stored,
                earliest,
            })
        })
        .await?
    }

}

#[derive(Debug, Clone)]
pub struct Summary {
    pub peaks: BTreeMap<String, i64>,
    pub stored: i64,
    pub earliest: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index_names(path: &Path) -> Vec<String> {
        let connection = Connection::open(path).unwrap();
        let mut statement = connection
            .prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND name GLOB 'idx_*' ORDER BY name")
            .unwrap();
        statement.query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    }

    fn informations(store: &Store) -> Vec<Option<String>> {
        let connection = store.connection.lock().unwrap();
        let mut statement = connection.prepare("SELECT raw_information FROM samples ORDER BY ts").unwrap();
        statement.query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap()
    }

    /// Five readings in a row carrying the given information blocks.
    fn with_information(blocks: &[&str]) -> Vec<Sample> {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 20).unwrap();
        crate::test_support::SyntheticDay::clear(date).samples()[80..80 + blocks.len()]
            .iter()
            .zip(blocks)
            .map(|(sample, block)| Sample { information: serde_json::json!([block]), ..sample.clone() })
            .collect()
    }

    #[test]
    fn a_daily_table_from_before_the_later_columns_gets_them_at_start() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("solax.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch(
                "CREATE TABLE daily (date TEXT PRIMARY KEY, fingerprint TEXT NOT NULL,
                    samples INTEGER NOT NULL, yield_raw INTEGER, peak_raw INTEGER, peak_ts INTEGER,
                    producing_from INTEGER, producing_until INTEGER, lifetime_raw INTEGER,
                    fault_readings INTEGER NOT NULL);
                 INSERT INTO daily VALUES ('2026-09-20', 'older', 169, 1, 1, 1, 1, 1, 1, 0);",
            )
            .unwrap();

        let _store = Store::open(&path, crate::test_support::registry(), crate::test_support::TIMEZONE).unwrap();

        let connection = Connection::open(&path).unwrap();
        let mut statement = connection.prepare("SELECT name FROM pragma_table_info('daily')").unwrap();
        let columns: Vec<String> = statement.query_map([], |row| row.get(0)).unwrap().collect::<rusqlite::Result<_>>().unwrap();
        for later in ["limit_seconds", "derate_count", "derate_seconds"] {
            assert!(columns.iter().any(|column| column == later), "{later} missing from {columns:?}");
        }
    }

    #[test]
    fn a_file_is_compacted_once_a_fifth_of_it_is_free() {
        assert!(!worth_compacting(1000, 0), "a file growing as usual has nothing free");
        assert!(!worth_compacting(1000, 199));
        assert!(worth_compacting(1000, 200));
        assert!(worth_compacting(8500, 3000), "an older server's indexes and repeats cleared");
        assert!(!worth_compacting(0, 0));
    }

    #[test]
    fn only_the_hero_is_indexed_and_indexes_an_older_server_made_are_dropped() {
        let (folder, store) = crate::test_support::store();
        drop(store);
        let path = folder.path().join("solax.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE INDEX IF NOT EXISTS idx_m_pv_volts ON samples(m_pv_volts);")
            .unwrap();

        let _store = Store::open(&path, crate::test_support::registry(), crate::test_support::TIMEZONE).unwrap();

        assert_eq!(index_names(&path), vec!["idx_m_pv_power"]);
    }

    #[tokio::test]
    async fn a_reading_stores_the_information_block_only_when_it_changes() {
        let (_folder, store) = crate::test_support::store();
        for sample in with_information(&["A", "A", "B", "B", "A"]) {
            store.insert_sample(sample).await.unwrap();
        }
        let block = |text: &str| Some(format!("[\"{text}\"]"));
        assert_eq!(informations(&store), vec![block("A"), None, block("B"), None, block("A")]);
    }

    #[test]
    fn the_blocks_an_older_server_repeated_are_cleared_at_start_keeping_each_change() {
        let (folder, store) = crate::test_support::store();
        store.insert_samples_in_one_transaction(&with_information(&["A", "A", "B", "B"]));
        drop(store);

        let store = Store::open(&folder.path().join("solax.db"), crate::test_support::registry(), crate::test_support::TIMEZONE)
            .unwrap();

        let block = |text: &str| Some(format!("[\"{text}\"]"));
        assert_eq!(informations(&store), vec![block("A"), None, block("B"), None]);
    }

    #[test]
    fn a_database_it_cannot_write_is_refused_with_the_fix_named() {
        let (folder, store) = crate::test_support::store();
        drop(store);
        if !crate::test_support::make_read_only(folder.path()) {
            return;
        }
        // A registry with one metric more, which needs a new column.
        let grown = format!("{}\n[[inverter]]\nkey = \"extra\"\nlabel = \"Extra\"\nindex = 27\n", crate::test_support::REGISTRY);
        let registry = Arc::new(crate::metrics::parse(&grown).unwrap());

        let Err(error) = Store::open(&folder.path().join("solax.db"), registry, crate::test_support::TIMEZONE) else {
            panic!("opened a database it cannot write");
        };

        let message = format!("{error:#}");
        assert!(message.contains("cannot be written"), "{message}");
        assert!(message.contains("owner"), "{message}");
    }

    #[test]
    fn slots_sql_matches_the_stored_schema() {
        assert_eq!(
            slots_sql(&DataSlots::One { index: 4, signed: false }),
            "json_extract(raw_data,'$[4]')"
        );
        assert_eq!(
            slots_sql(&DataSlots::One { index: 1, signed: true }),
            "(CASE WHEN json_extract(raw_data,'$[1]') < 32768 THEN json_extract(raw_data,'$[1]') \
             ELSE json_extract(raw_data,'$[1]') - 65536 END)"
        );
        assert_eq!(
            slots_sql(&DataSlots::Two { low: 19, high: 20, signed: false }),
            "(json_extract(raw_data,'$[19]') + 65536 * json_extract(raw_data,'$[20]'))"
        );
        assert_eq!(
            slots_sql(&DataSlots::Two { low: 19, high: 20, signed: true }),
            "(CASE WHEN json_extract(raw_data,'$[20]') < 32768 \
             THEN json_extract(raw_data,'$[19]') + 65536 * json_extract(raw_data,'$[20]') \
             ELSE json_extract(raw_data,'$[19]') + 65536 * json_extract(raw_data,'$[20]') - 4294967296 END)"
        );
    }
}
