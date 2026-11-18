//! Snapshot comparison by path, one directory level at a time (lazy, like the
//! tree view). Node ids differ between snapshots, so directories are matched
//! by name under the same parent path. Memory is bounded by `MAX_CHILDREN`
//! per side for the level being compared.

use std::collections::BTreeMap;

use rusqlite::OptionalExtension;
use serde::Serialize;

use crate::reader::{QueryError, Snapshot};

/// Max subdirectories read per side for one comparison level.
pub const MAX_CHILDREN: u32 = 20_000;

#[derive(Debug, Clone, Serialize)]
pub struct CompareRow {
    pub name: String,
    /// "added" | "removed" | "grew" | "shrank" | "same" | "files" (direct files of this folder)
    pub status: &'static str,
    pub a_size: Option<u64>,
    pub b_size: Option<u64>,
    pub a_files: Option<u64>,
    pub b_files: Option<u64>,
    /// b - a (bytes)
    pub delta: i64,
    /// Relative growth vs A (None when A is absent or zero)
    pub pct: Option<f64>,
    /// Has subdirectories in A or B (can drill down).
    pub has_children: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct CompareLevel {
    pub path: Vec<String>,
    pub a_found: bool,
    pub b_found: bool,
    pub a_total: Option<u64>,
    pub b_total: Option<u64>,
    pub rows: Vec<CompareRow>,
    /// More children than MAX_CHILDREN on one side: comparison is partial.
    pub truncated: bool,
}

struct Child {
    size: u64,
    files: u64,
    dirs: u64,
}

fn resolve(s: &Snapshot, path: &[String]) -> Result<Option<i64>, QueryError> {
    let mut id = 1i64;
    let mut st = s.conn().prepare_cached("SELECT id FROM dirs WHERE parent=?1 AND name=?2")?;
    for name in path {
        match st.query_row(rusqlite::params![id, name], |r| r.get::<_, i64>(0)).optional()? {
            Some(x) => id = x,
            None => return Ok(None),
        }
    }
    Ok(Some(id))
}

fn level(s: &Snapshot, id: i64) -> Result<(BTreeMap<String, Child>, Child, u64, bool), QueryError> {
    let mut st = s.conn().prepare_cached(
        "SELECT name, COALESCE(t_size,d_size,0), COALESCE(t_files,d_files,0), COALESCE(t_dirs,d_dirs,0) FROM dirs WHERE parent=?1 LIMIT ?2",
    )?;
    let mut map = BTreeMap::new();
    let rows = st.query_map(rusqlite::params![id, MAX_CHILDREN + 1], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, i64>(3)?))
    })?;
    let mut n = 0;
    for row in rows {
        let (name, size, files, dirs) = row?;
        n += 1;
        if n > MAX_CHILDREN {
            break;
        }
        map.insert(name, Child { size: size.max(0) as u64, files: files.max(0) as u64, dirs: dirs.max(0) as u64 });
    }
    let (d_size, d_files, total): (i64, i64, i64) = s.conn().query_row(
        "SELECT d_size, d_files, COALESCE(t_size, d_size) FROM dirs WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok((map, Child { size: d_size.max(0) as u64, files: d_files.max(0) as u64, dirs: 0 }, total.max(0) as u64, n > MAX_CHILDREN))
}

/// Compares the children of `path` (names from the root) in A (older) and B (newer).
/// Rows are sorted by absolute size change, largest first, capped at `limit`.
pub fn compare_children(a: &Snapshot, b: &Snapshot, path: &[String], limit: usize) -> Result<CompareLevel, QueryError> {
    let ida = resolve(a, path)?;
    let idb = resolve(b, path)?;
    let la = ida.map(|id| level(a, id)).transpose()?;
    let lb = idb.map(|id| level(b, id)).transpose()?;
    let empty = BTreeMap::new();
    let (ma, mb) = (la.as_ref().map(|l| &l.0).unwrap_or(&empty), lb.as_ref().map(|l| &l.0).unwrap_or(&empty));
    let mut rows = Vec::new();
    for name in ma.keys().chain(mb.keys().filter(|k| !ma.contains_key(*k))) {
        let (ca, cb) = (ma.get(name), mb.get(name));
        let delta = cb.map_or(0, |c| c.size as i64) - ca.map_or(0, |c| c.size as i64);
        let status = match (ca, cb) {
            (None, Some(_)) => "added",
            (Some(_), None) => "removed",
            _ if delta > 0 => "grew",
            _ if delta < 0 => "shrank",
            _ => "same",
        };
        rows.push(CompareRow {
            name: name.clone(),
            status,
            a_size: ca.map(|c| c.size),
            b_size: cb.map(|c| c.size),
            a_files: ca.map(|c| c.files),
            b_files: cb.map(|c| c.files),
            delta,
            pct: ca.filter(|c| c.size > 0).map(|c| delta as f64 / c.size as f64 * 100.0),
            has_children: ca.is_some_and(|c| c.dirs > 0) || cb.is_some_and(|c| c.dirs > 0),
        });
    }
    // Direct files of this folder, as one row.
    let (fa, fb) = (la.as_ref().map(|l| &l.1), lb.as_ref().map(|l| &l.1));
    if fa.is_some_and(|f| f.files > 0) || fb.is_some_and(|f| f.files > 0) {
        let delta = fb.map_or(0, |c| c.size as i64) - fa.map_or(0, |c| c.size as i64);
        rows.push(CompareRow {
            name: "(arquivos nesta pasta)".into(),
            status: "files",
            a_size: fa.map(|c| c.size),
            b_size: fb.map(|c| c.size),
            a_files: fa.map(|c| c.files),
            b_files: fb.map(|c| c.files),
            delta,
            pct: fa.filter(|c| c.size > 0).map(|c| delta as f64 / c.size as f64 * 100.0),
            has_children: false,
        });
    }
    rows.sort_by_key(|r| std::cmp::Reverse(r.delta.unsigned_abs()));
    rows.truncate(limit.clamp(1, 5000));
    Ok(CompareLevel {
        path: path.to_vec(),
        a_found: ida.is_some(),
        b_found: idb.is_some(),
        a_total: la.as_ref().map(|l| l.2),
        b_total: lb.as_ref().map(|l| l.2),
        truncated: la.as_ref().is_some_and(|l| l.3) || lb.as_ref().is_some_and(|l| l.3),
        rows,
    })
}
