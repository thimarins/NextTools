//! Thin `#[tauri::command]` wrappers. Heavy work runs on the blocking pool
//! (`spawn_blocking`) so the webview never waits on disk or SQLite.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use nexttool_core::alert::Alert;
use nexttool_core::task::TaskId;
use nexttool_store::TaskRecord;
use nextcopy_core::DryRunReport;
use nextsize_index::{DirRow, FileRow, Summary};
use tauri::{AppHandle, State};

use crate::ipc::{ApiError, ApiResult, AppInfo, Core, CopyRequest, DirInfo, Overview, Paged, QueryRequest, ScanRequest, Stats, TaskDetail};

type St<'a> = State<'a, Arc<Core>>;

async fn blocking<T: Send + 'static>(core: &Arc<Core>, f: impl FnOnce(&Core) -> ApiResult<T> + Send + 'static) -> ApiResult<T> {
    let c = core.clone();
    tauri::async_runtime::spawn_blocking(move || f(&c)).await.map_err(|e| ApiError::new("internal", e.to_string()))?
}

#[tauri::command]
pub fn app_info(core: St<'_>, app: AppHandle) -> AppInfo {
    let _ = app;
    core.app_info()
}

/// Confirmed exit: pause running tasks safely (resumable), then quit.
#[tauri::command]
pub async fn app_quit(core: St<'_>, app: AppHandle) -> ApiResult<()> {
    let c = core.inner().clone();
    tauri::async_runtime::spawn_blocking(move || c.mgr.shutdown(Duration::from_secs(20)))
        .await
        .map_err(|e| ApiError::new("internal", e.to_string()))?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
pub async fn overview(core: St<'_>) -> ApiResult<Overview> {
    blocking(core.inner(), |c| c.overview()).await
}

#[tauri::command]
pub async fn tasks_list(core: St<'_>, states: Option<Vec<String>>, limit: Option<u32>, offset: Option<u32>) -> ApiResult<Vec<TaskRecord>> {
    blocking(core.inner(), move |c| c.tasks_list(states, limit, offset.unwrap_or(0))).await
}

#[tauri::command]
pub async fn task_get(core: St<'_>, id: TaskId) -> ApiResult<TaskDetail> {
    blocking(core.inner(), move |c| c.task_get(id)).await
}

#[tauri::command]
pub async fn task_pause(core: St<'_>, id: TaskId) -> ApiResult<bool> {
    blocking(core.inner(), move |c| c.task_pause(id)).await
}

#[tauri::command]
pub async fn task_resume(core: St<'_>, id: TaskId) -> ApiResult<()> {
    blocking(core.inner(), move |c| c.task_resume(id)).await
}

#[tauri::command]
pub async fn task_cancel(core: St<'_>, id: TaskId) -> ApiResult<bool> {
    blocking(core.inner(), move |c| c.task_cancel(id)).await
}

#[tauri::command]
pub async fn size_scan_submit(core: St<'_>, req: ScanRequest) -> ApiResult<TaskId> {
    blocking(core.inner(), move |c| c.size_scan_submit(req)).await
}

#[tauri::command]
pub async fn snapshot_summary(core: St<'_>, path: String) -> ApiResult<Summary> {
    blocking(core.inner(), move |c| c.snapshot_summary(&path)).await
}

#[tauri::command]
pub async fn snapshot_list_dir(core: St<'_>, path: String, dir_id: i64, offset: Option<u32>, limit: Option<u32>, request_id: String) -> ApiResult<Paged<DirRow>> {
    blocking(core.inner(), move |c| c.snapshot_list_dir(&path, dir_id, offset.unwrap_or(0), limit, request_id)).await
}

#[tauri::command]
pub async fn snapshot_list_subdirs(core: St<'_>, path: String, dir_id: i64, offset: Option<u32>, limit: Option<u32>, request_id: String) -> ApiResult<Paged<DirRow>> {
    blocking(core.inner(), move |c| c.snapshot_list_subdirs(&path, dir_id, offset.unwrap_or(0), limit, request_id)).await
}

#[tauri::command]
pub async fn snapshot_dir_info(core: St<'_>, path: String, dir_id: i64) -> ApiResult<DirInfo> {
    blocking(core.inner(), move |c| c.snapshot_dir_info(&path, dir_id)).await
}

#[tauri::command]
pub async fn snapshots_list(core: St<'_>) -> ApiResult<Vec<nextsize_index::SnapshotEntry>> {
    blocking(core.inner(), |c| c.snapshots_list()).await
}

#[tauri::command]
pub async fn snapshot_set_label(core: St<'_>, path: String, label: String) -> ApiResult<()> {
    blocking(core.inner(), move |c| c.snapshot_set_label(&path, &label)).await
}

#[tauri::command]
pub async fn snapshot_delete(core: St<'_>, path: String, confirm_name: String) -> ApiResult<()> {
    blocking(core.inner(), move |c| c.snapshot_delete(&path, &confirm_name)).await
}

#[tauri::command]
pub async fn snapshot_import(core: St<'_>, src: String) -> ApiResult<String> {
    blocking(core.inner(), move |c| c.snapshot_import(&src)).await
}

#[tauri::command]
pub async fn snapshot_export(core: St<'_>, path: String, dest: String) -> ApiResult<()> {
    blocking(core.inner(), move |c| c.snapshot_export(&path, &dest)).await
}

#[tauri::command]
pub async fn snapshot_build_indexes(core: St<'_>, path: String) -> ApiResult<u64> {
    blocking(core.inner(), move |c| c.snapshot_build_indexes(&path)).await
}

#[tauri::command]
pub async fn snapshot_compare(core: St<'_>, a: String, b: String, rel: Vec<String>, limit: Option<u32>) -> ApiResult<nextsize_index::CompareLevel> {
    blocking(core.inner(), move |c| c.snapshot_compare(&a, &b, rel, limit)).await
}

#[tauri::command]
pub async fn owners_lookup(core: St<'_>, paths: Vec<String>) -> ApiResult<Vec<crate::ipc::OwnerInfo>> {
    blocking(core.inner(), move |c| c.owners_lookup(paths)).await
}

#[tauri::command]
pub async fn snapshot_top_files(core: St<'_>, path: String, limit: Option<u32>) -> ApiResult<Vec<FileRow>> {
    blocking(core.inner(), move |c| c.snapshot_top_files(&path, limit)).await
}

#[tauri::command]
pub async fn snapshot_top_dirs(core: St<'_>, path: String, limit: Option<u32>) -> ApiResult<Vec<DirRow>> {
    blocking(core.inner(), move |c| c.snapshot_top_dirs(&path, limit)).await
}

#[tauri::command]
pub async fn snapshot_query(core: St<'_>, path: String, query: QueryRequest, request_id: String) -> ApiResult<Paged<FileRow>> {
    blocking(core.inner(), move |c| c.snapshot_query(&path, query, request_id)).await
}

#[tauri::command]
pub fn snapshot_cancel_query(core: St<'_>, request_id: String) -> bool {
    core.snapshot_cancel_query(&request_id)
}

#[tauri::command]
pub async fn snapshot_stats(core: St<'_>, path: String) -> ApiResult<Stats> {
    blocking(core.inner(), move |c| c.snapshot_stats(&path)).await
}

#[tauri::command]
pub async fn copy_plan(core: St<'_>, req: CopyRequest) -> ApiResult<DryRunReport> {
    blocking(core.inner(), move |c| c.copy_plan(req)).await
}

#[tauri::command]
pub async fn copy_submit(core: St<'_>, req: CopyRequest) -> ApiResult<TaskId> {
    blocking(core.inner(), move |c| c.copy_submit(req)).await
}

#[tauri::command]
pub async fn alerts_list(core: St<'_>, include_acknowledged: Option<bool>, limit: Option<u32>) -> ApiResult<Vec<Alert>> {
    blocking(core.inner(), move |c| c.alerts_list(include_acknowledged.unwrap_or(false), limit)).await
}

#[tauri::command]
pub async fn alerts_ack(core: St<'_>, id: i64) -> ApiResult<bool> {
    blocking(core.inner(), move |c| c.alerts_ack(id)).await
}

#[tauri::command]
pub async fn settings_get(core: St<'_>) -> ApiResult<HashMap<String, String>> {
    blocking(core.inner(), |c| c.settings_get()).await
}

#[tauri::command]
pub async fn settings_set(core: St<'_>, key: String, value: String) -> ApiResult<()> {
    blocking(core.inner(), move |c| c.settings_set(&key, &value)).await
}

#[tauri::command]
pub async fn schedules_list(core: St<'_>) -> ApiResult<Vec<crate::ipc::ScheduleView>> {
    blocking(core.inner(), |c| c.schedules_list()).await
}

#[tauri::command]
pub async fn schedule_create(core: St<'_>, title: String, kind: String, request: serde_json::Value, recurrence: nexttool_tasks::schedule::Recurrence) -> ApiResult<i64> {
    blocking(core.inner(), move |c| c.schedule_create(title, &kind, request, recurrence)).await
}

#[tauri::command]
pub async fn schedule_set_enabled(core: St<'_>, id: i64, enabled: bool) -> ApiResult<bool> {
    blocking(core.inner(), move |c| c.schedule_set_enabled(id, enabled)).await
}

#[tauri::command]
pub async fn schedule_delete(core: St<'_>, id: i64) -> ApiResult<bool> {
    blocking(core.inner(), move |c| c.schedule_delete(id)).await
}