use nexttool_core::fs::EntryKind;

/// Internal node id. Unique per snapshot across files and directories.
pub type NodeId = u64;

/// Id of the scan root directory in every snapshot.
pub const ROOT_ID: NodeId = 1;

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
/// A directory is complete only once its `last` chunk has been committed.
/// If the process stops before that, the directory (and anything below it)
/// is discarded and re-enumerated on resume.
#[derive(Debug)]
pub struct DirChunk {
    pub dir_id: NodeId,
    pub dir_depth: u32,
    pub entries: Vec<EntryRecord>,
    pub last: bool,
    /// Set on the last chunk when enumeration stopped with an error.
    pub enum_error: bool,
}
