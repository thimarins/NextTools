//! Robocopy engine for NextCopy (Windows).
//!
//! Design rules (ADR-010/011):
//! - `Robocopy.exe` is started from `%SystemRoot%\System32` by absolute path,
//!   with structured arguments, never through a shell.
//! - Truth comes first from the exit code (bitmask), then from the job
//!   summary *by position* (Dirs/Files/Bytes rows), then from per-line data
//!   that does not depend on the UI language (sizes, paths, `(0x……)` codes).
//!   Localized labels ("New File", "Novo arquivo"…) are never interpreted.
//! - Pause = stop the process; resume = run again (Robocopy skips files that
//!   are already identical). Mid-file resume only with restartable mode (/Z).

pub mod args;
pub mod exit;
pub mod log;
#[cfg(windows)]
mod runner;

#[cfg(windows)]
pub use runner::RobocopyEngine;
