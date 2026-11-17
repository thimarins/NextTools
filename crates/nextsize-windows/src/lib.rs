//! Windows-specific scanning backends.
//!
//! Phase 0 provides the "compatible" mode: `FindFirstFileExW` with
//! `FindExInfoBasic` + `FIND_FIRST_EX_LARGE_FETCH`. It needs no privileges and
//! works on local NTFS/ReFS/FAT and on SMB shares (UNC paths).
//! The NTFS fast mode (MFT / USN enumeration) is Phase 3.

#[cfg(windows)]
mod find;
#[cfg(windows)]
mod path;
#[cfg(windows)]
mod sys;

#[cfg(windows)]
pub use find::FindEnumerator;
#[cfg(windows)]
pub use path::to_extended_wide;
#[cfg(windows)]
pub use sys::{enter_background_mode, is_remote_path};

/// Non-Windows stubs so dependents compile everywhere.
#[cfg(not(windows))]
pub fn is_remote_path(_p: &std::path::Path) -> bool {
    false
}
#[cfg(not(windows))]
pub fn enter_background_mode() -> bool {
    false
}
