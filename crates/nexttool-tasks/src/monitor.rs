//! Evidence-based activity diagnosis. Never kills anything: it only
//! classifies what it observes and says how confident it is.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    /// Progress counters, I/O or log are moving.
    Active,
    /// Moving, but persistently much slower than this task's own baseline.
    Slow,
    /// No observable progress, I/O or log growth for the stall window. May be
    /// a blocked OS call (e.g. unresponsive share) — or a legitimately long
    /// single operation. Not a definitive hang.
    NoObservableProgress,
    /// Process is gone.
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnosis {
    pub activity: Activity,
    pub confidence: Confidence,
    pub evidence: Vec<String>,
    pub rate_bytes_s: f64,
    pub baseline_bytes_s: f64,
}

/// One observation of a running task.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sample {
    /// Seconds since task start.
    pub t: f64,
    /// Monotonic work counter (files done, entries scanned…).
    pub work: u64,
    /// Bytes processed (copy) — or 0.
    pub bytes: u64,
    /// Process I/O transfer counters (read + write).
    pub io: u64,
    /// Log size in bytes.
    pub log: u64,
    pub alive: bool,
}

pub struct ActivityMonitor {
    stall_after_s: f64,
    warmup_s: f64,
    slow_ratio: f64,
    slow_after_s: f64,
    last: Option<Sample>,
    last_change_t: f64,
    short: Ewma,
    long: Ewma,
    slow_since: Option<f64>,
}

struct Ewma {
    tau: f64,
    v: Option<f64>,
}

impl Ewma {
    fn push(&mut self, x: f64, dt: f64) {
        let a = 1.0 - (-dt / self.tau).exp();
        self.v = Some(match self.v {
            None => x,
            Some(v) => v + a * (x - v),
        });
    }
}

impl ActivityMonitor {
    /// `stall_after_s`: no change at all for this long → NoObservableProgress.
    pub fn new(stall_after_s: f64) -> Self {
        ActivityMonitor {
            stall_after_s,
            warmup_s: 120.0,
            slow_ratio: 0.3,
            slow_after_s: 180.0,
            last: None,
            last_change_t: 0.0,
            short: Ewma { tau: 30.0, v: None },
            long: Ewma { tau: 600.0, v: None },
            slow_since: None,
        }
    }

    pub fn observe(&mut self, s: Sample) -> Diagnosis {
        let mut ev = Vec::new();
        if !s.alive {
            self.last = Some(s);
            return Diagnosis { activity: Activity::Exited, confidence: Confidence::High, evidence: vec!["processo encerrado".into()], rate_bytes_s: 0.0, baseline_bytes_s: 0.0 };
        }
        let (rate, changed) = match self.last {
            Some(p) if s.t > p.t => {
                let dt = s.t - p.t;
                let rate = s.bytes.saturating_sub(p.bytes) as f64 / dt;
                self.short.push(rate, dt);
                if s.t >= self.warmup_s / 4.0 {
                    self.long.push(rate, dt);
                }
                (rate, s.work != p.work || s.bytes != p.bytes || s.io != p.io || s.log != p.log)
            }
            Some(_) => (0.0, false),
            None => (0.0, true),
        };
        if changed {
            self.last_change_t = s.t;
        }
        self.last = Some(s);
        let short = self.short.v.unwrap_or(rate);
        let long = self.long.v.unwrap_or(short);
        let idle = s.t - self.last_change_t;
        if idle >= self.stall_after_s {
            ev.push(format!("nenhuma alteração em progresso, I/O do processo ou log há {:.0} s", idle));
            // Confidence grows with time without any signal, but never "High":
            // a single huge file on a slow link can legitimately look like this
            // only if I/O counters also stop, which is unusual — hence Medium.
            let conf = if idle >= self.stall_after_s * 3.0 { Confidence::Medium } else { Confidence::Low };
            return Diagnosis { activity: Activity::NoObservableProgress, confidence: conf, evidence: ev, rate_bytes_s: short, baseline_bytes_s: long };
        }
        if s.t >= self.warmup_s && long > 0.0 && short < long * self.slow_ratio {
            let since = *self.slow_since.get_or_insert(s.t);
            if s.t - since >= self.slow_after_s {
                ev.push(format!(
                    "taxa recente {:.1} MB/s está abaixo de {:.0}% da média desta tarefa ({:.1} MB/s) há {:.0} s",
                    short / 1e6,
                    self.slow_ratio * 100.0,
                    long / 1e6,
                    s.t - since
                ));
                return Diagnosis { activity: Activity::Slow, confidence: Confidence::Medium, evidence: ev, rate_bytes_s: short, baseline_bytes_s: long };
            }
        } else {
            self.slow_since = None;
        }
        Diagnosis { activity: Activity::Active, confidence: Confidence::High, evidence: ev, rate_bytes_s: short, baseline_bytes_s: long }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_then_stalled_then_resumes() {
        let mut m = ActivityMonitor::new(60.0);
        let mut bytes = 0;
        for t in 0..30 {
            bytes += 10_000_000;
            let d = m.observe(Sample { t: t as f64, work: t, bytes, io: bytes, log: 0, alive: true });
            assert_eq!(d.activity, Activity::Active);
        }
        // Nothing moves for 61 s.
        let mut d = m.observe(Sample { t: 30.0, work: 29, bytes, io: bytes, log: 0, alive: true });
        for t in 31..=91 {
            d = m.observe(Sample { t: t as f64, work: 29, bytes, io: bytes, log: 0, alive: true });
        }
        assert_eq!(d.activity, Activity::NoObservableProgress);
        assert_eq!(d.confidence, Confidence::Low);
        // I/O alone moving is enough to be "active" (large file being copied).
        let d = m.observe(Sample { t: 92.0, work: 29, bytes, io: bytes + 1, log: 0, alive: true });
        assert_eq!(d.activity, Activity::Active);
    }

    #[test]
    fn persistent_slowdown_is_flagged_not_transient_dip() {
        let mut m = ActivityMonitor::new(600.0);
        let mut b = 0u64;
        let mut t = 0.0;
        while t < 600.0 {
            b += 100_000_000;
            m.observe(Sample { t, work: t as u64, bytes: b, io: b, log: 0, alive: true });
            t += 1.0;
        }
        // Short dip (20 s) → still active.
        for _ in 0..20 {
            b += 5_000_000;
            let d = m.observe(Sample { t, work: t as u64, bytes: b, io: b, log: 0, alive: true });
            assert_ne!(d.activity, Activity::Slow);
            t += 1.0;
        }
        let mut last = None;
        for _ in 0..400 {
            b += 5_000_000;
            last = Some(m.observe(Sample { t, work: t as u64, bytes: b, io: b, log: 0, alive: true }));
            t += 1.0;
        }
        let d = last.unwrap();
        assert_eq!(d.activity, Activity::Slow, "{d:?}");
        assert!(!d.evidence.is_empty());
    }

    #[test]
    fn exited() {
        let mut m = ActivityMonitor::new(60.0);
        assert_eq!(m.observe(Sample { alive: false, ..Default::default() }).activity, Activity::Exited);
    }
}
