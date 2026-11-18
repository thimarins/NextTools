//! IPC logic, independent of Tauri so it can be unit-tested. Every input from
//! the webview is validated here; every list is paginated and capped.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nexttool_core::alert::Alert;
use nexttool_core::security::validate_user_path;
use nexttool_core::task::{TaskId, TaskKind, TaskState};
use nexttool_store::{AppStore, RecoveredTask, TaskRecord};
use nexttool_tasks::runners::default_factory;
use nexttool_tasks::{CopySpec, SizeScanSpec, TaskManager, TaskSpec};
use nextcopy_core::{CopyJob, CopyProfile, DryRunReport, VerifyLevel};
use nextsize_core::Profile;
use nextsize_index::{DirRow, ExtRow, FileQuery, FileRow, QueryError, Snapshot, Summary, YearRow};
use serde::{Deserialize, Serialize};

/// Error returned to the UI: stable `code` for logic, `message` for people.
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        ApiError { code, message: message.into() }
    }
}

impl From<nexttool_store::StoreError> for ApiError {
    fn from(e: nexttool_store::StoreError) -> Self {
        ApiError::new("store", e.to_string())
    }
}

impl From<nextsize_index::IndexError> for ApiError {
    fn from(e: nextsize_index::IndexError) -> Self {
        ApiError::new("snapshot", e.to_string())
    }
}

impl From<QueryError> for ApiError {
    fn from(e: QueryError) -> Self {
        let code = match e {
            QueryError::Cancelled => "cancelled",
            QueryError::TimeLimit => "time_limit",
            QueryError::NotFinalized => "not_finalized",
            QueryError::Sqlite(_) => "query",
        };
        ApiError::new(code, e.to_string())
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug, Serialize)]
pub struct AppInfo {
    pub version: &'static str,
    pub data_dir: String,
    pub portable: bool,
    pub platform: &'static str,
    pub robocopy: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Overview {
    pub active: Vec<TaskRecord>,
    pub failed_recent: Vec<TaskRecord>,
    pub recent_scans: Vec<TaskRecord>,
    pub recent_copies: Vec<TaskRecord>,
    pub open_alerts: Vec<Alert>,
    pub open_alert_count: usize,
}

#[derive(Debug, Serialize)]
pub struct TaskDetail {
    pub task: TaskRecord,
    pub events: Vec<(i64, String, String)>,
}

#[derive(Debug, Deserialize)]
pub struct ScanRequest {
    pub root: String,
    /// Optional explicit snapshot path; default: <data>/snapshots/<name>-<ts>.nxs
    pub snapshot: Option<String>,
    pub profile: String,
}

#[derive(Debug, Deserialize)]
pub struct CopyRequest {
    pub source: String,
    pub dest: String,
    pub profile: String,
    pub threads: Option<u32>,
    pub verify: Option<String>,
    #[serde(default)]
    pub exclude_files: Vec<String>,
    #[serde(default)]
    pub exclude_dirs: Vec<String>,
    pub approved_deletions: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
pub struct QueryRequest {
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    pub ext: Option<String>,
    pub modified_before: Option<i64>,
    pub modified_after: Option<i64>,
    pub year: Option<i32>,
    pub name_contains: Option<String>,
    #[serde(default)]
    pub offset: u32,
    pub limit: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct Paged<T> {
    pub request_id: String,
    pub rows: Vec<T>,
    pub elapsed_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct DirInfo {
    /// Root first: (dir id, name). The root's name is the full scanned path.
    pub chain: Vec<(i64, String)>,
    pub subdirs: u64,
    pub files: u64,
}

#[derive(Debug, Serialize)]
pub struct Stats {
    pub extensions: Vec<ExtRow>,
    pub years: Vec<YearRow>,
}

pub struct Core {
    pub mgr: Arc<TaskManager>,
    pub data_dir: PathBuf,
    queries: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

/// Paths from the UI must already be absolute and free of `.`/`..`: they are
/// validated as typed, never resolved against the process working directory.
fn abs_path(s: &str) -> ApiResult<PathBuf> {
    validate_user_path(Path::new(s.trim())).map_err(|e| ApiError::new("invalid_path", e.to_string()))
}

/// Snapshot paths must be absolute `.nxs` files that exist.
fn snapshot_path(s: &str) -> ApiResult<PathBuf> {
    let p = abs_path(s)?;
    if p.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("nxs")) != Some(true) {
        return Err(ApiError::new("invalid_path", "o snapshot precisa ter extensão .nxs"));
    }
    if !p.is_file() {
        return Err(ApiError::new("not_found", format!("snapshot não encontrado: {}", p.display())));
    }
    Ok(p)
}

fn page(limit: Option<u32>) -> u32 {
    limit.unwrap_or(100).clamp(1, 1000)
}

fn sanitize(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    let s = s.trim_matches('_').to_string();
    if s.is_empty() { "raiz".into() } else { s.chars().take(60).collect() }
}

impl Core {
    /// Opens the store, runs crash recovery (alerts raised for interrupted
    /// tasks) and builds the task manager.
    pub fn open(data_dir: PathBuf) -> ApiResult<(Core, Vec<RecoveredTask>)> {
        std::fs::create_dir_all(&data_dir).map_err(|e| ApiError::new("io", e.to_string()))?;
        let (store, note) = AppStore::open(&data_dir.join("app.db"))?;
        if let Some(n) = note {
            let d = nexttool_tasks::alerts::for_error(nexttool_core::task::Module::System, 0, nexttool_core::ErrorKind::CorruptState, "app.db", false, n);
            store.raise_alert(&d)?;
        }
        let recovered = store.recover_interrupted()?;
        for r in recovered.iter().filter(|r| !r.owned_by_live_process) {
            store.raise_alert(&nexttool_tasks::alerts::interrupted(r.task.module, r.task.id, &r.task.title, r.child_still_running))?;
        }
        let max_running = store.get_setting("max_running")?.and_then(|v| v.parse().ok()).unwrap_or(2);
        let mgr = Arc::new(TaskManager::new(store, data_dir.clone(), default_factory(), max_running));
        Ok((Core { mgr, data_dir, queries: Mutex::new(HashMap::new()) }, recovered))
    }

    pub fn app_info(&self) -> AppInfo {
        let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf));
        AppInfo {
            version: env!("CARGO_PKG_VERSION"),
            data_dir: self.data_dir.display().to_string(),
            portable: exe_dir.is_some_and(|d| d.join("portable.flag").exists()),
            platform: std::env::consts::OS,
            robocopy: robocopy_path(),
        }
    }

    pub fn overview(&self) -> ApiResult<Overview> {
        let s = self.mgr.store();
        let active = s.list_tasks(Some(&[TaskState::Running, TaskState::Queued, TaskState::WaitingRetry, TaskState::Cancelling, TaskState::Paused, TaskState::Interrupted]), 50, 0)?;
        let failed_recent = s.list_tasks(Some(&[TaskState::Failed]), 5, 0)?;
        let recent = s.list_tasks(None, 100, 0)?;
        let recent_scans = recent.iter().filter(|t| t.kind == TaskKind::SizeScan).take(5).cloned().collect();
        let recent_copies = recent.iter().filter(|t| t.kind == TaskKind::Copy).take(5).cloned().collect();
        let open_alerts = s.list_alerts(false, 100)?;
        let open_alert_count = open_alerts.len();
        Ok(Overview { active, failed_recent, recent_scans, recent_copies, open_alerts: open_alerts.into_iter().take(5).collect(), open_alert_count })
    }

    pub fn tasks_list(&self, states: Option<Vec<String>>, limit: Option<u32>, offset: u32) -> ApiResult<Vec<TaskRecord>> {
        let parsed: Option<Vec<TaskState>> = match states {
            Some(v) => Some(v.iter().map(|s| TaskState::parse(s).ok_or_else(|| ApiError::new("invalid_argument", format!("estado desconhecido: {s}")))).collect::<Result<_, _>>()?),
            None => None,
        };
        Ok(self.mgr.store().list_tasks(parsed.as_deref(), page(limit), offset)?)
    }

    pub fn task_get(&self, id: TaskId) -> ApiResult<TaskDetail> {
        let s = self.mgr.store();
        Ok(TaskDetail { task: s.get_task(id)?, events: s.events(id, 200)? })
    }

    pub fn task_pause(&self, id: TaskId) -> ApiResult<bool> {
        Ok(self.mgr.pause(id))
    }

    pub fn task_resume(&self, id: TaskId) -> ApiResult<()> {
        let t = self.mgr.store().get_task(id)?;
        if !matches!(t.state, TaskState::Paused | TaskState::Interrupted | TaskState::Failed) {
            return Err(ApiError::new("invalid_state", format!("a tarefa está em '{}' e não pode ser retomada", t.state.as_str())));
        }
        Ok(self.mgr.resume(id)?)
    }

    pub fn task_cancel(&self, id: TaskId) -> ApiResult<bool> {
        Ok(self.mgr.cancel(id)?)
    }

    pub fn size_scan_submit(&self, req: ScanRequest) -> ApiResult<TaskId> {
        let root = abs_path(&req.root)?;
        if !root.is_dir() {
            return Err(ApiError::new("not_found", format!("diretório não encontrado ou inacessível: {}", root.display())));
        }
        let profile = Profile::parse(&req.profile).ok_or_else(|| ApiError::new("invalid_argument", "perfil inválido"))?;
        let snapshot = match req.snapshot.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(s) => {
                let p = abs_path(s)?;
                if p.extension().and_then(|e| e.to_str()) != Some("nxs") {
                    return Err(ApiError::new("invalid_path", "o snapshot precisa ter extensão .nxs"));
                }
                p
            }
            None => {
                let name = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| root.to_string_lossy().into_owned());
                self.data_dir.join("snapshots").join(format!("{}-{}.nxs", sanitize(&name), nexttool_core::time::now_unix()))
            }
        };
        if let Some(dir) = snapshot.parent() {
            std::fs::create_dir_all(dir).map_err(|e| ApiError::new("io", e.to_string()))?;
        }
        let spec = TaskSpec::SizeScan(SizeScanSpec { root: root.clone(), snapshot, profile, backend: Default::default(), threads: None });
        Ok(self.mgr.submit(spec.to_new_task(format!("Análise {}", root.display())))?)
    }

    fn open_snapshot(&self, path: &str) -> ApiResult<Snapshot> {
        Ok(Snapshot::open(&snapshot_path(path)?)?)
    }

    pub fn snapshot_summary(&self, path: &str) -> ApiResult<Summary> {
        Ok(self.open_snapshot(path)?.summary()?)
    }

    pub fn snapshot_list_dir(&self, path: &str, dir_id: i64, offset: u32, limit: Option<u32>, request_id: String) -> ApiResult<Paged<DirRow>> {
        let t = std::time::Instant::now();
        let rows = self.open_snapshot(path)?.list_dir(dir_id, offset, page(limit))?;
        Ok(Paged { request_id, rows, elapsed_ms: t.elapsed().as_millis() as u64 })
    }

    /// Tree view: only subdirectories, largest first, paginated.
    pub fn snapshot_list_subdirs(&self, path: &str, dir_id: i64, offset: u32, limit: Option<u32>, request_id: String) -> ApiResult<Paged<DirRow>> {
        let t = std::time::Instant::now();
        let rows = self.open_snapshot(path)?.list_subdirs(dir_id, offset, page(limit))?;
        Ok(Paged { request_id, rows, elapsed_ms: t.elapsed().as_millis() as u64 })
    }

    /// Breadcrumb chain and child counts (for pagination) of one directory.
    pub fn snapshot_dir_info(&self, path: &str, dir_id: i64) -> ApiResult<DirInfo> {
        let s = self.open_snapshot(path)?;
        let chain = s.dir_chain(dir_id)?;
        let (subdirs, files) = s.child_counts(dir_id)?;
        Ok(DirInfo { chain, subdirs, files })
    }

    pub fn snapshot_top_files(&self, path: &str, limit: Option<u32>) -> ApiResult<Vec<FileRow>> {
        Ok(self.open_snapshot(path)?.top_files(page(limit))?)
    }

    pub fn snapshot_top_dirs(&self, path: &str, limit: Option<u32>) -> ApiResult<Vec<DirRow>> {
        Ok(self.open_snapshot(path)?.top_dirs(page(limit))?)
    }

    /// Cancellable query: the token is registered under `request_id` so the
    /// UI can cancel it; a newer request with the same id replaces it.
    pub fn snapshot_query(&self, path: &str, q: QueryRequest, request_id: String) -> ApiResult<Paged<FileRow>> {
        let snap = self.open_snapshot(path)?;
        let token = snap.cancel_token();
        if let Some(old) = self.queries.lock().unwrap_or_else(|p| p.into_inner()).insert(request_id.clone(), token) {
            old.store(true, Ordering::SeqCst);
        }
        let t = std::time::Instant::now();
        let fq = FileQuery {
            min_size: q.min_size,
            max_size: q.max_size,
            ext: q.ext.filter(|e| !e.trim().is_empty()),
            modified_before: q.modified_before,
            modified_after: q.modified_after,
            year: q.year,
            name_contains: q.name_contains.filter(|n| !n.trim().is_empty()),
            sort_by_size_desc: true,
            offset: q.offset,
            limit: page(q.limit),
            time_limit: Some(Duration::from_secs(30)),
        };
        let res = snap.query_files(&fq);
        self.queries.lock().unwrap_or_else(|p| p.into_inner()).remove(&request_id);
        Ok(Paged { request_id, rows: res?, elapsed_ms: t.elapsed().as_millis() as u64 })
    }

    pub fn snapshot_cancel_query(&self, request_id: &str) -> bool {
        match self.queries.lock().unwrap_or_else(|p| p.into_inner()).get(request_id) {
            Some(t) => {
                t.store(true, Ordering::SeqCst);
                true
            }
            None => false,
        }
    }

    pub fn snapshot_stats(&self, path: &str) -> ApiResult<Stats> {
        let s = self.open_snapshot(path)?;
        Ok(Stats { extensions: s.ext_stats(30)?, years: s.year_stats()? })
    }

    fn copy_job(&self, req: &CopyRequest) -> ApiResult<CopyJob> {
        let profile = CopyProfile::parse(&req.profile).ok_or_else(|| ApiError::new("invalid_argument", "perfil de cópia inválido"))?;
        let mut job = CopyJob::new(abs_path(&req.source)?, abs_path(&req.dest)?, profile);
        if let Some(t) = req.threads {
            job.options.threads = t;
        }
        if let Some(v) = &req.verify {
            job.options.verify = match v.as_str() {
                "basic" => VerifyLevel::Basic,
                "standard" => VerifyLevel::Standard,
                "strict" => VerifyLevel::Strict,
                _ => return Err(ApiError::new("invalid_argument", "nível de verificação inválido")),
            };
        }
        job.options.filters.exclude_files = req.exclude_files.iter().filter(|s| !s.trim().is_empty()).cloned().collect();
        job.options.filters.exclude_dirs = req.exclude_dirs.iter().filter(|s| !s.trim().is_empty()).cloned().collect();
        job.approved_deletions = req.approved_deletions;
        job.validate().map_err(|e| ApiError::new("invalid_job", e.to_string()))?;
        Ok(job)
    }

    pub fn copy_plan(&self, req: CopyRequest) -> ApiResult<DryRunReport> {
        let job = self.copy_job(&req)?;
        if !job.source.is_dir() {
            return Err(ApiError::new("not_found", format!("origem não encontrada ou inacessível: {}", job.source.display())));
        }
        #[cfg(windows)]
        {
            use nextcopy_core::CopyEngine;
            let eng = nextcopy_robocopy::RobocopyEngine::locate().map_err(|e| ApiError::new("engine", e.to_string()))?;
            eng.dry_run(&job, &self.data_dir.join("logs").join("copy").join("plans"), &Default::default())
                .map_err(|e| ApiError::new("engine", e.to_string()))
        }
        #[cfg(not(windows))]
        Err(ApiError::new("unsupported", "NextCopy v1 requer Windows (Robocopy)"))
    }

    pub fn copy_submit(&self, req: CopyRequest) -> ApiResult<TaskId> {
        let job = self.copy_job(&req)?;
        if job.options.mirror && job.approved_deletions.is_none() {
            return Err(ApiError::new("approval_required", "o espelhamento exclui arquivos no destino: gere a prévia e aprove as exclusões"));
        }
        let title = format!("Cópia {} → {}", job.source.display(), job.dest.display());
        Ok(self.mgr.submit(TaskSpec::Copy(CopySpec { job }).to_new_task(title))?)
    }

    pub fn alerts_list(&self, include_acknowledged: bool, limit: Option<u32>) -> ApiResult<Vec<Alert>> {
        Ok(self.mgr.store().list_alerts(include_acknowledged, page(limit))?)
    }

    pub fn alerts_ack(&self, id: i64) -> ApiResult<bool> {
        Ok(self.mgr.store().ack_alert(id)?)
    }

    pub fn settings_get(&self) -> ApiResult<HashMap<String, String>> {
        let s = self.mgr.store();
        let mut out = HashMap::new();
        for (k, _, _) in nexttool_store::SETTINGS {
            if let Some(v) = s.get_setting(k)? {
                out.insert(k.to_string(), v);
            }
        }
        Ok(out)
    }

    pub fn settings_set(&self, key: &str, value: &str) -> ApiResult<()> {
        nexttool_store::validate_setting(key, value).map_err(|e| ApiError::new("invalid_argument", e.to_string()))?;
        Ok(self.mgr.store().set_setting(key, value)?)
    }
}

fn robocopy_path() -> Option<String> {
    #[cfg(windows)]
    {
        nextcopy_robocopy::RobocopyEngine::locate().ok().map(|e| e.exe().display().to_string())
    }
    #[cfg(not(windows))]
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn core() -> (tempfile::TempDir, Core) {
        let t = tempfile::tempdir().unwrap();
        let (c, rec) = Core::open(t.path().join("data")).unwrap();
        assert!(rec.is_empty());
        (t, c)
    }

    #[test]
    fn command_list_matches_capability() {
        use crate::command_list::COMMANDS;
        let cap: serde_json::Value = serde_json::from_str(include_str!("../capabilities/default.json")).unwrap();
        let perms: Vec<String> = cap["permissions"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
        for c in COMMANDS {
            let p = format!("allow-{}", c.replace('_', "-"));
            assert!(perms.contains(&p), "capability is missing {p}");
        }
        let allows = perms.iter().filter(|p| p.starts_with("allow-")).count();
        assert_eq!(allows, COMMANDS.len(), "capability allows commands that do not exist");
    }

    #[test]
    fn scan_end_to_end_through_ipc_layer() {
        let (t, c) = core();
        let root = t.path().join("tree");
        std::fs::create_dir_all(root.join("a")).unwrap();
        for i in 0..50 {
            std::fs::write(root.join("a").join(format!("{i}.bak")), vec![0u8; i * 10]).unwrap();
        }
        let id = c.size_scan_submit(ScanRequest { root: root.display().to_string(), snapshot: None, profile: "balanced".into() }).unwrap();
        let done = c.mgr.run_until_done(&[id], &mut |_| {}).unwrap();
        assert_eq!(done[0].state, TaskState::Completed, "{:?}", done[0]);
        let snap: serde_json::Value = serde_json::from_str(done[0].checkpoint_json.as_ref().unwrap()).unwrap();
        let path = snap["snapshot"].as_str().unwrap().to_string();
        let s = c.snapshot_summary(&path).unwrap();
        assert_eq!(s.files, 50);
        assert!(s.complete);
        let top = c.snapshot_top_files(&path, Some(3)).unwrap();
        assert_eq!(top[0].size, 490);
        let q = c.snapshot_query(&path, QueryRequest { ext: Some("bak".into()), min_size: Some(250), ..Default::default() }, "r1".into()).unwrap();
        assert_eq!(q.request_id, "r1");
        assert_eq!(q.rows.len(), 25);
        let ls = c.snapshot_list_dir(&path, 1, 0, Some(10), "r2".into()).unwrap();
        assert_eq!(ls.rows.len(), 1);
        let sub = c.snapshot_list_subdirs(&path, 1, 0, Some(10), "r3".into()).unwrap();
        assert_eq!(sub.rows.len(), 1);
        assert_eq!(sub.rows[0].name, "a");
        let info = c.snapshot_dir_info(&path, sub.rows[0].id).unwrap();
        assert_eq!(info.chain.len(), 2);
        assert_eq!((info.subdirs, info.files), (0, 50));
        assert_eq!(q.rows[0].parent_id, sub.rows[0].id, "file rows carry their folder id");
        assert!(c.snapshot_stats(&path).unwrap().extensions.iter().any(|e| e.ext == "bak"));
        let ov = c.overview().unwrap();
        assert_eq!(ov.recent_scans.len(), 1);
        assert!(ov.active.is_empty());
    }

    #[test]
    fn inputs_are_validated() {
        let (t, c) = core();
        assert_eq!(c.size_scan_submit(ScanRequest { root: r"..\x".into(), snapshot: None, profile: "balanced".into() }).unwrap_err().code, "invalid_path");
        assert_eq!(c.size_scan_submit(ScanRequest { root: t.path().join("nope").display().to_string(), snapshot: None, profile: "balanced".into() }).unwrap_err().code, "not_found");
        assert_eq!(c.snapshot_summary(&t.path().join("x.txt").display().to_string()).unwrap_err().code, "invalid_path");
        assert_eq!(c.snapshot_summary(&t.path().join("x.nxs").display().to_string()).unwrap_err().code, "not_found");
        assert_eq!(c.settings_set("theme", "purple").unwrap_err().code, "invalid_argument");
        assert_eq!(c.settings_set("shell", "cmd").unwrap_err().code, "invalid_argument");
        c.settings_set("theme", "dark").unwrap();
        c.settings_set("accent", "142").unwrap();
        assert_eq!(c.settings_get().unwrap()["theme"], "dark");
        let req = CopyRequest {
            source: t.path().display().to_string(),
            dest: t.path().join("inside").display().to_string(),
            profile: "safe".into(),
            threads: None,
            verify: None,
            exclude_files: vec![],
            exclude_dirs: vec![],
            approved_deletions: None,
        };
        assert_eq!(c.copy_submit(req).unwrap_err().code, "invalid_job", "dest inside source must be refused");
        let mirror = CopyRequest {
            source: t.path().join("a").display().to_string(),
            dest: t.path().join("b").display().to_string(),
            profile: "mirror".into(),
            threads: None,
            verify: None,
            exclude_files: vec![],
            exclude_dirs: vec![],
            approved_deletions: None,
        };
        assert_eq!(c.copy_submit(mirror).unwrap_err().code, "approval_required");
        assert!(c.tasks_list(Some(vec!["bogus".into()]), None, 0).is_err());
    }

    #[test]
    fn resume_only_from_resumable_states() {
        let (_t, c) = core();
        let st = c.mgr.store().create_task(&nexttool_store::NewTask {
            kind: TaskKind::Copy,
            title: "x".into(),
            spec_json: "{}".into(),
            priority: Default::default(),
            resource_keys: vec![],
            max_attempts: 1,
        });
        let id = st.unwrap();
        assert_eq!(c.task_resume(id).unwrap_err().code, "invalid_state");
    }
}
