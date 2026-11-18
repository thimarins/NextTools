//! End-to-end NextSize tests on real files: enumerator → engine → SQLite
//! snapshot → queries, plus resume after interruption.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::Arc;

use nexttool_core::fs::{flags, make_enumerator, Backend};
use nextsize_core::{Profile, ScanOptions, ScanProgress, ScanState, Tuning};
use nextsize_index::{open_for_resume, FileQuery, IndexWriter, Snapshot};
use nextsize_scanner::{run_scan, ScanControl, ScanReport, ScanStart};

struct Expect {
    files: u64,
    bytes: u64,
}

/// Independent reference walk (std only, does not follow links/junctions).
fn reference_walk(root: &Path) -> Expect {
    let mut e = Expect { files: 0, bytes: 0 };
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for ent in rd.flatten() {
            let md = fs::symlink_metadata(ent.path()).unwrap();
            let ft = md.file_type();
            #[cfg(windows)]
            let reparse = {
                use std::os::windows::fs::MetadataExt;
                md.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let reparse = ft.is_symlink();
            if ft.is_dir() && !reparse {
                stack.push(ent.path());
            } else if !ft.is_dir() {
                e.files += 1;
                e.bytes += md.len();
            }
        }
    }
    e
}

fn build_tree(root: &Path) {
    fs::create_dir_all(root).unwrap();
    for d in 0..20 {
        let dir = root.join(format!("d{d:02}"));
        fs::create_dir(&dir).unwrap();
        for f in 0..50 {
            fs::write(dir.join(format!("f{f}.txt")), vec![b'x'; (d * 50 + f) as usize]).unwrap();
        }
        let sub = dir.join("sub");
        fs::create_dir(&sub).unwrap();
        for f in 0..10 {
            fs::write(sub.join(format!("s{f}.bak")), vec![b'y'; 1000 + f]).unwrap();
        }
    }
    // Edge cases: unicode, empty dirs, deep + long path, hidden.
    fs::write(root.join("relatório ação ✓ 日本.pdf"), vec![1u8; 4096]).unwrap();
    fs::create_dir_all(root.join("vazio").join("vazio2")).unwrap();
    let mut deep = root.join("deep");
    for i in 0..25 {
        deep.push(format!("nivel_profundo_com_nome_longo_{i:02}"));
    }
    let long = PathBuf::from(format!(r"\\?\{}", deep.display()));
    #[cfg(windows)]
    fs::create_dir_all(&long).unwrap();
    #[cfg(windows)]
    fs::write(long.join("final.dat"), vec![2u8; 777]).unwrap();
    #[cfg(not(windows))]
    {
        let _ = long;
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("final.dat"), vec![2u8; 777]).unwrap();
    }
}

fn opts(root: &Path, backend: Backend, frontier_cap: usize) -> ScanOptions {
    let mut tuning = Tuning::for_profile(Profile::Balanced, false, 4);
    tuning.frontier_mem_cap = frontier_cap;
    tuning.chunk_size = 64; // force multi-chunk directories
    tuning.commit_rows = 200; // many checkpoints
    ScanOptions { root: root.to_path_buf(), profile: Profile::Balanced, backend, tuning, cross_mounts: false, spill_dir: Some(std::env::temp_dir()) }
}

fn scan(o: &ScanOptions, db: &Path) -> ScanReport {
    let en = make_enumerator(o.backend);
    let progress = Arc::new(ScanProgress::default());
    let w = IndexWriter::create(db, o, en.name(), progress.clone()).unwrap();
    run_scan(o, en, Box::new(w), ScanStart::Fresh, Arc::new(ScanControl::new(o.tuning.threads)), progress)
}

#[test]
fn scan_matches_reference_walk_for_both_backends() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("tree");
    build_tree(&root);
    let exp = reference_walk(&root);
    assert!(exp.files > 1000);
    for backend in [Backend::Native, Backend::Portable] {
        let db = t.path().join(format!("{backend:?}.nxs"));
        let r = scan(&opts(&root, backend, 64), &db);
        assert_eq!(r.outcome.state, ScanState::Completed, "{r:?}");
        assert_eq!(r.outcome.files, exp.files, "{backend:?} files");
        assert_eq!(r.outcome.bytes, exp.bytes, "{backend:?} bytes");
        let s = Snapshot::open(&db).unwrap();
        assert!(s.verify().unwrap().is_empty(), "{:?}", s.verify());
        let sum = s.summary().unwrap();
        assert!(sum.complete);
        assert_eq!(sum.files, exp.files);
        // Rankings and queries.
        let top = s.top_files(5).unwrap();
        assert_eq!(top[0].size, 4096);
        assert!(top[0].path.ends_with("relatório ação ✓ 日本.pdf"), "{}", top[0].path);
        let baks = s.query_files(&FileQuery { ext: Some(".BAK".into()), limit: 1000, ..Default::default() }).unwrap();
        assert_eq!(baks.len(), 200);
        let big = s.query_files(&FileQuery { min_size: Some(1005), ext: Some("bak".into()), limit: 1000, ..Default::default() }).unwrap();
        assert_eq!(big.len(), 100);
        let deep = s.query_files(&FileQuery { name_contains: Some("final.dat".into()), limit: 10, ..Default::default() }).unwrap();
        assert_eq!(deep.len(), 1);
        assert!(deep[0].path.len() > 260, "long path rebuilt: {}", deep[0].path.len());
        let ls = s.list_dir(1, 0, 100).unwrap();
        assert!(ls.iter().any(|r| r.is_dir && r.name == "vazio" && r.size == 0));
        let years = s.year_stats().unwrap();
        assert_eq!(years.iter().map(|y| y.files).sum::<u64>(), exp.files);
    }
}

#[test]
fn spill_to_disk_keeps_results_identical() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("wide");
    fs::create_dir_all(&root).unwrap();
    for i in 0..3000 {
        let d = root.join(format!("w{i:04}"));
        fs::create_dir(&d).unwrap();
        fs::write(d.join("a"), b"12345").unwrap();
    }
    let o = opts(&root, Backend::Native, 64);
    let en = make_enumerator(o.backend);
    let progress = Arc::new(ScanProgress::default());
    let w = IndexWriter::create(&t.path().join("w.nxs"), &o, en.name(), progress.clone()).unwrap();
    let p2 = progress.clone();
    let h = std::thread::spawn(move || run_scan(&o, en, Box::new(w), ScanStart::Fresh, Arc::new(ScanControl::new(2)), p2));
    let mut max_spill = 0;
    while !h.is_finished() {
        max_spill = max_spill.max(progress.spilled_dirs.load(Relaxed));
        std::thread::yield_now();
    }
    let r = h.join().unwrap();
    assert_eq!(r.outcome.state, ScanState::Completed);
    assert_eq!(r.outcome.files, 3000);
    assert_eq!(r.outcome.dirs, 3001);
    assert!(max_spill > 1000, "frontier should have spilled, max {max_spill}");
}

#[test]
fn cancel_then_resume_gives_exact_totals_without_duplicates() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("tree");
    build_tree(&root);
    let exp = reference_walk(&root);
    let db = t.path().join("r.nxs");
    let mut o = opts(&root, Backend::Native, 64);
    o.tuning.threads = 2;

    // Run 1: cancel after ~300 files.
    let en = make_enumerator(o.backend);
    let progress = Arc::new(ScanProgress::default());
    let w = IndexWriter::create(&db, &o, en.name(), progress.clone()).unwrap();
    let control = Arc::new(ScanControl::new(2));
    let (c2, p2, o2) = (control.clone(), progress.clone(), o.clone());
    let h = std::thread::spawn(move || run_scan(&o2, en, Box::new(w), ScanStart::Fresh, c2, p2));
    while progress.files.load(Relaxed) < 300 && !h.is_finished() {
        std::thread::yield_now();
    }
    control.cancel();
    let r1 = h.join().unwrap();
    assert_eq!(r1.outcome.state, ScanState::Interrupted);
    {
        let s = Snapshot::open(&db).unwrap();
        let sum = s.summary().unwrap();
        assert!(!sum.complete, "interrupted snapshot must not look complete");
        assert_eq!(sum.state, "interrupted");
    }

    // Run 2: resume.
    let progress = Arc::new(ScanProgress::default());
    let (w, plan) = open_for_resume(&db, 8192, progress.clone()).unwrap();
    assert!(plan.pending_dirs > 0);
    let r2 = run_scan(&o, make_enumerator(o.backend), Box::new(w), ScanStart::Resume { next_id: plan.next_id, pending: plan.pending }, Arc::new(ScanControl::new(2)), progress);
    assert_eq!(r2.outcome.state, ScanState::Completed, "{r2:?}");
    assert_eq!(r2.outcome.files, exp.files);
    assert_eq!(r2.outcome.bytes, exp.bytes);
    let s = Snapshot::open(&db).unwrap();
    assert!(s.verify().unwrap().is_empty(), "{:?}", s.verify());
    assert_eq!(s.summary().unwrap().resumed_count, 1);
    // Completed snapshots are not resumable.
    assert!(open_for_resume(&db, 8192, Arc::new(ScanProgress::default())).is_err());
}

#[test]
fn in_memory_aggregates_equal_sql_aggregates() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("tree");
    build_tree(&root);
    let db = t.path().join("a.nxs");
    let r = scan(&opts(&root, Backend::Native, 64), &db);
    assert_eq!(r.outcome.state, ScanState::Completed);
    let c = rusqlite::Connection::open(&db).unwrap();
    let ext_mem: Vec<(i64, i64, i64)> = c.prepare("SELECT ext, files, size FROM ext_stats ORDER BY ext").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(|x| x.unwrap()).collect();
    let ext_sql: Vec<(i64, i64, i64)> = c.prepare("SELECT ext, COUNT(*), SUM(size) FROM files GROUP BY ext ORDER BY ext").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(|x| x.unwrap()).collect();
    assert_eq!(ext_mem, ext_sql);
    let y_mem: Vec<(Option<i64>, i64, i64)> = c.prepare("SELECT year, files, size FROM year_stats ORDER BY year").unwrap().query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(|x| x.unwrap()).collect();
    let y_sql: Vec<(Option<i64>, i64, i64)> = c
        .prepare("SELECT CAST(strftime('%Y', mtime, 'unixepoch') AS INTEGER) y, COUNT(*), SUM(size) FROM files GROUP BY y ORDER BY y")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(|x| x.unwrap())
        .collect();
    assert_eq!(y_mem, y_sql);
}

#[test]
fn partial_snapshot_is_queryable_and_labeled() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("tree");
    build_tree(&root);
    let db = t.path().join("p.nxs");
    let mut o = opts(&root, Backend::Native, 64);
    o.tuning.threads = 1;
    let en = make_enumerator(o.backend);
    let progress = Arc::new(ScanProgress::default());
    let w = IndexWriter::create(&db, &o, en.name(), progress.clone()).unwrap();
    let control = Arc::new(ScanControl::new(1));
    let (c2, p2, o2) = (control.clone(), progress.clone(), o.clone());
    let h = std::thread::spawn(move || run_scan(&o2, en, Box::new(w), ScanStart::Fresh, c2, p2));
    while progress.rows_committed.load(Relaxed) < 200 && !h.is_finished() {
        std::thread::yield_now();
    }
    control.cancel();
    assert_eq!(h.join().unwrap().outcome.state, ScanState::Interrupted);

    let s = Snapshot::open(&db).unwrap();
    let sum = s.summary().unwrap();
    assert!(!sum.complete && !sum.finalized);
    assert!(sum.files > 0, "committed counters visible: {sum:?}");
    // Every read path works without finalization (table scans, time-limited).
    assert!(!s.list_dir(1, 0, 50).unwrap().is_empty());
    assert!(!s.list_subdirs(1, 0, 50).unwrap().is_empty());
    assert!(!s.top_files(5).unwrap().is_empty());
    let _ = s.top_dirs(5).unwrap();
    let q = s.query_files(&FileQuery { ext: Some("txt".into()), limit: 10, ..Default::default() }).unwrap();
    assert!(q.len() <= 10);
    let (d, f) = s.child_counts(1).unwrap();
    assert!(d + f > 0);
}

#[test]
fn crash_during_finalization_is_recoverable() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("tree");
    build_tree(&root);
    let exp = reference_walk(&root);
    let db = t.path().join("f.nxs");
    let o = opts(&root, Backend::Native, 64);
    assert_eq!(scan(&o, &db).outcome.state, ScanState::Completed);
    // Simulate a crash in the middle of finalization: state marker left as
    // "finalizing", some indexes missing, roll-up and aggregates half-done.
    {
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute_batch(
            "UPDATE meta SET value='finalizing' WHERE key='state';
             DROP INDEX idx_dirs_tsize; DROP INDEX idx_files_ext;
             UPDATE dirs SET t_size=NULL, t_files=NULL WHERE depth < 2;
             DELETE FROM ext_stats; DELETE FROM year_stats;",
        )
        .unwrap();
    }
    {
        let s = Snapshot::open(&db).unwrap();
        assert!(!s.summary().unwrap().complete, "a crashed finalization must not look complete");
    }
    let progress = Arc::new(ScanProgress::default());
    let (w, plan) = open_for_resume(&db, 8192, progress.clone()).unwrap();
    assert_eq!(plan.pending_dirs, 0, "all directories were already enumerated");
    let r = run_scan(&o, make_enumerator(o.backend), Box::new(w), ScanStart::Resume { next_id: plan.next_id, pending: plan.pending }, Arc::new(ScanControl::new(2)), progress);
    assert_eq!(r.outcome.state, ScanState::Completed, "{r:?}");
    let s = Snapshot::open(&db).unwrap();
    assert!(s.verify().unwrap().is_empty(), "{:?}", s.verify());
    let sum = s.summary().unwrap();
    assert!(sum.complete && sum.finalized);
    assert_eq!((sum.files, sum.bytes), (exp.files, exp.bytes));
    assert_eq!(s.ext_stats(100).unwrap().iter().map(|e| e.files).sum::<u64>(), exp.files);
}

#[test]
fn categories_from_extensions() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("tree");
    build_tree(&root);
    let exp = reference_walk(&root);
    let db = t.path().join("c.nxs");
    scan(&opts(&root, Backend::Native, 64), &db);
    let s = Snapshot::open(&db).unwrap();
    let cats = s.category_stats().unwrap();
    assert_eq!(cats.iter().map(|c| c.files).sum::<u64>(), exp.files);
    assert_eq!(cats.iter().map(|c| c.size).sum::<u64>(), exp.bytes);
    let get = |id: &str| cats.iter().find(|c| c.id == id).map(|c| c.files).unwrap_or(0);
    assert_eq!(get("documents"), 1001, "1000 .txt + 1 .pdf");
    assert_eq!(get("backups"), 200);
    assert_eq!(get("other"), 1, ".dat");
    let q = |c: &str| s.query_files(&FileQuery { category: Some(c.into()), limit: 5000, ..Default::default() }).unwrap().len();
    assert_eq!(q("documents"), 1001);
    assert_eq!(q("backups"), 200);
    assert_eq!(q("other"), 1);
}

#[test]
fn filter_indexes_are_deferred_and_recoverable() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("tree");
    build_tree(&root);
    let db = t.path().join("d.nxs");
    assert_eq!(scan(&opts(&root, Backend::Native, 64), &db).outcome.state, ScanState::Completed);
    {
        let s = Snapshot::open(&db).unwrap();
        let sum = s.summary().unwrap();
        assert!(sum.complete && sum.finalized && sum.filter_indexes_ready);
        assert!(sum.usable_ms.is_some());
    }
    // Crash after the snapshot became usable but before filter indexes finished.
    {
        let c = rusqlite::Connection::open(&db).unwrap();
        c.execute_batch("DROP INDEX idx_files_ext; DROP INDEX idx_files_mtime; UPDATE meta SET value='pending' WHERE key='filter_indexes';").unwrap();
    }
    {
        let s = Snapshot::open(&db).unwrap();
        let sum = s.summary().unwrap();
        assert!(sum.complete && !sum.filter_indexes_ready, "usable, filters pending");
        let q = s.query_files(&FileQuery { ext: Some("bak".into()), limit: 1000, ..Default::default() }).unwrap();
        assert_eq!(q.len(), 200, "filters still work (time-limited scan)");
    }
    nextsize_index::build_filter_indexes(&db).unwrap();
    let s = Snapshot::open(&db).unwrap();
    assert!(s.summary().unwrap().filter_indexes_ready);
    assert!(s.verify().unwrap().is_empty());
}

#[test]
fn compare_two_real_scans_by_path() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("tree");
    build_tree(&root);
    let (a, b) = (t.path().join("a.nxs"), t.path().join("b.nxs"));
    let o = opts(&root, Backend::Native, 64);
    scan(&o, &a);
    // Changes: d01 grows by 1 MB (new file in d01\sub), d02 removed, "novo" added.
    fs::write(root.join("d01").join("sub").join("grande.bin"), vec![0u8; 1 << 20]).unwrap();
    fs::remove_dir_all(root.join("d02")).unwrap();
    fs::create_dir(root.join("novo")).unwrap();
    fs::write(root.join("novo").join("x.dat"), vec![1u8; 5000]).unwrap();
    scan(&o, &b);
    let (sa, sb) = (Snapshot::open(&a).unwrap(), Snapshot::open(&b).unwrap());
    let lvl = nextsize_index::compare_children(&sa, &sb, &[], 100).unwrap();
    assert!(lvl.a_found && lvl.b_found);
    let row = |n: &str| lvl.rows.iter().find(|r| r.name == n).unwrap_or_else(|| panic!("{n} missing: {:?}", lvl.rows));
    assert_eq!(row("d01").status, "grew");
    assert_eq!(row("d01").delta, 1 << 20);
    assert_eq!(row("d02").status, "removed");
    assert_eq!(row("novo").status, "added");
    assert_eq!(row("novo").delta, 5000);
    assert_eq!(row("d03").status, "same");
    assert_eq!(lvl.rows[0].name, "d01", "largest change first");
    assert_eq!(lvl.b_total.unwrap() as i64 - lvl.a_total.unwrap() as i64, lvl.rows.iter().map(|r| r.delta).sum::<i64>());
    // Drill down.
    let sub = nextsize_index::compare_children(&sa, &sb, &["d01".into()], 100).unwrap();
    assert_eq!(sub.rows[0].name, "sub");
    assert_eq!(sub.rows[0].delta, 1 << 20);
    let gone = nextsize_index::compare_children(&sa, &sb, &["d02".into()], 100).unwrap();
    assert!(gone.a_found && !gone.b_found);
}

#[test]
fn catalog_label_and_import_validation() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("tree");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("a.txt"), b"x").unwrap();
    let dir = t.path().join("snaps");
    fs::create_dir_all(&dir).unwrap();
    let db = dir.join("s1.nxs");
    scan(&opts(&root, Backend::Native, 64), &db);
    fs::write(dir.join("lixo.nxs"), b"not sqlite at all").unwrap();
    nextsize_index::set_label(&db, "Antes da limpeza\u{7}").unwrap();
    let list = nextsize_index::list_snapshots(&dir, 50);
    assert_eq!(list.len(), 2);
    let good = list.iter().find(|e| e.file_name == "s1.nxs").unwrap();
    assert_eq!(good.label.as_deref(), Some("Antes da limpeza"), "control chars stripped");
    assert_eq!(good.files, Some(1));
    assert!(list.iter().find(|e| e.file_name == "lixo.nxs").unwrap().invalid.is_some());
    assert!(nextsize_index::validate_for_import(&db).is_ok());
    assert!(nextsize_index::validate_for_import(&dir.join("lixo.nxs")).is_err());
}

#[cfg(windows)]
#[test]
fn junction_loop_is_not_followed_and_access_denied_is_reported() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("tree");
    fs::create_dir_all(root.join("a")).unwrap();
    fs::write(root.join("a").join("x.txt"), b"abc").unwrap();
    // Junction pointing back to the root: a loop if it were followed.
    let st = Command::new("cmd").args(["/C", "mklink", "/J"]).arg(root.join("a").join("loop")).arg(&root).output().unwrap();
    assert!(st.status.success(), "mklink failed: {}", String::from_utf8_lossy(&st.stderr));
    // Directory the current user may not list.
    let denied = root.join("negado");
    fs::create_dir(&denied).unwrap();
    fs::write(denied.join("secret.txt"), b"s").unwrap();
    let user = std::env::var("USERNAME").unwrap();
    let st = Command::new("icacls").arg(&denied).args(["/deny", &format!("{user}:(RD)")]).output().unwrap();
    assert!(st.status.success());

    let db = t.path().join("j.nxs");
    let r = scan(&opts(&root, Backend::Native, 64), &db);
    // Restore permissions before asserting so the temp dir can be cleaned.
    let _ = Command::new("icacls").arg(&denied).args(["/remove:d", &user]).output();

    assert_eq!(r.outcome.state, ScanState::CompletedWithWarnings, "{r:?}");
    assert_eq!(r.outcome.files, 1, "only a\\x.txt is readable; loop not followed");
    let s = Snapshot::open(&db).unwrap();
    let issues = s.issues(0, 10).unwrap();
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].kind, "access_denied");
    let ls = s.list_dir(2, 0, 10).unwrap_or_default();
    let _ = ls;
    let a_children = s.top_dirs(10).unwrap();
    let loop_row = a_children.iter().find(|d| d.name == "loop").expect("junction recorded");
    assert!(loop_row.flags & flags::NOT_FOLLOWED != 0);
    let neg = a_children.iter().find(|d| d.name == "negado").expect("denied dir recorded");
    assert!(neg.flags & flags::ENUM_ERROR != 0);
}
