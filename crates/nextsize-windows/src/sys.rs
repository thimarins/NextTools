use std::path::{Component, Path, Prefix};

use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
use windows_sys::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};

const DRIVE_REMOTE: u32 = 4;

/// Puts the calling thread in background processing mode: low CPU priority
/// and very low I/O and memory priority. Used by the Economy profile so scans
/// on production servers yield to real workloads.
pub fn enter_background_mode() -> bool {
    // SAFETY: pseudo-handle of the current thread; no memory is passed.
    unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN as i32) != 0 }
}

/// True for UNC paths and mapped network drives.
pub fn is_remote_path(path: &Path) -> bool {
    match path.components().next() {
        Some(Component::Prefix(p)) => match p.kind() {
            Prefix::UNC(..) | Prefix::VerbatimUNC(..) => true,
            Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => {
                let root: Vec<u16> = format!("{}:\\", letter as char).encode_utf16().chain([0]).collect();
                // SAFETY: NUL-terminated buffer that outlives the call.
                unsafe { GetDriveTypeW(root.as_ptr()) == DRIVE_REMOTE }
            }
            _ => false,
        },
        _ => false,
    }
}
