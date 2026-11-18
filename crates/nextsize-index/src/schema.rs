use rusqlite::{Connection, OptionalExtension};

use crate::IndexError;

/// Tables only; secondary indexes are built at finalize (much faster than
/// maintaining them during ingestion). See ADR-004.
pub const CREATE: &str = r#"
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
CREATE TABLE dirs(
  id INTEGER PRIMARY KEY,
  parent INTEGER NOT NULL,
  depth INTEGER NOT NULL,
  name TEXT NOT NULL,
  state INTEGER NOT NULL,          -- 0 pending, 1 enumerated
  flags INTEGER NOT NULL,
  mtime INTEGER, ctime INTEGER, atime INTEGER,
  d_files INTEGER NOT NULL DEFAULT 0, d_dirs INTEGER NOT NULL DEFAULT 0,
  d_size INTEGER NOT NULL DEFAULT 0, d_alloc INTEGER,
  t_files INTEGER, t_dirs INTEGER, t_size INTEGER, t_alloc INTEGER
);
CREATE TABLE files(
  id INTEGER PRIMARY KEY,
  parent INTEGER NOT NULL,
  name TEXT NOT NULL,
  ext INTEGER NOT NULL,            -- 0 = no extension
  kind INTEGER NOT NULL,           -- 0 file, 2 symlink, 3 other
  size INTEGER NOT NULL,
  alloc INTEGER,
  mtime INTEGER, ctime INTEGER, atime INTEGER,
  flags INTEGER NOT NULL,
  file_id INTEGER, nlink INTEGER
);
CREATE TABLE exts(id INTEGER PRIMARY KEY, ext TEXT NOT NULL UNIQUE);
CREATE TABLE issues(
  id INTEGER PRIMARY KEY, dir_id INTEGER, path TEXT NOT NULL, kind TEXT NOT NULL,
  os_code INTEGER, stage TEXT NOT NULL, severity TEXT NOT NULL, recoverable INTEGER NOT NULL,
  at INTEGER NOT NULL, message TEXT NOT NULL
);
CREATE TABLE ext_stats(ext INTEGER PRIMARY KEY, files INTEGER NOT NULL, size INTEGER NOT NULL);
CREATE TABLE year_stats(year INTEGER, files INTEGER NOT NULL, size INTEGER NOT NULL);
"#;

pub const FINAL_INDEXES: &[(&str, &str)] = &[
    ("idx_dirs_parent", "CREATE INDEX IF NOT EXISTS idx_dirs_parent ON dirs(parent)"),
    ("idx_dirs_depth", "CREATE INDEX IF NOT EXISTS idx_dirs_depth ON dirs(depth, parent)"),
    ("idx_files_parent", "CREATE INDEX IF NOT EXISTS idx_files_parent ON files(parent)"),
    ("idx_files_size", "CREATE INDEX IF NOT EXISTS idx_files_size ON files(size DESC)"),
    ("idx_files_mtime", "CREATE INDEX IF NOT EXISTS idx_files_mtime ON files(mtime)"),
    ("idx_files_ext", "CREATE INDEX IF NOT EXISTS idx_files_ext ON files(ext, size DESC)"),
];

pub const TSIZE_INDEX: &str = "CREATE INDEX IF NOT EXISTS idx_dirs_tsize ON dirs(t_size DESC)";

pub fn apply_pragmas(conn: &Connection, cache_kib: u64) -> rusqlite::Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "temp_store", "FILE")?;
    conn.pragma_update(None, "cache_size", -(cache_kib.max(2048) as i64))?;
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    Ok(())
}

pub fn get_meta(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM meta WHERE key=?1", [key], |r| r.get(0)).optional()
}

pub fn set_meta(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute("INSERT INTO meta(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [key, value])?;
    Ok(())
}

/// Verifies this is a NextSize snapshot of a supported schema version.
pub fn check_schema(conn: &Connection) -> Result<(), IndexError> {
    let has_meta: bool = conn
        .query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name='meta'", [], |_| Ok(true))
        .optional()
        .map_err(|e| IndexError::Corrupt(e.to_string()))?
        .unwrap_or(false);
    if !has_meta {
        return Err(IndexError::Corrupt("tabela meta ausente — não é um snapshot NextSize".into()));
    }
    let v = get_meta(conn, "schema_version")?.unwrap_or_default();
    if v != nextsize_core::SCHEMA_VERSION.to_string() {
        return Err(IndexError::Schema { found: v, supported: nextsize_core::SCHEMA_VERSION });
    }
    if get_meta(conn, "app")?.as_deref() != Some("nexttool-nextsize") {
        return Err(IndexError::Corrupt("assinatura 'app' ausente".into()));
    }
    Ok(())
}
