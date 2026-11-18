use std::io;
use std::path::PathBuf;
use std::sync::atomic::Ordering::Relaxed;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use nextsize_core::ScanProgress;

use crate::control::ScanControl;
use crate::engine::DirTask;
use crate::spill::SpillStack;

/// Pending directories: LIFO (depth-first keeps the set small), bounded in
/// RAM, overflow spilled to disk. Tracks active workers for termination.
pub struct Frontier {
    inner: Mutex<Inner>,
    cv: Condvar,
    mem_cap: usize,
    progress: Arc<ScanProgress>,
}

struct Inner {
    stack: Vec<DirTask>,
    spill: Option<SpillStack>,
    spill_dir: PathBuf,
    active: usize,
    done: bool,
}

impl Frontier {
    pub fn new(mem_cap: usize, spill_dir: PathBuf, progress: Arc<ScanProgress>) -> Self {
        Frontier {
            inner: Mutex::new(Inner { stack: Vec::new(), spill: None, spill_dir, active: 0, done: false }),
            cv: Condvar::new(),
            mem_cap: mem_cap.max(64),
            progress,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn publish(&self, g: &Inner) {
        let spilled = g.spill.as_ref().map_or(0, |s| s.count());
        self.progress.pending_dirs.store(g.stack.len() as u64 + spilled, Relaxed);
        self.progress.spilled_dirs.store(spilled, Relaxed);
    }

    pub fn push(&self, t: DirTask) -> io::Result<()> {
        let mut g = self.lock();
        if g.stack.len() >= self.mem_cap {
            if g.spill.is_none() {
                let dir = g.spill_dir.clone();
                g.spill = Some(SpillStack::create(&dir)?);
            }
            g.spill.as_mut().unwrap().push(&t)?;
        } else {
            g.stack.push(t);
        }
        self.publish(&g);
        drop(g);
        self.cv.notify_one();
        Ok(())
    }

    /// Next directory to read, or `None` when the scan is finished or cancelled.
    pub fn pop(&self, control: &ScanControl) -> io::Result<Option<DirTask>> {
        let mut g = self.lock();
        loop {
            if control.is_cancelled() || g.done {
                return Ok(None);
            }
            if let Some(t) = g.stack.pop() {
                g.active += 1;
                self.publish(&g);
                return Ok(Some(t));
            }
            // Refill from disk (half the RAM budget at a time).
            if let Some(sp) = g.spill.as_mut() {
                if sp.count() > 0 {
                    let mut batch = Vec::with_capacity(self.mem_cap / 2);
                    while batch.len() < self.mem_cap / 2 {
                        match sp.pop()? {
                            Some(t) => batch.push(t),
                            None => break,
                        }
                    }
                    batch.reverse(); // keep LIFO order
                    g.stack.extend(batch);
                    continue;
                }
            }
            if g.active == 0 {
                g.done = true;
                self.cv.notify_all();
                return Ok(None);
            }
            g = self.cv.wait_timeout(g, Duration::from_millis(200)).unwrap_or_else(|p| p.into_inner()).0;
        }
    }

    pub fn task_done(&self) {
        let mut g = self.lock();
        g.active -= 1;
        if g.active == 0 {
            self.cv.notify_all();
        }
    }

    /// True once all work is finished (nothing pending, no active worker).
    pub fn is_done(&self) -> bool {
        self.lock().done
    }

    pub fn wake_all(&self) {
        self.cv.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spills_beyond_cap_and_drains_everything() {
        let t = tempfile::tempdir().unwrap();
        let p = Arc::new(ScanProgress::default());
        let f = Frontier::new(64, t.path().to_path_buf(), p.clone());
        let c = ScanControl::new(1);
        for i in 0..1000 {
            f.push(DirTask { id: i, depth: 0, path: PathBuf::from(format!("/x/{i}")) }).unwrap();
        }
        assert_eq!(p.pending_dirs.load(Relaxed), 1000);
        assert!(p.spilled_dirs.load(Relaxed) > 900);
        let mut seen = std::collections::HashSet::new();
        while let Some(t) = f.pop(&c).unwrap() {
            assert!(seen.insert(t.id));
            f.task_done();
        }
        assert_eq!(seen.len(), 1000);
    }
}
