use std::path::Path;

use super::{flags as f, DirEnumerator, EntryKind, EntryRef, EnumError};
use crate::error::classify_io;
use crate::time::system_time_to_unix;

/// `std::fs::read_dir` enumerator. On Windows std fills metadata from the
/// find data (no extra syscall); on Unix it costs one `statx`/`lstat` per entry.
#[derive(Debug, Default, Clone, Copy)]
pub struct PortableEnumerator;

fn err(e: std::io::Error, partial: bool) -> EnumError {
    EnumError { kind: classify_io(&e), os_code: e.raw_os_error(), message: e.to_string(), partial }
}

impl DirEnumerator for PortableEnumerator {
    fn name(&self) -> &'static str {
        "std-readdir"
    }

    fn enumerate(&self, dir: &Path, on_entry: &mut dyn FnMut(&EntryRef<'_>) -> bool) -> Result<(), EnumError> {
        let rd = std::fs::read_dir(dir).map_err(|e| err(e, false))?;
        let mut delivered = false;
        for ent in rd {
            let ent = ent.map_err(|e| err(e, delivered))?;
            // symlink_metadata semantics: links are not followed.
            let md = match ent.metadata() {
                Ok(m) => m,
                // Entry vanished between listing and stat: skip it, it no longer exists.
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(err(e, delivered)),
            };
            let ft = md.file_type();
            let mut fl = 0u32;
            let kind = if ft.is_symlink() {
                fl |= f::SYMLINK;
                EntryKind::Symlink
            } else if ft.is_dir() {
                EntryKind::Dir
            } else if ft.is_file() {
                EntryKind::File
            } else {
                EntryKind::Other
            };
            #[allow(unused_mut)]
            let (mut alloc, mut file_id, mut nlink, mut dev, mut tag) = (None, None, None, None, 0u32);
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                let a = md.file_attributes();
                if a & 0x2 != 0 { fl |= f::HIDDEN; }
                if a & 0x4 != 0 { fl |= f::SYSTEM; }
                if a & 0x1 != 0 { fl |= f::READONLY; }
                if a & 0x400 != 0 { fl |= f::REPARSE; tag = 1; }
                if a & 0x0004_0000 != 0 { fl |= f::RECALL_ON_OPEN; }
                if a & 0x0040_0000 != 0 { fl |= f::RECALL_ON_DATA_ACCESS; }
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                alloc = Some(md.blocks().saturating_mul(512));
                file_id = Some(md.ino());
                nlink = u32::try_from(md.nlink()).ok();
                dev = Some(md.dev());
                if ent.file_name().as_encoded_bytes().first() == Some(&b'.') {
                    fl |= f::HIDDEN;
                }
            }
            let name = ent.file_name();
            let e = EntryRef {
                name: &name,
                kind,
                size: if kind == EntryKind::Dir { 0 } else { md.len() },
                alloc,
                mtime: md.modified().ok().and_then(system_time_to_unix),
                ctime: md.created().ok().and_then(system_time_to_unix),
                atime: md.accessed().ok().and_then(system_time_to_unix),
                flags: fl,
                reparse_tag: tag,
                file_id,
                nlink,
                dev,
            };
            delivered = true;
            if !on_entry(&e) {
                return Ok(());
            }
        }
        Ok(())
    }
}
