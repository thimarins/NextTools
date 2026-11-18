use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// Pause / resume / cancel / concurrency control shared with the UI.
///
/// Pause stops scheduling new directories; directories already being read
/// finish their current enumeration call (an OS call blocked on a slow share
/// cannot be interrupted, see ADR-009).
#[derive(Debug)]
pub struct ScanControl {
    cancel: AtomicBool,
    paused: AtomicBool,
    allowed: AtomicUsize,
    max: usize,
    lock: Mutex<()>,
    cv: Condvar,
}

impl ScanControl {
    pub fn new(max_threads: usize) -> Self {
        let max = max_threads.max(1);
        ScanControl {
            cancel: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            allowed: AtomicUsize::new(max),
            max,
            lock: Mutex::new(()),
            cv: Condvar::new(),
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.cv.notify_all();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
        self.cv.notify_all();
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Adjusts active enumeration threads (1..=max). Used by profiles today
    /// and by the adaptive controller later.
    pub fn set_concurrency(&self, n: usize) {
        self.allowed.store(n.clamp(1, self.max), Ordering::SeqCst);
        self.cv.notify_all();
    }

    pub fn concurrency(&self) -> usize {
        self.allowed.load(Ordering::Relaxed)
    }

    pub fn max(&self) -> usize {
        self.max
    }

    /// Blocks worker `idx` while paused or throttled. Returns false on cancel
    /// or when `finished()` reports that there is no more work (otherwise a
    /// throttled worker would wait forever after the scan ended).
    pub fn wait_runnable(&self, idx: usize, finished: &dyn Fn() -> bool) -> bool {
        loop {
            if self.is_cancelled() || finished() {
                return false;
            }
            if !self.is_paused() && idx < self.concurrency() {
                return true;
            }
            let g = self.lock.lock().unwrap_or_else(|p| p.into_inner());
            let _ = self.cv.wait_timeout(g, Duration::from_millis(200));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn pause_blocks_and_cancel_releases() {
        let c = Arc::new(ScanControl::new(4));
        assert!(c.wait_runnable(3, &|| false));
        c.set_concurrency(2);
        c.pause();
        let c2 = c.clone();
        let h = std::thread::spawn(move || c2.wait_runnable(0, &|| false));
        std::thread::sleep(Duration::from_millis(100));
        assert!(!h.is_finished());
        c.cancel();
        assert!(!h.join().unwrap());
    }

    #[test]
    fn throttled_worker_exits_when_work_is_finished() {
        let c = Arc::new(ScanControl::new(4));
        c.set_concurrency(1);
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (c2, d2) = (c.clone(), done.clone());
        let h = std::thread::spawn(move || c2.wait_runnable(3, &|| d2.load(Ordering::Relaxed)));
        std::thread::sleep(Duration::from_millis(100));
        assert!(!h.is_finished());
        done.store(true, Ordering::Relaxed);
        assert!(!h.join().unwrap());
    }
}
