//! Background loop: runs the scheduler and pushes changes to the UI as
//! events (≤ 4 Hz per task). The UI never polls the engines directly.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use nexttool_core::task::{TaskId, TaskState};
use tauri::{AppHandle, Emitter};

use crate::ipc::Core;

const ACTIVE: &[TaskState] = &[TaskState::Queued, TaskState::Running, TaskState::Cancelling, TaskState::WaitingRetry];

pub fn start(app: AppHandle, core: Arc<Core>) {
    std::thread::Builder::new()
        .name("nexttool-pump".into())
        .spawn(move || run(app, core))
        .expect("spawn event pump");
}

fn run(app: AppHandle, core: Arc<Core>) {
    // task id → (state, updated_at, progress) last sent
    let mut sent: HashMap<TaskId, (TaskState, i64, Option<String>)> = HashMap::new();
    let mut alerts: HashMap<i64, i64> = HashMap::new(); // id → count
    let mut first = true;
    loop {
        if let Err(e) = core.mgr.tick() {
            tracing::warn!("scheduler tick failed: {e}");
        }
        let active = core.mgr.store().list_tasks(Some(ACTIVE), 200, 0);
        if let Ok(active) = active {
            let ids: Vec<TaskId> = active.iter().map(|t| t.id).collect();
            for t in active {
                let key = (t.state, t.updated_at, t.progress_json.clone());
                if sent.get(&t.id) != Some(&key) {
                    let _ = app.emit("task://update", &t);
                    sent.insert(t.id, key);
                }
            }
            // Tasks that left the active set: send their final record once.
            let gone: Vec<TaskId> = sent.keys().filter(|id| !ids.contains(id)).copied().collect();
            for id in gone {
                sent.remove(&id);
                if let Ok(t) = core.mgr.store().get_task(id) {
                    let _ = app.emit("task://update", &t);
                }
            }
        }
        if let Ok(list) = core.mgr.store().list_alerts(false, 50) {
            for a in list {
                if alerts.get(&a.id) != Some(&a.count) {
                    if !first {
                        let _ = app.emit("alert://raised", &a);
                    }
                    alerts.insert(a.id, a.count);
                }
            }
        }
        first = false;
        std::thread::sleep(Duration::from_millis(250));
    }
}
