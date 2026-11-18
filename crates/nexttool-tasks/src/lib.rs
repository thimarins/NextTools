//! Task orchestration shared by NextSize and NextCopy. Engines never depend
//! on this crate; this crate adapts them into tasks (runners).

pub mod alerts;
pub mod manager;
pub mod monitor;
pub mod resources;
pub mod retry;
pub mod runners;

pub use manager::{TaskControl, TaskCtx, TaskManager, RunOutcome, TaskRunner};
pub use runners::{CopySpec, SizeScanSpec, TaskSpec};
