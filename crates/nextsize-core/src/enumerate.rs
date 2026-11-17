use std::path::Path;

use crate::issue::IssueKind;
use crate::model::EntryRef;

/// Error returned by a directory enumerator.
#[derive(Debug, Clone, thiserror::Error)]
#[error("{kind:?} (os code {os_code:?}): {message}")]
pub struct EnumError {
    pub kind: IssueKind,
    pub os_code: Option<i32>,
    pub message: String,
    /// Some entries were delivered before the error happened.
    pub partial: bool,
}

/// Lists the direct children of one directory. Implementations must:
/// - never open file contents and never modify anything;
/// - never follow links (they report them, the engine decides);
/// - skip `.` and `..`;
/// - stop as soon as the callback returns `false`.
pub trait DirEnumerator: Send + Sync {
    fn name(&self) -> &'static str;

    fn enumerate(
        &self,
        dir: &Path,
        on_entry: &mut dyn FnMut(&EntryRef<'_>) -> bool,
    ) -> Result<(), EnumError>;
}
