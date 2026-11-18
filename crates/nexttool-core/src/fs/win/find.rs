use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;

use crate::error::classify_os_error;
use crate::fs::{flags as f, DirEnumerator, EntryKind, EntryRef, EnumError};
use crate::time::filetime_to_unix;
use windows_sys::Win32::Foundation::{GetLastError, FILETIME, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    FindClose, FindExInfoBasic, FindExSearchNameMatch, FindFirstFileExW, FindNextFileW, FIND_FIRST_EX_LARGE_FETCH,
    WIN32_FIND_DATAW,
};

use super::path::to_extended_wide;

// File attribute bits (winnt.h). Declared locally to stay independent of
// windows-sys type aliases.
const A_READONLY: u32 = 0x0000_0001;
const A_HIDDEN: u32 = 0x0000_0002;
const A_SYSTEM: u32 = 0x0000_0004;
const A_DIRECTORY: u32 = 0x0000_0010;
const A_DEVICE: u32 = 0x0000_0040;
const A_SPARSE: u32 = 0x0000_0200;
const A_REPARSE: u32 = 0x0000_0400;
const A_COMPRESSED: u32 = 0x0000_0800;
const A_OFFLINE: u32 = 0x0000_1000;
const A_ENCRYPTED: u32 = 0x0000_4000;
const A_RECALL_ON_OPEN: u32 = 0x0004_0000;
const A_RECALL_ON_DATA_ACCESS: u32 = 0x0040_0000;

const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;

const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_NO_MORE_FILES: u32 = 18;

/// `FindFirstFileExW`-based enumerator (Windows "compatible" mode).
#[derive(Debug, Default, Clone, Copy)]
pub struct FindEnumerator;

struct FindHandle(*mut core::ffi::c_void);

impl Drop for FindHandle {
    fn drop(&mut self) {
        // SAFETY: handle came from a successful FindFirstFileExW.
        unsafe {
            FindClose(self.0 as _);
        }
    }
}

fn ft(t: &FILETIME) -> Option<i64> {
    filetime_to_unix(((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64)
}

fn map_error(code: u32, partial: bool) -> EnumError {
    EnumError {
        kind: classify_os_error(code),
        os_code: Some(code as i32),
        message: std::io::Error::from_raw_os_error(code as i32).to_string(),
        partial,
    }
}

impl DirEnumerator for FindEnumerator {
    fn name(&self) -> &'static str {
        "win32-find"
    }

    fn enumerate(&self, dir: &Path, on_entry: &mut dyn FnMut(&EntryRef<'_>) -> bool) -> Result<(), EnumError> {
        let pattern = to_extended_wide(dir, r"\*");
        // SAFETY: zeroed POD struct, filled by the API.
        let mut data: WIN32_FIND_DATAW = unsafe { std::mem::zeroed() };
        // SAFETY: pattern is NUL-terminated and alive; data is a valid out pointer.
        let h = unsafe {
            FindFirstFileExW(
                pattern.as_ptr(),
                FindExInfoBasic,
                &mut data as *mut _ as *mut _,
                FindExSearchNameMatch,
                std::ptr::null(),
                FIND_FIRST_EX_LARGE_FETCH,
            )
        };
        if h == INVALID_HANDLE_VALUE {
            // SAFETY: trivially safe.
            let code = unsafe { GetLastError() };
            // An empty volume root returns FILE_NOT_FOUND instead of "." entries.
            if code == ERROR_FILE_NOT_FOUND && dir.parent().is_none() {
                return Ok(());
            }
            return Err(map_error(code, false));
        }
        let handle = FindHandle(h as _);
        let mut delivered = false;

        loop {
            let len = data.cFileName.iter().position(|&c| c == 0).unwrap_or(data.cFileName.len());
            let wname = &data.cFileName[..len];
            let is_dot = wname == [b'.' as u16] || wname == [b'.' as u16, b'.' as u16];
            if !is_dot {
                let a = data.dwFileAttributes;
                let mut fl = 0u32;
                if a & A_HIDDEN != 0 { fl |= f::HIDDEN; }
                if a & A_SYSTEM != 0 { fl |= f::SYSTEM; }
                if a & A_READONLY != 0 { fl |= f::READONLY; }
                if a & A_SPARSE != 0 { fl |= f::SPARSE; }
                if a & A_COMPRESSED != 0 { fl |= f::COMPRESSED; }
                if a & A_OFFLINE != 0 { fl |= f::OFFLINE; }
                if a & A_ENCRYPTED != 0 { fl |= f::ENCRYPTED; }
                if a & A_RECALL_ON_OPEN != 0 { fl |= f::RECALL_ON_OPEN; }
                if a & A_RECALL_ON_DATA_ACCESS != 0 { fl |= f::RECALL_ON_DATA_ACCESS; }
                // dwReserved0 holds the reparse tag only when the attribute is set.
                let tag = if a & A_REPARSE != 0 { fl |= f::REPARSE; data.dwReserved0 } else { 0 };
                if tag == IO_REPARSE_TAG_SYMLINK {
                    fl |= f::SYMLINK;
                }
                let kind = if a & A_DIRECTORY != 0 {
                    EntryKind::Dir
                } else if tag == IO_REPARSE_TAG_SYMLINK {
                    EntryKind::Symlink
                } else if a & A_DEVICE != 0 {
                    EntryKind::Other
                } else {
                    EntryKind::File
                };
                let size = if kind == EntryKind::Dir {
                    0
                } else {
                    ((data.nFileSizeHigh as u64) << 32) | data.nFileSizeLow as u64
                };
                let name = OsString::from_wide(wname);
                let e = EntryRef {
                    name: &name,
                    kind,
                    size,
                    alloc: None, // not reported by FindFirstFile; see ADR-007
                    mtime: ft(&data.ftLastWriteTime),
                    ctime: ft(&data.ftCreationTime),
                    atime: ft(&data.ftLastAccessTime),
                    flags: fl,
                    reparse_tag: tag,
                    file_id: None,
                    nlink: None,
                    dev: None,
                };
                delivered = true;
                if !on_entry(&e) {
                    return Ok(());
                }
            }
            // SAFETY: valid handle and out pointer.
            let ok = unsafe { FindNextFileW(handle.0 as _, &mut data) };
            if ok == 0 {
                // SAFETY: trivially safe.
                let code = unsafe { GetLastError() };
                if code == ERROR_NO_MORE_FILES {
                    return Ok(());
                }
                return Err(map_error(code, delivered));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn lists_files_dirs_and_sizes() {
        let t = tempfile::tempdir().unwrap();
        fs::write(t.path().join("a.txt"), b"hello").unwrap();
        fs::create_dir(t.path().join("sub")).unwrap();
        fs::write(t.path().join("ação ✓.bin"), vec![0u8; 1000]).unwrap();
        let mut seen = Vec::new();
        FindEnumerator
            .enumerate(t.path(), &mut |e| {
                seen.push((e.name.to_string_lossy().into_owned(), e.kind, e.size, e.mtime.is_some()));
                true
            })
            .unwrap();
        seen.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[0], ("a.txt".into(), EntryKind::File, 5, true));
        assert_eq!(seen[1], ("ação ✓.bin".into(), EntryKind::File, 1000, true));
        assert_eq!(seen[2].1, EntryKind::Dir);
    }

    #[test]
    fn long_paths_work() {
        let t = tempfile::tempdir().unwrap();
        let mut p = t.path().to_path_buf();
        for i in 0..30 {
            p.push(format!("very_long_directory_name_number_{i:03}"));
        }
        assert!(p.as_os_str().len() > 600);
        let wide = to_extended_wide(&p, "");
        let parent_ok = std::fs::create_dir_all(Path::new(&String::from_utf16_lossy(&wide[..wide.len() - 1])));
        parent_ok.unwrap();
        let mut n = 0;
        FindEnumerator.enumerate(p.parent().unwrap(), &mut |_| { n += 1; true }).unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn missing_dir_is_not_found() {
        let t = tempfile::tempdir().unwrap();
        let err = FindEnumerator.enumerate(&t.path().join("nope"), &mut |_| true).unwrap_err();
        assert_eq!(err.kind, crate::error::ErrorKind::NotFound);
    }
}
