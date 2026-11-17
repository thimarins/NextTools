//! File extension normalization.

/// Max extension length kept. Longer "extensions" are usually not extensions
/// (e.g. `report.final-version-2023`) and would bloat the dictionary.
pub const MAX_EXT_LEN: usize = 16;

/// Returns the normalized (lowercase) extension of a file name, or `None`.
/// `.gitignore` has no extension; `archive.tar.gz` has `gz`.
pub fn normalize_ext(name: &str) -> Option<String> {
    let dot = name.rfind('.')?;
    if dot == 0 || dot + 1 == name.len() {
        return None;
    }
    let ext = &name[dot + 1..];
    if ext.len() > MAX_EXT_LEN || ext.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    Some(ext.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes() {
        assert_eq!(normalize_ext("a.TXT").as_deref(), Some("txt"));
        assert_eq!(normalize_ext("x.tar.gz").as_deref(), Some("gz"));
        assert_eq!(normalize_ext(".gitignore"), None);
        assert_eq!(normalize_ext("noext"), None);
        assert_eq!(normalize_ext("trailing."), None);
        assert_eq!(normalize_ext("a.b c"), None);
        assert_eq!(normalize_ext("a.abcdefghijklmnopq"), None);
        assert_eq!(normalize_ext("ação.PDF").as_deref(), Some("pdf"));
    }
}
