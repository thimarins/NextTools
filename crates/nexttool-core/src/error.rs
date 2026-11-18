use serde::{Deserialize, Serialize};

/// Platform-neutral classification of a failure. Drives retry policy,
/// alert wording and whether user intervention is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    AccessDenied,
    NotFound,
    PathTooLong,
    /// Connection lost, share unavailable, server unreachable.
    Network,
    Timeout,
    /// Bad credentials / logon failure / credential conflict.
    Authentication,
    DiskFull,
    /// File in use by another process (sharing violation / lock).
    Locked,
    InvalidName,
    /// Content or metadata mismatch found by verification.
    Integrity,
    /// Our own persisted state is unreadable or inconsistent.
    CorruptState,
    Cancelled,
    Io,
    Internal,
}

impl ErrorKind {
    pub fn as_str(self) -> &'static str {
        use ErrorKind::*;
        match self {
            AccessDenied => "access_denied",
            NotFound => "not_found",
            PathTooLong => "path_too_long",
            Network => "network",
            Timeout => "timeout",
            Authentication => "authentication",
            DiskFull => "disk_full",
            Locked => "locked",
            InvalidName => "invalid_name",
            Integrity => "integrity",
            CorruptState => "corrupt_state",
            Cancelled => "cancelled",
            Io => "io",
            Internal => "internal",
        }
    }

    /// Worth retrying automatically (with backoff and a bounded count).
    pub fn is_transient(self) -> bool {
        matches!(self, ErrorKind::Network | ErrorKind::Timeout | ErrorKind::Locked)
    }

    /// Retrying will not help until a person changes something.
    pub fn needs_user(self) -> bool {
        matches!(
            self,
            ErrorKind::AccessDenied
                | ErrorKind::Authentication
                | ErrorKind::DiskFull
                | ErrorKind::PathTooLong
                | ErrorKind::CorruptState
                | ErrorKind::Integrity
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Warning => "warning",
            Severity::Error => "error",
            Severity::Critical => "critical",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        [Severity::Info, Severity::Warning, Severity::Error, Severity::Critical]
            .into_iter()
            .find(|v| v.as_str() == s)
    }
}

/// Classifies a Win32 error code (also what Robocopy prints as `0x…`).
/// On Unix, errno values are mapped by `classify_errno`.
pub fn classify_os_error(code: u32) -> ErrorKind {
    match code {
        5 | 1307 => ErrorKind::AccessDenied,             // ACCESS_DENIED, INVALID_OWNER
        2 | 3 | 267 | 161 => ErrorKind::NotFound,        // FILE/PATH_NOT_FOUND, DIRECTORY, BAD_PATHNAME
        206 => ErrorKind::PathTooLong,                   // FILENAME_EXCED_RANGE
        123 => ErrorKind::InvalidName,                   // INVALID_NAME
        32 | 33 => ErrorKind::Locked,                    // SHARING/LOCK_VIOLATION
        39 | 112 | 1295 => ErrorKind::DiskFull,          // HANDLE_DISK_FULL, DISK_FULL, DISK_QUOTA
        121 | 1460 => ErrorKind::Timeout,                // SEM_TIMEOUT, TIMEOUT
        86 | 1326 | 1219 | 1244 | 1909 | 1311 => ErrorKind::Authentication, // bad pwd, logon fail, cred conflict, not authenticated, locked out, no logon servers
        51 | 53 | 54 | 55 | 58 | 59 | 64 | 67 | 1203 | 1222 | 1231 | 1232 | 1236 | 1240 | 2250 => ErrorKind::Network,
        1117 | 23 | 1392 => ErrorKind::Io,               // device I/O, CRC, corrupt file
        _ => ErrorKind::Io,
    }
}

/// Classifies a Unix errno.
pub fn classify_errno(errno: i32) -> ErrorKind {
    match errno {
        1 | 13 => ErrorKind::AccessDenied,     // EPERM, EACCES
        2 | 20 => ErrorKind::NotFound,         // ENOENT, ENOTDIR
        36 => ErrorKind::PathTooLong,          // ENAMETOOLONG
        28 | 122 => ErrorKind::DiskFull,       // ENOSPC, EDQUOT
        110 => ErrorKind::Timeout,             // ETIMEDOUT
        112 | 113 | 101 | 103 | 104 | 116 => ErrorKind::Network, // EHOSTDOWN, EHOSTUNREACH, ENETUNREACH, ECONNABORTED, ECONNRESET, ESTALE
        _ => ErrorKind::Io,
    }
}

/// Classifies a std::io::Error on the current platform.
pub fn classify_io(e: &std::io::Error) -> ErrorKind {
    if let Some(code) = e.raw_os_error() {
        #[cfg(windows)]
        return classify_os_error(code as u32);
        #[cfg(not(windows))]
        return classify_errno(code);
    }
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => ErrorKind::AccessDenied,
        std::io::ErrorKind::NotFound => ErrorKind::NotFound,
        std::io::ErrorKind::TimedOut => ErrorKind::Timeout,
        _ => ErrorKind::Io,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classification() {
        assert_eq!(classify_os_error(5), ErrorKind::AccessDenied);
        assert_eq!(classify_os_error(64), ErrorKind::Network);
        assert_eq!(classify_os_error(112), ErrorKind::DiskFull);
        assert_eq!(classify_os_error(1326), ErrorKind::Authentication);
        assert!(ErrorKind::Network.is_transient());
        assert!(!ErrorKind::DiskFull.is_transient());
        assert!(ErrorKind::DiskFull.needs_user());
    }
}
