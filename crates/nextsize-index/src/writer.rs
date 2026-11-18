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
}

fn i64s(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
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
        Ok(Self::from_conn(conn, path, opts, progress))
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
            progress,
            timings: Vec::new(),
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
        if self.in_tx {
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
                        acc.files += 1;
                        acc.size = acc.size.saturating_add(i64s(e.size));
                        if let Some(a) = e.alloc {
                            acc.alloc = Some(acc.alloc.unwrap_or(0).saturating_add(i64s(a)));
                        }
                        self.conn
                            .prepare_cached(
                                "INSERT INTO files(id,parent,name,ext,kind,size,alloc,mtime,ctime,atime,flags,file_id,nlink) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
                            )?
                            .execute(params![
                                e.id as i64,
                                c.dir_id as i64,
                                &*e.name,
                                ext,
                                e.kind as u8,
                                i64s(e.size),
                                e.alloc.map(i64s),
                                e.mtime,
                                e.ctime,
                                e.atime,
                                e.flags,
                                e.file_id.map(|v| v as i64),
                                e.nlink
                            ])?;
                    }
                }
                self.rows_since += n;
                if c.last {
                    let extra = if c.enum_error { flags::ENUM_ERROR } else { 0 };
                    self.conn
                        .prepare_cached(
                            "UPDATE dirs SET state=1, d_files=?1, d_dirs=?2, d_size=?3, d_alloc=?4, flags=flags|?5 WHERE id=?6",
                        )?
                        .execute(params![acc.files, acc.dirs, acc.size, acc.alloc, extra, c.dir_id as i64])?;
                    self.rows_since += 1;
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
        self.timed(
            "aggregates",
            "BEGIN;
             DELETE FROM ext_stats; DELETE FROM year_stats;
             INSERT INTO ext_stats SELECT ext, COUNT(*), SUM(size) FROM files GROUP BY ext;
             INSERT INTO year_stats SELECT CAST(strftime('%Y', mtime, 'unixepoch') AS INTEGER), COUNT(*), SUM(size) FROM files GROUP BY 1;
             COMMIT;",
        )?;
        self.timed("analyze", "ANALYZE")?;
        Ok(())
    }
}

impl ScanSink for IndexWriter {
    fn write(&mut self, msg: SinkMsg) -> Result<(), SinkError> {
        self.write_msg(msg).map_err(sqlite_to_sink)
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
                self.finalize()?;
            }
            let finalize_ms = t.elapsed().as_millis() as u64;
            let timings = serde_json::to_string(&self.timings).unwrap_or_default();
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
