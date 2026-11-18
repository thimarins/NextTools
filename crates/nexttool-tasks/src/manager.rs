use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use nexttool_core::alert::AlertDraft;
use nexttool_core::task::{TaskId, TaskState};
use nexttool_core::time::now_unix;
use nexttool_core::ErrorKind;
use nexttool_store::{AppStore, NewTask, StoreError, TaskRecord};

use crate::retry::RetryPolicy;

/// Max tasks waiting in the queue (bounded).
pub const MAX_QUEUED: u64 = 1_000;

/// Pause / cancel requests for one task.
#[derive(Debug, Default)]
pub struct TaskControl {
    cancel: AtomicBool,
    pause: AtomicBool,
}

impl TaskControl {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
    pub fn pause(&self) {
        self.pause.store(true, Ordering::SeqCst);
    }
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    pub fn is_pause_requested(&self) -> bool {
        self.pause.load(Ordering::Relaxed)
    }
    pub fn stop_requested(&self) -> bool {
        self.is_cancelled() || self.is_pause_requested()
    }
}

/// What a runner reports back. The manager persists it; runners never set
/// terminal states themselves.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub state: TaskState,
    pub error: Option<(ErrorKind, String)>,
    pub result_json: Option<String>,
    /// Target described in alerts (path / share).
    pub target: String,
}

impl RunOutcome {
    pub fn failed(kind: ErrorKind, msg: impl Into<String>, target: impl Into<String>) -> Self {
        RunOutcome { state: TaskState::Failed, error: Some((kind, msg.into())), result_json: None, target: target.into() }
    }
}

pub trait TaskRunner: Send {
    fn run(&mut self, ctx: &TaskCtx) -> RunOutcome;
}

/// Context handed to a runner.
pub struct TaskCtx {
    pub id: TaskId,
    pub title: String,
    pub module: nexttool_core::task::Module,
    pub data_dir: PathBuf,
    pub control: Arc<TaskControl>,
    store: Arc<Mutex<AppStore>>,
    last_progress: Mutex<Option<Instant>>,
}

fn lock(s: &Mutex<AppStore>) -> MutexGuard<'_, AppStore> {
    s.lock().unwrap_or_else(|p| p.into_inner())
}

impl TaskCtx {
    /// Persists progress at most once per second.
    pub fn progress(&self, json: &str) {
        let mut g = self.last_progress.lock().unwrap_or_else(|p| p.into_inner());
        if g.is_some_and(|t| t.elapsed() < Duration::from_secs(1)) {
            return;
        }
        *g = Some(Instant::now());
        if let Err(e) = lock(&self.store).set_progress(self.id, json) {
            tracing::warn!("progress persist failed: {e}");
        }
    }

    pub fn checkpoint(&self, json: &str) {
        if let Err(e) = lock(&self.store).set_checkpoint(self.id, json) {
            tracing::warn!("checkpoint persist failed: {e}");
        }
    }

    pub fn event(&self, level: &str, msg: &str) {
        let _ = lock(&self.store).add_event(self.id, level, msg);
    }

    pub fn alert(&self, d: &AlertDraft) {
        if let Err(e) = lock(&self.store).raise_alert(d) {
            tracing::warn!("alert persist failed: {e}");
        }
    }

    pub fn set_child(&self, pid: Option<u32>) {
        let child = pid.map(|p| (p, nexttool_core::process::start_of(p)));
        let _ = lock(&self.store).set_child(self.id, child);
    }
}

/// Builds a runner from a stored task (kind + spec JSON).
pub type RunnerFactory = Arc<dyn Fn(&TaskRecord) -> Result<Box<dyn TaskRunner>, String> + Send + Sync>;

struct Running {
    control: Arc<TaskControl>,
    keys: Vec<String>,
    handle: Option<JoinHandle<()>>,
}

/// Queue + scheduler. Rules:
/// - at most `max_running` tasks at once (bounded);
/// - two tasks sharing a resource key (volume / share) never run together;
/// - higher priority first, FIFO within a priority;
/// - transient failures retried with bounded backoff; others need the user.
pub struct TaskManager {
    store: Arc<Mutex<AppStore>>,
    data_dir: PathBuf,
    factory: RunnerFactory,
    running: Arc<Mutex<HashMap<TaskId, Running>>>,
    max_running: usize,
    retry: RetryPolicy,
    shutdown: Arc<AtomicBool>,
}

impl TaskManager {
    pub fn new(store: AppStore, data_dir: PathBuf, factory: RunnerFactory, max_running: usize) -> Self {
        TaskManager {
            store: Arc::new(Mutex::new(store)),
            data_dir,
            factory,
            running: Arc::new(Mutex::new(HashMap::new())),
            max_running: max_running.clamp(1, 16),
            retry: RetryPolicy::default(),
            shutdown: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn with_retry_policy(mut self, p: RetryPolicy) -> Self {
        self.retry = p;
        self
    }

    pub fn store(&self) -> MutexGuard<'_, AppStore> {
        lock(&self.store)
    }

    pub fn submit(&self, t: NewTask) -> Result<TaskId, StoreError> {
        let s = self.store();
        if s.count_in_states(&[TaskState::Queued, TaskState::WaitingRetry])? >= MAX_QUEUED {
            return Err(StoreError::Io(std::io::Error::other(format!("fila cheia ({MAX_QUEUED} tarefas aguardando)"))));
        }
        let nt = NewTask { max_attempts: t.max_attempts.max(1).min(self.retry.max_attempts), ..t };
        s.create_task(&nt)
    }

    pub fn pause(&self, id: TaskId) -> bool {
        self.control_of(id).map(|c| c.pause()).is_some()
    }

    pub fn cancel(&self, id: TaskId) -> Result<bool, StoreError> {
        if let Some(c) = self.control_of(id) {
            c.cancel();
            self.store().set_state(id, TaskState::Cancelling, None)?;
            return Ok(true);
        }
        // Not running: cancel a queued / waiting / paused task directly.
        let t = self.store().get_task(id)?;
        if matches!(t.state, TaskState::Queued | TaskState::WaitingRetry | TaskState::Paused | TaskState::Interrupted) {
            self.store().set_state(id, TaskState::Cancelled, None)?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Re-queues a paused / interrupted / failed task (explicit user action).
    pub fn resume(&self, id: TaskId) -> Result<(), StoreError> {
        self.store().requeue(id)
    }

    fn control_of(&self, id: TaskId) -> Option<Arc<TaskControl>> {
        self.running.lock().unwrap_or_else(|p| p.into_inner()).get(&id).map(|r| r.control.clone())
    }

    pub fn running_ids(&self) -> Vec<TaskId> {
        self.running.lock().unwrap_or_else(|p| p.into_inner()).keys().copied().collect()
    }

    /// One scheduling pass. Returns the tasks started.
    pub fn tick(&self) -> Result<Vec<TaskId>, StoreError> {
        self.reap();
        self.store().release_due_retries(now_unix())?;
        let mut started = Vec::new();
        let queued = self.store().queued(100)?;
        for t in queued {
            let mut running = self.running.lock().unwrap_or_else(|p| p.into_inner());
            if running.len() >= self.max_running {
                break;
            }
            let busy = running.values().any(|r| r.keys.iter().any(|k| t.resource_keys.contains(k)));
            if busy {
                continue; // competing for the same volume/share: wait
            }
            let control = Arc::new(TaskControl::default());
            {
                let s = self.store();
                s.claim(t.id, std::process::id(), nexttool_core::process::current_start())?;
                s.set_state(t.id, TaskState::Running, None)?;
            }
            let handle = self.spawn(t.clone(), control.clone());
            running.insert(t.id, Running { control, keys: t.resource_keys.clone(), handle: Some(handle) });
            started.push(t.id);
        }
        Ok(started)
    }

    fn reap(&self) {
        let mut running = self.running.lock().unwrap_or_else(|p| p.into_inner());
        let done: Vec<TaskId> = running.iter().filter(|(_, r)| r.handle.as_ref().is_none_or(|h| h.is_finished())).map(|(id, _)| *id).collect();
        for id in done {
            if let Some(mut r) = running.remove(&id) {
                if let Some(h) = r.handle.take() {
                    let _ = h.join();
                }
            }
        }
    }

    fn spawn(&self, t: TaskRecord, control: Arc<TaskControl>) -> JoinHandle<()> {
        let store = self.store.clone();
        let factory = self.factory.clone();
        let data_dir = self.data_dir.clone();
        let retry = self.retry;
        std::thread::Builder::new()
            .name(format!("nexttool-task-{}", t.id))
            .spawn(move || {
                let ctx = TaskCtx {
                    id: t.id,
                    title: t.title.clone(),
                    module: t.module,
                    data_dir,
                    control: control.clone(),
                    store: store.clone(),
                    last_progress: Mutex::new(None),
                };
                let outcome = match factory(&t) {
                    Ok(mut runner) => {
                        // A panicking runner must never leave the task "running".
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runner.run(&ctx))) {
                            Ok(o) => o,
                            Err(_) => RunOutcome::failed(ErrorKind::Internal, "falha interna (pânico) no executor da tarefa", &t.title),
                        }
                    }
                    Err(e) => RunOutcome::failed(ErrorKind::Internal, e, &t.title),
                };
                finish(&store, &t, &ctx, outcome, retry);
            })
            .expect("spawn task thread")
    }

    /// Runs the scheduler until every task in `ids` reached a non-active state.
    /// Used by the CLI; the desktop runs `tick` from its own loop.
    pub fn run_until_done(&self, ids: &[TaskId], on_tick: &mut dyn FnMut(&[TaskRecord])) -> Result<Vec<TaskRecord>, StoreError> {
        loop {
            self.tick()?;
            let recs: Vec<TaskRecord> = ids.iter().map(|id| self.store().get_task(*id)).collect::<Result<_, _>>()?;
            on_tick(&recs);
            let running = self.running_ids();
            let active = recs.iter().any(|r| matches!(r.state, TaskState::Queued | TaskState::Running | TaskState::Cancelling) || running.contains(&r.id));
            if !active || self.shutdown.load(Ordering::Relaxed) {
                self.reap();
                return ids.iter().map(|id| self.store().get_task(*id)).collect();
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }

    /// Requests a safe stop of every running task (pause semantics) and waits.
    pub fn shutdown(&self, timeout: Duration) {
        self.shutdown.store(true, Ordering::SeqCst);
        for id in self.running_ids() {
            self.pause(id);
        }
        let t0 = Instant::now();
        while !self.running_ids().is_empty() && t0.elapsed() < timeout {
            self.reap();
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

fn finish(store: &Arc<Mutex<AppStore>>, t: &TaskRecord, ctx: &TaskCtx, o: RunOutcome, retry: RetryPolicy) {
    let s = lock(store);
    if let Some(json) = &o.result_json {
        let _ = s.set_result(t.id, json);
    }
    let _ = s.set_child(t.id, None);
    let attempt = s.get_task(t.id).map(|r| r.attempt).unwrap_or(1);
    let mut final_state = o.state;
    let mut will_retry = false;
    if o.state == TaskState::Failed {
        if let Some((kind, _)) = &o.error {
            if retry.should_retry(*kind, attempt) && attempt < t.max_attempts {
                will_retry = true;
            }
        }
    }
    if will_retry {
        let at = now_unix() + retry.delay_s(attempt) as i64;
        let (k, m) = o.error.clone().unwrap();
        let _ = s.set_state(t.id, TaskState::WaitingRetry, Some((k, &m)));
        let _ = s.schedule_retry(t.id, at);
        final_state = TaskState::WaitingRetry;
    } else {
        let err = o.error.as_ref().map(|(k, m)| (*k, m.as_str()));
        let _ = s.set_state(t.id, final_state, err);
    }
    if let Some((kind, msg)) = &o.error {
        if final_state != TaskState::Cancelled {
            let d = crate::alerts::for_error(ctx.module, t.id, *kind, &o.target, will_retry, msg.clone());
            let _ = s.raise_alert(&d);
        }
    }
    tracing::info!(task = t.id, state = final_state.as_str(), "task finished");
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexttool_core::task::{Priority, TaskKind};

    struct Fake {
        outcome: RunOutcome,
        sleep_ms: u64,
    }

    impl TaskRunner for Fake {
        fn run(&mut self, ctx: &TaskCtx) -> RunOutcome {
            let t0 = Instant::now();
            while t0.elapsed() < Duration::from_millis(self.sleep_ms) {
                if ctx.control.is_cancelled() {
                    return RunOutcome { state: TaskState::Cancelled, error: None, result_json: None, target: String::new() };
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            self.outcome.clone()
        }
    }

    fn mgr(factory: RunnerFactory, max: usize) -> (tempfile::TempDir, TaskManager) {
        let t = tempfile::tempdir().unwrap();
        let (s, _) = AppStore::open(&t.path().join("app.db")).unwrap();
        let m = TaskManager::new(s, t.path().to_path_buf(), factory, max)
            .with_retry_policy(RetryPolicy { base_s: 0, factor: 1, max_s: 0, max_attempts: 3 });
        (t, m)
    }

    fn task(keys: &[&str]) -> NewTask {
        NewTask { kind: TaskKind::Copy, title: "x".into(), spec_json: "{}".into(), priority: Priority::Normal, resource_keys: keys.iter().map(|k| k.to_string()).collect(), max_attempts: 3 }
    }

    #[test]
    fn same_resource_runs_sequentially_and_limit_is_respected() {
        let f: RunnerFactory = Arc::new(|_| {
            Ok(Box::new(Fake { outcome: RunOutcome { state: TaskState::Completed, error: None, result_json: None, target: String::new() }, sleep_ms: 200 }))
        });
        let (_t, m) = mgr(f, 2);
        let a = m.submit(task(&["vol:c:"])).unwrap();
        let b = m.submit(task(&["vol:c:"])).unwrap();
        let c = m.submit(task(&["vol:d:"])).unwrap();
        let started = m.tick().unwrap();
        assert_eq!(started, vec![a, c], "b shares vol:c: with a and must wait");
        let done = m.run_until_done(&[a, b, c], &mut |_| {}).unwrap();
        assert!(done.iter().all(|r| r.state == TaskState::Completed));
    }

    #[test]
    fn transient_failure_is_retried_then_gives_up() {
        let f: RunnerFactory = Arc::new(|_| Ok(Box::new(Fake { outcome: RunOutcome::failed(ErrorKind::Network, "rede", r"\\nas\x"), sleep_ms: 0 })));
        let (_t, m) = mgr(f, 1);
        let id = m.submit(task(&["a"])).unwrap();
        let r = m.run_until_done(&[id], &mut |_| {}).unwrap();
        // run_until_done returns when not active; waiting_retry → keep ticking until final.
        let mut rec = r[0].clone();
        for _ in 0..50 {
            if rec.state == TaskState::Failed {
                break;
            }
            rec = m.run_until_done(&[id], &mut |_| {}).unwrap()[0].clone();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(rec.state, TaskState::Failed);
        assert_eq!(rec.attempt, 3);
        let alerts = m.store().list_alerts(false, 10).unwrap();
        assert_eq!(alerts.len(), 1, "grouped into one alert");
        assert!(alerts[0].count >= 3);
    }

    #[test]
    fn permanent_failure_is_not_retried_and_panic_is_contained() {
        let f: RunnerFactory = Arc::new(|t: &TaskRecord| {
            if t.title == "panic" {
                struct P;
                impl TaskRunner for P {
                    fn run(&mut self, _: &TaskCtx) -> RunOutcome {
                        panic!("boom")
                    }
                }
                Ok(Box::new(P))
            } else {
                Ok(Box::new(Fake { outcome: RunOutcome::failed(ErrorKind::AccessDenied, "negado", r"C:\x"), sleep_ms: 0 }))
            }
        });
        let (_t, m) = mgr(f, 2);
        let a = m.submit(task(&["a"])).unwrap();
        let b = m.submit(NewTask { title: "panic".into(), ..task(&["b"]) }).unwrap();
        let r = m.run_until_done(&[a, b], &mut |_| {}).unwrap();
        assert_eq!(r[0].state, TaskState::Failed);
        assert_eq!(r[0].attempt, 1);
        assert_eq!(r[1].state, TaskState::Failed);
        assert_eq!(r[1].error_kind.as_deref(), Some("internal"));
    }

    #[test]
    fn cancel_running_task() {
        let f: RunnerFactory = Arc::new(|_| Ok(Box::new(Fake { outcome: RunOutcome { state: TaskState::Completed, error: None, result_json: None, target: String::new() }, sleep_ms: 10_000 })));
        let (_t, m) = mgr(f, 1);
        let id = m.submit(task(&["a"])).unwrap();
        m.tick().unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert!(m.cancel(id).unwrap());
        let r = m.run_until_done(&[id], &mut |_| {}).unwrap();
        assert_eq!(r[0].state, TaskState::Cancelled);
    }
}
