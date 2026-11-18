//! Recovery of an interrupted snapshot (cancel, crash, power loss, disk full).
//!
//! Invariant: a directory row with `state = 1` has *all* its direct entries
//! committed. A directory with `state = 0` may have some committed entries
//! (chunks) and even committed descendants. On resume each pending directory's
//! committed subtree is deleted and the directory is re-enumerated, so nothing
//! is lost and nothing is counted twice. Cost: at most the work that was in
//! flight at the time of the interruption is redone.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

use nextsize_core::{ScanOptions, ScanProgress, ScanState};
use nextsize_scanner::DirTask;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

use crate::schema::{self, get_meta, set_meta};
use crate::writer::IndexWriter;
use crate::IndexError;

pub struct ResumePlan {
    pub next_id: u64,
    pub pending_dirs: u64,
    pub discarded_rows: u64,
    pub root: PathBuf,
    pub options: Option<ScanOptions>,
    /// Streams pending directories from a separate read connection.
    pub pending: Box<dyn Iterator<Item = DirTask> + Send>,
}

/// Opens an interrupted snapshot, repairs it and returns the writer plus the
/// list of directories still to read.
pub fn open_for_resume(path: &Path, cache_kib: u64, progress: Arc<ScanProgress>) -> Result<(IndexWriter, ResumePlan), IndexError> {
    let conn = Connection::open(path)?;
    schema::check_schema(&conn)?;
    schema::apply_pragmas(&conn, cache_kib)?;
    let state = get_meta(&conn, "state")?.unwrap_or_default();
    let resumable = matches!(
        ScanState::parse(&state),
        Some(ScanState::Scanning | ScanState::Interrupted | ScanState::Paused | ScanState::Preparing | ScanState::Cancelling | ScanState::Failed)
    );
    if !resumable {
        return Err(IndexError::NotResumable(state));
    }
    let root = PathBuf::from(get_meta(&conn, "root")?.unwrap_or_default());
    let options: Option<ScanOptions> = get_meta(&conn, "options")?.and_then(|s| serde_json::from_str(&s).ok());

    // Indexes needed for subtree cleanup; dropped afterwards to keep ingest fast.
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_dirs_parent ON dirs(parent);
         CREATE INDEX IF NOT EXISTS idx_files_parent ON files(parent);",
    )?;
    // Finalization artifacts are rebuilt at the end.
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_dirs_depth; DROP INDEX IF EXISTS idx_files_size; DROP INDEX IF EXISTS idx_files_mtime;
         DROP INDEX IF EXISTS idx_files_ext; DROP INDEX IF EXISTS idx_dirs_tsize;",
    )?;

    let mut discarded: u64 = 0;
    let mut last_id: i64 = 0;
    loop {
        let ids: Vec<i64> = {
            let mut st = conn.prepare("SELECT id FROM dirs WHERE state=0 AND id>?1 ORDER BY id LIMIT 1000")?;
            let rows = st.query_map([last_id], |r| r.get(0))?;
            rows.collect::<Result<_, _>>()?
        };
        if ids.is_empty() {
            break;
        }
        last_id = *ids.last().unwrap();
        for id in ids {
            // May already have been removed as part of an ancestor's subtree.
            let still: Option<i64> = conn.query_row("SELECT state FROM dirs WHERE id=?1", [id], |r| r.get(0)).optional()?;
            if still != Some(0) {
                continue;
            }
            conn.execute_batch("BEGIN")?;
            conn.execute("DROP TABLE IF EXISTS temp.doomed", [])?;
            conn.execute(
                "CREATE TEMP TABLE doomed AS WITH RECURSIVE sub(id) AS (
                     SELECT id FROM dirs WHERE parent=?1
                     UNION ALL SELECT d.id FROM dirs d JOIN sub ON d.parent=sub.id)
                 SELECT id FROM sub",
                [id],
            )?;
            discarded += conn.execute("DELETE FROM files WHERE parent=?1 OR parent IN (SELECT id FROM temp.doomed)", [id])? as u64;
            discarded += conn.execute("DELETE FROM issues WHERE dir_id=?1 OR dir_id IN (SELECT id FROM temp.doomed)", [id])? as u64;
            discarded += conn.execute("DELETE FROM dirs WHERE id IN (SELECT id FROM temp.doomed)", [])? as u64;
            conn.execute("UPDATE dirs SET d_files=0,d_dirs=0,d_size=0,d_alloc=NULL, flags=flags & ~?2 WHERE id=?1", params![id, nexttool_core::fs::flags::ENUM_ERROR])?;
            conn.execute("DROP TABLE temp.doomed", [])?;
            conn.execute_batch("COMMIT")?;
        }
    }
    conn.execute_batch("DROP INDEX IF EXISTS idx_files_parent;")?;

    let next_id: i64 = conn.query_row(
        "SELECT MAX(COALESCE((SELECT MAX(id) FROM dirs),0), COALESCE((SELECT MAX(id) FROM files),0)) + 1",
        [],
        |r| r.get(0),
    )?;
    let pending_dirs: i64 = conn.query_row("SELECT COUNT(*) FROM dirs WHERE state=0", [], |r| r.get(0))?;
    let (files, bytes): (i64, i64) = conn.query_row("SELECT COUNT(*), COALESCE(SUM(size),0) FROM files", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let dirs_done: i64 = conn.query_row("SELECT COUNT(*) FROM dirs WHERE state=1 AND (flags & ?1)=0", [nexttool_core::fs::flags::NOT_FOLLOWED], |r| r.get(0))?;
    let issues: i64 = conn.query_row("SELECT COUNT(*) FROM issues", [], |r| r.get(0))?;
    progress.files.store(files as u64, Relaxed);
    progress.bytes.store(bytes as u64, Relaxed);
    progress.dirs.store(dirs_done as u64, Relaxed);
    progress.issues.store(issues as u64, Relaxed);

    let resumed: i64 = get_meta(&conn, "resumed_count")?.and_then(|s| s.parse().ok()).unwrap_or(0);
    set_meta(&conn, "resumed_count", &(resumed + 1).to_string())?;
    set_meta(&conn, "state", ScanState::Scanning.as_str())?;

    let pending = PendingIter::new(path, root.clone())?;
    let opts_for_writer = options.clone().unwrap_or_else(|| ScanOptions {
        root: root.clone(),
        profile: Default::default(),
        backend: Default::default(),
        tuning: nextsize_core::Tuning::for_profile(Default::default(), false, 4),
        cross_mounts: false,
        spill_dir: None,
    });
    let writer = IndexWriter::from_conn(conn, path, &opts_for_writer, progress);
    Ok((
        writer,
        ResumePlan {
            next_id: next_id as u64,
            pending_dirs: pending_dirs as u64,
            discarded_rows: discarded,
            root,
            options,
            pending: Box::new(pending),
        },
    ))
}

/// Pages through pending directories (state=0) and rebuilds their paths.
struct PendingIter {
    conn: Connection,
    root: PathBuf,
    last_id: i64,
    page: std::vec::IntoIter<(i64, u32)>,
}

impl PendingIter {
    fn new(path: &Path, root: PathBuf) -> Result<Self, IndexError> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        Ok(PendingIter { conn, root, last_id: 0, page: Vec::new().into_iter() })
    }

    fn path_of(&self, mut id: i64) -> rusqlite::Result<PathBuf> {
        let mut names = Vec::new();
        let mut st = self.conn.prepare_cached("SELECT parent, name FROM dirs WHERE id=?1")?;
        while id != nextsize_core::ROOT_ID as i64 {
            let (parent, name): (i64, String) = st.query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            names.push(name);
            id = parent;
        }
        let mut p = self.root.clone();
        for n in names.iter().rev() {
            p.push(n);
        }
        Ok(p)
    }
}

impl Iterator for PendingIter {
    type Item = DirTask;

    fn next(&mut self) -> Option<DirTask> {
        loop {
            if let Some((id, depth)) = self.page.next() {
                // Known limitation: a pending directory whose name (or an
                // ancestor's) was not valid Unicode (NAME_LOSSY) rebuilds to a
                // wrong path; its enumeration then fails and is recorded as an
                // issue (never silently dropped). See KNOWN_ISSUES.md.
                match self.path_of(id) {
                    Ok(path) => return Some(DirTask { id: id as u64, depth, path }),
                    Err(e) => {
                        tracing::warn!("resume: cannot rebuild path of dir {id}: {e}");
                        continue;
                    }
                }
            }
            let rows: Vec<(i64, u32)> = {
                let mut st = self.conn.prepare("SELECT id, depth FROM dirs WHERE state=0 AND id>?1 ORDER BY id LIMIT 10000").ok()?;
                let it = st.query_map([self.last_id], |r| Ok((r.get(0)?, r.get(1)?))).ok()?;
                it.flatten().collect()
            };
            if rows.is_empty() {
                return None;
            }
            self.last_id = rows.last().unwrap().0;
            self.page = rows.into_iter();
        }
    }
}
