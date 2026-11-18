use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crossbeam_channel::{bounded, RecvTimeoutError, Sender};
use nexttool_core::fs::{enter_background_mode, flags, DirEnumerator, EntryKind};
use nexttool_core::time::now_unix;
use nextsize_core::{
    DirChunk, EntryRecord, ErrorKind, NodeId, ScanIssue, ScanOptions, ScanOutcome, ScanProgress, ScanSink, ScanState,
    Severity, SinkMsg, Stage, ROOT_ID,
};
use serde::Serialize;

use crate::control::ScanControl;
use crate::frontier::Frontier;

/// A directory waiting to be enumerated.
#[derive(Debug, Clone)]
pub struct DirTask {
    pub id: NodeId,
    pub depth: u32,
    pub path: PathBuf,
}

pub enum ScanStart {
    /// New snapshot: the root gets `ROOT_ID`.
    Fresh,
    /// Continue an interrupted snapshot. `pending` is streamed (never
    /// materialized) into the frontier, which spills to disk if needed.
    Resume { next_id: NodeId, pending: Box<dyn Iterator<Item = DirTask> + Send> },
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanReport {
    pub outcome: ScanOutcome,
    pub enumerator: &'static str,
    pub threads: usize,
    pub fatal_error: Option<String>,
    pub scan_ms: u64,
    pub finish_ms: u64,
}

struct Ctx {
    opts: ScanOptions,
    enumerator: Arc<dyn DirEnumerator>,
    control: Arc<ScanControl>,
    progress: Arc<ScanProgress>,
    frontier: Frontier,
    next_id: AtomicU64,
    root_dev: Option<u64>,
    fatal: Mutex<Option<String>>,
}

impl Ctx {
    fn set_fatal(&self, msg: String) {
        let mut g = self.fatal.lock().unwrap_or_else(|p| p.into_inner());
        if g.is_none() {
            tracing::error!("fatal scan error: {msg}");
            *g = Some(msg);
        }
        drop(g);
        self.control.cancel();
        self.frontier.wake_all();
    }
}

fn root_dev(_root: &Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return std::fs::symlink_metadata(_root).ok().map(|m| m.dev());
    }
    #[allow(unreachable_code)]
    None
}

/// Runs a scan to completion, cancellation or fatal error. Blocking: call it
/// from a background thread. The sink receives everything on one dedicated
/// writer thread; `finish` is called on the calling thread at the end.
pub fn run_scan(
    opts: &ScanOptions,
    enumerator: Arc<dyn DirEnumerator>,
    mut sink: Box<dyn ScanSink>,
    start: ScanStart,
    control: Arc<ScanControl>,
    progress: Arc<ScanProgress>,
) -> ScanReport {
    let t0 = Instant::now();
    progress.set_state(ScanState::Preparing);
    let tuning = opts.tuning.clone();
    // Never spawn more workers than the control can ever allow.
    let threads = tuning.threads.max(1).min(control.max());
    let spill_dir = opts.spill_dir.clone().unwrap_or_else(std::env::temp_dir);

    let ctx = Arc::new(Ctx {
        opts: opts.clone(),
        enumerator: enumerator.clone(),
        control: control.clone(),
        progress: progress.clone(),
        frontier: Frontier::new(tuning.frontier_mem_cap, spill_dir, progress.clone()),
        next_id: AtomicU64::new(ROOT_ID + 1),
        root_dev: root_dev(&opts.root),
        fatal: Mutex::new(None),
    });

    let seed = match start {
        ScanStart::Fresh => ctx.frontier.push(DirTask { id: ROOT_ID, depth: 0, path: opts.root.clone() }),
        ScanStart::Resume { next_id, pending } => {
            ctx.next_id.store(next_id, Relaxed);
            let mut r = Ok(());
            for t in pending {
                if let Err(e) = ctx.frontier.push(t) {
                    r = Err(e);
                    break;
                }
            }
            r
        }
    };
    if let Err(e) = seed {
        ctx.set_fatal(format!("não foi possível preparar a fila de diretórios: {e}"));
    }

    let (tx, rx) = bounded::<SinkMsg>(tuning.channel_cap.max(1));
    progress.set_state(ScanState::Scanning);

    // Writer thread: the only place that touches persistence.
    let wctx = ctx.clone();
    let writer = std::thread::Builder::new()
        .name("nexttool-writer".into())
        .spawn(move || {
            let mut checkpointed = false;
            loop {
                let wait = Instant::now();
                let got = rx.recv_timeout(Duration::from_millis(250));
                wctx.progress.writer_wait_ns.fetch_add(wait.elapsed().as_nanos() as u64, Relaxed);
                match got {
                    Ok(msg) => {
                        wctx.progress.queued_chunks.fetch_sub(1, Relaxed);
                        if let Err(e) = sink.write(msg) {
                            wctx.set_fatal(format!("falha de persistência: {e}"));
                            break;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => {
                        let r = if wctx.control.is_paused() && !checkpointed {
                            checkpointed = true;
                            sink.checkpoint()
                        } else {
                            if !wctx.control.is_paused() {
                                checkpointed = false;
                            }
                            sink.tick()
                        };
                        if let Err(e) = r {
                            wctx.set_fatal(format!("falha de persistência: {e}"));
                            break;
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            sink
        })
        .expect("spawn writer thread");

    let mut workers = Vec::with_capacity(threads);
    for idx in 0..threads {
        let c = ctx.clone();
        let tx = tx.clone();
        workers.push(
            std::thread::Builder::new()
                .name(format!("nexttool-scan-{idx}"))
                .spawn(move || worker(idx, c, tx))
                .expect("spawn scan worker"),
        );
    }
    drop(tx);

    // Pause state mirror for the UI.
    let mut joined = 0;
    while joined < workers.len() {
        if workers.iter().all(|w| w.is_finished()) {
            break;
        }
        let st = progress.state();
        if control.is_cancelled() && st != ScanState::Cancelling {
            progress.set_state(ScanState::Cancelling);
        } else if !control.is_cancelled() && control.is_paused() && st != ScanState::Paused {
            progress.set_state(ScanState::Paused);
        } else if !control.is_cancelled() && !control.is_paused() && st == ScanState::Paused {
            progress.set_state(ScanState::Scanning);
        }
        std::thread::sleep(Duration::from_millis(50));
        joined = workers.iter().filter(|w| w.is_finished()).count();
    }
    for w in workers {
        if w.join().is_err() {
            ctx.set_fatal("um thread de varredura terminou com pânico".into());
        }
    }
    let sink_back = writer.join();
    let scan_ms = t0.elapsed().as_millis() as u64;

    let fatal = ctx.fatal.lock().unwrap_or_else(|p| p.into_inner()).clone();
    let issues = progress.issues.load(Relaxed);
    let state = if fatal.is_some() {
        ScanState::Failed
    } else if control.is_cancelled() {
        ScanState::Interrupted
    } else if issues > 0 {
        ScanState::CompletedWithWarnings
    } else {
        ScanState::Completed
    };
    let mut outcome = ScanOutcome {
        state,
        files: progress.files.load(Relaxed),
        dirs: progress.dirs.load(Relaxed),
        bytes: progress.bytes.load(Relaxed),
        issues,
        elapsed_ms: scan_ms,
    };

    let t1 = Instant::now();
    let mut fatal_error = fatal;
    match sink_back {
        Ok(mut sink) => {
            if outcome.state.is_terminal_ok() {
                progress.set_state(ScanState::Finalizing);
            }
            if let Err(e) = sink.finish(&outcome) {
                fatal_error.get_or_insert_with(|| format!("falha ao finalizar o índice: {e}"));
                outcome.state = ScanState::Failed;
            }
        }
        Err(_) => {
            fatal_error.get_or_insert_with(|| "o thread de persistência terminou com pânico".into());
            outcome.state = ScanState::Failed;
        }
    }
    progress.set_state(outcome.state);
    ScanReport {
        outcome,
        enumerator: enumerator.name(),
        threads,
        fatal_error,
        scan_ms,
        finish_ms: t1.elapsed().as_millis() as u64,
    }
}

fn worker(idx: usize, ctx: Arc<Ctx>, tx: Sender<SinkMsg>) {
    if ctx.opts.tuning.background_priority {
        enter_background_mode();
    }
    loop {
        if !ctx.control.wait_runnable(idx, &|| ctx.frontier.is_done()) {
            break;
        }
        let task = match ctx.frontier.pop(&ctx.control) {
            Ok(Some(t)) => t,
            Ok(None) => break,
            Err(e) => {
                ctx.set_fatal(format!("falha no arquivo temporário da fila de diretórios: {e}"));
                break;
            }
        };
        let ok = process_dir(&ctx, &tx, task);
        ctx.frontier.task_done();
        if !ok {
            break;
        }
    }
}

fn send(ctx: &Ctx, tx: &Sender<SinkMsg>, msg: SinkMsg) -> bool {
    ctx.progress.queued_chunks.fetch_add(1, Relaxed);
    let res = match tx.try_send(msg) {
        Ok(()) => Ok(()),
        Err(crossbeam_channel::TrySendError::Full(msg)) => {
            // Backpressure: measure how long the writer makes us wait.
            let t = Instant::now();
            let r = tx.send(msg).map_err(|_| ());
            ctx.progress.send_blocked_ns.fetch_add(t.elapsed().as_nanos() as u64, Relaxed);
            r
        }
        Err(crossbeam_channel::TrySendError::Disconnected(_)) => Err(()),
    };
    if res.is_err() {
        ctx.progress.queued_chunks.fetch_sub(1, Relaxed);
        return false;
    }
    true
}

/// Enumerates one directory. Returns false if the worker must stop.
fn process_dir(ctx: &Ctx, tx: &Sender<SinkMsg>, task: DirTask) -> bool {
    let chunk_size = ctx.opts.tuning.chunk_size.max(1);
    let mut buf: Vec<EntryRecord> = Vec::new();
    let mut children: Vec<DirTask> = Vec::new();
    let mut stopped = false;

    // Sends the buffered entries, then makes their subdirectories schedulable
    // (so a child's rows never reach the writer before its own row).
    let flush = |buf: &mut Vec<EntryRecord>, children: &mut Vec<DirTask>, last: bool, enum_error: bool| -> bool {
        let entries = std::mem::take(buf);
        let mut files = 0u64;
        let mut bytes = 0u64;
        for e in &entries {
            if e.kind != EntryKind::Dir {
                files += 1;
                bytes = bytes.saturating_add(e.size);
            }
        }
        let msg = SinkMsg::Chunk(DirChunk { dir_id: task.id, dir_depth: task.depth, entries, last, enum_error });
        if !send(ctx, tx, msg) {
            return false;
        }
        ctx.progress.files.fetch_add(files, Relaxed);
        ctx.progress.bytes.fetch_add(bytes, Relaxed);
        for c in children.drain(..) {
            if let Err(e) = ctx.frontier.push(c) {
                ctx.set_fatal(format!("falha no arquivo temporário da fila de diretórios: {e}"));
                return false;
            }
        }
        true
    };

    let res = ctx.enumerator.enumerate(&task.path, &mut |e| {
        if ctx.control.is_cancelled() {
            stopped = true;
            return false;
        }
        let id = ctx.next_id.fetch_add(1, Relaxed);
        let mut fl = e.flags;
        let name: Box<str> = match e.name.to_str() {
            Some(s) => s.into(),
            None => {
                fl |= flags::NAME_LOSSY;
                e.name.to_string_lossy().into()
            }
        };
        if e.kind == EntryKind::Dir {
            if fl & (flags::REPARSE | flags::SYMLINK) != 0 {
                fl |= flags::NOT_FOLLOWED; // junctions, symlinks, mount points: never followed
            } else if !ctx.opts.cross_mounts && ctx.root_dev.is_some() && e.dev.is_some() && e.dev != ctx.root_dev {
                fl |= flags::OTHER_MOUNT | flags::NOT_FOLLOWED;
            } else {
                children.push(DirTask { id, depth: task.depth + 1, path: task.path.join(e.name) });
            }
        }
        if buf.capacity() == 0 {
            buf.reserve(chunk_size.min(1024));
        }
        buf.push(EntryRecord {
            id,
            name,
            kind: e.kind,
            size: e.size,
            alloc: e.alloc,
            mtime: e.mtime,
            ctime: e.ctime,
            atime: e.atime,
            flags: fl,
            file_id: e.file_id,
            nlink: e.nlink,
        });
        if buf.len() >= chunk_size && !flush(&mut buf, &mut children, false, false) {
            stopped = true;
            return false;
        }
        true
    });

    if stopped || ctx.control.is_cancelled() {
        // Directory stays incomplete; resume discards and re-reads it.
        return !ctx.control.is_cancelled() && !stopped;
    }

    let enum_error = match res {
        Ok(()) => false,
        Err(e) => {
            ctx.progress.issues.fetch_add(1, Relaxed);
            // Directory removed while scanning: a warning, not an error.
            let severity = if e.kind == ErrorKind::NotFound { Severity::Warning } else { Severity::Error };
            let issue = ScanIssue {
                dir_id: Some(task.id),
                path: task.path.to_string_lossy().into_owned(),
                kind: e.kind,
                os_code: e.os_code,
                stage: Stage::Enumerate,
                severity,
                recoverable: e.kind.is_transient(),
                at: now_unix(),
                message: if e.partial { format!("{} (listagem parcial)", e.message) } else { e.message.clone() },
            };
            tracing::debug!(path = %issue.path, kind = issue.kind.as_str(), "enumeration error");
            if !send(ctx, tx, SinkMsg::Issue(issue)) {
                return false;
            }
            true
        }
    };
    if !flush(&mut buf, &mut children, true, enum_error) {
        return false;
    }
    ctx.progress.dirs.fetch_add(1, Relaxed);
    true
}
