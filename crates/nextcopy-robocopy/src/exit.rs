//! Robocopy exit code semantics (documented bitmask):
//! 1 = files copied, 2 = extra files/dirs in destination, 4 = mismatched
//! files/dirs, 8 = some files or dirs could not be copied (retries
//! exhausted), 16 = fatal error (nothing copied: usage error, access denied
//! to source/dest root, out of memory…). 0 = nothing to do.

use nextcopy_core::{CopyOutcome, CopyStats};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitInfo {
    pub code: i32,
    pub copied: bool,
    pub extras: bool,
    pub mismatch: bool,
    pub failures: bool,
    pub fatal: bool,
}

pub fn decode(code: i32) -> ExitInfo {
    ExitInfo {
        code,
        copied: code & 1 != 0,
        extras: code & 2 != 0,
        mismatch: code & 4 != 0,
        failures: code & 8 != 0,
        fatal: code & 16 != 0 || code < 0,
    }
}

/// Combines the exit code with the parsed summary. The summary can only make
/// the verdict *worse*, never better (e.g. summary says FAILED > 0).
pub fn classify(code: Option<i32>, stats: Option<&CopyStats>, mirror: bool) -> CopyOutcome {
    let Some(code) = code else { return CopyOutcome::Cancelled };
    let e = decode(code);
    if e.fatal {
        return CopyOutcome::Failed;
    }
    let failed_in_summary = stats.is_some_and(|s| s.files_failed > 0 || s.dirs_failed > 0);
    if e.failures || failed_in_summary {
        return CopyOutcome::PartialFailure;
    }
    // Extras are expected in a non-mirror copy (destination has other files);
    // in mirror mode they were deleted, which is the requested behavior.
    let _ = mirror;
    if e.mismatch {
        return CopyOutcome::CompletedWithWarnings;
    }
    CopyOutcome::Completed
}

/// Applies what the log showed on top of the exit-code verdict. Robocopy can
/// exit with 0 after failing to access a whole directory (no files counted,
/// nothing copied), so a directory-level error is always a partial failure,
/// and any observed error prevents a clean "Completed".
pub fn reconcile(outcome: CopyOutcome, error_events: u64, dir_error_events: u64) -> CopyOutcome {
    match outcome {
        CopyOutcome::Completed | CopyOutcome::CompletedWithWarnings if dir_error_events > 0 => CopyOutcome::PartialFailure,
        CopyOutcome::Completed if error_events > 0 => CopyOutcome::CompletedWithWarnings,
        o => o,
    }
}

#[cfg(test)]
mod tests {
    use crate::log::LogState;

    /// Real output captured on Windows 11 (en-US): destination folder denies
    /// writes; Robocopy prints an access error and still exits with 0.
    #[test]
    fn denied_destination_dir_with_exit_zero_is_not_success() {
        let real = "2026/11/18 07:23:03 ERROR 5 (0x00000005) Accessing Destination Directory C:\\NextToolBench\\deny-test\\dst\\\n\
Access is denied.\n\
               Total    Copied   Skipped  Mismatch    FAILED    Extras\n\
    Dirs :         1         1         0         0         0         0\n\
   Files :         0         0         0         0         0         0\n\
   Bytes :         0         0         0         0         0         0\n";
        let mut st = LogState::new(std::path::Path::new(r"C:\NextToolBench\deny-test\dst"));
        for l in real.lines() {
            st.apply(l);
        }
        assert_eq!(st.dir_error_events, 1);
        let by_code = super::classify(Some(0), st.summary_stats().as_ref(), false);
        assert_eq!(by_code, CopyOutcome::Completed, "exit code alone says success");
        assert_eq!(super::reconcile(by_code, st.error_events, st.dir_error_events), CopyOutcome::PartialFailure);
        assert_eq!(st.errors_sample[0].kind, nexttool_core::ErrorKind::AccessDenied);
    }

    #[test]
    fn retried_file_error_downgrades_to_warning() {
        assert_eq!(super::reconcile(CopyOutcome::Completed, 1, 0), CopyOutcome::CompletedWithWarnings);
        assert_eq!(super::reconcile(CopyOutcome::Completed, 0, 0), CopyOutcome::Completed);
        assert_eq!(super::reconcile(CopyOutcome::Failed, 3, 3), CopyOutcome::Failed);
    }

    use nextcopy_core::CopyOutcome;
}

#[cfg(test)]
mod code_tests {
    use super::*;

    #[test]
    fn codes() {
        assert_eq!(classify(Some(0), None, false), CopyOutcome::Completed);
        assert_eq!(classify(Some(1), None, false), CopyOutcome::Completed);
        assert_eq!(classify(Some(3), None, false), CopyOutcome::Completed);
        assert_eq!(classify(Some(4), None, false), CopyOutcome::CompletedWithWarnings);
        assert_eq!(classify(Some(9), None, false), CopyOutcome::PartialFailure);
        assert_eq!(classify(Some(16), None, false), CopyOutcome::Failed);
        assert_eq!(classify(None, None, false), CopyOutcome::Cancelled);
        let s = CopyStats { files_failed: 2, ..Default::default() };
        assert_eq!(classify(Some(1), Some(&s), false), CopyOutcome::PartialFailure);
    }
}
