//! In-memory synthetic directory tree for **storage-layer** benchmarks.
//!
//! It isolates the cost of the pipeline + SQLite (ingestion, checkpoints,
//! indexes, roll-up) from filesystem I/O, so 10 M+ record runs do not need
//! millions of real files (which would permanently grow the NTFS MFT).
//! Results are always labeled `synthetic-index` and are NOT filesystem scan
//! numbers.
//!
//! Tree: every directory at depth < `levels` has `fanout` subdirectories;
//! every directory has `files` files. Depth is derived from the path.

use std::ffi::OsString;
use std::path::Path;

use nexttool_core::fs::{DirEnumerator, EntryKind, EntryRef, EnumError};

pub struct SynthEnumerator {
    pub root_depth: usize,
    pub levels: usize,
    pub fanout: usize,
    pub files: usize,
}

const EXTS: [&str; 12] = ["txt", "log", "pdf", "docx", "xlsx", "jpg", "png", "bak", "dat", "csv", "mp4", "zip"];

impl SynthEnumerator {
    pub fn new(root: &Path, levels: usize, fanout: usize, files: usize) -> Self {
        SynthEnumerator { root_depth: root.components().count(), levels, fanout, files }
    }

    pub fn totals(&self) -> (u64, u64) {
        // dirs = sum_{d=0..levels} fanout^d ; files = dirs * files
        let mut dirs = 0u64;
        let mut level = 1u64;
        for _ in 0..=self.levels {
            dirs += level;
            level *= self.fanout as u64;
        }
        (dirs * self.files as u64, dirs)
    }
}

fn mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

impl DirEnumerator for SynthEnumerator {
    fn name(&self) -> &'static str {
        "synthetic-index"
    }

    fn enumerate(&self, dir: &Path, on_entry: &mut dyn FnMut(&EntryRef<'_>) -> bool) -> Result<(), EnumError> {
        let depth = dir.components().count().saturating_sub(self.root_depth);
        let seed = mix(dir.as_os_str().len() as u64 ^ (depth as u64) << 32 ^ dir.to_string_lossy().bytes().fold(0u64, |a, b| a.wrapping_mul(131).wrapping_add(b as u64)));
        for i in 0..self.files {
            let r = mix(seed ^ i as u64);
            let name = OsString::from(format!("arquivo_{i:05}_{:x}.{}", r & 0xffff, EXTS[(r % EXTS.len() as u64) as usize]));
            // Sizes: mostly small, some large (long tail), deterministic.
            let size = match r % 100 {
                0 => (r >> 8) % (4 << 30),
                1..=9 => (r >> 8) % (64 << 20),
                _ => (r >> 8) % (256 << 10),
            };
            let mtime = 1_262_304_000 + (r >> 16) as i64 % 520_000_000; // 2010..2026
            let e = EntryRef {
                name: &name,
                kind: EntryKind::File,
                size,
                alloc: None,
                mtime: Some(mtime),
                ctime: Some(mtime),
                atime: Some(mtime),
                flags: 0,
                reparse_tag: 0,
                file_id: None,
                nlink: None,
                dev: None,
            };
            if !on_entry(&e) {
                return Ok(());
            }
        }
        if depth < self.levels {
            for d in 0..self.fanout {
                let name = OsString::from(format!("pasta_{d:03}"));
                let e = EntryRef {
                    name: &name,
                    kind: EntryKind::Dir,
                    size: 0,
                    alloc: None,
                    mtime: Some(1_600_000_000),
                    ctime: None,
                    atime: None,
                    flags: 0,
                    reparse_tag: 0,
                    file_id: None,
                    nlink: None,
                    dev: None,
                };
                if !on_entry(&e) {
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}
