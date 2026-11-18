use std::ffi::OsString;
use std::path::Path;

use nextcopy_core::{CopyOptions, OverwritePolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Copy,
    /// `/L`: list only, nothing is copied, deleted or timestamped.
    DryRun,
}

/// Builds the argument vector. Paths must already be validated
/// (`CopyJob::validate`). `files`: explicit file names for a selection group
/// (empty = whole tree with include filters).
pub fn build_args(src: &Path, dst: &Path, files: &[String], recursive: bool, o: &CopyOptions, mode: Mode, log: &Path) -> Vec<OsString> {
    let mut a: Vec<OsString> = vec![strip_trailing(src), strip_trailing(dst)];
    if files.is_empty() {
        for p in &o.filters.include_files {
            a.push(p.into());
        }
    } else {
        for f in files {
            a.push(f.into());
        }
    }
    let mut s = |v: &str| a.push(v.into());
    if recursive {
        if o.mirror {
            s("/MIR");
        } else if o.include_empty_dirs {
            s("/E");
        } else {
            s("/S");
        }
    }
    let mut copy = String::from("/COPY:");
    if o.flags.data { copy.push('D'); }
    if o.flags.attributes { copy.push('A'); }
    if o.flags.timestamps { copy.push('T'); }
    if o.flags.security { copy.push('S'); }
    if o.flags.owner { copy.push('O'); }
    if copy.len() == 6 {
        copy.push('D');
    }
    s(&copy);
    s(if o.copy_dir_timestamps { "/DCOPY:DAT" } else { "/DCOPY:DA" });
    s(&format!("/R:{}", o.retries));
    s(&format!("/W:{}", o.retry_wait_secs));
    if o.threads > 1 {
        s(&format!("/MT:{}", o.threads.min(128)));
    }
    if o.restartable {
        s("/Z");
    }
    if o.unbuffered {
        s("/J");
    }
    if o.exclude_junctions {
        s("/XJ");
    }
    match o.overwrite {
        OverwritePolicy::ReplaceChanged => {}
        OverwritePolicy::ReplaceOlderOnly => s("/XO"),
        OverwritePolicy::NeverReplace => {
            s("/XC");
            s("/XN");
            s("/XO");
        }
    }
    if let Some(v) = o.filters.max_size { s(&format!("/MAX:{v}")); }
    if let Some(v) = o.filters.min_size { s(&format!("/MIN:{v}")); }
    if let Some(v) = o.filters.max_age_days { s(&format!("/MAXAGE:{v}")); }
    if let Some(v) = o.filters.min_age_days { s(&format!("/MINAGE:{v}")); }
    if mode == Mode::DryRun {
        s("/L");
        s("/X");
    }
    // Machine-friendly logging: full paths, exact bytes, no % progress, no job header.
    s("/FP");
    s("/BYTES");
    s("/NP");
    s("/NJH");
    if mode == Mode::Copy {
        s("/NDL");
        if !o.list_files_in_log {
            s("/NFL");
        }
    }
    if !o.filters.exclude_files.is_empty() {
        a.push("/XF".into());
        a.extend(o.filters.exclude_files.iter().map(OsString::from));
    }
    if !o.filters.exclude_dirs.is_empty() {
        a.push("/XD".into());
        a.extend(o.filters.exclude_dirs.iter().map(OsString::from));
    }
    let mut l = OsString::from("/UNILOG:");
    l.push(log.as_os_str());
    a.push(l);
    a
}

/// Robocopy treats `"C:\dir\"` as an escaped quote; never pass a trailing
/// backslash (except for a bare drive root, which needs `C:\.`).
fn strip_trailing(p: &Path) -> OsString {
    let s = p.to_string_lossy();
    let t = s.trim_end_matches(['\\', '/']);
    if t.len() == 2 && t.ends_with(':') {
        return format!("{t}\\.").into();
    }
    t.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nextcopy_core::CopyProfile;
    use std::path::PathBuf;

    fn v(a: &[OsString]) -> Vec<String> {
        a.iter().map(|x| x.to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn safe_profile_args() {
        let o = CopyProfile::Safe.options();
        let a = v(&build_args(Path::new(r"C:\src\"), Path::new(r"\\nas\share\dst"), &[], true, &o, Mode::Copy, &PathBuf::from(r"C:\logs\r.log")));
        assert_eq!(a[0], r"C:\src");
        assert_eq!(a[1], r"\\nas\share\dst");
        for want in ["/E", "/COPY:DAT", "/DCOPY:DAT", "/R:3", "/W:10", "/MT:8", "/XJ", "/FP", "/BYTES", "/NP", "/NJH", "/NDL"] {
            assert!(a.contains(&want.to_string()), "missing {want} in {a:?}");
        }
        assert!(!a.contains(&"/MIR".to_string()));
        assert!(!a.contains(&"/L".to_string()));
        assert_eq!(a.last().unwrap(), r"/UNILOG:C:\logs\r.log");
    }

    #[test]
    fn mirror_dry_run_and_filters() {
        let mut o = CopyProfile::Mirror.options();
        o.filters.exclude_files = vec!["*.tmp".into(), "~$*".into()];
        o.filters.exclude_dirs = vec!["$RECYCLE.BIN".into()];
        o.overwrite = OverwritePolicy::NeverReplace;
        let a = v(&build_args(Path::new(r"D:\"), Path::new(r"E:\bk"), &[], true, &o, Mode::DryRun, Path::new(r"C:\l.log")));
        assert_eq!(a[0], r"D:\.");
        assert!(a.contains(&"/MIR".into()) && a.contains(&"/L".into()) && a.contains(&"/X".into()));
        assert!(a.contains(&"/XC".into()) && a.contains(&"/XN".into()) && a.contains(&"/XO".into()));
        let xf = a.iter().position(|x| x == "/XF").unwrap();
        assert_eq!(&a[xf + 1..xf + 3], &["*.tmp".to_string(), "~$*".to_string()]);
    }

    #[test]
    fn selection_is_not_recursive() {
        let o = CopyProfile::Safe.options();
        let a = v(&build_args(Path::new(r"C:\s\a"), Path::new(r"D:\d\a"), &["x y.txt".into()], false, &o, Mode::Copy, Path::new(r"C:\l.log")));
        assert_eq!(a[2], "x y.txt");
        assert!(!a.contains(&"/E".into()) && !a.contains(&"/S".into()) && !a.contains(&"/MIR".into()));
    }
}
