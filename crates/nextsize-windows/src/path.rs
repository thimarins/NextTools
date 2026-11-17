use std::os::windows::ffi::OsStrExt;
use std::path::Path;

/// Converts an absolute, normalized path to a NUL-terminated wide string with
/// the `\\?\` (or `\\?\UNC\`) prefix, lifting the MAX_PATH limit.
/// `suffix` is appended (e.g. `\*` for enumeration patterns).
///
/// The caller must pass an absolute path without `.`/`..` components; the
/// extended prefix disables Win32 normalization.
pub fn to_extended_wide(path: &Path, suffix: &str) -> Vec<u16> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let bs = b'\\' as u16;
    let starts = |p: &str| wide.len() >= p.len() && wide.iter().zip(p.encode_utf16()).all(|(a, b)| *a == b);

    let mut out: Vec<u16> = Vec::with_capacity(wide.len() + 12 + suffix.len());
    if starts(r"\\?\") || starts(r"\\.\") {
        out.extend_from_slice(&wide);
    } else if starts(r"\\") {
        out.extend(r"\\?\UNC\".encode_utf16());
        out.extend_from_slice(&wide[2..]);
    } else {
        out.extend(r"\\?\".encode_utf16());
        // Normalize forward slashes: the extended prefix does not.
        out.extend(wide.iter().map(|&c| if c == b'/' as u16 { bs } else { c }));
    }
    if !suffix.is_empty() {
        if out.last() == Some(&bs) && suffix.starts_with('\\') {
            out.extend(suffix[1..].encode_utf16());
        } else {
            out.extend(suffix.encode_utf16());
        }
    }
    out.push(0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: Vec<u16>) -> String {
        String::from_utf16(&v[..v.len() - 1]).unwrap()
    }

    #[test]
    fn prefixes() {
        assert_eq!(s(to_extended_wide(Path::new(r"C:\data"), r"\*")), r"\\?\C:\data\*");
        assert_eq!(s(to_extended_wide(Path::new(r"C:\"), r"\*")), r"\\?\C:\*");
        assert_eq!(s(to_extended_wide(Path::new(r"\\nas\share\x"), r"\*")), r"\\?\UNC\nas\share\x\*");
        assert_eq!(s(to_extended_wide(Path::new(r"\\?\D:\x"), "")), r"\\?\D:\x");
        assert_eq!(s(to_extended_wide(Path::new("C:/a/b"), "")), r"\\?\C:\a\b");
    }
}
