use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use rusqlite::{params_from_iter, types::Value, Connection, OpenFlags, OptionalExtension};
use serde::Serialize;

use crate::schema::{check_schema, get_meta};
use crate::IndexError;

/// Max page size any query returns. The UI pages; it never asks for millions.
pub const MAX_PAGE: u32 = 10_000;

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub root: String,
    pub state: String,
    /// True only for completed snapshots; partial data must be labeled.
    pub complete: bool,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    /// Sum of allocated sizes when the platform reported them; else None ("não disponível").
    pub alloc_bytes: Option<u64>,
    pub issues: u64,
    pub enumerator: String,
    pub source_kind: String,
    pub profile: String,
    pub resumed_count: u64,
    pub hardlink_policy: String,
    pub scan_ms: Option<u64>,
    pub finalize_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileRow {
    pub id: i64,
    pub path: String,
    pub size: u64,
    pub mtime: Option<i64>,
    pub flags: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct DirRow {
    pub id: i64,
    pub name: String,
    pub path: Option<String>,
    pub is_dir: bool,
    pub size: u64,
    pub files: u64,
    pub dirs: u64,
    pub mtime: Option<i64>,
    pub flags: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtRow {
    pub ext: String,
    pub files: u64,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct YearRow {
    pub year: Option<i64>,
    pub files: u64,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct IssueRow {
    pub path: String,
    pub kind: String,
    pub os_code: Option<i64>,
    pub severity: String,
    pub message: String,
    pub at: i64,
}

/// Combined filter over files. All conditions are AND-ed.
#[derive(Debug, Clone, Default)]
pub struct FileQuery {
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub ext: Option<String>,
    pub modified_before: Option<i64>,
    pub modified_after: Option<i64>,
    pub year: Option<i32>,
    pub name_contains: Option<String>,
    pub sort_by_size_desc: bool,
    pub offset: u32,
    pub limit: u32,
    /// Hard limit on query time; returns `QueryError::TimeLimit` after it.
    pub time_limit: Option<Duration>,
}

#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("consulta cancelada")]
    Cancelled,
    #[error("consulta excedeu o tempo limite")]
    TimeLimit,
    #[error("snapshot incompleto: índices ainda não construídos")]
    NotFinalized,
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// Read-only access to a snapshot. Opening reads only the `meta` table, so
/// the summary is available immediately regardless of snapshot size.
pub struct Snapshot {
    conn: Connection,
    path: PathBuf,
    cancel: Arc<AtomicBool>,
}

fn opt_i64(s: Option<String>) -> Option<i64> {
    s.and_then(|v| v.parse().ok())
}

impl Snapshot {
    pub fn open(path: &Path) -> Result<Self, IndexError> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        conn.pragma_update(None, "cache_size", -16384)?;
        check_schema(&conn)?;
        Ok(Snapshot { conn, path: path.to_path_buf(), cancel: Arc::new(AtomicBool::new(false)) })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Token to cancel a running query from another thread.
    pub fn cancel_token(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    fn meta(&self, k: &str) -> Result<Option<String>, IndexError> {
        Ok(get_meta(&self.conn, k)?)
    }

    pub fn summary(&self) -> Result<Summary, IndexError> {
        let state = self.meta("state")?.unwrap_or_default();
        let complete = matches!(state.as_str(), "completed" | "completed_with_warnings");
        let n = |k: &str| -> Result<u64, IndexError> { Ok(opt_i64(self.meta(k)?).unwrap_or(0).max(0) as u64) };
        let alloc_bytes: Option<i64> = if complete {
            self.conn.query_row("SELECT t_alloc FROM dirs WHERE id=1", [], |r| r.get(0)).optional()?.flatten()
        } else {
            None
        };
        Ok(Summary {
            root: self.meta("root")?.unwrap_or_default(),
            complete,
            started_at: opt_i64(self.meta("started_at")?),
            finished_at: opt_i64(self.meta("finished_at")?),
            files: n("files")?,
            dirs: n("dirs")?,
            bytes: n("bytes")?,
            alloc_bytes: alloc_bytes.map(|v| v.max(0) as u64),
            issues: n("issues")?,
            enumerator: self.meta("enumerator")?.unwrap_or_default(),
            source_kind: self.meta("source_kind")?.unwrap_or_default(),
            profile: self.meta("profile")?.unwrap_or_default().trim_matches('"').to_string(),
            resumed_count: n("resumed_count")?,
            hardlink_policy: self.meta("hardlink_policy")?.unwrap_or_default(),
            scan_ms: opt_i64(self.meta("scan_ms")?).map(|v| v as u64),
            finalize_ms: opt_i64(self.meta("finalize_ms")?).map(|v| v as u64),
            state,
        })
    }

    fn require_final(&self) -> Result<(), QueryError> {
        let ok: Option<i64> = self
            .conn
            .query_row("SELECT 1 FROM sqlite_master WHERE type='index' AND name='idx_dirs_tsize'", [], |r| r.get(0))
            .optional()?;
        if ok.is_none() {
            return Err(QueryError::NotFinalized);
        }
        Ok(())
    }

    /// Full path of a directory, rebuilt from the parent chain.
    pub fn dir_path(&self, mut id: i64) -> rusqlite::Result<String> {
        let mut names = Vec::new();
        let mut st = self.conn.prepare_cached("SELECT parent, name FROM dirs WHERE id=?1")?;
        loop {
            let (parent, name): (i64, String) = st.query_row([id], |r| Ok((r.get(0)?, r.get(1)?)))?;
            names.push(name);
            if parent == 0 {
                break;
            }
            id = parent;
        }
        let mut p = PathBuf::new();
        for n in names.iter().rev() {
            p.push(n);
        }
        Ok(p.to_string_lossy().into_owned())
    }

    /// Installs a progress handler that aborts on cancel or time limit.
    fn guard(&self, limit: Option<Duration>) {
        self.cancel.store(false, Ordering::SeqCst);
        let cancel = self.cancel.clone();
        let deadline = limit.map(|d| Instant::now() + d);
        self.conn.progress_handler(
            10_000,
            Some(move || cancel.load(Ordering::Relaxed) || deadline.is_some_and(|d| Instant::now() > d)),
        );
    }

    fn unguard(&self) {
        self.conn.progress_handler(0, None::<fn() -> bool>);
    }

    fn map_interrupt(&self, e: rusqlite::Error, limit: Option<Duration>) -> QueryError {
        if let rusqlite::Error::SqliteFailure(ref f, _) = e {
            if f.code == rusqlite::ErrorCode::OperationInterrupted {
                return if self.cancel.load(Ordering::Relaxed) || limit.is_none() { QueryError::Cancelled } else { QueryError::TimeLimit };
            }
        }
        QueryError::Sqlite(e)
    }

    pub fn top_files(&self, limit: u32) -> Result<Vec<FileRow>, QueryError> {
        self.query_files(&FileQuery { sort_by_size_desc: true, limit, ..Default::default() })
    }

    pub fn top_dirs(&self, limit: u32) -> Result<Vec<DirRow>, QueryError> {
        self.require_final()?;
        let mut st = self.conn.prepare(
            "SELECT id, name, t_size, t_files, t_dirs, mtime, flags FROM dirs WHERE id<>1 ORDER BY t_size DESC LIMIT ?1",
        )?;
        let rows: Vec<(i64, String, i64, i64, i64, Option<i64>, u32)> = st
            .query_map([limit.min(MAX_PAGE)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))?
            .collect::<Result<_, _>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (id, name, size, files, dirs, mtime, flags) in rows {
            out.push(DirRow {
                id,
                name,
                path: Some(self.dir_path(id)?),
                is_dir: true,
                size: size.max(0) as u64,
                files: files.max(0) as u64,
                dirs: dirs.max(0) as u64,
                mtime,
                flags,
            });
        }
        Ok(out)
    }

    /// Children of a directory (subdirectories with totals + files), largest first, paginated.
    pub fn list_dir(&self, dir_id: i64, offset: u32, limit: u32) -> Result<Vec<DirRow>, QueryError> {
        self.require_final()?;
        let mut st = self.conn.prepare(
            "SELECT id, name, 1, COALESCE(t_size,0), COALESCE(t_files,0), COALESCE(t_dirs,0), mtime, flags FROM dirs WHERE parent=?1
             UNION ALL
             SELECT id, name, 0, size, 1, 0, mtime, flags FROM files WHERE parent=?1
             ORDER BY 4 DESC LIMIT ?2 OFFSET ?3",
        )?;
        let rows = st
            .query_map(rusqlite::params![dir_id, limit.min(MAX_PAGE), offset], |r| {
                Ok(DirRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    path: None,
                    is_dir: r.get::<_, i64>(2)? == 1,
                    size: r.get::<_, i64>(3)?.max(0) as u64,
                    files: r.get::<_, i64>(4)?.max(0) as u64,
                    dirs: r.get::<_, i64>(5)?.max(0) as u64,
                    mtime: r.get(6)?,
                    flags: r.get(7)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn query_files(&self, q: &FileQuery) -> Result<Vec<FileRow>, QueryError> {
        self.require_final()?;
        let mut sql = String::from("SELECT f.id, f.parent, f.name, f.size, f.mtime, f.flags FROM files f");
        let mut cond: Vec<String> = Vec::new();
        let mut args: Vec<Value> = Vec::new();
        if let Some(e) = &q.ext {
            sql.push_str(" JOIN exts x ON x.id = f.ext");
            cond.push("x.ext = ?".into());
            args.push(Value::Text(e.trim_start_matches('.').to_lowercase()));
        }
        if let Some(v) = q.min_size {
            cond.push("f.size >= ?".into());
            args.push(Value::Integer(v.min(i64::MAX as u64) as i64));
        }
        if let Some(v) = q.max_size {
            cond.push("f.size <= ?".into());
            args.push(Value::Integer(v.min(i64::MAX as u64) as i64));
        }
        if let Some(v) = q.modified_before {
            cond.push("f.mtime < ?".into());
            args.push(Value::Integer(v));
        }
        if let Some(v) = q.modified_after {
            cond.push("f.mtime >= ?".into());
            args.push(Value::Integer(v));
        }
        if let Some(y) = q.year {
            // Range on mtime keeps the index usable.
            let start = days_from_civil(y as i64, 1, 1) * 86_400;
            let end = days_from_civil(y as i64 + 1, 1, 1) * 86_400;
            cond.push("f.mtime >= ? AND f.mtime < ?".into());
            args.push(Value::Integer(start));
            args.push(Value::Integer(end));
        }
        if let Some(n) = &q.name_contains {
            cond.push("instr(lower(f.name), ?) > 0".into());
            args.push(Value::Text(n.to_lowercase()));
        }
        if !cond.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&cond.join(" AND "));
        }
        if q.sort_by_size_desc {
            sql.push_str(" ORDER BY f.size DESC");
        }
        sql.push_str(" LIMIT ? OFFSET ?");
        let limit = if q.limit == 0 { 100 } else { q.limit.min(MAX_PAGE) };
        args.push(Value::Integer(limit as i64));
        args.push(Value::Integer(q.offset as i64));

        self.guard(q.time_limit);
        let res = (|| -> rusqlite::Result<Vec<(i64, i64, String, i64, Option<i64>, u32)>> {
            let mut st = self.conn.prepare(&sql)?;
            let rows = st.query_map(params_from_iter(args.iter()), |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
            })?;
            rows.collect()
        })();
        self.unguard();
        let rows = res.map_err(|e| self.map_interrupt(e, q.time_limit))?;
        let mut out = Vec::with_capacity(rows.len());
        for (id, parent, name, size, mtime, flags) in rows {
            let mut p = PathBuf::from(self.dir_path(parent)?);
            p.push(name);
            out.push(FileRow { id, path: p.to_string_lossy().into_owned(), size: size.max(0) as u64, mtime, flags });
        }
        Ok(out)
    }

    pub fn ext_stats(&self, limit: u32) -> Result<Vec<ExtRow>, QueryError> {
        let mut st = self.conn.prepare(
            "SELECT COALESCE(x.ext, '(sem extensão)'), s.files, s.size FROM ext_stats s LEFT JOIN exts x ON x.id=s.ext ORDER BY s.size DESC LIMIT ?1",
        )?;
        let rows = st
            .query_map([limit.min(MAX_PAGE)], |r| {
                Ok(ExtRow { ext: r.get(0)?, files: r.get::<_, i64>(1)? as u64, size: r.get::<_, i64>(2)?.max(0) as u64 })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn year_stats(&self) -> Result<Vec<YearRow>, QueryError> {
        let mut st = self.conn.prepare("SELECT year, files, size FROM year_stats ORDER BY year")?;
        let rows = st
            .query_map([], |r| Ok(YearRow { year: r.get(0)?, files: r.get::<_, i64>(1)? as u64, size: r.get::<_, i64>(2)?.max(0) as u64 }))?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn issues(&self, offset: u32, limit: u32) -> Result<Vec<IssueRow>, QueryError> {
        let mut st = self.conn.prepare("SELECT path, kind, os_code, severity, message, at FROM issues ORDER BY id LIMIT ?1 OFFSET ?2")?;
        let rows = st
            .query_map([limit.min(MAX_PAGE), offset], |r| {
                Ok(IssueRow { path: r.get(0)?, kind: r.get(1)?, os_code: r.get(2)?, severity: r.get(3)?, message: r.get(4)?, at: r.get(5)? })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Integrity check of the snapshot file (SQLite quick_check + invariants).
    pub fn verify(&self) -> Result<Vec<String>, QueryError> {
        let mut problems = Vec::new();
        let qc: String = self.conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if qc != "ok" {
            problems.push(format!("quick_check: {qc}"));
        }
        let orphans: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM files f WHERE NOT EXISTS (SELECT 1 FROM dirs d WHERE d.id=f.parent)",
            [],
            |r| r.get(0),
        )?;
        if orphans > 0 {
            problems.push(format!("{orphans} arquivos sem diretório pai"));
        }
        let s = self.summary().map_err(|e| QueryError::Sqlite(rusqlite::Error::InvalidParameterName(e.to_string())))?;
        if s.complete {
            let (f, b): (i64, i64) = self.conn.query_row("SELECT COUNT(*), COALESCE(SUM(size),0) FROM files", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
            if f as u64 != s.files || b as u64 != s.bytes {
                problems.push(format!("contadores divergentes: meta {}/{} vs tabela {f}/{b}", s.files, s.bytes));
            }
            let (tf, tb): (i64, i64) = self.conn.query_row("SELECT t_files, t_size FROM dirs WHERE id=1", [], |r| Ok((r.get(0)?, r.get(1)?)))?;
            if tf != f || tb != b {
                problems.push(format!("roll-up divergente: raiz {tf}/{tb} vs tabela {f}/{b}"));
            }
            let pending: i64 = self.conn.query_row("SELECT COUNT(*) FROM dirs WHERE state=0", [], |r| r.get(0))?;
            if pending > 0 {
                problems.push(format!("{pending} diretórios pendentes em snapshot marcado como completo"));
            }
        }
        Ok(problems)
    }
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    #[test]
    fn civil() {
        assert_eq!(super::days_from_civil(1970, 1, 1), 0);
        assert_eq!(super::days_from_civil(2020, 1, 1) * 86_400, 1_577_836_800);
    }
}
