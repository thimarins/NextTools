use std::sync::atomic::{AtomicBool, Ordering};

use nexttool_core::ErrorKind;
use serde::{Deserialize, Serialize};

/// Pause / cancel requests for a running copy. With Robocopy, both stop the
/// process; "pause" means the task is resumable by re-running (Robocopy skips
/// files already identical in the destination).
#[derive(Debug, Default)]
pub struct CopyControl {
    cancel: AtomicBool,
    pause: AtomicBool,
}

impl CopyControl {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
    pub fn pause(&self) {
        self.pause.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    pub fn is_pause_requested(&self) -> bool {
        self.pause.load(Ordering::Relaxed)
    }
    pub fn stop_requested(&self) -> bool {
        self.is_cancelled() || self.is_pause_requested()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyStats {
    pub files_total: u64,
    pub files_copied: u64,
    pub files_skipped: u64,
    pub files_mismatch: u64,
    pub files_failed: u64,
    pub files_extra: u64,
    pub dirs_total: u64,
    pub dirs_copied: u64,
    pub dirs_failed: u64,
    pub bytes_total: u64,
    pub bytes_copied: u64,
    pub bytes_failed: u64,
    /// Number of retry events seen in the log.
    pub retries: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileError {
    pub os_code: u32,
    pub kind: ErrorKind,
    pub path: String,
    /// Final failure (retries exhausted) vs. a transient retry.
    pub final_failure: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CopyEvent {
    Started { pid: u32, run_index: usize, runs: usize },
    Progress { files_done: u64, bytes_done: u64, errors: u64, io_read: u64, io_write: u64, log_bytes: u64 },
    Error(FileError),
    Finished { exit_code: Option<i32> },
}

/// Overall outcome. Anything not fully successful is never "Completed".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CopyOutcome {
    /// Everything copied (or already identical); no failures.
    Completed,
    /// Completed but with mismatches/extras worth reviewing; no failures.
    CompletedWithWarnings,
    /// Some files or directories failed to copy.
    PartialFailure,
    /// Engine-level fatal error (usage error, source/dest unavailable…).
    Failed,
    /// Stopped by pause request; re-run continues.
    Paused,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopyResult {
    pub outcome: CopyOutcome,
    pub stats: CopyStats,
    /// Exit code of each engine run (None = terminated by us).
    pub exit_codes: Vec<Option<i32>>,
    /// Dominant error class when failed (drives retry policy and alerts).
    pub dominant_error: Option<ErrorKind>,
    pub errors_sample: Vec<FileError>,
    pub log_files: Vec<String>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DryRunReport {
    pub would_copy_files: u64,
    pub would_copy_bytes: u64,
    /// Files and directories that would be DELETED in the destination (mirror).
    pub would_delete_files: u64,
    pub would_delete_dirs: u64,
    pub delete_sample: Vec<String>,
    pub errors: u64,
    pub exit_code: Option<i32>,
    pub log_file: String,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("tarefa inválida: {0}")]
    InvalidJob(String),
    #[error("motor indisponível: {0}")]
    Unavailable(String),
    #[error("espelhamento exige aprovação de exclusões após prévia (dry-run)")]
    MirrorNotApproved,
    #[error("a prévia atual encontrou {found} exclusões, mas foram aprovadas {approved}; refaça a prévia")]
    MirrorApprovalMismatch { approved: u64, found: u64 },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}
