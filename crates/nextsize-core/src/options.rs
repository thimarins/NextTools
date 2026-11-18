use std::path::PathBuf;

use nexttool_core::fs::Backend;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Low concurrency, background I/O priority. For production servers.
    Economy,
    /// Recommended default.
    #[default]
    Balanced,
    /// Uses more resources. For local disks and maintenance windows.
    Fast,
}

impl Profile {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "economy" | "eco" | "economico" => Some(Profile::Economy),
            "balanced" | "equilibrado" => Some(Profile::Balanced),
            "fast" | "rapido" | "alto" => Some(Profile::Fast),
            _ => None,
        }
    }
}

/// Concrete resource limits derived from a profile. Every queue and buffer in
/// the engine is bounded by one of these numbers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tuning {
    /// Enumeration (I/O) threads.
    pub threads: usize,
    /// Max entries per chunk sent to the writer.
    pub chunk_size: usize,
    /// Max chunks waiting for the writer (backpressure).
    pub channel_cap: usize,
    /// Max pending directories kept in RAM; the rest spills to disk.
    pub frontier_mem_cap: usize,
    /// Commit (checkpoint) after this many rows…
    pub commit_rows: usize,
    /// …or after this many milliseconds.
    pub commit_interval_ms: u64,
    /// SQLite page cache in KiB.
    pub sqlite_cache_kib: u64,
    /// Run enumeration threads in background (low CPU and I/O priority) mode.
    pub background_priority: bool,
    /// Adapt concurrency to the server (network sources). `threads` is then
    /// the starting point and `max_threads` the ceiling.
    #[serde(default)]
    pub adaptive: bool,
    #[serde(default)]
    pub max_threads: usize,
    #[serde(default = "default_interval")]
    pub adaptive_interval_ms: u64,
}

fn default_interval() -> u64 {
    2_000
}

impl Tuning {
    /// Threads to spawn: the ceiling when adaptive, else the fixed count.
    pub fn spawn_threads(&self) -> usize {
        if self.adaptive {
            self.max_threads.max(self.threads).max(1)
        } else {
            self.threads.max(1)
        }
    }

    pub fn for_profile(profile: Profile, remote: bool, cores: usize) -> Tuning {
        let mut t = Self::base(profile, remote, cores);
        if remote {
            // Network sources: start conservative, adapt to the server.
            t.adaptive = true;
            t.max_threads = match profile {
                Profile::Economy => 4,
                Profile::Balanced => 16,
                Profile::Fast => 32,
            };
        } else {
            t.max_threads = t.threads;
        }
        t
    }

    fn base(profile: Profile, remote: bool, cores: usize) -> Tuning {
        let cores = cores.max(1);
        match profile {
            Profile::Economy => Tuning {
                threads: 2,
                chunk_size: 1024,
                channel_cap: 8,
                frontier_mem_cap: 8 * 1024,
                commit_rows: 20_000,
                commit_interval_ms: 2_000,
                sqlite_cache_kib: 8 * 1024,
                background_priority: true,
                adaptive: false,
                max_threads: 0,
                adaptive_interval_ms: 2_000,
            },
            Profile::Balanced => Tuning {
                threads: if remote { 4 } else { cores.clamp(2, 8) },
                chunk_size: 4096,
                channel_cap: 32,
                frontier_mem_cap: 32 * 1024,
                commit_rows: 100_000,
                commit_interval_ms: 1_000,
                sqlite_cache_kib: 32 * 1024,
                background_priority: false,
                adaptive: false,
                max_threads: 0,
                adaptive_interval_ms: 2_000,
            },
            Profile::Fast => Tuning {
                threads: if remote { 16 } else { (cores * 2).clamp(4, 32) },
                chunk_size: 8192,
                channel_cap: 64,
                frontier_mem_cap: 64 * 1024,
                commit_rows: 250_000,
                commit_interval_ms: 1_000,
                sqlite_cache_kib: 128 * 1024,
                background_priority: false,
                adaptive: false,
                max_threads: 0,
                adaptive_interval_ms: 2_000,
            },
        }
    }

    /// Rough upper bound of RAM held by engine-managed queues (not the process).
    pub fn queue_budget_bytes(&self) -> u64 {
        // ~120 bytes per in-flight entry record (struct + short name),
        // ~300 bytes per in-memory pending directory (task + path).
        let chunks = (self.channel_cap + self.threads) as u64 * self.chunk_size as u64 * 120;
        let frontier = self.frontier_mem_cap as u64 * 300;
        chunks + frontier + self.sqlite_cache_kib * 1024
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOptions {
    pub root: PathBuf,
    pub profile: Profile,
    pub backend: Backend,
    pub tuning: Tuning,
    /// Traverse into other mounted filesystems (Unix). Off by default.
    pub cross_mounts: bool,
    /// Where spilled frontier files go (defaults to the system temp dir).
    pub spill_dir: Option<PathBuf>,
}
