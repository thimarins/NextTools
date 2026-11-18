/// Formats bytes using binary units (KiB = 1024) with pt-BR-friendly labels.
pub fn format_bytes(b: u64) -> String {
    const U: [&str; 7] = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    if b < 1024 {
        return format!("{b} B");
    }
    let mut v = b as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", U[i])
}

/// Thousands separator (dot, pt-BR style).
pub fn format_count(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push('.');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fmt() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_count(1234567), "1.234.567");
        assert_eq!(format_count(12), "12");
    }
}
