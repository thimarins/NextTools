//! Windows enumeration. Phase 0 provides the "compatible" mode:
//! `FindFirstFileExW` + `FindExInfoBasic` + `FIND_FIRST_EX_LARGE_FETCH`.
//! No privileges required; works on local volumes and SMB (UNC) paths.
//! NTFS fast mode (MFT / USN) is Phase 5.

mod find;
mod owner;
mod path;
mod sys;

pub use find::FindEnumerator;
pub use owner::{Owner, OwnerResolver};
pub use path::to_extended_wide;
pub use sys::{enter_background_mode, is_remote_path};
