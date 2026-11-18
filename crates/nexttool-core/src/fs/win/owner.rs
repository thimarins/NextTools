//! Owner (SID → DOMAIN\name) lookup, read-only. Used on demand for a bounded
//! set of paths (never during the scan), with a SID → name cache.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use windows_sys::Win32::Foundation::{LocalFree, ERROR_SUCCESS};
use windows_sys::Win32::Security::Authorization::{ConvertSidToStringSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT};
use windows_sys::Win32::Security::{LookupAccountSidW, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SID_NAME_USE};

use super::path::to_extended_wide;
use crate::error::{classify_os_error, ErrorKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owner {
    /// "DOMAIN\name", or the SID string when the account no longer resolves.
    pub name: String,
    pub sid: String,
    /// The SID could not be resolved to an account (deleted user, other domain).
    pub orphan: bool,
}

fn wide_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

/// Bounded SID → resolved-name cache (domain lookups can be slow).
pub struct OwnerResolver {
    cache: Mutex<HashMap<String, (String, bool)>>,
}

const CACHE_CAP: usize = 10_000;

impl Default for OwnerResolver {
    fn default() -> Self {
        OwnerResolver { cache: Mutex::new(HashMap::new()) }
    }
}

impl OwnerResolver {
    pub fn owner_of(&self, path: &Path) -> Result<Owner, (ErrorKind, String)> {
        let wide = to_extended_wide(path, "");
        let mut owner: PSID = std::ptr::null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: valid NUL-terminated path; out pointers to locals; only the owner is requested.
        let rc = unsafe {
            GetNamedSecurityInfoW(wide.as_ptr(), SE_FILE_OBJECT, OWNER_SECURITY_INFORMATION, &mut owner, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), &mut sd)
        };
        if rc != ERROR_SUCCESS {
            return Err((classify_os_error(rc), std::io::Error::from_raw_os_error(rc as i32).to_string()));
        }
        // SID → string.
        let mut sid_w: *mut u16 = std::ptr::null_mut();
        // SAFETY: owner points into `sd`, valid until LocalFree(sd).
        let sid = if unsafe { ConvertSidToStringSidW(owner, &mut sid_w) } != 0 {
            let mut n = 0;
            // SAFETY: sid_w is a NUL-terminated string allocated by the API.
            while unsafe { *sid_w.add(n) } != 0 {
                n += 1;
            }
            // SAFETY: n valid u16 before the terminator.
            let s = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(sid_w, n) });
            // SAFETY: allocated with LocalAlloc by the API.
            unsafe { LocalFree(sid_w as _) };
            s
        } else {
            String::from("(SID ilegível)")
        };
        let cached = self.cache.lock().unwrap_or_else(|p| p.into_inner()).get(&sid).cloned();
        let (name, orphan) = match cached {
            Some(v) => v,
            None => {
                let mut name = [0u16; 256];
                let mut dom = [0u16; 256];
                let (mut nl, mut dl) = (name.len() as u32, dom.len() as u32);
                let mut use_: SID_NAME_USE = 0;
                // SAFETY: buffers and lengths are consistent; owner is valid.
                let ok = unsafe { LookupAccountSidW(std::ptr::null(), owner, name.as_mut_ptr(), &mut nl, dom.as_mut_ptr(), &mut dl, &mut use_) } != 0;
                let v = if ok {
                    let (d, n) = (wide_to_string(&dom), wide_to_string(&name));
                    (if d.is_empty() { n } else { format!("{d}\\{n}") }, false)
                } else {
                    (sid.clone(), true)
                };
                let mut c = self.cache.lock().unwrap_or_else(|p| p.into_inner());
                if c.len() < CACHE_CAP {
                    c.insert(sid.clone(), v.clone());
                }
                v
            }
        };
        // SAFETY: allocated by GetNamedSecurityInfoW.
        unsafe { LocalFree(sd as _) };
        Ok(Owner { name, sid, orphan })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_file_has_current_user_as_owner() {
        let t = tempfile::tempdir().unwrap();
        let f = t.path().join("x.txt");
        std::fs::write(&f, b"x").unwrap();
        let r = OwnerResolver::default();
        let o = r.owner_of(&f).unwrap();
        let user = std::env::var("USERNAME").unwrap().to_lowercase();
        assert!(o.sid.starts_with("S-1-"), "{o:?}");
        // Owner is the current user, or the Administrators group when elevated.
        assert!(o.name.to_lowercase().ends_with(&user) || o.name.to_lowercase().contains("admin"), "{o:?}");
        // Second lookup hits the cache and returns the same.
        assert_eq!(r.owner_of(&f).unwrap(), o);
        assert!(r.owner_of(&t.path().join("missing")).is_err());
    }
}
