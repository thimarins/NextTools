//! Snapshot maintenance: deferred filter indexes, validation for import,
//! catalog listing. Never touches the scanned data, only `.nxs` files.

use std::path::{Path, PathBuf};
use std::time::Instant;

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::schema::{self, check_schema, get_meta, set_meta};
use crate::IndexError;

/// Builds the filter indexes of a completed snapshot if they are missing
/// (e.g. the process stopped after the snapshot became usable). Idempotent.
/// Returns the elapsed milliseconds.
pub fn build_filter_indexes(path: &Path) -> Result<u64, IndexError> {
    let t = Instant::now();
    let conn = Connection::open(path)?;
    check_schema(&conn)?;
    schema::apply_pragmas(&conn, 16 * 1024)?;
    conn.pragma_update(None, "threads", 2)?;
    for (_, sql) in schema::FILTER_INDEXES {
        conn.execute_batch(sql)?;
    }
    set_meta(&conn, "filter_indexes", "ready")?;
    conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
    Ok(t.elapsed().as_millis() as u64)
}

/// One snapshot file found in a catalog folder.
#[derive(Debug, Clone, Serialize)]
pub struct SnapshotEntry {
    pub path: String,
    pub file_name: String,
    pub size_bytes: u64,
    pub modified: i64,
    pub root: Option<String>,
    pub state: Option<String>,
    pub files: Option<u64>,
    pub bytes: Option<u64>,
    pub started_at: Option<i64>,
    pub label: Option<String>,
    /// Not a valid NextSize snapshot (wrong signature/schema, corrupt).
    pub invalid: Option<String>,
}

/// Lists `.nxs` files in `dir` (non-recursive), newest first, reading only
/// each file's `meta` table (bounded: at most `limit` entries).
pub fn list_snapshots(dir: &Path, limit: usize) -> Vec<SnapshotEntry> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut files: Vec<(PathBuf, std::fs::Metadata)> = rd
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("nxs")))
        .filter_map(|e| e.metadata().ok().map(|m| (e.path(), m)))
        .collect();
    files.sort_by_key(|(_, m)| std::cmp::Reverse(m.modified().ok()));
    files
        .into_iter()
        .take(limit)
        .map(|(p, m)| {
            let mut e = SnapshotEntry {
                path: p.display().to_string(),
                file_name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                size_bytes: m.len(),
                modified: m.modified().ok().and_then(nexttool_core::time::system_time_to_unix).unwrap_or(0),
                root: None,
                state: None,
                files: None,
                bytes: None,
                started_at: None,
                label: None,
                invalid: None,
            };
            match read_meta(&p) {
                Ok(mm) => {
                    e.root = mm.0;
                    e.state = mm.1;
                    e.files = mm.2;
                    e.bytes = mm.3;
                    e.started_at = mm.4;
                    e.label = mm.5;
                }
                Err(err) => e.invalid = Some(err.to_string()),
            }
            e
        })
        .collect()
}

type MetaTuple = (Option<String>, Option<String>, Option<u64>, Option<u64>, Option<i64>, Option<String>);

fn read_meta(p: &Path) -> Result<MetaTuple, IndexError> {
    let conn = Connection::open_with_flags(p, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    check_schema(&conn)?;
    let n = |k: &str| get_meta(&conn, k).ok().flatten().and_then(|v| v.parse::<u64>().ok());
    Ok((
        get_meta(&conn, "root")?,
        get_meta(&conn, "state")?,
        n("files"),
        n("bytes"),
        get_meta(&conn, "started_at")?.and_then(|v| v.parse().ok()),
        get_meta(&conn, "label")?,
    ))
}

/// Sets a human label on a snapshot (the file name is not changed, so paths
/// referenced by tasks stay valid).
pub fn set_label(path: &Path, label: &str) -> Result<(), IndexError> {
    let label: String = label.chars().filter(|c| !c.is_control()).take(120).collect();
    let conn = Connection::open(path)?;
    check_schema(&conn)?;
    set_meta(&conn, "label", label.trim())?;
    Ok(())
}

/// Consistent, compacted copy of a snapshot (includes WAL content) via
/// `VACUUM INTO`, from a read-only connection. Refuses to overwrite.
pub fn export_snapshot(src: &Path, dest: &Path) -> Result<(), IndexError> {
    if dest.exists() {
        return Err(IndexError::AlreadyExists(dest.display().to_string()));
    }
    let conn = Connection::open_with_flags(src, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    check_schema(&conn)?;
    conn.execute("VACUUM INTO ?1", [dest.to_string_lossy()])?;
    Ok(())
}

/// Validates a snapshot from elsewhere before importing it: signature,
/// schema version and SQLite integrity. Opened read-only; a hostile file
/// cannot run code (SQLite has no executable content here) and the copy is
/// done only after validation.
pub fn validate_for_import(path: &Path) -> Result<(), IndexError> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(IndexError::Corrupt("não é um arquivo".into()));
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
    check_schema(&conn)?;
    let qc: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
    if qc != "ok" {
        return Err(IndexError::Corrupt(format!("quick_check: {qc}")));
    }
    Ok(())
}
