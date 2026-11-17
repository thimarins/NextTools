//! NextSize core: shared data model, contracts between crates, tuning profiles
//! and error types. This crate has no platform code and no I/O.

pub mod enumerate;
pub mod ext;
pub mod issue;
pub mod model;
pub mod options;
pub mod progress;
pub mod sink;
pub mod time;

pub use enumerate::{DirEnumerator, EnumError};
pub use issue::{IssueKind, ScanIssue, Severity, Stage};
pub use model::{flags, DirChunk, EntryKind, EntryRecord, EntryRef, NodeId, ROOT_ID};
pub use options::{Backend, Profile, ScanOptions, Tuning};
pub use progress::{ScanProgress, ScanState};
pub use sink::{ScanOutcome, ScanSink, SinkError, SinkMsg};

/// Version of the on-disk index schema. Bump on any incompatible change.
pub const SCHEMA_VERSION: u32 = 1;
