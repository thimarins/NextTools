/// Case-insensitive `*` / `?` wildcard match on a file or directory name
/// (Robocopy /XF /XD semantics for name patterns).
pub fn wild_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let n: Vec<char> = name.to_lowercase().chars().collect();
    let (mut pi, mut ni) = (0usize, 0usize);
    let (mut star, mut mark) = (None::<usize>, 0usize);
    while ni < n.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == n[ni]) {
            pi += 1;
            ni += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            pi += 1;
            mark = ni;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ni = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::wild_match;
    #[test]
    fn matches() {
        assert!(wild_match("*.tmp", "a.TMP"));
        assert!(wild_match("~$*", "~$doc.docx"));
        assert!(wild_match("a?c", "abc"));
        assert!(!wild_match("*.tmp", "a.tmpx"));
        assert!(wild_match("*", ""));
        assert!(wild_match("$RECYCLE.BIN", "$Recycle.Bin"));
    }
}
