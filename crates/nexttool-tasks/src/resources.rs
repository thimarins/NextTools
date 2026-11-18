use std::path::{Component, Path, Prefix};

/// Resource key used to keep tasks from competing for the same volume or
/// share: `vol:c:` for local drives, `unc:\\server\share` for network shares,
/// `fs:/` style for Unix (first component).
pub fn resource_key(path: &Path) -> String {
    match path.components().next() {
        Some(Component::Prefix(p)) => match p.kind() {
            Prefix::Disk(l) | Prefix::VerbatimDisk(l) => format!("vol:{}:", (l as char).to_ascii_lowercase()),
            Prefix::UNC(s, sh) | Prefix::VerbatimUNC(s, sh) => {
                format!("unc:\\\\{}\\{}", s.to_string_lossy().to_lowercase(), sh.to_string_lossy().to_lowercase())
            }
            _ => format!("path:{}", path.to_string_lossy().to_lowercase()),
        },
        _ => {
            let first = path.components().nth(1).map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
            format!("fs:/{first}")
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn keys() {
        assert_eq!(resource_key(Path::new(r"C:\data\x")), "vol:c:");
        assert_eq!(resource_key(Path::new(r"\\NAS01\Share\a")), r"unc:\\nas01\share");
    }
}
