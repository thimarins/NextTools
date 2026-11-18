use nexttool_core::{ErrorKind, Severity};
use serde::{Deserialize, Serialize};

use crate::model::NodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Enumerate,
    Persist,
    Finalize,
    Resume,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Enumerate => "enumerate",
            Stage::Persist => "persist",
            Stage::Finalize => "finalize",
            Stage::Resume => "resume",
        }
    }
}

/// A recorded problem. One failing directory never aborts the whole scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanIssue {
    pub dir_id: Option<NodeId>,
    pub path: String,
    pub kind: ErrorKind,
    pub os_code: Option<i32>,
    pub stage: Stage,
    pub severity: Severity,
    pub recoverable: bool,
    /// Unix seconds.
    pub at: i64,
    pub message: String,
}
