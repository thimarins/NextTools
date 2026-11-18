//! NextCopy model and engine contract. Engine-agnostic: Robocopy is the v1
//! engine (`nextcopy-robocopy`); a native Rust engine may implement the same
//! `CopyEngine` trait later.

pub mod diagnostics;
mod job;
mod profile;
mod result;

pub use job::{CopyFlags, CopyJob, CopyOptions, Filters, OverwritePolicy, VerifyLevel, MAX_INDIVIDUAL_FILES, MAX_FILE_GROUPS};
pub use profile::CopyProfile;
pub use result::{CopyControl, CopyEvent, CopyOutcome, CopyResult, CopyStats, DryRunReport, EngineError, FileError};

use std::path::Path;

/// A copy engine. Implementations must:
/// - never use a shell, only structured arguments;
/// - never delete in the destination unless `job.options.mirror` is set AND a
///   matching dry-run approval is supplied;
/// - report incomplete operations as incomplete.
pub trait CopyEngine: Send + Sync {
    fn name(&self) -> &'static str;

    /// Lists what would be copied and deleted, without touching anything.
    fn dry_run(&self, job: &CopyJob, log_dir: &Path, control: &CopyControl) -> Result<DryRunReport, EngineError>;

    /// Performs the copy. `on_event` receives throttled progress.
    fn run(
        &self,
        job: &CopyJob,
        log_dir: &Path,
        control: &CopyControl,
        on_event: &mut dyn FnMut(CopyEvent),
    ) -> Result<CopyResult, EngineError>;
}
