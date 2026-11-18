//! Process identity and resource readings. A process is identified by
//! (pid, start time) so a recycled pid is never mistaken for an old process.

use serde::Serialize;

#[derive(Debug, Clone, Copy, Default, Serialize)]
pub struct ProcStats {
    pub working_set: u64,
    pub peak_working_set: u64,
    /// Private committed bytes (Windows PagefileUsage / Linux VmData approximation).
    pub private_bytes: u64,
    pub peak_private_bytes: u64,
    pub user_cpu_ms: u64,
    pub kernel_cpu_ms: u64,
    pub io_read_bytes: u64,
    pub io_write_bytes: u64,
}

#[cfg(windows)]
mod imp {
    use super::ProcStats;
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetExitCodeProcess, GetProcessIoCounters, GetProcessTimes, OpenProcess, IO_COUNTERS,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    const STILL_ACTIVE: u32 = 259;

    fn ft(t: FILETIME) -> u64 {
        ((t.dwHighDateTime as u64) << 32) | t.dwLowDateTime as u64
    }

    fn times(h: HANDLE) -> Option<(u64, u64, u64)> {
        let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut c, mut e, mut k, mut u) = (z, z, z, z);
        // SAFETY: valid handle, out pointers to locals.
        let ok = unsafe { GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) };
        (ok != 0).then(|| (ft(c), ft(k), ft(u)))
    }

    pub fn current_start() -> Option<u64> {
        // SAFETY: pseudo handle.
        times(unsafe { GetCurrentProcess() }).map(|t| t.0)
    }

    pub fn start_of(pid: u32) -> Option<u64> {
        // SAFETY: plain query; handle closed below.
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h.is_null() {
            return None;
        }
        let t = times(h).map(|t| t.0);
        // SAFETY: handle from OpenProcess.
        unsafe { CloseHandle(h) };
        t
    }

    pub fn alive(pid: u32, start: u64) -> Option<bool> {
        // SAFETY: plain query; handle closed below.
        let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if h.is_null() {
            return Some(false); // no such process (or not ours to query: treated as gone)
        }
        let mut code = 0u32;
        // SAFETY: valid handle.
        let running = unsafe { GetExitCodeProcess(h, &mut code) } != 0 && code == STILL_ACTIVE;
        let same = times(h).map(|t| t.0 == start);
        // SAFETY: handle from OpenProcess.
        unsafe { CloseHandle(h) };
        Some(running && same.unwrap_or(false))
    }

    pub fn self_stats() -> ProcStats {
        let mut s = ProcStats::default();
        // SAFETY: pseudo handle + correctly sized out structs.
        unsafe {
            let h = GetCurrentProcess();
            let mut m: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
            m.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
            if GetProcessMemoryInfo(h, &mut m as *mut _ as *mut _, m.cb) != 0 {
                s.working_set = m.WorkingSetSize as u64;
                s.peak_working_set = m.PeakWorkingSetSize as u64;
                s.private_bytes = m.PrivateUsage as u64;
                s.peak_private_bytes = m.PeakPagefileUsage as u64;
            }
            if let Some((_, k, u)) = times(h) {
                s.kernel_cpu_ms = k / 10_000;
                s.user_cpu_ms = u / 10_000;
            }
            let mut io: IO_COUNTERS = std::mem::zeroed();
            if GetProcessIoCounters(h, &mut io) != 0 {
                s.io_read_bytes = io.ReadTransferCount;
                s.io_write_bytes = io.WriteTransferCount;
            }
        }
        s
    }
}

#[cfg(unix)]
mod imp {
    use super::ProcStats;

    fn stat_field(pid: u32, idx: usize) -> Option<u64> {
        let s = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        let after = &s[s.rfind(')')? + 2..];
        after.split_whitespace().nth(idx - 3).and_then(|v| v.parse().ok())
    }

    pub fn current_start() -> Option<u64> {
        stat_field(std::process::id(), 22)
    }

    pub fn alive(pid: u32, start: u64) -> Option<bool> {
        Some(stat_field(pid, 22) == Some(start))
    }

    pub fn start_of(pid: u32) -> Option<u64> {
        stat_field(pid, 22)
    }

    pub fn self_stats() -> ProcStats {
        let mut s = ProcStats::default();
        if let Ok(st) = std::fs::read_to_string("/proc/self/status") {
            for l in st.lines() {
                let kb = |l: &str| l.split_whitespace().nth(1).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * 1024;
                if l.starts_with("VmRSS:") { s.working_set = kb(l); }
                if l.starts_with("VmHWM:") { s.peak_working_set = kb(l); }
                if l.starts_with("VmData:") { s.private_bytes = kb(l); }
            }
            s.peak_private_bytes = s.private_bytes;
        }
        let tick_ms = 10; // USER_HZ = 100 on Linux
        s.user_cpu_ms = stat_field(std::process::id(), 14).unwrap_or(0) * tick_ms;
        s.kernel_cpu_ms = stat_field(std::process::id(), 15).unwrap_or(0) * tick_ms;
        if let Ok(io) = std::fs::read_to_string("/proc/self/io") {
            for l in io.lines() {
                let v = |l: &str| l.split_whitespace().nth(1).and_then(|v| v.parse().ok()).unwrap_or(0);
                if l.starts_with("read_bytes:") { s.io_read_bytes = v(l); }
                if l.starts_with("write_bytes:") { s.io_write_bytes = v(l); }
            }
        }
        s
    }
}

/// Start time of the current process (opaque, comparable).
pub fn current_start() -> Option<u64> {
    imp::current_start()
}

/// Whether process `pid` with start time `start` is still running.
pub fn is_alive(pid: u32, start: u64) -> Option<bool> {
    imp::alive(pid, start)
}

pub fn self_stats() -> ProcStats {
    imp::self_stats()
}

/// Start time of another process (to record child process identity).
pub fn start_of(pid: u32) -> Option<u64> {
    imp::start_of(pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity() {
        let st = current_start().expect("start time");
        assert_eq!(is_alive(std::process::id(), st), Some(true));
        assert_eq!(is_alive(std::process::id(), st + 1), Some(false));
        let s = self_stats();
        assert!(s.working_set > 0 && s.peak_working_set >= s.working_set);
    }
}
