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

#[cfg(test)]
mod tests {
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
