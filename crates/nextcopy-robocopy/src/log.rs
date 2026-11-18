//! Language-independent interpretation of a Robocopy `/UNILOG` log
//! (UTF-16LE) produced with `/FP /BYTES /NP /NJH`.
//!
//! Only structural features are used:
//! - file/dir lines: `\t<label>\t…\t<number>\t<full path>` → number + path
//!   (label ignored; a path ending in `\` is a directory line);
//! - error lines: contain `(0x` + 8 hex digits + `)` → Win32 code + path;
//! - job summary: lines whose text after the first `:` is exactly six
//!   integers; by position they are Dirs, Files, Bytes.
//! Whether a path is under the destination root distinguishes "extra"
//! (deletion candidates in /MIR) from source entries.

use std::path::Path;

use nexttool_core::classify_os_error;
use nexttool_core::security::is_within;
use nextcopy_core::{CopyStats, FileError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    File { size: u64, path: String },
    Dir { count: i64, path: String },
    Error { code: u32, path: Option<String> },
    SummaryRow([u64; 6]),
    Other,
}

fn path_start(s: &str) -> Option<usize> {
    let b = s.as_bytes();
    (0..b.len()).find(|&i| {
        (i + 2 < b.len() && b[i].is_ascii_alphabetic() && b[i + 1] == b':' && b[i + 2] == b'\\')
            || (i + 1 < b.len() && b[i] == b'\\' && b[i + 1] == b'\\')
    })
}

fn hex_code(s: &str) -> Option<(u32, usize)> {
    let i = s.find("(0x")?;
    let hex = s.get(i + 3..i + 11)?;
    if s.as_bytes().get(i + 11) != Some(&b')') {
        return None;
    }
    u32::from_str_radix(hex, 16).ok().map(|c| (c, i + 12))
}

pub fn parse_line(line: &str) -> Line {
    let line = line.trim_end_matches(['\r', '\n']);
    if let Some((code, after)) = hex_code(line) {
        let rest = &line[after..];
        let path = path_start(rest).map(|p| rest[p..].trim_end().to_string());
        return Line::Error { code, path };
    }
    if line.contains('\t') {
        let fields: Vec<&str> = line.split('\t').collect();
        if let Some(last) = fields.last() {
            let last_t = last.trim();
            if path_start(last_t) == Some(0) {
                // The number is the last token of the last non-empty field
                // before the path (dir lines put label and count in one field).
                if let Some(num) = fields[..fields.len() - 1]
                    .iter()
                    .rev()
                    .map(|f| f.trim())
                    .find(|f| !f.is_empty())
                    .and_then(|f| f.split_whitespace().last())
                {
                    if let Ok(n) = num.parse::<i64>() {
                        return if last_t.ends_with('\\') {
                            Line::Dir { count: n, path: last_t.to_string() }
                        } else {
                            Line::File { size: n.max(0) as u64, path: last_t.to_string() }
                        };
                    }
                }
            }
        }
    }
    if path_start(line).is_none() {
        if let Some(c) = line.find(':') {
            let toks: Vec<&str> = line[c + 1..].split_whitespace().collect();
            if toks.len() == 6 {
                let mut row = [0u64; 6];
                if toks.iter().enumerate().all(|(i, t)| t.parse::<u64>().map(|v| row[i] = v).is_ok()) {
                    return Line::SummaryRow(row);
                }
            }
        }
    }
    Line::Other
}

/// Incremental UTF-16LE decoder for a log file being written.
#[derive(Default)]
pub struct Utf16Lines {
    odd: Option<u8>,
    high: Option<u16>,
    buf: String,
}

impl Utf16Lines {
    pub fn feed(&mut self, bytes: &[u8], out: &mut Vec<String>) {
        let mut i = 0;
        let mut units: Vec<u16> = Vec::with_capacity(bytes.len() / 2 + 1);
        if let Some(b0) = self.odd.take() {
            if let Some(&b1) = bytes.first() {
                units.push(u16::from_le_bytes([b0, b1]));
                i = 1;
            } else {
                self.odd = Some(b0);
                return;
            }
        }
        while i + 1 < bytes.len() {
            units.push(u16::from_le_bytes([bytes[i], bytes[i + 1]]));
            i += 2;
        }
        if i < bytes.len() {
            self.odd = Some(bytes[i]);
        }
        for u in units {
            let ch = if let Some(h) = self.high.take() {
                if (0xDC00..0xE000).contains(&u) {
                    char::from_u32(0x10000 + (((h as u32) - 0xD800) << 10) + (u as u32 - 0xDC00))
                } else {
                    self.push_char('\u{FFFD}', out);
                    self.unit(u)
                }
            } else {
                self.unit(u)
            };
            if let Some(c) = ch {
                self.push_char(c, out);
            }
        }
    }

    fn unit(&mut self, u: u16) -> Option<char> {
        if (0xD800..0xDC00).contains(&u) {
            self.high = Some(u);
            None
        } else if (0xDC00..0xE000).contains(&u) {
            Some('\u{FFFD}')
        } else {
            char::from_u32(u as u32)
        }
    }

    fn push_char(&mut self, c: char, out: &mut Vec<String>) {
        match c {
            '\u{FEFF}' => {}
            '\n' => {
                let l = std::mem::take(&mut self.buf);
                out.push(l.trim_end_matches('\r').to_string());
            }
            c => {
                // Guard against a pathological unterminated line.
                if self.buf.len() < 64 * 1024 {
                    self.buf.push(c);
                }
            }
        }
    }

    pub fn finish(&mut self, out: &mut Vec<String>) {
        if !self.buf.is_empty() {
            out.push(std::mem::take(&mut self.buf));
        }
    }
}

const ERROR_SAMPLE_CAP: usize = 200;
const DELETE_SAMPLE_CAP: usize = 200;

/// Accumulates what a log tells us. All collections are capped.
#[derive(Debug, Default)]
pub struct LogState {
    pub dst_root: String,
    pub files_done: u64,
    pub bytes_done: u64,
    pub new_dirs: u64,
    pub extra_files: u64,
    pub extra_dirs: u64,
    pub delete_sample: Vec<String>,
    pub error_events: u64,
    /// Errors on a whole directory ("Accessing Destination Directory X\…").
    /// Robocopy may still exit with 0 in this case (verified on Windows 11):
    /// the directory was simply not processed.
    pub dir_error_events: u64,
    pub errors_sample: Vec<FileError>,
    pub summary: Vec<[u64; 6]>,
}

impl LogState {
    pub fn new(dst_root: &Path) -> Self {
        LogState { dst_root: dst_root.to_string_lossy().into_owned(), ..Default::default() }
    }

    fn in_dst(&self, p: &str) -> bool {
        is_within(Path::new(p), Path::new(&self.dst_root))
    }

    /// Returns the error (if the line was one) so the caller can emit an event.
    pub fn apply(&mut self, line: &str) -> Option<FileError> {
        match parse_line(line) {
            Line::File { size, path } => {
                if self.in_dst(&path) {
                    self.extra_files += 1;
                    if self.delete_sample.len() < DELETE_SAMPLE_CAP {
                        self.delete_sample.push(path);
                    }
                } else {
                    self.files_done += 1;
                    self.bytes_done = self.bytes_done.saturating_add(size);
                }
            }
            Line::Dir { path, .. } => {
                if self.in_dst(&path) {
                    self.extra_dirs += 1;
                    if self.delete_sample.len() < DELETE_SAMPLE_CAP {
                        self.delete_sample.push(path);
                    }
                } else {
                    self.new_dirs += 1;
                }
            }
            Line::Error { code, path } => {
                self.error_events += 1;
                if path.as_deref().is_none_or(|p| p.ends_with('\\')) {
                    self.dir_error_events += 1;
                }
                let fe = FileError { os_code: code, kind: classify_os_error(code), path: path.unwrap_or_default(), final_failure: false };
                if self.errors_sample.len() < ERROR_SAMPLE_CAP && !self.errors_sample.iter().any(|e| e.path == fe.path && e.os_code == fe.os_code) {
                    self.errors_sample.push(fe.clone());
                }
                return Some(fe);
            }
            Line::SummaryRow(r) => {
                if self.summary.len() < 3 {
                    self.summary.push(r);
                }
            }
            Line::Other => {}
        }
        None
    }

    /// Authoritative stats from the job summary, if it was written.
    pub fn summary_stats(&self) -> Option<CopyStats> {
        if self.summary.len() < 3 {
            return None;
        }
        let (d, f, b) = (self.summary[0], self.summary[1], self.summary[2]);
        Some(CopyStats {
            dirs_total: d[0],
            dirs_copied: d[1],
            dirs_failed: d[4],
            files_total: f[0],
            files_copied: f[1],
            files_skipped: f[2],
            files_mismatch: f[3],
            files_failed: f[4],
            files_extra: f[5],
            bytes_total: b[0],
            bytes_copied: b[1],
            bytes_failed: b[4],
            retries: 0,
        })
    }

    /// Best available stats: summary when present, else what the lines showed.
    pub fn stats(&self) -> CopyStats {
        let mut s = self.summary_stats().unwrap_or(CopyStats {
            files_copied: self.files_done,
            bytes_copied: self.bytes_done,
            files_extra: self.extra_files,
            dirs_copied: self.new_dirs,
            ..Default::default()
        });
        s.retries = self.error_events.saturating_sub(self.errors_sample.len() as u64);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(state: &mut LogState, text: &str) {
        for l in text.lines() {
            state.apply(l);
        }
    }

    /// Lines as written by en-US Robocopy with /FP /BYTES /NP /NJH /L /MIR.
    const EN: &str = "\t                   3\tC:\\src\\\n\
\t    New File  \t\t     1048576\tC:\\src\\a.bin\n\
\t    Newer     \t\t          12\tC:\\src\\b.txt\n\
\t*EXTRA File \t\t         100\tD:\\dst\\old.txt\n\
\t*EXTRA Dir        -1\tD:\\dst\\olddir\\\n\
2026/11/17 21:00:00 ERROR 5 (0x00000005) Copying File C:\\src\\locked.txt\n\
Access is denied.\n\
------------------------------------------------------------------------------\n\
\n\
               Total    Copied   Skipped  Mismatch    FAILED    Extras\n\
    Dirs :         2         1         1         0         0         1\n\
   Files :         4         2         1         0         1         1\n\
   Bytes :   1048788   1048588       100         0       100       100\n\
   Times :   0:00:00   0:00:00                       0:00:00   0:00:00\n";

    /// Same structure with pt-BR labels (fixture constructed from the
    /// documented format — not captured from a pt-BR system; see TEST_PLAN).
    const PT: &str = "\t    Novo arquivo  \t\t     1048576\tC:\\src\\a.bin\n\
\t    Mais novo \t\t          12\tC:\\src\\b.txt\n\
\t*Arquivo EXTRA \t\t         100\tD:\\dst\\old.txt\n\
2026/11/17 21:00:00 ERRO 5 (0x00000005) Copiando arquivo C:\\src\\locked.txt\n\
Acesso negado.\n\
               Total   Copiado   Ignorado  Incompatível    FALHA    Extras\n\
 Diretórios :         2         1         1         0         0         1\n\
  Arquivos :         4         2         1         0         1         1\n\
     Bytes :   1048788   1048588       100         0       100       100\n\
    Tempos :   0:00:00   0:00:00                       0:00:00   0:00:00\n";

    #[test]
    fn same_result_in_both_languages() {
        for text in [EN, PT] {
            let mut s = LogState::new(Path::new(r"D:\dst"));
            feed(&mut s, text);
            assert_eq!(s.files_done, 2, "{text}");
            assert_eq!(s.bytes_done, 1_048_588);
            assert_eq!(s.extra_files, 1);
            assert_eq!(s.errors_sample.len(), 1);
            assert_eq!(s.errors_sample[0].os_code, 5);
            assert_eq!(s.errors_sample[0].path, r"C:\src\locked.txt");
            assert_eq!(s.errors_sample[0].kind, nexttool_core::ErrorKind::AccessDenied);
            let st = s.summary_stats().expect("summary parsed");
            assert_eq!((st.files_total, st.files_copied, st.files_failed, st.files_extra), (4, 2, 1, 1));
            assert_eq!(st.bytes_copied, 1_048_588);
            assert_eq!(st.dirs_total, 2);
        }
        let mut s = LogState::new(Path::new(r"D:\dst"));
        feed(&mut s, EN);
        assert_eq!(s.extra_dirs, 1);
        assert_eq!(s.delete_sample, vec![r"D:\dst\old.txt".to_string(), r"D:\dst\olddir\".to_string()]);
    }

    #[test]
    fn utf16_decoder_handles_split_reads() {
        let text = "\u{FEFF}linha ação 😀\r\nsegunda\r\n";
        let bytes: Vec<u8> = text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        let mut d = Utf16Lines::default();
        let mut out = Vec::new();
        for chunk in bytes.chunks(3) {
            d.feed(chunk, &mut out);
        }
        d.finish(&mut out);
        assert_eq!(out, vec!["linha ação 😀".to_string(), "segunda".to_string()]);
    }

    #[test]
    fn unc_paths() {
        assert_eq!(
            parse_line("\t    New File  \t\t   5\t\\\\nas\\share\\x.txt"),
            Line::File { size: 5, path: r"\\nas\share\x.txt".into() }
        );
        assert_eq!(parse_line("   Times :   0:00:01   0:00:00   0:00:00   0:00:00"), Line::Other);
        assert_eq!(parse_line("   Speed :   1915756 Bytes/sec."), Line::Other);
    }
}
