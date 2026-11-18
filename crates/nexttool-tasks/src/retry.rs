use nexttool_core::ErrorKind;

/// Bounded exponential backoff for task-level retries (Robocopy already
/// retries per file with /R /W; this covers whole-run failures such as a
/// share that disappeared).
#[derive(Debug, Clone, Copy)]
pub struct RetryPolicy {
    pub base_s: u64,
    pub factor: u64,
    pub max_s: u64,
    pub max_attempts: u32,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy { base_s: 30, factor: 2, max_s: 900, max_attempts: 5 }
    }
}

impl RetryPolicy {
    /// Delay before attempt `attempt + 1` (attempt counts from 1).
    pub fn delay_s(&self, attempt: u32) -> u64 {
        let mut d = self.base_s;
        for _ in 1..attempt {
            d = d.saturating_mul(self.factor);
            if d >= self.max_s {
                return self.max_s;
            }
        }
        d.min(self.max_s)
    }

    /// Retry only transient errors, never beyond `max_attempts`.
    pub fn should_retry(&self, kind: ErrorKind, attempt: u32) -> bool {
        kind.is_transient() && attempt < self.max_attempts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backoff() {
        let p = RetryPolicy::default();
        assert_eq!(p.delay_s(1), 30);
        assert_eq!(p.delay_s(2), 60);
        assert_eq!(p.delay_s(3), 120);
        assert_eq!(p.delay_s(10), 900);
        assert!(p.should_retry(ErrorKind::Network, 1));
        assert!(!p.should_retry(ErrorKind::Network, 5));
        assert!(!p.should_retry(ErrorKind::AccessDenied, 1));
        assert!(!p.should_retry(ErrorKind::DiskFull, 1));
    }
}
