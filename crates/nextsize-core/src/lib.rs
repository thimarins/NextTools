//! NextSize scan model and pipeline contracts. No I/O here.

pub mod ext;
pub mod issue;
pub mod model;
pub mod options;
pub mod progress;
pub mod sink;

pub use issue::{ScanIssue, Stage};
pub use model::{DirChunk, EntryRecord, NodeId, ROOT_ID};
pub use nexttool_core::fs::{flags, Backend, DirEnumerator, EntryKind, EntryRef, EnumError};
pub use nexttool_core::{ErrorKind, Severity};
pub use options::{Profile, ScanOptions, Tuning};
pub use progress::{ProgressSnapshot, ScanProgress, ScanState};
pub use sink::{NullSink, ScanOutcome, ScanSink, SinkError, SinkMsg};

/// Version of the on-disk snapshot schema. Bump on incompatible change.
pub const SCHEMA_VERSION: u32 = 1;
