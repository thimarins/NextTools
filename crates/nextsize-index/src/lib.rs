//! NextSize snapshot storage. One snapshot = one SQLite file (`.nxs`), which
//! is also the unit of partitioning (per source / volume / date).
//!
//! - `writer`: the `ScanSink` used during a scan (batched transactions =
//!   checkpoints), finalization (indexes, roll-up, aggregates).
//! - `resume`: recovery of interrupted snapshots.
//! - `reader`: summary, rankings, directory listing, filtered queries with
//!   pagination, time limit and cancellation.

mod reader;
mod resume;
mod schema;
mod writer;

pub use reader::{DirRow, ExtRow, FileQuery, FileRow, IssueRow, QueryError, Snapshot, Summary, YearRow};
pub use resume::{open_for_resume, ResumePlan};
pub use writer::IndexWriter;

#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("arquivo de snapshot já existe: {0}")]
    AlreadyExists(String),
    #[error("versão de schema incompatível: arquivo {found}, suportada {supported}")]
    Schema { found: String, supported: u32 },
    #[error("snapshot não é retomável (estado: {0})")]
    NotResumable(String),
    #[error("snapshot inválido ou corrompido: {0}")]
    Corrupt(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

pub(crate) fn sqlite_to_sink(e: rusqlite::Error) -> nextsize_core::SinkError {
    if let rusqlite::Error::SqliteFailure(ref f, _) = e {
        if f.code == rusqlite::ErrorCode::DiskFull {
            return nextsize_core::SinkError::DiskFull(e.to_string());
        }
    }
    nextsize_core::SinkError::Storage(e.to_string())
}
