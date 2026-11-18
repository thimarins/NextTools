//! NextTool shared core. Small on purpose: only what both NextSize and
//! NextCopy need. No engine logic and no persistence live here.
//!
//! - `error`: platform-neutral error classification (shared by scan and copy).
//! - `fs`: read-only directory enumeration (Win32 FindFirstFileExW / std).
//! - `task`, `alert`: data models used by the task manager, store and UI.
//! - `security`: path validation and overlap checks.

pub mod alert;
pub mod error;
pub mod fs;
pub mod process;
pub mod security;
pub mod task;
pub mod time;
pub mod units;

pub use error::{classify_os_error, ErrorKind, Severity};
