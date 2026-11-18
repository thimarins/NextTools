use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use nexttool_core::ErrorKind;
use nextcopy_core::{
    CopyControl, CopyEngine, CopyEvent, CopyJob, CopyOutcome, CopyResult, CopyStats, DryRunReport, EngineError,
};
use windows_sys::Win32::System::Threading::{GetProcessIoCounters, IO_COUNTERS};

use crate::args::{build_args, Mode};
use crate::exit::classify;
use crate::log::{LogState, Utf16Lines};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const POLL: Duration = Duration::from_millis(300);
const PROGRESS_EVERY: Duration = Duration::from_secs(1);

/// Robocopy-backed engine.
#[derive(Debug, Clone)]
pub struct RobocopyEngine {
    exe: PathBuf,
}

impl RobocopyEngine {
    /// Locates `%SystemRoot%\System32\Robocopy.exe` (absolute path; PATH is
    /// never searched, preventing binary planting).
    pub fn locate() -> Result<Self, EngineError> {
        let root = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let exe = root.join("System32").join("Robocopy.exe");
        if !exe.is_file() {
            return Err(EngineError::Unavailable(format!("Robocopy não encontrado em {}", exe.display())));
        }
        Ok(RobocopyEngine { exe })
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }
}

/// One Robocopy invocation (a whole tree, or one selection group).
struct Run {
    src: PathBuf,
    dst: PathBuf,
    files: Vec<String>,
    recursive: bool,
}

fn runs_for(job: &CopyJob) -> Vec<Run> {
    if job.files.is_empty() {
        return vec![Run { src: job.source.clone(), dst: job.dest.clone(), files: vec![], recursive: true }];
    }
    job.file_groups()
        .into_iter()
        .map(|(rel, names)| Run { src: job.source.join(&rel), dst: job.dest.join(&rel), files: names, recursive: false })
        .collect()
}

fn log_path(dir: &Path, tag: &str, idx: usize) -> PathBuf {
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    dir.join(format!("robocopy-{tag}-{ts}-{idx}.log"))
}

fn io_counters(child: &Child) -> (u64, u64) {
    let mut c: IO_COUNTERS = unsafe { std::mem::zeroed() };
    // SAFETY: valid process handle owned by `child`; out pointer to a local.
    let ok = unsafe { GetProcessIoCounters(child.as_raw_handle() as _, &mut c) };
    if ok != 0 {
        (c.ReadTransferCount, c.WriteTransferCount)
    } else {
        (0, 0)
    }
}

/// Tails a growing UTF-16 log file.
struct Tail {
    path: PathBuf,
    file: Option<File>,
    pos: u64,
    dec: Utf16Lines,
}

impl Tail {
    fn new(path: PathBuf) -> Self {
        Tail { path, file: None, pos: 0, dec: Utf16Lines::default() }
    }

    fn read(&mut self, out: &mut Vec<String>) {
        if self.file.is_none() {
            self.file = File::open(&self.path).ok();
        }
        let Some(f) = self.file.as_mut() else { return };
        if f.seek(SeekFrom::Start(self.pos)).is_err() {
            return;
        }
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            match f.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    self.pos += n as u64;
                    self.dec.feed(&buf[..n], out);
                }
            }
        }
    }
}

struct RunOutput {
    exit_code: Option<i32>,
    state: LogState,
    log: PathBuf,
}

impl RobocopyEngine {
    fn exec(
        &self,
        run: &Run,
        job: &CopyJob,
        mode: Mode,
        log_dir: &Path,
        control: &CopyControl,
        idx: usize,
        runs: usize,
        on_event: &mut dyn FnMut(CopyEvent),
    ) -> Result<RunOutput, EngineError> {
        std::fs::create_dir_all(log_dir)?;
        let log = log_path(log_dir, if mode == Mode::DryRun { "plan" } else { "copy" }, idx);
        let args = build_args(&run.src, &run.dst, &run.files, run.recursive, &job.options, mode, &log);
        tracing::info!(exe = %self.exe.display(), ?args, "starting robocopy");
        let mut child = Command::new(&self.exe)
            .args(&args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
        on_event(CopyEvent::Started { pid: child.id(), run_index: idx, runs });

        let mut state = LogState::new(&run.dst);
        let mut tail = Tail::new(log.clone());
        let mut lines = Vec::new();
        let mut last_progress = Instant::now() - PROGRESS_EVERY;
        let mut stopped_by_us = false;
        let exit_code = loop {
            if control.stop_requested() && !stopped_by_us {
                // No graceful stop exists for Robocopy; terminate. The file in
                // progress is left incomplete and is re-copied on the next run
                // (or resumed mid-file with /Z). See ADR-011.
                let _ = child.kill();
                stopped_by_us = true;
            }
            let status = child.try_wait()?;
            tail.read(&mut lines);
            for l in lines.drain(..) {
                if let Some(e) = state.apply(&l) {
                    on_event(CopyEvent::Error(e));
                }
            }
            if last_progress.elapsed() >= PROGRESS_EVERY {
                let (r, w) = io_counters(&child);
                on_event(CopyEvent::Progress {
                    files_done: state.files_done,
                    bytes_done: state.bytes_done,
                    errors: state.error_events,
                    io_read: r,
                    io_write: w,
                    log_bytes: tail.pos,
                });
                last_progress = Instant::now();
            }
            if let Some(st) = status {
                break if stopped_by_us { None } else { st.code() };
            }
            std::thread::sleep(POLL);
        };
        // Final read after exit.
        tail.read(&mut lines);
        tail.dec.finish(&mut lines);
        for l in lines.drain(..) {
            if let Some(e) = state.apply(&l) {
                on_event(CopyEvent::Error(e));
            }
        }
        on_event(CopyEvent::Finished { exit_code });
        Ok(RunOutput { exit_code, state, log })
    }
}

fn dominant(states: &[&LogState]) -> Option<ErrorKind> {
    let mut counts: std::collections::HashMap<ErrorKind, usize> = Default::default();
    for s in states {
        for e in &s.errors_sample {
            *counts.entry(e.kind).or_default() += 1;
        }
    }
    counts.into_iter().max_by_key(|(_, c)| *c).map(|(k, _)| k)
}

fn add(a: &mut CopyStats, b: &CopyStats) {
    a.files_total += b.files_total;
    a.files_copied += b.files_copied;
    a.files_skipped += b.files_skipped;
    a.files_mismatch += b.files_mismatch;
    a.files_failed += b.files_failed;
    a.files_extra += b.files_extra;
    a.dirs_total += b.dirs_total;
    a.dirs_copied += b.dirs_copied;
    a.dirs_failed += b.dirs_failed;
    a.bytes_total += b.bytes_total;
    a.bytes_copied += b.bytes_copied;
    a.bytes_failed += b.bytes_failed;
    a.retries += b.retries;
}

fn worst(a: CopyOutcome, b: CopyOutcome) -> CopyOutcome {
    fn rank(o: CopyOutcome) -> u8 {
        match o {
            CopyOutcome::Completed => 0,
            CopyOutcome::CompletedWithWarnings => 1,
            CopyOutcome::PartialFailure => 2,
            CopyOutcome::Paused => 3,
            CopyOutcome::Cancelled => 4,
            CopyOutcome::Failed => 5,
        }
    }
    if rank(b) > rank(a) { b } else { a }
}

impl CopyEngine for RobocopyEngine {
    fn name(&self) -> &'static str {
        "robocopy"
    }

    fn dry_run(&self, job: &CopyJob, log_dir: &Path, control: &CopyControl) -> Result<DryRunReport, EngineError> {
        job.validate().map_err(|e| EngineError::InvalidJob(e.to_string()))?;
        let runs = runs_for(job);
        let mut rep = DryRunReport {
            would_copy_files: 0,
            would_copy_bytes: 0,
            would_delete_files: 0,
            would_delete_dirs: 0,
            delete_sample: vec![],
            errors: 0,
            exit_code: Some(0),
            log_file: String::new(),
        };
        let n = runs.len();
        for (i, run) in runs.iter().enumerate() {
            let out = self.exec(run, job, Mode::DryRun, log_dir, control, i, n, &mut |_| {})?;
            rep.would_copy_files += out.state.files_done;
            rep.would_copy_bytes += out.state.bytes_done;
            if job.options.mirror {
                rep.would_delete_files += out.state.extra_files;
                rep.would_delete_dirs += out.state.extra_dirs;
                rep.delete_sample.extend(out.state.delete_sample.iter().cloned());
            }
            rep.errors += out.state.error_events;
            rep.exit_code = match (rep.exit_code, out.exit_code) {
                (Some(a), Some(b)) => Some(a | b),
                _ => None,
            };
            rep.log_file = out.log.to_string_lossy().into_owned();
        }
        rep.delete_sample.truncate(200);
        Ok(rep)
    }

    fn run(&self, job: &CopyJob, log_dir: &Path, control: &CopyControl, on_event: &mut dyn FnMut(CopyEvent)) -> Result<CopyResult, EngineError> {
        job.validate().map_err(|e| EngineError::InvalidJob(e.to_string()))?;
        let t0 = Instant::now();
        if job.options.mirror {
            let approved = job.approved_deletions.ok_or(EngineError::MirrorNotApproved)?;
            // Re-check right before deleting anything: the destination may have changed.
            let plan = self.dry_run(job, log_dir, control)?;
            let found = plan.would_delete_files + plan.would_delete_dirs;
            if found > approved {
                return Err(EngineError::MirrorApprovalMismatch { approved, found });
            }
        }
        let runs = runs_for(job);
        let n = runs.len();
        let mut stats = CopyStats::default();
        let mut outcome = CopyOutcome::Completed;
        let mut exit_codes = Vec::new();
        let mut states = Vec::new();
        let mut logs = Vec::new();
        for (i, run) in runs.iter().enumerate() {
            if control.stop_requested() {
                outcome = worst(outcome, if control.is_cancelled() { CopyOutcome::Cancelled } else { CopyOutcome::Paused });
                break;
            }
            let out = self.exec(run, job, Mode::Copy, log_dir, control, i, n, on_event)?;
            let s = out.state.stats();
            let summary = out.state.summary_stats();
            let mut o = classify(out.exit_code, summary.as_ref(), job.options.mirror);
            if o == CopyOutcome::Cancelled && control.is_pause_requested() && !control.is_cancelled() {
                o = CopyOutcome::Paused;
            }
            // Robocopy finished "successfully" but wrote no summary: we cannot
            // confirm the totals, so do not claim a clean completion.
            if o == CopyOutcome::Completed && summary.is_none() {
                o = CopyOutcome::CompletedWithWarnings;
            }
            add(&mut stats, &s);
            outcome = worst(outcome, o);
            exit_codes.push(out.exit_code);
            logs.push(out.log.to_string_lossy().into_owned());
            states.push(out.state);
        }
        let refs: Vec<&LogState> = states.iter().collect();
        // Errors *observed* in the log (a path may have succeeded on a later
        // retry). The authoritative failure count is `stats.files_failed`.
        let errors_sample: Vec<_> = states.iter().flat_map(|s| s.errors_sample.iter().cloned()).take(200).collect();
        Ok(CopyResult {
            outcome,
            stats,
            exit_codes,
            dominant_error: dominant(&refs),
            errors_sample,
            log_files: logs,
            elapsed_ms: t0.elapsed().as_millis() as u64,
        })
    }
}
