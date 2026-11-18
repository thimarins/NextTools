//! Adaptive concurrency against a simulated network server whose throughput
//! peaks at 3 concurrent requests (beyond that, contention adds latency).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nexttool_core::fs::{DirEnumerator, EntryKind, EntryRef, EnumError};
use nextsize_core::{NullSink, Profile, ScanOptions, ScanProgress, ScanState, Tuning};
use nextsize_scanner::{run_scan, ScanControl, ScanStart};

struct SlowServer {
    active: AtomicUsize,
    optimum: usize,
    dirs_per_level: usize,
    depth: usize,
    root_depth: usize,
}

impl DirEnumerator for SlowServer {
    fn name(&self) -> &'static str {
        "simulated-smb"
    }
    fn enumerate(&self, dir: &Path, f: &mut dyn FnMut(&EntryRef<'_>) -> bool) -> Result<(), EnumError> {
        let n = self.active.fetch_add(1, Relaxed) + 1;
        // Base latency 3 ms; each request beyond the optimum adds contention for everyone.
        let extra = n.saturating_sub(self.optimum) as u64;
        std::thread::sleep(Duration::from_micros(3_000 + 3_000 * extra * extra));
        self.active.fetch_sub(1, Relaxed);
        let depth = dir.components().count() - self.root_depth;
        for i in 0..5 {
            let name = OsString::from(format!("f{i}.dat"));
            let e = EntryRef { name: &name, kind: EntryKind::File, size: 100, alloc: None, mtime: Some(0), ctime: None, atime: None, flags: 0, reparse_tag: 0, file_id: None, nlink: None, dev: None };
            f(&e);
        }
        if depth < self.depth {
            for i in 0..self.dirs_per_level {
                let name = OsString::from(format!("d{i}"));
                let e = EntryRef { name: &name, kind: EntryKind::Dir, size: 0, alloc: None, mtime: Some(0), ctime: None, atime: None, flags: 0, reparse_tag: 0, file_id: None, nlink: None, dev: None };
                f(&e);
            }
        }
        Ok(())
    }
}

#[test]
fn adaptive_engine_backs_off_from_oversubscription_and_stays_exact() {
    let root = PathBuf::from(r"C:\simulated\share");
    // 1 + 12 + 144 + 1728 = 1885 dirs, 5 files each.
    let server = Arc::new(SlowServer { active: AtomicUsize::new(0), optimum: 3, dirs_per_level: 12, depth: 3, root_depth: root.components().count() });
    let mut tuning = Tuning::for_profile(Profile::Balanced, true, 8);
    tuning.threads = 8; // start well above the server optimum
    tuning.max_threads = 12;
    tuning.adaptive_interval_ms = 120;
    assert!(tuning.adaptive);
    let opts = ScanOptions { root: root.clone(), profile: Profile::Balanced, backend: Default::default(), tuning: tuning.clone(), cross_mounts: false, spill_dir: Some(std::env::temp_dir()) };
    let progress = Arc::new(ScanProgress::default());
    let control = Arc::new(ScanControl::new(tuning.spawn_threads()));
    let samples = Arc::new(Mutex::new(Vec::new()));
    let (p2, s2) = (progress.clone(), samples.clone());
    let sampler = std::thread::spawn(move || {
        for _ in 0..400 {
            s2.lock().unwrap().push(p2.concurrency.load(Relaxed));
            if matches!(p2.state(), ScanState::Completed | ScanState::CompletedWithWarnings | ScanState::Failed) {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    });
    let r = run_scan(&opts, server.clone(), Box::new(NullSink), ScanStart::Fresh, control, progress);
    sampler.join().unwrap();
    assert_eq!(r.outcome.state, ScanState::Completed);
    assert_eq!(r.outcome.dirs, 1885);
    assert_eq!(r.outcome.files, 1885 * 5, "totals stay exact while concurrency changes");
    let s = samples.lock().unwrap();
    let (mn, mx) = (*s.iter().min().unwrap(), *s.iter().max().unwrap());
    assert!(mx <= 12, "never above the ceiling: {s:?}");
    assert!(mn < 8, "controller should back off from 8 toward the optimum (3): {s:?}");
}
