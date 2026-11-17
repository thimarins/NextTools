use serde::Serialize;

use crate::issue::ScanIssue;
use crate::model::DirChunk;
use crate::progress::ScanState;

#[derive(Debug)]
pub enum SinkMsg {
    Chunk(DirChunk),
    Issue(ScanIssue),
}

#[derive(Debug, thiserror::Error)]
pub enum SinkError {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("disk full or cache location out of space: {0}")]
    DiskFull(String),
    #[error("{0}")]
    Other(String),
}

/// How the scan ended, handed to the sink so it can mark the snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct ScanOutcome {
    pub state: ScanState,
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub issues: u64,
    pub elapsed_ms: u64,
}

/// Consumer of scan output. Runs on a single dedicated writer thread; the
/// engine guarantees calls are never concurrent.
pub trait ScanSink: Send {
    fn write(&mut self, msg: SinkMsg) -> Result<(), SinkError>;

    /// Called periodically while idle so time-based commits still happen.
    fn tick(&mut self) -> Result<(), SinkError> {
        Ok(())
    }

    /// Flush everything durable (used for pause and before finish).
    fn checkpoint(&mut self) -> Result<(), SinkError> {
        Ok(())
    }

    /// Final call. For a completed scan the sink builds indexes/aggregates.
    fn finish(&mut self, outcome: &ScanOutcome) -> Result<(), SinkError>;
}

/// Discards everything; used to benchmark pure enumeration speed.
#[derive(Debug, Default)]
pub struct NullSink;

impl ScanSink for NullSink {
    fn write(&mut self, _msg: SinkMsg) -> Result<(), SinkError> {
        Ok(())
    }
    fn finish(&mut self, _outcome: &ScanOutcome) -> Result<(), SinkError> {
        Ok(())
    }
}
