//! Read-only directory enumeration shared by NextSize (scan) and NextCopy
//! (verification). Enumerators never open file contents, never follow links
//! and never modify anything.

use std::ffi::OsStr;
use std::path::Path;

use crate::error::ErrorKind;

mod portable;
#[cfg(windows)]
pub mod win;

pub use portable::PortableEnumerator;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EntryKind {
    File = 0,
    Dir = 1,
    /// Symbolic link (never followed).
    Symlink = 2,
    /// Device, socket, fifo or anything else.
    Other = 3,
}

impl EntryKind {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => EntryKind::File,
            1 => EntryKind::Dir,
            2 => EntryKind::Symlink,
            _ => EntryKind::Other,
        }
    }
}

/// Portable attribute flags. Platform attributes are mapped onto these bits.
pub mod flags {
    pub const HIDDEN: u32 = 1 << 0;
    pub const SYSTEM: u32 = 1 << 1;
    pub const READONLY: u32 = 1 << 2;
    /// Windows reparse point (junction, symlink, mount point, cloud placeholder…).
    pub const REPARSE: u32 = 1 << 3;
    pub const SPARSE: u32 = 1 << 4;
    pub const COMPRESSED: u32 = 1 << 5;
    pub const OFFLINE: u32 = 1 << 6;
    /// Cloud placeholder: opening would trigger a download. Never opened.
    pub const RECALL_ON_OPEN: u32 = 1 << 7;
    pub const RECALL_ON_DATA_ACCESS: u32 = 1 << 8;
    /// The name was not valid Unicode and was stored with replacement characters.
    pub const NAME_LOSSY: u32 = 1 << 9;
    pub const ENCRYPTED: u32 = 1 << 10;
    /// Directory intentionally not traversed (link, junction, mount point).
    pub const NOT_FOLLOWED: u32 = 1 << 11;
    /// Directory on another filesystem than the scan root.
    pub const OTHER_MOUNT: u32 = 1 << 12;
    pub const SYMLINK: u32 = 1 << 13;
    /// Directory could not be fully enumerated.
    pub const ENUM_ERROR: u32 = 1 << 14;
}

/// Borrowed entry produced by an enumerator. Valid only during the callback.
#[derive(Debug, Clone)]
pub struct EntryRef<'a> {
    pub name: &'a OsStr,
    pub kind: EntryKind,
    /// Logical size in bytes (0 for directories).
    pub size: u64,
    /// Allocated size in bytes, when reported cheaply by the platform.
    pub alloc: Option<u64>,
    /// Unix seconds. `None` when not available.
    pub mtime: Option<i64>,
    pub ctime: Option<i64>,
    pub atime: Option<i64>,
    pub flags: u32,
    /// Windows reparse tag (0 when not a reparse point).
    pub reparse_tag: u32,
    /// Inode / file reference number, when available.
    pub file_id: Option<u64>,
    pub nlink: Option<u32>,
    /// Device id (Unix `st_dev`) for mount boundary detection.
    pub dev: Option<u64>,
}

#[derive(Debug, Clone, thiserror::Error)]
#[error("{kind:?} (os code {os_code:?}): {message}")]
pub struct EnumError {
    pub kind: ErrorKind,
    pub os_code: Option<i32>,
    pub message: String,
    /// Some entries were delivered before the error happened.
    pub partial: bool,
}

/// Lists the direct children of one directory. Implementations must skip
/// `.`/`..`, never follow links, and stop as soon as the callback returns false.
pub trait DirEnumerator: Send + Sync {
    fn name(&self) -> &'static str;

    fn enumerate(&self, dir: &Path, on_entry: &mut dyn FnMut(&EntryRef<'_>) -> bool) -> Result<(), EnumError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// Native API on this platform.
    #[default]
    Auto,
    /// Windows: FindFirstFileExW (FindExInfoBasic + LARGE_FETCH).
    Native,
    /// Rust std::fs (works everywhere; benchmark baseline).
    Portable,
}

/// Picks the enumerator for a backend on this platform.
pub fn make_enumerator(backend: Backend) -> std::sync::Arc<dyn DirEnumerator> {
    match backend {
        Backend::Portable => std::sync::Arc::new(PortableEnumerator),
        #[cfg(windows)]
        Backend::Auto | Backend::Native => std::sync::Arc::new(win::FindEnumerator),
        #[cfg(not(windows))]
        Backend::Auto | Backend::Native => std::sync::Arc::new(PortableEnumerator),
    }
}

/// True for UNC paths and mapped network drives (Windows). Unix: false
/// (network mounts are detected per filesystem type in a later phase).
pub fn is_remote_path(path: &Path) -> bool {
    #[cfg(windows)]
    {
        win::is_remote_path(path)
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// Lowers the calling thread's CPU and I/O priority (Economy profile).
pub fn enter_background_mode() -> bool {
    #[cfg(windows)]
    {
        win::enter_background_mode()
    }
    #[cfg(not(windows))]
    {
        false
    }
}
