//! Field scenarios through the full stack (TaskManager → runner → engine →
//! store/alerts). Scenarios that need infrastructure this machine does not
//! have are `#[ignore]` and enabled by environment variables; see
//! docs/TEST_PLAN.md §5 and scripts/field-tests.ps1.
#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use nexttool_core::task::TaskState;
use nexttool_store::AppStore;
use nexttool_tasks::retry::RetryPolicy;
use nexttool_tasks::runners::default_factory;
use nexttool_tasks::{CopySpec, SizeScanSpec, TaskManager, TaskSpec};
use nextcopy_core::{CopyJob, CopyProfile, VerifyLevel};

fn manager(dir: &Path, retry: RetryPolicy) -> TaskManager {
    let (s, _) = AppStore::open(&dir.join("app.db")).unwrap();
    TaskManager::new(s, dir.to_path_buf(), default_factory(), 2).with_retry_policy(retry)
}

fn no_retry_wait() -> RetryPolicy {
    RetryPolicy { base_s: 0, factor: 1, max_s: 0, max_attempts: 3 }
}

fn make_files(dir: &Path, n: usize, size: usize) {
    std::fs::create_dir_all(dir).unwrap();
    for i in 0..n {
        std::fs::write(dir.join(format!("f{i:04}.bin")), vec![(i % 251) as u8; size]).unwrap();
    }
}

fn run_copy(m: &TaskManager, job: CopyJob) -> nexttool_store::TaskRecord {
    let id = m.submit(TaskSpec::Copy(CopySpec { job }).to_new_task("field".into())).unwrap();
    let t0 = Instant::now();
    loop {
        let r = m.run_until_done(&[id], &mut |_| {}).unwrap();
        if r[0].state != TaskState::WaitingRetry || t0.elapsed() > Duration::from_secs(600) {
            return r[0].clone();
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Automated: destination folder denies writes to the current user.
#[test]
fn denied_destination_fails_without_retry_and_alerts() {
    let t = tempfile::tempdir().unwrap();
    let (src, dst) = (t.path().join("src"), t.path().join("dst"));
    make_files(&src, 5, 1000);
    std::fs::create_dir_all(&dst).unwrap();
    let user = std::env::var("USERNAME").unwrap();
    let ok = Command::new("icacls").arg(&dst).args(["/deny", &format!("{user}:(OI)(CI)(W,AD)")]).output().unwrap();
    assert!(ok.status.success());
    let m = manager(t.path(), no_retry_wait());
    let mut job = CopyJob::new(src, dst.clone(), CopyProfile::Safe);
    job.options.retries = 0;
    job.options.retry_wait_secs = 0;
    let rec = run_copy(&m, job);
    let _ = Command::new("icacls").arg(&dst).args(["/remove:d", &user]).output();
    assert_eq!(rec.state, TaskState::Failed, "{rec:?}");
    assert_eq!(rec.error_kind.as_deref(), Some("access_denied"));
    assert_eq!(rec.attempt, 1, "permission errors must not be retried automatically");
    let alerts = m.store().list_alerts(false, 10).unwrap();
    assert!(alerts.iter().any(|a| a.draft.title.contains("Permissão")), "{alerts:?}");
}

fn env_path(k: &str) -> Option<PathBuf> {
    std::env::var_os(k).map(PathBuf::from).filter(|p| !p.as_os_str().is_empty())
}

/// NEXTTOOL_TEST_SMB_DIR=\\servidor\share\pasta-de-teste (gravável, vazia).
/// Copia para o SMB, verifica por hash, analisa o SMB com o NextSize.
#[test]
#[ignore = "requer NEXTTOOL_TEST_SMB_DIR"]
fn smb_roundtrip_copy_verify_and_scan() {
    let Some(share) = env_path("NEXTTOOL_TEST_SMB_DIR") else { return };
    let t = tempfile::tempdir().unwrap();
    let src = t.path().join("src");
    make_files(&src, 300, 64 * 1024);
    let dest = share.join(format!("nexttool-{}", std::process::id()));
    let m = manager(t.path(), RetryPolicy::default());
    let mut job = CopyJob::new(src.clone(), dest.clone(), CopyProfile::Safe);
    job.options.verify = VerifyLevel::Strict;
    let rec = run_copy(&m, job);
    assert_eq!(rec.state, TaskState::Completed, "{rec:?}");
    let snap = t.path().join("smb.nxs");
    let id = m
        .submit(TaskSpec::SizeScan(SizeScanSpec { root: dest.clone(), snapshot: snap.clone(), profile: nextsize_core::Profile::Balanced, backend: Default::default(), threads: None }).to_new_task("smb scan".into()))
        .unwrap();
    let r = m.run_until_done(&[id], &mut |_| {}).unwrap();
    assert_eq!(r[0].state, TaskState::Completed, "{:?}", r[0]);
    let s = nextsize_index::Snapshot::open(&snap).unwrap().summary().unwrap();
    assert_eq!(s.files, 300);
    assert_eq!(s.source_kind, "remote");
    let _ = std::fs::remove_dir_all(&dest);
}

/// NEXTTOOL_TEST_SMALL_VOLUME=E:\ — volume pequeno (ex.: VHD de 64 MB) que
/// será enchido. Espera falha classificada como DiskFull, sem retentativa.
#[test]
#[ignore = "requer NEXTTOOL_TEST_SMALL_VOLUME"]
fn destination_out_of_space_is_classified_and_not_retried() {
    let Some(vol) = env_path("NEXTTOOL_TEST_SMALL_VOLUME") else { return };
    let t = tempfile::tempdir().unwrap();
    let src = t.path().join("src");
    // Larger than the test volume: 40 × 8 MB = 320 MB.
    make_files(&src, 40, 8 << 20);
    let m = manager(t.path(), no_retry_wait());
    let mut job = CopyJob::new(src, vol.join("nexttool-full"), CopyProfile::Safe);
    job.options.retries = 0;
    job.options.retry_wait_secs = 0;
    let rec = run_copy(&m, job);
    assert_eq!(rec.state, TaskState::Failed, "{rec:?}");
    assert_eq!(rec.error_kind.as_deref(), Some("disk_full"), "{rec:?}");
    assert_eq!(rec.attempt, 1);
    let _ = std::fs::remove_dir_all(vol.join("nexttool-full"));
}

/// NEXTTOOL_TEST_NET_INTERRUPT_DIR=\\servidor\share\pasta. Durante o teste,
/// desconecte a rede (ou pare o compartilhamento) quando o console pedir e
/// reconecte em até 60 s. Espera: falha de rede → aguardando nova tentativa →
/// retomada → concluída com verificação rigorosa aprovada.
#[test]
#[ignore = "requer NEXTTOOL_TEST_NET_INTERRUPT_DIR e operador"]
fn network_drop_is_retried_and_completes_identical() {
    let Some(share) = env_path("NEXTTOOL_TEST_NET_INTERRUPT_DIR") else { return };
    let t = tempfile::tempdir().unwrap();
    let src = t.path().join("src");
    make_files(&src, 400, 4 << 20); // 1.6 GB: long enough to disconnect mid-copy
    let m = manager(t.path(), RetryPolicy { base_s: 30, factor: 1, max_s: 30, max_attempts: 5 });
    let mut job = CopyJob::new(src, share.join(format!("nexttool-net-{}", std::process::id())), CopyProfile::Safe);
    job.options.verify = VerifyLevel::Strict;
    job.options.retries = 2;
    job.options.retry_wait_secs = 5;
    eprintln!(">>> DESCONECTE a rede agora e reconecte em até 60 s <<<");
    let rec = run_copy(&m, job);
    let events = m.store().events(rec.id, 200).unwrap();
    eprintln!("{events:#?}");
    assert_eq!(rec.state, TaskState::Completed, "{rec:?}");
    assert!(rec.attempt >= 2, "esperava ao menos uma retentativa após a queda");
}

/// NEXTTOOL_TEST_PTBR_LOG=caminho de um log capturado em Windows pt-BR com
/// scripts/capture-robocopy-log.ps1. Valida o parser contra a saída real.
#[test]
#[ignore = "requer NEXTTOOL_TEST_PTBR_LOG"]
fn real_ptbr_robocopy_log_is_parsed() {
    let Some(p) = env_path("NEXTTOOL_TEST_PTBR_LOG") else { return };
    let bytes = std::fs::read(&p).unwrap();
    let mut dec = nextcopy_robocopy::log::Utf16Lines::default();
    let mut lines = Vec::new();
    dec.feed(&bytes, &mut lines);
    dec.finish(&mut lines);
    let mut st = nextcopy_robocopy::log::LogState::new(Path::new(&std::env::var("NEXTTOOL_TEST_PTBR_DEST").unwrap_or_default()));
    for l in &lines {
        st.apply(l);
    }
    let s = st.summary_stats().expect("resumo (Diretórios/Arquivos/Bytes) não reconhecido no log");
    // With /R:0 each attempted file is listed once; failed ones are listed too.
    assert_eq!(st.files_done, s.files_copied + s.files_failed, "linhas de arquivo × resumo divergentes: {s:?}");
    assert_eq!(st.bytes_done, s.bytes_copied + s.bytes_failed, "bytes × resumo divergentes: {s:?}");
    assert!(s.files_failed >= 1 && st.error_events >= 1, "o script mantém um arquivo bloqueado: deveria haver 1 falha");
    assert!(st.errors_sample.iter().any(|e| e.os_code == 32), "erro 32 (arquivo em uso) esperado");
    eprintln!("pt-BR ok: {s:?}, {} erros observados", st.error_events);
}
