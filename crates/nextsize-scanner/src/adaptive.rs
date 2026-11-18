//! Adaptive enumeration concurrency for network sources (SMB/NFS/NAS).
//!
//! More threads are not always faster on a remote server. The controller
//! measures directories completed per second and:
//! - every few quiet intervals, probes one more thread and keeps it only if
//!   the rate improved by at least `GAIN` (otherwise reverts and backs off);
//! - halves concurrency when network/timeout errors appear (multiplicative
//!   decrease), protecting production servers;
//! - never grows while the writer is saturated (persistence is the
//!   bottleneck, more enumeration threads would not help);
//! - stays within [1, max].

/// Minimum relative improvement to keep a probed thread.
pub const GAIN: f64 = 0.05;

/// Fewer threads are kept if the rate drops by at most this fraction
/// (same throughput with less load on the server is better).
pub const TOLERANCE_DOWN: f64 = 0.03;

#[derive(Debug, Clone)]
pub struct Adaptive {
    pub current: usize,
    pub max: usize,
    probing: Option<(usize, f64)>, // (previous level, rate before probe)
    cooldown: u32,
    best_rate: f64,
    /// Next probe direction: true = up, false = down.
    up: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Hold,
    Probe,
    Keep,
    Revert,
    BackOffErrors,
}

impl Adaptive {
    pub fn new(initial: usize, max: usize) -> Self {
        let max = max.max(1);
        Adaptive { current: initial.clamp(1, max), max, probing: None, cooldown: 1, best_rate: 0.0, up: true }
    }

    /// One control step with the rate observed during the last interval.
    pub fn step(&mut self, rate: f64, net_errors: u64, writer_saturated: bool) -> Decision {
        if net_errors > 0 {
            self.current = (self.current / 2).max(1);
            self.probing = None;
            self.cooldown = 5;
            return Decision::BackOffErrors;
        }
        if let Some((prev, before)) = self.probing.take() {
            let went_up = self.current > prev;
            let keep = if went_up { rate >= before * (1.0 + GAIN) } else { rate >= before * (1.0 - TOLERANCE_DOWN) };
            if keep {
                // Keep going in the same direction soon.
                self.best_rate = rate;
                self.cooldown = 1;
                self.up = went_up;
                return Decision::Keep;
            }
            self.current = prev;
            self.cooldown = 4; // wait before probing again
            self.up = !went_up; // try the other direction next time
            return Decision::Revert;
        }
        self.best_rate = self.best_rate.max(rate);
        if self.cooldown > 0 {
            self.cooldown -= 1;
            return Decision::Hold;
        }
        if rate <= 0.0 {
            return Decision::Hold;
        }
        let can_up = !writer_saturated && self.current < self.max;
        let can_down = self.current > 1;
        let go_up = if self.up { can_up } else { !can_down && can_up };
        if !go_up && !can_down {
            return Decision::Hold;
        }
        self.probing = Some((self.current, rate));
        if go_up {
            self.current += 1;
        } else {
            self.current -= 1;
        }
        Decision::Probe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Simulated server: throughput grows linearly up to `k` concurrent
    /// requests, then degrades (contention).
    fn server(threads: usize, k: usize) -> f64 {
        if threads <= k {
            100.0 * threads as f64
        } else {
            100.0 * k as f64 * (1.0 - 0.08 * (threads - k) as f64)
        }
    }

    #[test]
    fn converges_near_server_optimum_and_respects_max() {
        for k in [2usize, 6, 11] {
            let mut a = Adaptive::new(4, 16);
            for _ in 0..200 {
                let r = server(a.current, k);
                a.step(r, 0, false);
                assert!(a.current >= 1 && a.current <= 16);
            }
            assert!((a.current as i64 - k as i64).abs() <= 1, "k={k} ended at {}", a.current);
        }
        let mut a = Adaptive::new(4, 5);
        for _ in 0..100 {
            a.step(server(a.current, 30), 0, false);
        }
        assert_eq!(a.current, 5, "capped at max");
    }

    #[test]
    fn errors_halve_and_saturated_writer_blocks_growth() {
        let mut a = Adaptive::new(8, 16);
        assert_eq!(a.step(500.0, 3, false), Decision::BackOffErrors);
        assert_eq!(a.current, 4);
        assert_eq!(a.step(500.0, 1, false), Decision::BackOffErrors);
        assert_eq!(a.current, 2);
        // Writer is the bottleneck: never grows (may shed threads, which is fine).
        let mut b = Adaptive::new(3, 16);
        for _ in 0..50 {
            b.step(1000.0, 0, true);
            assert!(b.current <= 3, "grew to {} with saturated writer", b.current);
        }
    }
}
