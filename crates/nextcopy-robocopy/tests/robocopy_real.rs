//! Runs the real Robocopy.exe. Windows only.
#![cfg(windows)]

use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use nexttool_core::fs::PortableEnumerator;
use nextcopy_core::{CopyControl, CopyEngine, CopyJob, CopyOutcome, CopyProfile, EngineError, VerifyLevel};
use nextcopy_robocopy::RobocopyEngine;

fn tree(root: &Path, n: usize) {
    fs::create_dir_all(root.join("sub").join("deep")).unwrap();
    for i in 0..n {
        fs::write(root.join(format!("f{i}.txt")), format!("conteúdo {i}").repeat(i % 50 + 1)).unwrap();
    }
    fs::write(root.join("sub").join("ação ✓.bin"), vec![9u8; 100_000]).unwrap();
    fs::write(root.join("sub").join("deep").join("x.tmp"), b"tmp").unwrap();
    fs::create_dir_all(root.join("vazio")).unwrap();
}

fn job(src: &Path, dst: &Path, p: CopyProfile) -> CopyJob {
    let mut j = CopyJob::new(src.to_path_buf(), dst.to_path_buf(), p);
    j.options.retries = 0;
    j.options.retry_wait_secs = 0;
    j
}

#[test]
fn copy_is_complete_idempotent_and_verifies() {
    let t = tempfile::tempdir().unwrap();
    let (src, dst, logs) = (t.path().join("src"), t.path().join("dst"), t.path().join("logs"));
    tree(&src, 200);
    let e = RobocopyEngine::locate().unwrap();
    let j = job(&src, &dst, CopyProfile::Safe);
    let mut events = 0;
    let r = e.run(&j, &logs, &CopyControl::default(), &mut |_| events += 1).unwrap();
    assert_eq!(r.outcome, CopyOutcome::Completed, "{r:?}");
    assert_eq!(r.stats.files_copied, 202);
    assert_eq!(r.stats.files_failed, 0);
    assert!(events >= 2);
    assert_eq!(r.exit_codes, vec![Some(1)]);
    let rep = nextcopy_verify::verify(&j, VerifyLevel::Strict, &PortableEnumerator, &AtomicBool::new(false), &mut |_| {});
    assert!(rep.passed(), "{rep:?}");
    assert_eq!(rep.files_checked, 202);
    assert!(dst.join("vazio").is_dir(), "empty dirs copied with /E");

    // Second run: nothing to do (exit 0), still Completed.
    let r2 = e.run(&j, &logs, &CopyControl::default(), &mut |_| {}).unwrap();
    assert_eq!(r2.outcome, CopyOutcome::Completed);
    assert_eq!(r2.stats.files_copied, 0);
    assert_eq!(r2.stats.files_skipped, 202);
    assert_eq!(r2.exit_codes, vec![Some(0)]);
}

#[test]
fn mirror_requires_matching_approval() {
    let t = tempfile::tempdir().unwrap();
    let (src, dst, logs) = (t.path().join("src"), t.path().join("dst"), t.path().join("logs"));
    tree(&src, 10);
    fs::create_dir_all(dst.join("antigo")).unwrap();
    fs::write(dst.join("extra1.txt"), b"1").unwrap();
    fs::write(dst.join("antigo").join("extra2.txt"), b"2").unwrap();
    let e = RobocopyEngine::locate().unwrap();
    let mut j = job(&src, &dst, CopyProfile::Mirror);

    let plan = e.dry_run(&j, &logs, &CopyControl::default()).unwrap();
    assert_eq!(plan.would_copy_files, 12);
    assert_eq!(plan.would_delete_files + plan.would_delete_dirs, 3, "{plan:?}");
    assert!(dst.join("extra1.txt").exists(), "dry-run must not delete");
    assert!(!dst.join("f0.txt").exists(), "dry-run must not copy");

    assert!(matches!(e.run(&j, &logs, &CopyControl::default(), &mut |_| {}), Err(EngineError::MirrorNotApproved)));
    j.approved_deletions = Some(3);
    fs::write(dst.join("extra3-depois-da-previa.txt"), b"3").unwrap();
    assert!(matches!(e.run(&j, &logs, &CopyControl::default(), &mut |_| {}), Err(EngineError::MirrorApprovalMismatch { approved: 3, found: 4 })));
    assert!(dst.join("extra3-depois-da-previa.txt").exists());

    j.approved_deletions = Some(4);
    let r = e.run(&j, &logs, &CopyControl::default(), &mut |_| {}).unwrap();
    assert_eq!(r.outcome, CopyOutcome::Completed, "{r:?}");
    assert!(!dst.join("extra1.txt").exists() && !dst.join("antigo").exists());
    let rep = nextcopy_verify::verify(&j, VerifyLevel::Standard, &PortableEnumerator, &AtomicBool::new(false), &mut |_| {});
    assert!(rep.passed());
}

#[test]
fn locked_file_is_partial_failure_with_classified_error() {
    use std::os::windows::fs::OpenOptionsExt;
    let t = tempfile::tempdir().unwrap();
    let (src, dst, logs) = (t.path().join("src"), t.path().join("dst"), t.path().join("logs"));
    tree(&src, 5);
    let _lock = fs::OpenOptions::new().read(true).share_mode(0).open(src.join("f1.txt")).unwrap();
    let e = RobocopyEngine::locate().unwrap();
    let j = job(&src, &dst, CopyProfile::Safe);
    let r = e.run(&j, &logs, &CopyControl::default(), &mut |_| {}).unwrap();
    assert_eq!(r.outcome, CopyOutcome::PartialFailure, "{r:?}");
    assert_eq!(r.stats.files_failed, 1);
    assert!(r.exit_codes[0].unwrap() & 8 != 0);
    assert_eq!(r.dominant_error, Some(nexttool_core::ErrorKind::Locked));
    assert!(r.errors_sample.iter().any(|e| e.os_code == 32 && e.path.ends_with("f1.txt")));
}

#[test]
fn missing_source_is_fatal() {
    let t = tempfile::tempdir().unwrap();
    let e = RobocopyEngine::locate().unwrap();
    let j = job(&t.path().join("nao-existe"), &t.path().join("dst"), CopyProfile::Safe);
    let r = e.run(&j, &t.path().join("logs"), &CopyControl::default(), &mut |_| {}).unwrap();
    assert_eq!(r.outcome, CopyOutcome::Failed, "{r:?}");
    assert_eq!(r.dominant_error, Some(nexttool_core::ErrorKind::NotFound));
}

#[test]
fn pause_stops_process_and_rerun_completes_with_identical_content() {
    let t = tempfile::tempdir().unwrap();
    let (src, dst, logs) = (t.path().join("src"), t.path().join("dst"), t.path().join("logs"));
    fs::create_dir_all(&src).unwrap();
    // 120 × 4 MB with incompressible-ish content.
    let mut x: u64 = 0x1234_5678;
    for i in 0..120 {
        let mut buf = vec![0u8; 4 << 20];
        for b in buf.chunks_mut(8) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            b.copy_from_slice(&x.to_le_bytes()[..b.len()]);
        }
        fs::write(src.join(format!("big{i:03}.bin")), buf).unwrap();
    }
    let e = RobocopyEngine::locate().unwrap();
    let mut j = job(&src, &dst, CopyProfile::Safe);
    j.options.threads = 1;
    let control = Arc::new(CopyControl::default());
    let c2 = control.clone();
    let mut pid_seen = false;
    let r = e
        .run(&j, &logs, &control, &mut |ev| {
            // Files are in the OS cache, so the copy is very fast: pause on a
            // short timer after the process starts.
            if let nextcopy_core::CopyEvent::Started { .. } = ev {
                pid_seen = true;
                let c3 = c2.clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(120));
                    c3.pause();
                });
            }
        })
        .unwrap();
    assert!(pid_seen);
    assert_eq!(r.outcome, CopyOutcome::Paused, "{r:?}");
    assert_eq!(r.exit_codes, vec![None]);
    let copied_before = fs::read_dir(&dst).map(|d| d.count()).unwrap_or(0);
    assert!(copied_before < 120, "pause must stop before the end ({copied_before})");

    let r2 = e.run(&j, &logs, &CopyControl::default(), &mut |_| {}).unwrap();
    assert_eq!(r2.outcome, CopyOutcome::Completed, "{r2:?}");
    let rep = nextcopy_verify::verify(&j, VerifyLevel::Strict, &PortableEnumerator, &AtomicBool::new(false), &mut |_| {});
    assert!(rep.passed(), "content must be identical after pause/resume: {rep:?}");
    assert_eq!(rep.files_checked, 120);
}

#[test]
fn individual_selection_runs_one_process_per_directory() {
    let t = tempfile::tempdir().unwrap();
    let (src, dst, logs) = (t.path().join("src"), t.path().join("dst"), t.path().join("logs"));
    tree(&src, 10);
    let e = RobocopyEngine::locate().unwrap();
    let mut j = job(&src, &dst, CopyProfile::Safe);
    j.files = vec!["f1.txt".into(), "f2.txt".into(), r"sub\ação ✓.bin".into()];
    let mut starts = 0;
    let r = e.run(&j, &logs, &CopyControl::default(), &mut |ev| {
        if matches!(ev, nextcopy_core::CopyEvent::Started { .. }) {
            starts += 1;
        }
    })
    .unwrap();
    assert_eq!(starts, 2);
    assert_eq!(r.outcome, CopyOutcome::Completed, "{r:?}");
    assert_eq!(r.stats.files_copied, 3);
    assert!(dst.join("sub").join("ação ✓.bin").exists());
    assert!(!dst.join("f3.txt").exists());
    let rep = nextcopy_verify::verify(&j, VerifyLevel::Strict, &PortableEnumerator, &AtomicBool::new(false), &mut |_| {});
    assert!(rep.passed() && rep.files_checked == 3, "{rep:?}");
}
