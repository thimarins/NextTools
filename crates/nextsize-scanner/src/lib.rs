//! NextSize scan engine.
//!
//! Pipeline: N enumeration threads → bounded channel (backpressure) → one
//! writer thread that drives the `ScanSink`. Pending directories live in a
//! bounded in-memory stack that spills to a temp file, so engine RAM does not
//! grow with the number of files or directories. See docs/ARCHITECTURE.md.

mod control;
mod engine;
mod frontier;
mod spill;

pub use control::ScanControl;
pub use engine::{run_scan, DirTask, ScanReport, ScanStart};
