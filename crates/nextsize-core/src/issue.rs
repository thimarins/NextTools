use serde::{Deserialize, Serialize};

use crate::model::NodeId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueKind {
    AccessDenied,
    NotFound,
    PathTooLong,
    Network,
    Timeout,
    InvalidName,
    Io,
    Persistence,
    Internal,
}

impl IssueKind {
    pub fn as_str(self) -> &'static str {
        match self {
            IssueKind::AccessDenied => "access_denied",
            IssueKind::NotFound => "not_found",
            IssueKind::PathTooLong => "path_too_long",
            IssueKind::Network => "network",
            IssueKind::Timeout => "timeout",
            IssueKind::InvalidName => "invalid_name",
            IssueKind::Io => "io",
            IssueKind::Persistence => "persistence",
            IssueKind::Internal => "internal",
        }
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Error,
    Fatal,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Warning => "warning",
            Severity::Error => "error",
            Severity::Fatal => "fatal",
        }
    }
}

/// A recorded problem. One failing directory never aborts the whole scan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanIssue {
    pub dir_id: Option<NodeId>,
    pub path: String,
    pub kind: IssueKind,
    pub os_code: Option<i32>,
    pub stage: Stage,
    pub severity: Severity,
    pub recoverable: bool,
    /// Unix seconds.
    pub at: i64,
    pub message: String,
}
