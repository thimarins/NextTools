//! Post-copy verification.
//!
//! - Standard: every source file that the job's filters select must exist in
//!   the destination with the same size and modification time (±2 s for
//!   FAT/SMB rounding).
//! - Strict: Standard + full BLAKE3 hash of both files. Partial hashes are
//!   never used as proof. Cloud placeholders in the source are not opened
//!   (that would trigger a download); they are reported as "not verified".
//!
//! Read-only on both sides. Memory: one directory listing at a time plus the
//! pending-directory stack (see KNOWN_ISSUES for very wide trees).

mod wildcard;

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use nexttool_core::fs::{flags, DirEnumerator, EntryKind};
use nexttool_core::time::system_time_to_unix;
use nextcopy_core::{CopyJob, OverwritePolicy, VerifyLevel};
use serde::Serialize;

pub use wildcard::wild_match;

const MTIME_TOLERANCE_S: i64 = 2;
const SAMPLE_CAP: usize = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DivergenceKind {
    Missing,
    SizeMismatch,
    TimeMismatch,
    HashMismatch,
    /// Destination differs but the overwrite policy intentionally kept it.
    KeptByPolicy,
    /// Not verified (cloud placeholder, unreadable…).
    NotVerified,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Divergence {
    pub kind: DivergenceKind,
    pub rel_path: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct VerifyReport {
    pub level: String,
    pub files_checked: u64,
    pub files_ok: u64,
    pub bytes_checked: u64,
    pub bytes_hashed: u64,
    pub missing: u64,
    pub size_mismatch: u64,
    pub time_mismatch: u64,
    pub hash_mismatch: u64,
    pub kept_by_policy: u64,
    pub not_verified: u64,
    pub errors: u64,
    pub cancelled: bool,
    /// First divergences (capped); all of them go to the callback.
    pub sample: Vec<Divergence>,
    pub elapsed_ms: u64,
}

impl VerifyReport {
    /// True only when every selected file matched. Anything else must not be
    /// presented as "verified".
    pub fn passed(&self) -> bool {
        !self.cancelled && self.missing == 0 && self.size_mismatch == 0 && self.time_mismatch == 0 && self.hash_mismatch == 0 && self.errors == 0
    }
}

fn hash_file(p: &Path, cancel: &AtomicBool, buf: &mut [u8]) -> std::io::Result<Option<blake3::Hash>> {
    let mut f = File::open(p)?;
    let mut h = blake3::Hasher::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let n = f.read(buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(Some(h.finalize()))
}

fn days_ago(now: i64, mtime: Option<i64>) -> Option<i64> {
    mtime.map(|m| (now - m) / 86_400)
}

/// Verifies `job.dest` against `job.source` at `level`.
pub fn verify(
    job: &CopyJob,
    level: VerifyLevel,
    enumerator: &dyn DirEnumerator,
    cancel: &AtomicBool,
    on_divergence: &mut dyn FnMut(&Divergence),
) -> VerifyReport {
    let t0 = std::time::Instant::now();
    let mut rep = VerifyReport { level: format!("{level:?}").to_lowercase(), ..Default::default() };
    if level == VerifyLevel::Basic {
        rep.elapsed_ms = 0;
        return rep;
    }
    let strict = level == VerifyLevel::Strict;
    let hash_threads = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(1, 4);
    std::thread::scope(|scope| {
        // Strict mode: hashing runs on a fixed pool; the job queue is bounded,
        // so files in flight ≤ HASH_QUEUE + hash_threads.
        let (job_tx, job_rx) = crossbeam_channel::bounded::<HashJob>(HASH_QUEUE);
        let (res_tx, res_rx) = crossbeam_channel::unbounded::<HashResult>();
        if strict {
            for _ in 0..hash_threads {
                let (jr, rt) = (job_rx.clone(), res_tx.clone());
                scope.spawn(move || {
                    let mut buf = vec![0u8; 1 << 20];
                    for j in jr {
                        let hs = hash_file(&j.src, cancel, &mut buf);
                        let hd = hash_file(&j.dst, cancel, &mut buf);
                        let outcome = match (hs, hd) {
                            (Ok(Some(a)), Ok(Some(b))) => HashOutcome::Done(a, b),
                            (Ok(None), _) | (_, Ok(None)) => HashOutcome::Cancelled,
                            (Err(e), _) | (_, Err(e)) => HashOutcome::Error(e.to_string()),
                        };
                        if rt.send(HashResult { rel: j.rel, size: j.size, outcome }).is_err() {
                            break;
                        }
                    }
                });
            }
        }
        drop(res_tx);
        drop(job_rx);
        walk(job, level, enumerator, cancel, on_divergence, &mut rep, strict.then_some((&job_tx, &res_rx)));
        drop(job_tx);
        let mut push = |rep: &mut VerifyReport, d: Divergence| push_div(rep, on_divergence, d);
        for r in res_rx.iter() {
            apply_hash(&mut rep, &mut push, r);
        }
    });
    if cancel.load(Ordering::Relaxed) {
        rep.cancelled = true;
    }
    rep.elapsed_ms = t0.elapsed().as_millis() as u64;
    rep
}

const HASH_QUEUE: usize = 256;

struct HashJob {
    src: PathBuf,
    dst: PathBuf,
    size: u64,
    rel: String,
}

enum HashOutcome {
    Done(blake3::Hash, blake3::Hash),
    Cancelled,
    Error(String),
}

struct HashResult {
    rel: String,
    size: u64,
    outcome: HashOutcome,
}

fn push_div(rep: &mut VerifyReport, on_divergence: &mut dyn FnMut(&Divergence), d: Divergence) {
    on_divergence(&d);
    if rep.sample.len() < SAMPLE_CAP {
        rep.sample.push(d);
    }
}

fn apply_hash(rep: &mut VerifyReport, push: &mut dyn FnMut(&mut VerifyReport, Divergence), r: HashResult) {
    match r.outcome {
        HashOutcome::Done(a, b) => {
            rep.bytes_hashed += r.size * 2;
            if a == b {
                rep.files_ok += 1;
            } else {
                rep.hash_mismatch += 1;
                push(rep, Divergence { kind: DivergenceKind::HashMismatch, rel_path: r.rel, detail: format!("blake3 {} ≠ {}", a.to_hex(), b.to_hex()) });
            }
        }
        HashOutcome::Cancelled => rep.cancelled = true,
        HashOutcome::Error(e) => {
            rep.errors += 1;
            push(rep, Divergence { kind: DivergenceKind::Error, rel_path: r.rel, detail: e });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn walk(
    job: &CopyJob,
    level: VerifyLevel,
    enumerator: &dyn DirEnumerator,
    cancel: &AtomicBool,
    on_divergence: &mut dyn FnMut(&Divergence),
    rep: &mut VerifyReport,
    hashing: Option<(&crossbeam_channel::Sender<HashJob>, &crossbeam_channel::Receiver<HashResult>)>,
) {
    let f = &job.options.filters;
    let now = nexttool_core::time::now_unix();
    let mut push = |rep: &mut VerifyReport, d: Divergence| push_div(rep, on_divergence, d);

    // Explicit selection: verify only those files.
    let selection: Option<std::collections::HashSet<PathBuf>> =
        if job.files.is_empty() { None } else { Some(job.files.iter().cloned().collect()) };

    // Whole tree: start at the root. Selection: list only the parent
    // directories of the selected files (no recursion).
    let mut stack: Vec<PathBuf> = match &selection {
        None => vec![PathBuf::new()],
        Some(_) => job.file_groups().into_keys().collect(),
    };
    while let Some(rel) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            rep.cancelled = true;
            break;
        }
        let src_dir = job.source.join(&rel);
        let res = enumerator.enumerate(&src_dir, &mut |e| {
            if cancel.load(Ordering::Relaxed) {
                return false;
            }
            let name = e.name.to_string_lossy();
            let rel_path = rel.join(e.name);
            if e.kind == EntryKind::Dir {
                let is_link = e.flags & (flags::REPARSE | flags::SYMLINK) != 0;
                let excluded = f.exclude_dirs.iter().any(|p| wild_match(p, &name));
                if !(is_link && job.options.exclude_junctions) && !excluded && selection.is_none() {
                    stack.push(rel_path);
                }
                return true;
            }
            if let Some(sel) = &selection {
                if !sel.contains(&rel_path) {
                    return true;
                }
            } else {
                if !f.include_files.is_empty() && !f.include_files.iter().any(|p| wild_match(p, &name)) {
                    return true;
                }
                if f.exclude_files.iter().any(|p| wild_match(p, &name)) {
                    return true;
                }
                if f.min_size.is_some_and(|m| e.size < m) || f.max_size.is_some_and(|m| e.size > m) {
                    return true;
                }
                if let Some(age) = days_ago(now, e.mtime) {
                    if f.max_age_days.is_some_and(|m| age > m as i64) || f.min_age_days.is_some_and(|m| age < m as i64) {
                        return true;
                    }
                }
            }
            rep.files_checked += 1;
            rep.bytes_checked += e.size;
            let rel_s = rel_path.to_string_lossy().into_owned();
            let dst = job.dest.join(&rel_path);
            let md = match std::fs::symlink_metadata(&dst) {
                Ok(m) => m,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    rep.missing += 1;
                    push(rep, Divergence { kind: DivergenceKind::Missing, rel_path: rel_s, detail: String::new() });
                    return true;
                }
                Err(err) => {
                    rep.errors += 1;
                    push(rep, Divergence { kind: DivergenceKind::Error, rel_path: rel_s, detail: err.to_string() });
                    return true;
                }
            };
            let dmtime = md.modified().ok().and_then(system_time_to_unix);
            let size_ok = md.len() == e.size;
            let time_ok = match (e.mtime, dmtime) {
                (Some(a), Some(b)) => (a - b).abs() <= MTIME_TOLERANCE_S,
                _ => true,
            };
            if !size_ok || !time_ok {
                let kept = match job.options.overwrite {
                    OverwritePolicy::NeverReplace => true,
                    OverwritePolicy::ReplaceOlderOnly => matches!((e.mtime, dmtime), (Some(s), Some(d)) if d >= s),
                    OverwritePolicy::ReplaceChanged => false,
                };
                if kept {
                    rep.kept_by_policy += 1;
                    push(rep, Divergence { kind: DivergenceKind::KeptByPolicy, rel_path: rel_s, detail: "destino mantido pela política de sobrescrita".into() });
                } else if !size_ok {
                    rep.size_mismatch += 1;
                    push(rep, Divergence { kind: DivergenceKind::SizeMismatch, rel_path: rel_s, detail: format!("origem {} B, destino {} B", e.size, md.len()) });
                } else {
                    rep.time_mismatch += 1;
                    push(rep, Divergence { kind: DivergenceKind::TimeMismatch, rel_path: rel_s, detail: format!("origem {:?}, destino {:?}", e.mtime, dmtime) });
                }
                return true;
            }
            if level == VerifyLevel::Strict {
                if e.flags & (flags::RECALL_ON_DATA_ACCESS | flags::RECALL_ON_OPEN | flags::OFFLINE) != 0 {
                    rep.not_verified += 1;
                    push(rep, Divergence { kind: DivergenceKind::NotVerified, rel_path: rel_s, detail: "arquivo em nuvem/offline na origem: não aberto para evitar download".into() });
                    return true;
                }
                if let Some((tx, rx)) = hashing {
                    // Drain finished results so they never pile up.
                    while let Ok(r) = rx.try_recv() {
                        apply_hash(rep, &mut push, r);
                    }
                    if tx.send(HashJob { src: src_dir.join(e.name), dst, size: e.size, rel: rel_s }).is_err() {
                        return false;
                    }
                }
                return true;
            }
            rep.files_ok += 1;
            true
        });
        if let Err(e) = res {
            rep.errors += 1;
            push(rep, Divergence { kind: DivergenceKind::Error, rel_path: rel.to_string_lossy().into_owned(), detail: e.to_string() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nextcopy_core::CopyProfile;
    use nexttool_core::fs::PortableEnumerator;
    use std::fs;

    fn setup() -> (tempfile::TempDir, CopyJob) {
        let t = tempfile::tempdir().unwrap();
        let (s, d) = (t.path().join("src"), t.path().join("dst"));
        fs::create_dir_all(s.join("sub")).unwrap();
        fs::write(s.join("a.txt"), b"hello").unwrap();
        fs::write(s.join("sub").join("b.bin"), vec![7u8; 5000]).unwrap();
        fs::write(s.join("skip.tmp"), b"x").unwrap();
        copy_tree(&s, &d);
        let mut job = CopyJob::new(s, d, CopyProfile::Safe);
        job.options.filters.exclude_files = vec!["*.tmp".into()];
        (t, job)
    }

    fn copy_tree(s: &Path, d: &Path) {
        fs::create_dir_all(d).unwrap();
        for e in fs::read_dir(s).unwrap() {
            let e = e.unwrap();
            let to = d.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                copy_tree(&e.path(), &to);
            } else {
                fs::copy(e.path(), &to).unwrap();
                let m = fs::metadata(e.path()).unwrap().modified().unwrap();
                File::options().write(true).open(&to).unwrap().set_modified(m).unwrap();
            }
        }
    }

    #[test]
    fn strict_passes_on_identical_and_detects_corruption() {
        let (_t, job) = setup();
        let no = AtomicBool::new(false);
        fs::remove_file(job.dest.join("skip.tmp")).unwrap(); // excluded by filter: must not be reported
        let r = verify(&job, VerifyLevel::Strict, &PortableEnumerator, &no, &mut |_| {});
        assert!(r.passed(), "{r:?}");
        assert_eq!(r.files_checked, 2);

        // Same size, same mtime, different content: only Strict sees it.
        let p = job.dest.join("sub").join("b.bin");
        let m = fs::metadata(&p).unwrap().modified().unwrap();
        fs::write(&p, vec![8u8; 5000]).unwrap();
        File::options().write(true).open(&p).unwrap().set_modified(m).unwrap();
        assert!(verify(&job, VerifyLevel::Standard, &PortableEnumerator, &no, &mut |_| {}).passed());
        let r = verify(&job, VerifyLevel::Strict, &PortableEnumerator, &no, &mut |_| {});
        assert_eq!(r.hash_mismatch, 1);
        assert!(!r.passed());

        fs::remove_file(job.dest.join("a.txt")).unwrap();
        let r = verify(&job, VerifyLevel::Standard, &PortableEnumerator, &no, &mut |_| {});
        assert_eq!(r.missing, 1);
    }
}
