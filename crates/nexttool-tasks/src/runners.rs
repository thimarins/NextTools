//! Adapters that run the engines as tasks.

use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nexttool_core::fs::{make_enumerator, Backend};
use nexttool_core::task::{Module, TaskKind, TaskState};
use nexttool_core::ErrorKind;
use nexttool_store::{NewTask, TaskRecord};
use nextcopy_core::{CopyControl, CopyEngine, CopyEvent, CopyJob, CopyOutcome, EngineError, VerifyLevel};
use nextsize_core::{Profile, ScanOptions, ScanProgress, ScanState, Tuning};
use nextsize_index::{open_for_resume, IndexError, IndexWriter};
use nextsize_scanner::{run_scan, ScanControl, ScanStart};
use serde::{Deserialize, Serialize};

use crate::manager::{RunOutcome, RunnerFactory, TaskCtx, TaskRunner};
use crate::monitor::{Activity, ActivityMonitor, Sample};
use crate::resources::resource_key;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeScanSpec {
    pub root: PathBuf,
    pub snapshot: PathBuf,
    pub profile: Profile,
    pub backend: Backend,
    pub threads: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopySpec {
    pub job: CopyJob,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TaskSpec {
    SizeScan(SizeScanSpec),
    Copy(CopySpec),
}

impl TaskSpec {
    pub fn to_new_task(&self, title: String) -> NewTask {
        let (kind, keys, attempts) = match self {
            TaskSpec::SizeScan(s) => (TaskKind::SizeScan, vec![resource_key(&s.root)], 1),
            TaskSpec::Copy(c) => (TaskKind::Copy, vec![resource_key(&c.job.source), resource_key(&c.job.dest)], 5),
        };
        NewTask {
            kind,
            title,
            spec_json: serde_json::to_string(self).unwrap_or_default(),
            priority: Default::default(),
            resource_keys: keys,
            max_attempts: attempts,
        }
    }
}

/// Factory used by the CLI and desktop.
pub fn default_factory() -> RunnerFactory {
    Arc::new(|t: &TaskRecord| {
        let spec: TaskSpec = serde_json::from_str(&t.spec_json).map_err(|e| format!("especificação da tarefa inválida: {e}"))?;
        Ok(match spec {
            TaskSpec::SizeScan(s) => Box::new(SizeScanRunner { spec: s }) as Box<dyn TaskRunner>,
            TaskSpec::Copy(c) => Box::new(CopyRunner { spec: c }) as Box<dyn TaskRunner>,
        })
    })
}

// ------------------------------------------------------------------ NextSize

pub struct SizeScanRunner {
    pub spec: SizeScanSpec,
}

fn sink_kind(e: &str) -> ErrorKind {
    if e.contains("disco cheio") || e.to_lowercase().contains("disk full") {
        ErrorKind::DiskFull
    } else {
        ErrorKind::Io
    }
}

impl TaskRunner for SizeScanRunner {
    fn run(&mut self, ctx: &TaskCtx) -> RunOutcome {
        let s = &self.spec;
        let target = s.root.display().to_string();
        if let Err(e) = nexttool_core::security::validate_user_path(&s.root) {
            return RunOutcome::failed(ErrorKind::InvalidName, e.to_string(), target);
        }
        match std::fs::metadata(&s.root) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return RunOutcome::failed(ErrorKind::NotFound, "a origem não é um diretório", target),
            Err(e) => return RunOutcome::failed(nexttool_core::error::classify_io(&e), e.to_string(), target),
        }
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        let remote = nexttool_core::fs::is_remote_path(&s.root);
        let mut tuning = Tuning::for_profile(s.profile, remote, cores);
        if let Some(t) = s.threads {
            tuning.threads = t.clamp(1, 64);
        }
        let opts = ScanOptions {
            root: s.root.clone(),
            profile: s.profile,
            backend: s.backend,
            tuning: tuning.clone(),
            cross_mounts: false,
            spill_dir: Some(ctx.data_dir.join("tmp")),
        };
        let enumerator = make_enumerator(s.backend);
        let progress = Arc::new(ScanProgress::default());
        let (sink, start): (Box<dyn nextsize_core::ScanSink>, ScanStart) = if s.snapshot.exists() {
            match open_for_resume(&s.snapshot, tuning.sqlite_cache_kib, progress.clone()) {
                Ok((w, plan)) => {
                    ctx.event("info", &format!("retomando snapshot: {} diretórios pendentes, {} registros incompletos descartados", plan.pending_dirs, plan.discarded_rows));
                    (Box::new(w), ScanStart::Resume { next_id: plan.next_id, pending: plan.pending })
                }
                Err(IndexError::NotResumable(st)) => {
                    return RunOutcome::failed(ErrorKind::Internal, format!("o snapshot já existe e não é retomável (estado {st})"), s.snapshot.display().to_string())
                }
                Err(e) => return RunOutcome::failed(ErrorKind::CorruptState, e.to_string(), s.snapshot.display().to_string()),
            }
        } else {
            match IndexWriter::create(&s.snapshot, &opts, enumerator.name(), progress.clone()) {
                Ok(w) => (Box::new(w), ScanStart::Fresh),
                Err(e) => return RunOutcome::failed(ErrorKind::Io, e.to_string(), s.snapshot.display().to_string()),
            }
        };
        ctx.checkpoint(&serde_json::json!({ "snapshot": s.snapshot }).to_string());
        let control = Arc::new(ScanControl::new(tuning.threads));
        let (c2, p2) = (control.clone(), progress.clone());
        let handle = std::thread::spawn(move || run_scan(&opts, enumerator, sink, start, c2, p2));
        let t0 = Instant::now();
        let mut mon = ActivityMonitor::new(300.0);
        let mut alerted = false;
        while !handle.is_finished() {
            if ctx.control.stop_requested() {
                control.cancel();
            }
            let snap = progress.snapshot();
            ctx.progress(&serde_json::to_string(&snap).unwrap_or_default());
            let d = mon.observe(Sample { t: t0.elapsed().as_secs_f64(), work: snap.files + snap.dirs, bytes: 0, io: snap.rows_committed, log: 0, alive: true });
            if d.activity == Activity::NoObservableProgress && !alerted {
                if let Some(a) = crate::alerts::activity(Module::NextSize, ctx.id, &d) {
                    ctx.alert(&a);
                }
                alerted = true;
            } else if d.activity == Activity::Active {
                alerted = false;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        let report = match handle.join() {
            Ok(r) => r,
            Err(_) => return RunOutcome::failed(ErrorKind::Internal, "pânico no motor de varredura", target),
        };
        let result = serde_json::to_string(&report).ok();
        let state = match report.outcome.state {
            ScanState::Completed => TaskState::Completed,
            ScanState::CompletedWithWarnings => TaskState::CompletedWithWarnings,
            ScanState::Interrupted if ctx.control.is_cancelled() => TaskState::Cancelled,
            ScanState::Interrupted => TaskState::Paused,
            _ => TaskState::Failed,
        };
        let error = report.fatal_error.as_ref().map(|e| (sink_kind(e), e.clone()));
        RunOutcome { state, error, result_json: result, target }
    }
}

// ------------------------------------------------------------------ NextCopy

pub struct CopyRunner {
    pub spec: CopySpec,
}

#[derive(Serialize)]
struct CopyProgress {
    phase: &'static str,
    files_done: u64,
    bytes_done: u64,
    errors: u64,
    io_write: u64,
    rate_mb_s: f64,
    activity: Activity,
}

impl TaskRunner for CopyRunner {
    fn run(&mut self, ctx: &TaskCtx) -> RunOutcome {
        let job = &self.spec.job;
        let target = format!("{} → {}", job.source.display(), job.dest.display());
        if let Err(e) = job.validate() {
            return RunOutcome::failed(ErrorKind::InvalidName, e.to_string(), target);
        }
        if let Err(e) = std::fs::metadata(&job.source) {
            let k = nexttool_core::error::classify_io(&e);
            return RunOutcome::failed(k, format!("origem inacessível: {e}"), job.source.display().to_string());
        }
        #[cfg(windows)]
        let engine = match nextcopy_robocopy::RobocopyEngine::locate() {
            Ok(e) => e,
            Err(e) => return RunOutcome::failed(ErrorKind::Internal, e.to_string(), target),
        };
        #[cfg(not(windows))]
        return RunOutcome::failed(ErrorKind::Internal, "NextCopy v1 requer Windows (Robocopy)", target);

        #[cfg(windows)]
        {
            let log_dir = ctx.data_dir.join("logs").join("copy").join(ctx.id.to_string());
            let control = Arc::new(CopyControl::default());
            // Bridge task control → engine control.
            let (tc, cc) = (ctx.control.clone(), control.clone());
            let stop_bridge = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let sb = stop_bridge.clone();
            let bridge = std::thread::spawn(move || {
                while !sb.load(Relaxed) {
                    if tc.is_cancelled() {
                        cc.cancel();
                    } else if tc.is_pause_requested() {
                        cc.pause();
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            });
            let t0 = Instant::now();
            let mut mon = ActivityMonitor::new(600.0);
            let mut last_activity = Activity::Active;
            let res = engine.run(job, &log_dir, &control, &mut |ev| match ev {
                CopyEvent::Started { pid, run_index, runs } => {
                    ctx.set_child(Some(pid));
                    ctx.event("info", &format!("robocopy iniciado (pid {pid}, execução {}/{runs})", run_index + 1));
                }
                CopyEvent::Progress { files_done, bytes_done, errors, io_read, io_write, log_bytes } => {
                    let d = mon.observe(Sample { t: t0.elapsed().as_secs_f64(), work: files_done, bytes: io_write, io: io_read + io_write, log: log_bytes, alive: true });
                    if d.activity != last_activity && d.activity != Activity::Active {
                        if let Some(a) = crate::alerts::activity(Module::NextCopy, ctx.id, &d) {
                            ctx.alert(&a);
                        }
                    }
                    last_activity = d.activity;
                    let p = CopyProgress { phase: "copy", files_done, bytes_done, errors, io_write, rate_mb_s: d.rate_bytes_s / 1e6, activity: d.activity };
                    ctx.progress(&serde_json::to_string(&p).unwrap_or_default());
                }
                CopyEvent::Error(e) => ctx.event("warn", &format!("erro {} ({}) em {}", e.os_code, e.kind.as_str(), e.path)),
                CopyEvent::Finished { exit_code } => {
                    ctx.set_child(None);
                    ctx.event("info", &format!("robocopy terminou (código {exit_code:?})"));
                }
            });
            stop_bridge.store(true, Relaxed);
            let _ = bridge.join();

            let result = match res {
                Ok(r) => r,
                Err(EngineError::MirrorApprovalMismatch { approved, found }) => {
                    return RunOutcome::failed(ErrorKind::Integrity, format!("espelhamento bloqueado: {found} exclusões encontradas, {approved} aprovadas — refaça a prévia"), target)
                }
                Err(e) => return RunOutcome::failed(ErrorKind::Internal, e.to_string(), target),
            };
            let mut json = serde_json::to_value(&result).unwrap_or_default();
            let (state, error) = match result.outcome {
                CopyOutcome::Completed | CopyOutcome::CompletedWithWarnings => {
                    let base = if result.outcome == CopyOutcome::Completed { TaskState::Completed } else { TaskState::CompletedWithWarnings };
                    if job.options.verify == VerifyLevel::Basic {
                        (base, None)
                    } else {
                        ctx.progress(&serde_json::json!({ "phase": "verify" }).to_string());
                        ctx.event("info", &format!("verificação {:?} iniciada", job.options.verify));
                        let cancel = std::sync::atomic::AtomicBool::new(false);
                        let enumr = make_enumerator(Backend::Auto);
                        let tc = ctx.control.clone();
                        let rep = std::thread::scope(|sc| {
                            let h = sc.spawn(|| nextcopy_verify::verify(job, job.options.verify, enumr.as_ref(), &cancel, &mut |_| {}));
                            while !h.is_finished() {
                                if tc.stop_requested() {
                                    cancel.store(true, Relaxed);
                                }
                                std::thread::sleep(Duration::from_millis(100));
                            }
                            h.join().unwrap_or_default()
                        });
                        json["verify"] = serde_json::to_value(&rep).unwrap_or_default();
                        if rep.cancelled {
                            (if ctx.control.is_cancelled() { TaskState::Cancelled } else { TaskState::Paused }, None)
                        } else if rep.passed() {
                            (base, None)
                        } else {
                            let msg = format!(
                                "verificação falhou: {} ausentes, {} tamanho, {} data, {} hash, {} erros",
                                rep.missing, rep.size_mismatch, rep.time_mismatch, rep.hash_mismatch, rep.errors
                            );
                            (TaskState::Failed, Some((ErrorKind::Integrity, msg)))
                        }
                    }
                }
                CopyOutcome::PartialFailure | CopyOutcome::Failed => {
                    let kind = result.dominant_error.unwrap_or(ErrorKind::Io);
                    let codes: Vec<String> = result.exit_codes.iter().map(|c| c.map_or("encerrado".into(), |v| v.to_string())).collect();
                    let msg = format!(
                        "Robocopy: {} arquivo(s) e {} pasta(s) com falha (código de saída {})",
                        result.stats.files_failed,
                        result.stats.dirs_failed,
                        codes.join(", ")
                    );
                    (TaskState::Failed, Some((kind, msg)))
                }
                CopyOutcome::Paused => (TaskState::Paused, None),
                CopyOutcome::Cancelled => (TaskState::Cancelled, None),
            };
            RunOutcome { state, error, result_json: Some(json.to_string()), target }
        }
    }
}
