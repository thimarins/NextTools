//! Application store (`app.db`): tasks (with checkpoints and history),
//! task events, alerts (grouped), settings. Plus bounded, rotating log files.
//!
//! NextSize snapshots are NOT stored here (they live in their own `.nxs`
//! files); the store only keeps references to them.

mod logs;
mod settings;
mod store;

pub use logs::{init_logging, RotatingFile};
pub use settings::{validate_setting, SettingError, SETTINGS};
pub use store::{AppStore, NewTask, RecoveredTask, ScheduleRecord, StoreError, TaskRecord};

use std::path::PathBuf;

/// Data directory: `<exe dir>\data` when a `portable.flag` file sits next to
/// the executable (portable distribution), otherwise `%LOCALAPPDATA%\NextTool`
/// (`$XDG_DATA_HOME/nexttool` or `~/.local/share/nexttool` on Linux).
pub fn default_data_dir() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if dir.join("portable.flag").exists() {
                return dir.join("data");
            }
        }
    }
    #[cfg(windows)]
    if let Some(p) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(p).join("NextTool");
    }
    if let Some(p) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(p).join("nexttool");
    }
    if let Some(h) = std::env::var_os("HOME") {
        return PathBuf::from(h).join(".local/share/nexttool");
    }
    PathBuf::from("nexttool-data")
}
