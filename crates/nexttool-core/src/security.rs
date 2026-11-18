//! Path validation. Every path coming from the UI, CLI or a stored task is
//! validated before it reaches an engine or a child process.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    #[error("caminho vazio")]
    Empty,
    #[error("caminho contém caractere NUL ou de controle")]
    ControlChar,
    #[error("caminho precisa ser absoluto: {0}")]
    NotAbsolute(String),
    #[error("caminho contém '..' ou '.' (não permitido): {0}")]
    Traversal(String),
    #[error("caminhos de dispositivo (\\\\.\\) não são permitidos: {0}")]
    DevicePath(String),
    #[error("caracteres não permitidos em argumento de processo: {0}")]
    UnsafeChar(String),
    #[error("origem e destino se sobrepõem: {0}")]
    Overlap(String),
}

/// Validates a user-supplied path and returns it unchanged if acceptable.
/// Rules: absolute, no `.`/`..` components, no control characters, no
/// `\\.\` device namespace.
pub fn validate_user_path(p: &Path) -> Result<PathBuf, PathError> {
    let s = p.to_string_lossy();
    if s.trim().is_empty() {
        return Err(PathError::Empty);
    }
    if s.chars().any(|c| c.is_control()) {
        return Err(PathError::ControlChar);
    }
    if s.starts_with(r"\\.\") || s.starts_with("//./") {
        return Err(PathError::DevicePath(s.into_owned()));
    }
    if !p.is_absolute() {
        return Err(PathError::NotAbsolute(s.into_owned()));
    }
    if p.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) {
        return Err(PathError::Traversal(s.into_owned()));
    }
    Ok(p.to_path_buf())
}

/// Additional check for paths passed as arguments to a child process.
/// We never use a shell, but `"` would still confuse Windows argv parsing.
pub fn validate_process_arg_path(p: &Path) -> Result<PathBuf, PathError> {
    let v = validate_user_path(p)?;
    let s = v.to_string_lossy();
    if s.contains('"') || s.contains('*') || s.contains('?') || s.contains('|') || s.contains('<') || s.contains('>') {
        return Err(PathError::UnsafeChar(s.into_owned()));
    }
    Ok(v)
}

fn norm(p: &Path) -> String {
    let mut s = p.to_string_lossy().replace('/', "\\");
    while s.len() > 3 && s.ends_with('\\') {
        s.pop();
    }
    if cfg!(windows) {
        s = s.to_lowercase();
    }
    s
}

/// True if `child` is `root` or inside it (string-based, case-insensitive on
/// Windows; does not resolve links).
pub fn is_within(child: &Path, root: &Path) -> bool {
    let (c, r) = (norm(child), norm(root));
    c == r || (c.starts_with(&r) && (r.ends_with('\\') || c[r.len()..].starts_with('\\')))
}

/// Copy source and destination must not contain each other.
pub fn check_no_overlap(src: &Path, dst: &Path) -> Result<(), PathError> {
    if is_within(dst, src) || is_within(src, dst) {
        return Err(PathError::Overlap(format!("{} ↔ {}", src.display(), dst.display())));
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn validation() {
        assert!(validate_user_path(Path::new(r"C:\data")).is_ok());
        assert!(validate_user_path(Path::new(r"\\nas\share\x")).is_ok());
        assert_eq!(validate_user_path(Path::new("")), Err(PathError::Empty));
        assert!(matches!(validate_user_path(Path::new(r"data\x")), Err(PathError::NotAbsolute(_))));
        assert!(matches!(validate_user_path(Path::new(r"C:\a\..\b")), Err(PathError::Traversal(_))));
        assert!(matches!(validate_user_path(Path::new(r"\\.\PhysicalDrive0")), Err(PathError::DevicePath(_))));
        assert!(matches!(validate_process_arg_path(Path::new(r#"C:\a" /PURGE"#)), Err(PathError::UnsafeChar(_))));
    }

    #[test]
    fn overlap() {
        assert!(is_within(Path::new(r"C:\Data\Sub"), Path::new(r"c:\data")));
        assert!(!is_within(Path::new(r"C:\Data2"), Path::new(r"C:\Data")));
        assert!(is_within(Path::new(r"C:\x"), Path::new(r"C:\")));
        assert!(check_no_overlap(Path::new(r"C:\src"), Path::new(r"C:\src\backup")).is_err());
        assert!(check_no_overlap(Path::new(r"C:\src"), Path::new(r"D:\dst")).is_ok());
    }
}
