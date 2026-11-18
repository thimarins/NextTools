use std::time::{SystemTime, UNIX_EPOCH};

/// 100ns intervals between 1601-01-01 and 1970-01-01.
const FILETIME_UNIX_DIFF: u64 = 116_444_736_000_000_000;

/// Converts a Windows FILETIME (100ns since 1601) to Unix seconds.
/// Returns `None` for 0 (meaning "not set").
pub fn filetime_to_unix(ft: u64) -> Option<i64> {
    if ft == 0 {
        return None;
    }
    let secs_1601 = (ft / 10_000_000) as i64;
    Some(secs_1601 - (FILETIME_UNIX_DIFF / 10_000_000) as i64)
}

pub fn system_time_to_unix(t: SystemTime) -> Option<i64> {
    match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).ok(),
        Err(e) => i64::try_from(e.duration().as_secs()).ok().map(|s| -s),
    }
}

pub fn now_unix() -> i64 {
    system_time_to_unix(SystemTime::now()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filetime() {
        assert_eq!(filetime_to_unix(0), None);
        assert_eq!(filetime_to_unix(FILETIME_UNIX_DIFF), Some(0));
        // 2020-01-01T00:00:00Z
        assert_eq!(filetime_to_unix(132_223_104_000_000_000), Some(1_577_836_800));
    }
}
