use std::sync::atomic::{AtomicU64, AtomicU8, Ordering::Relaxed};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u8)]
pub enum ScanState {
    Preparing = 0,
    Scanning = 1,
    Paused = 2,
    Cancelling = 3,
    Finalizing = 4,
    /// Stopped before the end (cancel or crash); resumable.
    Interrupted = 5,
    Completed = 6,
    CompletedWithWarnings = 7,
    Failed = 8,
}

impl ScanState {
    pub fn from_u8(v: u8) -> Self {
        use ScanState::*;
        match v {
            0 => Preparing,
            1 => Scanning,
            2 => Paused,
            3 => Cancelling,
            4 => Finalizing,
            5 => Interrupted,
            6 => Completed,
            7 => CompletedWithWarnings,
            _ => Failed,
        }
    }

    pub fn as_str(self) -> &'static str {
        use ScanState::*;
        match self {
            Preparing => "preparing",
            Scanning => "scanning",
            Paused => "paused",
            Cancelling => "cancelling",
            Finalizing => "finalizing",
            Interrupted => "interrupted",
            Completed => "completed",
            CompletedWithWarnings => "completed_with_warnings",
            Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        (0..=8).map(ScanState::from_u8).find(|st| st.as_str() == s)
    }

    pub fn is_terminal_ok(self) -> bool {
        matches!(self, ScanState::Completed | ScanState::CompletedWithWarnings)
    }
}

/// Lock-free progress counters shared between engine, writer and UI.
/// Readers take a consistent-enough snapshot; counters are monotonic.
#[derive(Debug, Default)]
pub struct ScanProgress {
    state: AtomicU8,
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub issues: AtomicU64,
    pub pending_dirs: AtomicU64,
    pub spilled_dirs: AtomicU64,
    pub rows_committed: AtomicU64,
    pub commits: AtomicU64,
    pub queued_chunks: AtomicU64,
    /// Total time enumeration workers spent blocked because the writer queue
    /// was full (backpressure). High values = persistence is the bottleneck.
    pub send_blocked_ns: AtomicU64,
    /// Total time the writer waited for data. High values = enumeration
    /// (disk/network) is the bottleneck.
    pub writer_wait_ns: AtomicU64,
    /// Enumeration errors classified as network/timeout (adaptive control).
    pub net_errors: AtomicU64,
    /// Current number of active enumeration threads.
    pub concurrency: AtomicU64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProgressSnapshot {
    pub state: ScanState,
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub issues: u64,
    pub pending_dirs: u64,
    pub spilled_dirs: u64,
    pub rows_committed: u64,
    pub commits: u64,
    pub queued_chunks: u64,
    pub concurrency: u64,
}

impl ScanProgress {
    pub fn set_state(&self, s: ScanState) {
        self.state.store(s as u8, Relaxed);
    }

    pub fn state(&self) -> ScanState {
        ScanState::from_u8(self.state.load(Relaxed))
    }

    pub fn snapshot(&self) -> ProgressSnapshot {
        ProgressSnapshot {
            state: self.state(),
            files: self.files.load(Relaxed),
            dirs: self.dirs.load(Relaxed),
            bytes: self.bytes.load(Relaxed),
            issues: self.issues.load(Relaxed),
            pending_dirs: self.pending_dirs.load(Relaxed),
            spilled_dirs: self.spilled_dirs.load(Relaxed),
            rows_committed: self.rows_committed.load(Relaxed),
            commits: self.commits.load(Relaxed),
            queued_chunks: self.queued_chunks.load(Relaxed),
            concurrency: self.concurrency.load(Relaxed),
        }
    }
}
