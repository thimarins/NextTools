//! Task model shared by the task manager, the store and the UI.

use serde::{Deserialize, Serialize};

pub type TaskId = i64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Module {
    NextSize,
    NextCopy,
    System,
}

impl Module {
    pub fn as_str(self) -> &'static str {
        match self {
            Module::NextSize => "nextsize",
            Module::NextCopy => "nextcopy",
            Module::System => "system",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        [Module::NextSize, Module::NextCopy, Module::System].into_iter().find(|m| m.as_str() == s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    SizeScan,
    Copy,
    Verify,
}

impl TaskKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TaskKind::SizeScan => "size_scan",
            TaskKind::Copy => "copy",
            TaskKind::Verify => "verify",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        [TaskKind::SizeScan, TaskKind::Copy, TaskKind::Verify].into_iter().find(|m| m.as_str() == s)
    }
    pub fn module(self) -> Module {
        match self {
            TaskKind::SizeScan => Module::NextSize,
            TaskKind::Copy | TaskKind::Verify => Module::NextCopy,
        }
    }
}

/// Task lifecycle. Terminal states are never re-entered silently: a re-run
/// creates a new attempt recorded in history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Queued,
    Running,
    /// Stopped by the user; resumable. (Robocopy: process stopped, re-run resumes.)
    Paused,
    /// Waiting before an automatic retry.
    WaitingRetry,
    Cancelling,
    /// Found in Running state after a crash/power loss, or stopped by a failure
    /// that allows resuming. Never shown as completed.
    Interrupted,
    Completed,
    CompletedWithWarnings,
    Failed,
    Cancelled,
}

impl TaskState {
    pub fn as_str(self) -> &'static str {
        use TaskState::*;
        match self {
            Queued => "queued",
            Running => "running",
            Paused => "paused",
            WaitingRetry => "waiting_retry",
            Cancelling => "cancelling",
            Interrupted => "interrupted",
            Completed => "completed",
            CompletedWithWarnings => "completed_with_warnings",
            Failed => "failed",
            Cancelled => "cancelled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        use TaskState::*;
        [Queued, Running, Paused, WaitingRetry, Cancelling, Interrupted, Completed, CompletedWithWarnings, Failed, Cancelled]
            .into_iter()
            .find(|v| v.as_str() == s)
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, TaskState::Completed | TaskState::CompletedWithWarnings | TaskState::Failed | TaskState::Cancelled)
    }

    /// Was active when the process last ran: must be recovered on startup.
    pub fn is_active(self) -> bool {
        matches!(self, TaskState::Running | TaskState::Cancelling | TaskState::WaitingRetry)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low = 0,
    #[default]
    Normal = 1,
    High = 2,
}

impl Priority {
    pub fn from_i64(v: i64) -> Self {
        match v {
            0 => Priority::Low,
            2 => Priority::High,
            _ => Priority::Normal,
        }
    }
}
