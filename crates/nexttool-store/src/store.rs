use std::path::{Path, PathBuf};

use nexttool_core::alert::{Alert, AlertDraft};
use nexttool_core::task::{Module, Priority, TaskId, TaskKind, TaskState};
use nexttool_core::time::now_unix;
use nexttool_core::{ErrorKind, Severity};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

const SCHEMA_VERSION: i64 = 1;
/// Max events kept per task (older ones are pruned).
const EVENTS_PER_TASK: i64 = 5_000;
/// An open alert with the same fingerprint seen within this window is grouped.
const GROUP_WINDOW_S: i64 = 3_600;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tasks(
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL, module TEXT NOT NULL, title TEXT NOT NULL,
  spec TEXT NOT NULL, state TEXT NOT NULL, priority INTEGER NOT NULL DEFAULT 1,
  resource_keys TEXT NOT NULL DEFAULT '[]',
  attempt INTEGER NOT NULL DEFAULT 0, max_attempts INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL, started_at INTEGER, finished_at INTEGER, updated_at INTEGER NOT NULL,
  next_retry_at INTEGER,
  progress TEXT, result TEXT, checkpoint TEXT,
  error_kind TEXT, error_message TEXT,
  owner_pid INTEGER, owner_start INTEGER, child_pid INTEGER, child_start INTEGER
);
CREATE INDEX IF NOT EXISTS idx_tasks_state ON tasks(state);
CREATE TABLE IF NOT EXISTS task_events(
  id INTEGER PRIMARY KEY, task_id INTEGER NOT NULL, at INTEGER NOT NULL, level TEXT NOT NULL, message TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_task ON task_events(task_id, id);
CREATE TABLE IF NOT EXISTS alerts(
  id INTEGER PRIMARY KEY, fingerprint TEXT NOT NULL, module TEXT NOT NULL, task_id INTEGER,
  severity TEXT NOT NULL, title TEXT NOT NULL, problem TEXT NOT NULL, probable_cause TEXT,
  consequence TEXT NOT NULL, action TEXT NOT NULL, recoverable INTEGER NOT NULL, details TEXT,
  first_at INTEGER NOT NULL, last_at INTEGER NOT NULL, count INTEGER NOT NULL DEFAULT 1,
  acknowledged INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_alerts_fp ON alerts(fingerprint, acknowledged);
CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL) WITHOUT ROWID;
"#;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("versão do banco de dados ({found}) é mais nova que a suportada ({SCHEMA_VERSION}); atualize o NextTool")]
    TooNew { found: i64 },
    #[error("tarefa {0} não encontrada")]
    NoTask(TaskId),
}

#[derive(Debug, Clone)]
pub struct NewTask {
    pub kind: TaskKind,
    pub title: String,
    pub spec_json: String,
    pub priority: Priority,
    /// Resources this task competes for (e.g. "vol:c:", "unc:\\\\nas\\share").
    pub resource_keys: Vec<String>,
    pub max_attempts: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskRecord {
    pub id: TaskId,
    pub kind: TaskKind,
    pub module: Module,
    pub title: String,
    pub spec_json: String,
    pub state: TaskState,
    pub priority: Priority,
    pub resource_keys: Vec<String>,
    pub attempt: u32,
    pub max_attempts: u32,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub updated_at: i64,
    pub next_retry_at: Option<i64>,
    pub progress_json: Option<String>,
    pub result_json: Option<String>,
    pub checkpoint_json: Option<String>,
    pub error_kind: Option<String>,
    pub error_message: Option<String>,
    pub child_pid: Option<u32>,
}

/// A task found active at startup (the previous process ended unexpectedly).
#[derive(Debug, Clone, Serialize)]
pub struct RecoveredTask {
    pub task: TaskRecord,
    /// Another running NextTool instance still owns it: left untouched.
    pub owned_by_live_process: bool,
    /// Its child process (e.g. Robocopy) is still running.
    pub child_still_running: bool,
}

pub struct AppStore {
    conn: Connection,
    path: PathBuf,
}

fn row_task(r: &Row) -> rusqlite::Result<TaskRecord> {
    let kind: String = r.get("kind")?;
    let module: String = r.get("module")?;
    let state: String = r.get("state")?;
    let keys: String = r.get("resource_keys")?;
    Ok(TaskRecord {
        id: r.get("id")?,
        kind: TaskKind::parse(&kind).unwrap_or(TaskKind::SizeScan),
        module: Module::parse(&module).unwrap_or(Module::System),
        title: r.get("title")?,
        spec_json: r.get("spec")?,
        state: TaskState::parse(&state).unwrap_or(TaskState::Failed),
        priority: Priority::from_i64(r.get("priority")?),
        resource_keys: serde_json::from_str(&keys).unwrap_or_default(),
        attempt: r.get::<_, i64>("attempt")? as u32,
        max_attempts: r.get::<_, i64>("max_attempts")? as u32,
        created_at: r.get("created_at")?,
        started_at: r.get("started_at")?,
        finished_at: r.get("finished_at")?,
        updated_at: r.get("updated_at")?,
        next_retry_at: r.get("next_retry_at")?,
        progress_json: r.get("progress")?,
        result_json: r.get("result")?,
        checkpoint_json: r.get("checkpoint")?,
        error_kind: r.get("error_kind")?,
        error_message: r.get("error_message")?,
        child_pid: r.get::<_, Option<i64>>("child_pid")?.map(|v| v as u32),
    })
}

fn row_alert(r: &Row) -> rusqlite::Result<Alert> {
    let module: String = r.get("module")?;
    let sev: String = r.get("severity")?;
    Ok(Alert {
        id: r.get("id")?,
        draft: AlertDraft {
            module: Module::parse(&module).unwrap_or(Module::System),
            task_id: r.get("task_id")?,
            severity: Severity::parse(&sev).unwrap_or(Severity::Error),
            fingerprint: r.get("fingerprint")?,
            title: r.get("title")?,
            problem: r.get("problem")?,
            probable_cause: r.get("probable_cause")?,
            consequence: r.get("consequence")?,
            recommended_action: r.get("action")?,
            recoverable: r.get::<_, i64>("recoverable")? != 0,
            details: r.get("details")?,
        },
        first_at: r.get("first_at")?,
        last_at: r.get("last_at")?,
        count: r.get("count")?,
        acknowledged: r.get::<_, i64>("acknowledged")? != 0,
    })
}

impl AppStore {
    /// Opens (or creates) the store. If the file is corrupt it is moved aside
    /// (`app.db.corrupt-<ts>`), a new one is created, and the returned note
    /// explains what happened (the caller raises a critical alert).
    pub fn open(path: &Path) -> Result<(AppStore, Option<String>), StoreError> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut note = None;
        let conn = match Self::try_open(path) {
            Ok(c) => c,
            Err(e @ StoreError::TooNew { .. }) => return Err(e),
            Err(e) => {
                let ts = now_unix();
                let aside = path.with_extension(format!("db.corrupt-{ts}"));
                std::fs::rename(path, &aside)?;
                for ext in ["db-wal", "db-shm"] {
                    let side = path.with_extension(ext);
                    if side.exists() {
                        let _ = std::fs::rename(&side, aside.with_extension(format!("{ext}.corrupt-{ts}")));
                    }
                }
                note = Some(format!(
                    "O banco de estado do aplicativo estava corrompido ({e}). Ele foi preservado em {} e um novo foi criado.",
                    aside.display()
                ));
                Self::try_open(path)?
            }
        };
        Ok((AppStore { conn, path: path.to_path_buf() }, note))
    }

    fn try_open(path: &Path) -> Result<Connection, StoreError> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        let qc: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if qc != "ok" {
            return Err(StoreError::Sqlite(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CORRUPT),
                Some(qc),
            )));
        }
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?; // small DB; durability first
        let v: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v > SCHEMA_VERSION {
            return Err(StoreError::TooNew { found: v });
        }
        conn.execute_batch(SCHEMA)?;
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        Ok(conn)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    // ---------------------------------------------------------------- tasks

    pub fn create_task(&self, t: &NewTask) -> Result<TaskId, StoreError> {
        let now = now_unix();
        self.conn.execute(
            "INSERT INTO tasks(kind,module,title,spec,state,priority,resource_keys,max_attempts,created_at,updated_at)
             VALUES(?1,?2,?3,?4,'queued',?5,?6,?7,?8,?8)",
            params![
                t.kind.as_str(),
                t.kind.module().as_str(),
                t.title,
                t.spec_json,
                t.priority as i64,
                serde_json::to_string(&t.resource_keys).unwrap_or_else(|_| "[]".into()),
                t.max_attempts.max(1),
                now
            ],
        )?;
        let id = self.conn.last_insert_rowid();
        self.add_event(id, "info", "tarefa criada")?;
        Ok(id)
    }

    pub fn get_task(&self, id: TaskId) -> Result<TaskRecord, StoreError> {
        self.conn
            .query_row("SELECT * FROM tasks WHERE id=?1", [id], row_task)
            .optional()?
            .ok_or(StoreError::NoTask(id))
    }

    pub fn list_tasks(&self, states: Option<&[TaskState]>, limit: u32, offset: u32) -> Result<Vec<TaskRecord>, StoreError> {
        let mut sql = String::from("SELECT * FROM tasks");
        if let Some(st) = states {
            let list: Vec<String> = st.iter().map(|s| format!("'{}'", s.as_str())).collect();
            sql.push_str(&format!(" WHERE state IN ({})", list.join(",")));
        }
        sql.push_str(" ORDER BY id DESC LIMIT ?1 OFFSET ?2");
        let mut st = self.conn.prepare(&sql)?;
        let rows = st.query_map(params![limit.min(1000), offset], row_task)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Queued tasks in scheduling order (priority desc, then FIFO).
    pub fn queued(&self, limit: u32) -> Result<Vec<TaskRecord>, StoreError> {
        let mut st = self.conn.prepare("SELECT * FROM tasks WHERE state='queued' ORDER BY priority DESC, id ASC LIMIT ?1")?;
        let rows = st.query_map([limit], row_task)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn count_in_states(&self, states: &[TaskState]) -> Result<u64, StoreError> {
        let list: Vec<String> = states.iter().map(|s| format!("'{}'", s.as_str())).collect();
        let n: i64 = self.conn.query_row(&format!("SELECT COUNT(*) FROM tasks WHERE state IN ({})", list.join(",")), [], |r| r.get(0))?;
        Ok(n as u64)
    }

    pub fn set_state(&self, id: TaskId, state: TaskState, error: Option<(ErrorKind, &str)>) -> Result<(), StoreError> {
        let now = now_unix();
        let (ek, em) = match error {
            Some((k, m)) => (Some(k.as_str()), Some(m)),
            None => (None, None),
        };
        self.conn.execute(
            "UPDATE tasks SET state=?2, updated_at=?3,
               started_at = CASE WHEN ?2='running' AND started_at IS NULL THEN ?3 ELSE started_at END,
               finished_at = CASE WHEN ?2 IN ('completed','completed_with_warnings','failed','cancelled') THEN ?3 ELSE finished_at END,
               error_kind = COALESCE(?4, CASE WHEN ?2='running' THEN NULL ELSE error_kind END),
               error_message = COALESCE(?5, CASE WHEN ?2='running' THEN NULL ELSE error_message END)
             WHERE id=?1",
            params![id, state.as_str(), now, ek, em],
        )?;
        self.add_event(id, if error.is_some() { "error" } else { "info" }, &format!("estado: {}", state.as_str()))?;
        Ok(())
    }

    /// Atomically moves a *queued* task to running and records the owner.
    /// Returns false if another scheduler (another process) claimed it first.
    pub fn try_claim(&self, id: TaskId, pid: u32, start: Option<u64>) -> Result<bool, StoreError> {
        let now = now_unix();
        let n = self.conn.execute(
            "UPDATE tasks SET state='running', owner_pid=?2, owner_start=?3, attempt=attempt+1, updated_at=?4,
                    started_at=COALESCE(started_at, ?4), error_kind=NULL, error_message=NULL
             WHERE id=?1 AND state='queued'",
            params![id, pid, start.map(|v| v as i64), now],
        )?;
        if n == 1 {
            self.add_event(id, "info", "estado: running")?;
        }
        Ok(n == 1)
    }

    pub fn claim(&self, id: TaskId, pid: u32, start: Option<u64>) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE tasks SET owner_pid=?2, owner_start=?3, attempt=attempt+1, updated_at=?4 WHERE id=?1",
            params![id, pid, start.map(|v| v as i64), now_unix()],
        )?;
        Ok(())
    }

    pub fn set_child(&self, id: TaskId, child: Option<(u32, Option<u64>)>) -> Result<(), StoreError> {
        self.conn.execute(
            "UPDATE tasks SET child_pid=?2, child_start=?3 WHERE id=?1",
            params![id, child.map(|c| c.0), child.and_then(|c| c.1).map(|v| v as i64)],
        )?;
        Ok(())
    }

    pub fn set_progress(&self, id: TaskId, json: &str) -> Result<(), StoreError> {
        self.conn.execute("UPDATE tasks SET progress=?2, updated_at=?3 WHERE id=?1", params![id, json, now_unix()])?;
        Ok(())
    }

    pub fn set_result(&self, id: TaskId, json: &str) -> Result<(), StoreError> {
        self.conn.execute("UPDATE tasks SET result=?2 WHERE id=?1", params![id, json])?;
        Ok(())
    }

    pub fn set_checkpoint(&self, id: TaskId, json: &str) -> Result<(), StoreError> {
        self.conn.execute("UPDATE tasks SET checkpoint=?2, updated_at=?3 WHERE id=?1", params![id, json, now_unix()])?;
        Ok(())
    }

    pub fn schedule_retry(&self, id: TaskId, at: i64) -> Result<(), StoreError> {
        self.conn.execute("UPDATE tasks SET state='waiting_retry', next_retry_at=?2, updated_at=?3 WHERE id=?1", params![id, at, now_unix()])?;
        self.add_event(id, "warn", &format!("nova tentativa agendada para {at}"))?;
        Ok(())
    }

    /// Moves due retries back to the queue.
    pub fn release_due_retries(&self, now: i64) -> Result<usize, StoreError> {
        Ok(self.conn.execute("UPDATE tasks SET state='queued', next_retry_at=NULL WHERE state='waiting_retry' AND next_retry_at<=?1", [now])?)
    }

    /// Re-queues a paused/interrupted/failed task (explicit user action).
    pub fn requeue(&self, id: TaskId) -> Result<(), StoreError> {
        let t = self.get_task(id)?;
        if t.state == TaskState::Running || t.state == TaskState::Cancelling {
            return Ok(());
        }
        self.conn.execute(
            "UPDATE tasks SET state='queued', finished_at=NULL, next_retry_at=NULL, error_kind=NULL, error_message=NULL, updated_at=?2 WHERE id=?1",
            params![id, now_unix()],
        )?;
        self.add_event(id, "info", "tarefa recolocada na fila")?;
        Ok(())
    }

    pub fn add_event(&self, id: TaskId, level: &str, msg: &str) -> Result<(), StoreError> {
        self.conn.execute("INSERT INTO task_events(task_id,at,level,message) VALUES(?1,?2,?3,?4)", params![id, now_unix(), level, msg])?;
        let ev_id = self.conn.last_insert_rowid();
        if ev_id % 500 == 0 {
            self.conn.execute(
                "DELETE FROM task_events WHERE task_id=?1 AND id <= (SELECT id FROM task_events WHERE task_id=?1 ORDER BY id DESC LIMIT 1 OFFSET ?2)",
                params![id, EVENTS_PER_TASK],
            )?;
        }
        Ok(())
    }

    pub fn events(&self, id: TaskId, limit: u32) -> Result<Vec<(i64, String, String)>, StoreError> {
        let mut st = self.conn.prepare("SELECT at, level, message FROM task_events WHERE task_id=?1 ORDER BY id DESC LIMIT ?2")?;
        let rows = st.query_map(params![id, limit.min(5000)], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// Startup recovery: tasks left Running/Cancelling by a process that no
    /// longer exists become Interrupted (never Completed).
    pub fn recover_interrupted(&self) -> Result<Vec<RecoveredTask>, StoreError> {
        let rows: Vec<(TaskRecord, Option<i64>, Option<i64>, Option<i64>)> = {
            let mut st = self.conn.prepare("SELECT *, owner_pid AS op, owner_start AS os, child_start AS cs FROM tasks WHERE state IN ('running','cancelling')")?;
            let it = st.query_map([], |r| Ok((row_task(r)?, r.get("op")?, r.get("os")?, r.get("cs")?)))?;
            it.collect::<Result<_, _>>()?
        };
        let me = std::process::id();
        let mut out = Vec::new();
        for (task, op, os, cs) in rows {
            let owner_alive = match (op, os) {
                (Some(p), Some(s)) if p as u32 != me => nexttool_core::process::is_alive(p as u32, s as u64).unwrap_or(false),
                _ => false,
            };
            if owner_alive {
                out.push(RecoveredTask { task, owned_by_live_process: true, child_still_running: false });
                continue;
            }
            let child_alive = match (task.child_pid, cs) {
                (Some(p), Some(s)) => nexttool_core::process::is_alive(p, s as u64).unwrap_or(false),
                _ => false,
            };
            let msg = if child_alive {
                "interrompida: o NextTool foi encerrado inesperadamente, mas o processo filho ainda está em execução"
            } else {
                "interrompida: o NextTool foi encerrado inesperadamente (falha, reinicialização ou queda de energia)"
            };
            self.set_state(task.id, TaskState::Interrupted, Some((ErrorKind::Internal, msg)))?;
            let task = self.get_task(task.id)?;
            out.push(RecoveredTask { task, owned_by_live_process: false, child_still_running: child_alive });
        }
        Ok(out)
    }

    // --------------------------------------------------------------- alerts

    /// Raises or groups an alert. Returns (id, grouped).
    pub fn raise_alert(&self, d: &AlertDraft) -> Result<(i64, bool), StoreError> {
        let now = now_unix();
        let existing: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT id, severity FROM alerts WHERE fingerprint=?1 AND acknowledged=0 AND last_at>=?2 ORDER BY id DESC LIMIT 1",
                params![d.fingerprint, now - GROUP_WINDOW_S],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, sev)) = existing {
            let sev = Severity::parse(&sev).unwrap_or(d.severity).max(d.severity);
            self.conn.execute(
                "UPDATE alerts SET count=count+1, last_at=?2, severity=?3, details=COALESCE(?4,details) WHERE id=?1",
                params![id, now, sev.as_str(), d.details],
            )?;
            return Ok((id, true));
        }
        self.conn.execute(
            "INSERT INTO alerts(fingerprint,module,task_id,severity,title,problem,probable_cause,consequence,action,recoverable,details,first_at,last_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?12)",
            params![
                d.fingerprint,
                d.module.as_str(),
                d.task_id,
                d.severity.as_str(),
                d.title,
                d.problem,
                d.probable_cause,
                d.consequence,
                d.recommended_action,
                d.recoverable,
                d.details,
                now
            ],
        )?;
        Ok((self.conn.last_insert_rowid(), false))
    }

    pub fn list_alerts(&self, include_acknowledged: bool, limit: u32) -> Result<Vec<Alert>, StoreError> {
        let sql = if include_acknowledged {
            "SELECT * FROM alerts ORDER BY last_at DESC LIMIT ?1"
        } else {
            "SELECT * FROM alerts WHERE acknowledged=0 ORDER BY last_at DESC LIMIT ?1"
        };
        let mut st = self.conn.prepare(sql)?;
        let rows = st.query_map([limit.min(1000)], row_alert)?.collect::<Result<_, _>>()?;
        Ok(rows)
    }

    pub fn ack_alert(&self, id: i64) -> Result<bool, StoreError> {
        Ok(self.conn.execute("UPDATE alerts SET acknowledged=1 WHERE id=?1", [id])? > 0)
    }

    // ------------------------------------------------------------- settings

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self.conn.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0)).optional()?)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [key, value])?;
        Ok(())
    }

    /// Deletes events and acknowledged alerts older than `days`.
    pub fn purge(&self, days: u32) -> Result<(usize, usize), StoreError> {
        let cutoff = now_unix() - days as i64 * 86_400;
        let e = self.conn.execute("DELETE FROM task_events WHERE at < ?1", [cutoff])?;
        let a = self.conn.execute("DELETE FROM alerts WHERE acknowledged=1 AND last_at < ?1", [cutoff])?;
        Ok((e, a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, AppStore) {
        let t = tempfile::tempdir().unwrap();
        let (s, note) = AppStore::open(&t.path().join("app.db")).unwrap();
        assert!(note.is_none());
        (t, s)
    }

    fn new_task() -> NewTask {
        NewTask { kind: TaskKind::Copy, title: "t".into(), spec_json: "{}".into(), priority: Priority::Normal, resource_keys: vec!["vol:c:".into()], max_attempts: 3 }
    }

    #[test]
    fn task_lifecycle_and_crash_recovery() {
        let (_t, s) = store();
        let id = s.create_task(&new_task()).unwrap();
        assert_eq!(s.queued(10).unwrap().len(), 1);
        // Simulate a previous process (dead pid/start) that was running it.
        s.claim(id, 999_999, Some(1)).unwrap();
        s.set_state(id, TaskState::Running, None).unwrap();
        let rec = s.recover_interrupted().unwrap();
        assert_eq!(rec.len(), 1);
        assert!(!rec[0].owned_by_live_process);
        assert_eq!(rec[0].task.state, TaskState::Interrupted);
        assert!(rec[0].task.finished_at.is_none(), "interrupted is not finished");
        s.requeue(id).unwrap();
        assert_eq!(s.get_task(id).unwrap().state, TaskState::Queued);
    }

    #[test]
    fn live_child_process_is_detected_on_recovery() {
        let (_t, s) = store();
        let id = s.create_task(&new_task()).unwrap();
        // Dead owner, but the recorded child (our own test process) is alive.
        s.claim(id, 999_998, Some(1)).unwrap();
        s.set_child(id, Some((std::process::id(), nexttool_core::process::current_start()))).unwrap();
        s.set_state(id, TaskState::Running, None).unwrap();
        let rec = s.recover_interrupted().unwrap();
        assert!(rec[0].child_still_running);
        assert_eq!(rec[0].task.state, TaskState::Interrupted);
    }

    #[test]
    fn alerts_group_by_fingerprint() {
        let (_t, s) = store();
        let d = AlertDraft {
            module: Module::NextCopy,
            task_id: None,
            severity: Severity::Warning,
            fingerprint: "copy.network.x".into(),
            title: "Erro de rede".into(),
            problem: "p".into(),
            probable_cause: None,
            consequence: "c".into(),
            recommended_action: "a".into(),
            recoverable: true,
            details: None,
        };
        let (a, g1) = s.raise_alert(&d).unwrap();
        let (b, g2) = s.raise_alert(&AlertDraft { severity: Severity::Error, ..d.clone() }).unwrap();
        assert_eq!(a, b);
        assert!(!g1 && g2);
        let l = s.list_alerts(false, 10).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].count, 2);
        assert_eq!(l[0].draft.severity, Severity::Error);
        s.ack_alert(a).unwrap();
        let (c, g3) = s.raise_alert(&d).unwrap();
        assert!(c != a && !g3);
    }

    #[test]
    fn corrupt_store_is_moved_aside() {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().join("app.db");
        std::fs::write(&p, b"this is not a sqlite database at all, just garbage bytes......").unwrap();
        let (s, note) = AppStore::open(&p).unwrap();
        assert!(note.is_some());
        s.create_task(&new_task()).unwrap();
        let aside: Vec<_> = std::fs::read_dir(t.path()).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().contains("corrupt")).collect();
        assert_eq!(aside.len(), 1);
    }
}
