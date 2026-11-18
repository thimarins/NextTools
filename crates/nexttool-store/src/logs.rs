//! Size-bounded rotating log files: `nexttool.log`, `nexttool.1.log`, …
//! Total disk use ≤ max_bytes × (keep + 1).

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct RotatingFile {
    dir: PathBuf,
    base: String,
    max_bytes: u64,
    keep: usize,
    file: Option<File>,
    written: u64,
}

impl RotatingFile {
    pub fn new(dir: &Path, base: &str, max_bytes: u64, keep: usize) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let mut r = RotatingFile { dir: dir.to_path_buf(), base: base.to_string(), max_bytes: max_bytes.max(64 * 1024), keep, file: None, written: 0 };
        r.open()?;
        Ok(r)
    }

    fn name(&self, i: usize) -> PathBuf {
        if i == 0 {
            self.dir.join(format!("{}.log", self.base))
        } else {
            self.dir.join(format!("{}.{i}.log", self.base))
        }
    }

    fn open(&mut self) -> io::Result<()> {
        let p = self.name(0);
        let f = OpenOptions::new().create(true).append(true).open(&p)?;
        self.written = f.metadata().map(|m| m.len()).unwrap_or(0);
        self.file = Some(f);
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file = None;
        let _ = std::fs::remove_file(self.name(self.keep));
        for i in (0..self.keep).rev() {
            let from = self.name(i);
            if from.exists() {
                let _ = std::fs::rename(&from, self.name(i + 1));
            }
        }
        self.open()
    }
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.written + buf.len() as u64 > self.max_bytes {
            self.rotate()?;
        }
        let n = self.file.as_mut().ok_or_else(|| io::Error::other("log closed"))?.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        if let Some(f) = self.file.as_mut() {
            f.flush()?;
        }
        Ok(())
    }
}

#[derive(Clone)]
struct Shared(Arc<Mutex<RotatingFile>>);

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).flush()
    }
}

/// Logs to `<data>/logs/nexttool.log` (rotating, 10 MB × 6) and to stderr
/// for warnings. Filter with `NEXTTOOL_LOG` (e.g. `debug`). Never logs secrets:
/// NextTool does not handle credentials at all (Windows session is used).
pub fn init_logging(log_dir: &Path, stderr_level: &str) -> io::Result<()> {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    let file = Shared(Arc::new(Mutex::new(RotatingFile::new(log_dir, "nexttool", 10 * 1024 * 1024, 5)?)));
    let file_filter = EnvFilter::try_from_env("NEXTTOOL_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let file_layer = fmt::layer().with_ansi(false).with_target(false).with_writer(move || file.clone()).with_filter(file_filter);
    let err_layer = fmt::layer().with_target(false).with_writer(io::stderr).with_filter(EnvFilter::new(stderr_level));
    let _ = tracing_subscriber::registry().with(file_layer).with(err_layer).try_init();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_bounds_disk_use() {
        let t = tempfile::tempdir().unwrap();
        let mut r = RotatingFile::new(t.path(), "x", 64 * 1024, 2).unwrap();
        let line = vec![b'a'; 1000];
        for _ in 0..1000 {
            r.write_all(&line).unwrap();
        }
        r.flush().unwrap();
        let files: Vec<_> = std::fs::read_dir(t.path()).unwrap().map(|e| e.unwrap().metadata().unwrap().len()).collect();
        assert_eq!(files.len(), 3);
        assert!(files.iter().all(|&l| l <= 64 * 1024));
    }
}
