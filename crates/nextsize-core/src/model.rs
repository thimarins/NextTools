use std::ffi::OsStr;

/// Internal node id. Unique per snapshot across files and directories.
pub type NodeId = u64;

/// Id of the scan root directory in every snapshot.
pub const ROOT_ID: NodeId = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum EntryKind {
    File = 0,
    Dir = 1,
    /// Symbolic link to a file (never followed).
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

/// Portable attribute flags stored per entry. Platform specific attributes are
/// mapped onto these bits by each enumerator.
pub mod flags {
    pub const HIDDEN: u32 = 1 << 0;
    pub const SYSTEM: u32 = 1 << 1;
    pub const READONLY: u32 = 1 << 2;
    /// Windows reparse point (junction, symlink, mount point, cloud placeholder…).
    pub const REPARSE: u32 = 1 << 3;
    pub const SPARSE: u32 = 1 << 4;
    pub const COMPRESSED: u32 = 1 << 5;
    pub const OFFLINE: u32 = 1 << 6;
    /// Cloud placeholder: opening would trigger a download. Never opened by NextSize.
    pub const RECALL_ON_OPEN: u32 = 1 << 7;
    pub const RECALL_ON_DATA_ACCESS: u32 = 1 << 8;
    /// The name was not valid Unicode and was stored with replacement characters.
    pub const NAME_LOSSY: u32 = 1 << 9;
    pub const ENCRYPTED: u32 = 1 << 10;
    /// Directory that was intentionally not traversed (link, junction, mount point).
    pub const NOT_FOLLOWED: u32 = 1 << 11;
    /// Directory lives on another filesystem/mount than the scan root.
    pub const OTHER_MOUNT: u32 = 1 << 12;
    /// Symlink (Unix) or symlink reparse tag (Windows).
    pub const SYMLINK: u32 = 1 << 13;
    /// Directory could not be fully enumerated (see issues table).
    pub const ENUM_ERROR: u32 = 1 << 14;
}

/// Borrowed entry produced by an enumerator. Valid only during the callback.
#[derive(Debug, Clone)]
pub struct EntryRef<'a> {
    pub name: &'a OsStr,
    pub kind: EntryKind,
    /// Logical size in bytes (0 for directories).
    pub size: u64,
    /// Allocated size in bytes, when the platform reports it cheaply.
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
    /// Hard link count, when available.
    pub nlink: Option<u32>,
    /// Device id (Unix `st_dev`), used for mount boundary detection.
    pub dev: Option<u64>,
}

/// Owned, compact entry record sent to the persistence layer.
#[derive(Debug, Clone)]
pub struct EntryRecord {
    pub id: NodeId,
    pub name: Box<str>,
    pub kind: EntryKind,
    pub size: u64,
    pub alloc: Option<u64>,
    pub mtime: Option<i64>,
    pub ctime: Option<i64>,
    pub atime: Option<i64>,
    pub flags: u32,
    pub file_id: Option<u64>,
    pub nlink: Option<u32>,
}

/// A bounded slice of one directory's entries. A directory with many entries
/// is delivered as several chunks; the final one has `last = true`.
///
/// Invariant: chunks of one directory arrive in order and only the producer of
/// that directory sends them. A directory is complete only once its `last`
/// chunk has been committed.
#[derive(Debug)]
pub struct DirChunk {
    pub dir_id: NodeId,
    pub dir_depth: u32,
    pub entries: Vec<EntryRecord>,
    pub last: bool,
    /// Set on the last chunk when enumeration stopped with an error.
    pub enum_error: bool,
}
