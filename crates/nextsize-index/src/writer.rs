use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::Instant;

use nexttool_core::fs::{flags, EntryKind};
use nexttool_core::time::now_unix;
use nextsize_core::ext::normalize_ext;
use nextsize_core::{
    NodeId, ScanOptions, ScanOutcome, ScanProgress, ScanSink, SinkError, SinkMsg, ROOT_ID, SCHEMA_VERSION,
};
use rusqlite::{params, Connection};

use crate::schema::{self, set_meta};
use crate::{sqlite_to_sink, IndexError};

/// Max distinct extensions kept in the dictionary (bounded RAM). Beyond
/// this, new extensions are stored as 0 ("sem extensão/outros").
const EXT_DICT_CAP: usize = 200_000;

#[derive(Default, Clone, Copy)]
struct DirAcc {
    files: i64,
    dirs: i64,
    size: i64,
    alloc: Option<i64>,
}

/// Persists scan output in batched transactions. Each COMMIT is a durable
/// checkpoint: after a crash, everything committed is kept and incomplete
/// directories are re-read (see `resume`).
pub struct IndexWriter {
    conn: Connection,
    path: PathBuf,
    commit_rows: usize,
    commit_interval_ms: u64,
    rows_since: usize,
    last_commit: Instant,
    in_tx: bool,
    exts: HashMap<String, i64>,
    next_ext: i64,
    /// Accumulators only for directories with chunks in flight (≈ threads).
    acc: HashMap<NodeId, DirAcc>,
    progress: Arc<ScanProgress>,
    pub(crate) timings: Vec<(String, u64)>,
    /// Counters of rows written in this snapshot, persisted in `meta` with
    /// every commit so a partial snapshot shows real (labeled) numbers.
    c_files: u64,
    c_bytes: u64,
    c_dirs: u64,
    c_issues: u64,
    /// Time spent inserting rows (excluding commits) and committing.
    t_write: std::time::Duration,
    t_commit: std::time::Duration,
    /// Aggregates maintained during ingestion (bounded: ≤ EXT_DICT_CAP + 1
    /// extension buckets, one bucket per year). Valid only for a snapshot
    /// written in one uninterrupted session; after a resume they are
    /// recomputed with SQL at finalize.
    agg_ext: HashMap<i64, (i64, i64)>,
    agg_year: HashMap<Option<i64>, (i64, i64)>,
    aggs_valid: bool,
    /// Page cache during finalization. SQLite's sorter may use up to this
    /// amount *per sorter thread*, so threads × cache bounds finalize memory.
    finalize_cache_kib: u64,
    finalize_threads: u64,
    /// Reused buffer for batched file inserts.
    file_batch: Vec<(nextsize_core::EntryRecord, i64)>,
}

/// Rows per multi-row INSERT (13 params each → 832 bound parameters).
const FILE_BATCH: usize = 64;

fn i64s(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// UTC year of a Unix timestamp (same result as SQLite strftime('%Y', t, 'unixepoch')).
pub(crate) fn year_of(unix: i64) -> i64 {
    let days = unix.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + if m <= 2 { 1 } else { 0 }
}

/// Extension slice of a name if it is already normalized (lowercase ASCII),
/// so the common case needs no allocation.
fn ext_slice(name: &str) -> Option<&str> {
    let dot = name.rfind('.')?;
    if dot == 0 || dot + 1 == name.len() {
        return None;
    }
    let e = &name[dot + 1..];
    (e.len() <= nextsize_core::ext::MAX_EXT_LEN && e.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')).then_some(e)
}

impl IndexWriter {
    /// Creates a new snapshot file. Fails if it already exists.
    pub fn create(path: &Path, opts: &ScanOptions, enumerator: &str, progress: Arc<ScanProgress>) -> Result<Self, IndexError> {
        if path.exists() {
            return Err(IndexError::AlreadyExists(path.display().to_string()));
        }
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "page_size", 8192)?;
        schema::apply_pragmas(&conn, opts.tuning.sqlite_cache_kib)?;
        conn.execute_batch(schema::CREATE)?;
        let root = opts.root.to_string_lossy().into_owned();
        let now = now_unix().to_string();
        let remote = nexttool_core::fs::is_remote_path(&opts.root);
        for (k, v) in [
            ("app", "nexttool-nextsize".to_string()),
            ("schema_version", SCHEMA_VERSION.to_string()),
            ("root", root.clone()),
            ("source_kind", if remote { "remote" } else { "local" }.to_string()),
            ("platform", std::env::consts::OS.to_string()),
            ("enumerator", enumerator.to_string()),
            ("profile", serde_json::to_string(&opts.profile).unwrap_or_default()),
            ("options", serde_json::to_string(opts).unwrap_or_default()),
            ("started_at", now),
            ("state", "scanning".to_string()),
            ("resumed_count", "0".to_string()),
            ("hardlink_policy", "per_path".to_string()),
        ] {
            set_meta(&conn, k, &v)?;
        }
        conn.execute(
            "INSERT INTO dirs(id,parent,depth,name,state,flags) VALUES(?1,0,0,?2,0,0)",
            params![ROOT_ID as i64, root],
        )?;
        let mut w = Self::from_conn(conn, path, opts, progress);
        w.aggs_valid = true; // fresh snapshot: in-memory aggregates see every row
        Ok(w)
    }

    pub(crate) fn from_conn(conn: Connection, path: &Path, opts: &ScanOptions, progress: Arc<ScanProgress>) -> Self {
        let mut exts = HashMap::new();
        let mut next_ext = 1;
        if let Ok(mut st) = conn.prepare("SELECT id, ext FROM exts") {
            if let Ok(rows) = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))) {
                for (id, e) in rows.flatten() {
                    next_ext = next_ext.max(id + 1);
                    exts.insert(e, id);
                }
            }
        }
        IndexWriter {
            conn,
            path: path.to_path_buf(),
            commit_rows: opts.tuning.commit_rows.max(1),
            commit_interval_ms: opts.tuning.commit_interval_ms,
            rows_since: 0,
            last_commit: Instant::now(),
            in_tx: false,
            exts,
            next_ext,
            acc: HashMap::new(),
            c_files: progress.files.load(Relaxed),
            c_bytes: progress.bytes.load(Relaxed),
            c_dirs: progress.dirs.load(Relaxed),
            c_issues: progress.issues.load(Relaxed),
            progress,
            timings: Vec::new(),
            t_write: Default::default(),
            t_commit: Default::default(),
            agg_ext: HashMap::new(),
            agg_year: HashMap::new(),
            aggs_valid: false,
            // Measured (BENCHMARKS §8): parallel sorting with a modest cache
            // beats a big cache; keep threads × cache within the profile's
            // SQLite budget (Economy 1×8 MB, Balanced 2×16 MB, Fast 4×16 MB).
            finalize_threads: (opts.tuning.sqlite_cache_kib / (16 * 1024)).clamp(1, 4),
            finalize_cache_kib: (opts.tuning.sqlite_cache_kib / (opts.tuning.sqlite_cache_kib / (16 * 1024)).clamp(1, 4)).clamp(8 * 1024, 16 * 1024),
            file_batch: Vec::with_capacity(FILE_BATCH),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn begin(&mut self) -> rusqlite::Result<()> {
        if !self.in_tx {
            self.conn.execute_batch("BEGIN")?;
            self.in_tx = true;
        }
        Ok(())
    }

    fn commit(&mut self) -> rusqlite::Result<()> {
        let t = Instant::now();
        let r = self.commit_inner();
        self.t_commit += t.elapsed();
        r
    }

    fn commit_inner(&mut self) -> rusqlite::Result<()> {
        if self.in_tx {
            // Same transaction as the rows: counters always match committed data.
            for (k, v) in [("files", self.c_files), ("bytes", self.c_bytes), ("dirs", self.c_dirs), ("issues", self.c_issues)] {
                set_meta(&self.conn, k, &v.to_string())?;
            }
            self.conn.execute_batch("COMMIT")?;
            self.in_tx = false;
            self.progress.rows_committed.fetch_add(self.rows_since as u64, Relaxed);
            self.progress.commits.fetch_add(1, Relaxed);
        }
        self.rows_since = 0;
        self.last_commit = Instant::now();
        Ok(())
    }

    fn ext_id(&mut self, name: &str) -> rusqlite::Result<i64> {
        if let Some(e) = ext_slice(name) {
            if let Some(&id) = self.exts.get(e) {
                return Ok(id);
            }
        }
        let Some(ext) = normalize_ext(name) else { return Ok(0) };
        if let Some(&id) = self.exts.get(&ext) {
            return Ok(id);
        }
        if self.exts.len() >= EXT_DICT_CAP {
            return Ok(0);
        }
        let id = self.next_ext;
        self.next_ext += 1;
        self.conn.prepare_cached("INSERT INTO exts(id,ext) VALUES(?1,?2)")?.execute(params![id, ext])?;
        self.exts.insert(ext, id);
        Ok(id)
    }

    fn write_msg(&mut self, msg: SinkMsg) -> rusqlite::Result<()> {
        self.begin()?;
        match msg {
            SinkMsg::Issue(i) => {
                self.conn
                    .prepare_cached(
                        "INSERT INTO issues(dir_id,path,kind,os_code,stage,severity,recoverable,at,message) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                    )?
                    .execute(params![
                        i.dir_id.map(|d| d as i64),
                        i.path,
                        i.kind.as_str(),
                        i.os_code,
                        i.stage.as_str(),
                        i.severity.as_str(),
                        i.recoverable,
                        i.at,
                        i.message
                    ])?;
                self.rows_since += 1;
                self.c_issues += 1;
            }
            SinkMsg::Chunk(c) => {
                let mut acc = self.acc.remove(&c.dir_id).unwrap_or_default();
                let n = c.entries.len();
                for e in c.entries {
                    if e.kind == EntryKind::Dir {
                        acc.dirs += 1;
                        // Not-followed dirs are complete leaves (state 1, zero totals).
                        let state = if e.flags & flags::NOT_FOLLOWED != 0 { 1 } else { 0 };
                        self.conn
                            .prepare_cached(
                                "INSERT INTO dirs(id,parent,depth,name,state,flags,mtime,ctime,atime) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                            )?
                            .execute(params![
                                e.id as i64,
                                c.dir_id as i64,
                                c.dir_depth + 1,
                                &*e.name,
                                state,
                                e.flags,
                                e.mtime,
                                e.ctime,
                                e.atime
                            ])?;
                    } else {
                        let ext = self.ext_id(&e.name)?;
                        self.c_files += 1;
                        self.c_bytes = self.c_bytes.saturating_add(e.size);
                        acc.files += 1;
                        acc.size = acc.size.saturating_add(i64s(e.size));
                        if let Some(a) = e.alloc {
                            acc.alloc = Some(acc.alloc.unwrap_or(0).saturating_add(i64s(a)));
                        }
                        let ae = self.agg_ext.entry(ext).or_insert((0, 0));
                        ae.0 += 1;
                        ae.1 = ae.1.saturating_add(i64s(e.size));
                        let ay = self.agg_year.entry(e.mtime.map(year_of)).or_insert((0, 0));
                        ay.0 += 1;
                        ay.1 = ay.1.saturating_add(i64s(e.size));
                        self.file_batch.push((e, ext));
                        if self.file_batch.len() == FILE_BATCH {
                            self.flush_files(c.dir_id)?;
                        }
                    }
                }
                self.flush_files(c.dir_id)?;
                self.rows_since += n;
                if c.last {
                    let extra = if c.enum_error { flags::ENUM_ERROR } else { 0 };
                    self.conn
                        .prepare_cached(
                            "UPDATE dirs SET state=1, d_files=?1, d_dirs=?2, d_size=?3, d_alloc=?4, flags=flags|?5 WHERE id=?6",
                        )?
                        .execute(params![acc.files, acc.dirs, acc.size, acc.alloc, extra, c.dir_id as i64])?;
                    self.rows_since += 1;
                    self.c_dirs += 1;
                } else {
                    self.acc.insert(c.dir_id, acc);
                }
            }
        }
        if self.rows_since >= self.commit_rows || self.last_commit.elapsed().as_millis() as u64 >= self.commit_interval_ms {
            self.commit()?;
        }
        Ok(())
    }

    /// Inserts the buffered files of one directory: full batches with one
    /// multi-row statement, the remainder row by row. Parameters are bound
    /// directly (no per-row allocation).
    fn flush_files(&mut self, parent: NodeId) -> rusqlite::Result<()> {
        if self.file_batch.is_empty() {
            return Ok(());
        }
        static SQL: std::sync::OnceLock<Vec<(usize, String)>> = std::sync::OnceLock::new();
        let sqls = SQL.get_or_init(|| {
            [FILE_BATCH, 16, 4, 1]
                .iter()
                .map(|&k| {
                    let rows = vec!["(?,?,?,?,?,?,?,?,?,?,?,?,?)"; k].join(",");
                    (k, format!("INSERT INTO files(id,parent,name,ext,kind,size,alloc,mtime,ctime,atime,flags,file_id,nlink) VALUES {rows}"))
                })
                .collect()
        });
        let batch = std::mem::take(&mut self.file_batch);
        let rows: &[(nextsize_core::EntryRecord, i64)] = &batch;
        let mut start = 0;
        while start < rows.len() {
            let left = rows.len() - start;
            let (k, sql) = sqls.iter().find(|(k, _)| *k <= left).expect("size 1 always fits");
            let group = &rows[start..start + k];
            start += k;
            let mut st = self.conn.prepare_cached(sql)?;
            let mut i = 1;
            for (e, ext) in group {
                st.raw_bind_parameter(i, e.id as i64)?;
                st.raw_bind_parameter(i + 1, parent as i64)?;
                st.raw_bind_parameter(i + 2, &*e.name)?;
                st.raw_bind_parameter(i + 3, *ext)?;
                st.raw_bind_parameter(i + 4, e.kind as u8)?;
                st.raw_bind_parameter(i + 5, i64s(e.size))?;
                st.raw_bind_parameter(i + 6, e.alloc.map(i64s))?;
                st.raw_bind_parameter(i + 7, e.mtime)?;
                st.raw_bind_parameter(i + 8, e.ctime)?;
                st.raw_bind_parameter(i + 9, e.atime)?;
                st.raw_bind_parameter(i + 10, e.flags)?;
                st.raw_bind_parameter(i + 11, e.file_id.map(|v| v as i64))?;
                st.raw_bind_parameter(i + 12, e.nlink)?;
                i += 13;
            }
            st.raw_execute()?;
        }
        let mut batch = batch;
        batch.clear();
        self.file_batch = batch;
        Ok(())
    }

    fn timed(&mut self, label: &str, sql: &str) -> rusqlite::Result<()> {
        let t = Instant::now();
        self.conn.execute_batch(sql)?;
        self.timings.push((label.to_string(), t.elapsed().as_millis() as u64));
        Ok(())
    }

    /// Builds indexes, rolls up directory totals and aggregates.
    fn finalize(&mut self) -> rusqlite::Result<()> {
        for (name, sql) in schema::FINAL_INDEXES {
            self.timed(name, sql)?;
        }
        let t = Instant::now();
        self.conn.execute_batch("BEGIN")?;
        self.conn.execute_batch("UPDATE dirs SET t_files=d_files, t_dirs=d_dirs, t_size=d_size, t_alloc=d_alloc")?;
        let max_depth: i64 = self.conn.query_row("SELECT COALESCE(MAX(depth),0) FROM dirs", [], |r| r.get(0))?;
        {
            let mut st = self.conn.prepare(
                "UPDATE dirs SET t_files = t_files + c.f, t_dirs = t_dirs + c.d, t_size = t_size + c.s,
                        t_alloc = CASE WHEN c.a IS NULL THEN t_alloc ELSE COALESCE(t_alloc,0) + c.a END
                 FROM (SELECT parent, SUM(t_files) f, SUM(t_dirs) d, SUM(t_size) s, SUM(t_alloc) a
                       FROM dirs WHERE depth = ?1 GROUP BY parent) AS c
                 WHERE dirs.id = c.parent",
            )?;
            for depth in (1..=max_depth).rev() {
                st.execute([depth])?;
            }
        }
        self.conn.execute_batch("COMMIT")?;
        self.timings.push(("rollup".into(), t.elapsed().as_millis() as u64));
        self.timed("idx_dirs_tsize", schema::TSIZE_INDEX)?;
        if self.aggs_valid {
            // Computed during ingestion: no extra pass over `files`.
            let t = Instant::now();
            self.conn.execute_batch("BEGIN; DELETE FROM ext_stats; DELETE FROM year_stats;")?;
            {
                let mut st = self.conn.prepare("INSERT INTO ext_stats(ext,files,size) VALUES(?1,?2,?3)")?;
                for (ext, (n, s)) in &self.agg_ext {
                    st.execute(params![ext, n, s])?;
                }
                let mut st = self.conn.prepare("INSERT INTO year_stats(year,files,size) VALUES(?1,?2,?3)")?;
                for (y, (n, s)) in &self.agg_year {
                    st.execute(params![y, n, s])?;
                }
            }
            self.conn.execute_batch("COMMIT")?;
            self.timings.push(("aggregates(memory)".into(), t.elapsed().as_millis() as u64));
        } else {
            // After a resume the in-memory totals are incomplete: recompute.
            self.timed(
                "aggregates(sql)",
                "BEGIN;
                 DELETE FROM ext_stats; DELETE FROM year_stats;
                 INSERT INTO ext_stats SELECT ext, COUNT(*), SUM(size) FROM files GROUP BY ext;
                 INSERT INTO year_stats SELECT CAST(strftime('%Y', mtime, 'unixepoch') AS INTEGER), COUNT(*), SUM(size) FROM files GROUP BY 1;
                 COMMIT;",
            )?;
        }
        // Approximate statistics are enough for the query planner and much cheaper.
        self.timed("analyze", "PRAGMA analysis_limit=1000; ANALYZE;")?;
        Ok(())
    }

    /// Finalization with temporary resources: parallel sorter threads and a
    /// larger page cache (restored afterwards).
    fn finalize_with_resources(&mut self) -> rusqlite::Result<()> {
        // Diagnostic overrides (benchmarks only; documented in BENCHMARKS.md).
        let env = |k: &str| std::env::var(k).ok().and_then(|v| v.parse::<i64>().ok());
        let threads = env("NEXTTOOL_FIN_THREADS").unwrap_or(self.finalize_threads as i64);
        let cache_kib = env("NEXTTOOL_FIN_CACHE_KIB").unwrap_or(self.finalize_cache_kib as i64);
        self.conn.pragma_update(None, "threads", threads)?;
        let normal: i64 = self.conn.query_row("PRAGMA cache_size", [], |r| r.get(0))?;
        self.conn.pragma_update(None, "cache_size", -cache_kib)?;
        let r = self.finalize();
        let _ = self.conn.pragma_update(None, "cache_size", normal);
        let _ = self.conn.pragma_update(None, "threads", 0);
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn year_of_matches_sqlite() {
        let c = Connection::open_in_memory().unwrap();
        for t in [-2_208_988_800i64, -1, 0, 86_399, 951_782_400, 1_582_934_400, 1_609_459_199, 1_609_459_200, 4_102_444_800, 1_798_761_599] {
            let y: i64 = c.query_row("SELECT CAST(strftime('%Y', ?1, 'unixepoch') AS INTEGER)", [t], |r| r.get(0)).unwrap();
            assert_eq!(year_of(t), y, "t={t}");
        }
    }

    #[test]
    fn ext_slice_agrees_with_normalize() {
        for n in ["a.txt", "A.TXT", "x.tar.gz", ".gitignore", "noext", "a.b c", "a.mp4", "r.Jpeg", "z.7z", "a.abcdefghijklmnopq", "ação.pdf"] {
            if let Some(e) = ext_slice(n) {
                assert_eq!(Some(e.to_string()), normalize_ext(n), "{n}");
            }
        }
    }
}

impl ScanSink for IndexWriter {
    fn write(&mut self, msg: SinkMsg) -> Result<(), SinkError> {
        let t = Instant::now();
        let c0 = self.t_commit;
        let r = self.write_msg(msg).map_err(sqlite_to_sink);
        self.t_write += t.elapsed().saturating_sub(self.t_commit - c0);
        r
    }

    fn tick(&mut self) -> Result<(), SinkError> {
        if self.in_tx && self.last_commit.elapsed().as_millis() as u64 >= self.commit_interval_ms {
            self.commit().map_err(sqlite_to_sink)?;
        }
        Ok(())
    }

    fn checkpoint(&mut self) -> Result<(), SinkError> {
        self.commit().map_err(sqlite_to_sink)
    }

    fn finish(&mut self, o: &ScanOutcome) -> Result<(), SinkError> {
        let r = (|| -> rusqlite::Result<()> {
            self.commit()?;
            let t = Instant::now();
            if o.state.is_terminal_ok() {
                // Durable marker: a crash during finalization leaves the
                // snapshot resumable (resume re-runs the idempotent finalize).
                set_meta(&self.conn, "state", nextsize_core::ScanState::Finalizing.as_str())?;
                self.finalize_with_resources()?;
            }
            let finalize_ms = t.elapsed().as_millis() as u64;
            let timings = serde_json::to_string(&self.timings).unwrap_or_default();
            let ingest = serde_json::json!({
                "insert_ms": self.t_write.as_millis() as u64,
                "commit_ms": self.t_commit.as_millis() as u64,
                "commits": self.progress.commits.load(Relaxed),
            })
            .to_string();
            set_meta(&self.conn, "ingest_timings", &ingest)?;
            for (k, v) in [
                ("state", o.state.as_str().to_string()),
                ("finished_at", now_unix().to_string()),
                ("files", o.files.to_string()),
                ("dirs", o.dirs.to_string()),
                ("bytes", o.bytes.to_string()),
                ("issues", o.issues.to_string()),
                ("scan_ms", o.elapsed_ms.to_string()),
                ("finalize_ms", finalize_ms.to_string()),
                ("finalize_timings", timings),
            ] {
                set_meta(&self.conn, k, &v)?;
            }
            self.conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
            Ok(())
        })();
        r.map_err(sqlite_to_sink)
    }
}
