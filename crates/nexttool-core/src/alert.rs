//! Alert model for the global alert center.

use serde::{Deserialize, Serialize};

use crate::error::Severity;
use crate::task::{Module, TaskId};

/// A new alert to raise. Alerts with the same `fingerprint` that are still
/// open are grouped (count + last_at updated) instead of duplicated.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertDraft {
    pub module: Module,
    pub task_id: Option<TaskId>,
    pub severity: Severity,
    /// Stable grouping key, e.g. "copy.network.dest:\\\\nas\\share".
    pub fingerprint: String,
    pub title: String,
    pub problem: String,
    /// Only when there is evidence; never guessed.
    pub probable_cause: Option<String>,
    pub consequence: String,
    pub recommended_action: String,
    pub recoverable: bool,
    /// Technical details for administrators (codes, paths, counters).
    pub details: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: i64,
    pub draft: AlertDraft,
    pub first_at: i64,
    pub last_at: i64,
    pub count: i64,
    pub acknowledged: bool,
}
