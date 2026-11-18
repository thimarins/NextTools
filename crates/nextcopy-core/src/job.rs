use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nexttool_core::security::{check_no_overlap, validate_process_arg_path, PathError};
use serde::{Deserialize, Serialize};

use crate::profile::CopyProfile;

/// Max individually selected files per task (v1). Each distinct parent
/// directory becomes one sequential engine run; see ADR-012.
pub const MAX_INDIVIDUAL_FILES: usize = 500;
/// Max distinct parent directories for individual selection (= engine runs).
pub const MAX_FILE_GROUPS: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum OverwritePolicy {
    /// Copy new files and files that differ (size/timestamp) — Robocopy default.
    #[default]
    ReplaceChanged,
    /// Copy only new files and files newer in the source (never replace a newer destination file).
    ReplaceOlderOnly,
    /// Never replace any existing destination file.
    NeverReplace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum VerifyLevel {
    /// Engine result, error counts and statistics only.
    #[default]
    Basic,
    /// Existence + size + modification time of every file in the destination.
    Standard,
    /// Standard + full content hash (BLAKE3) of source and destination.
    Strict,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyFlags {
    pub data: bool,
    pub attributes: bool,
    pub timestamps: bool,
    /// NTFS ACLs (requires appropriate rights on the destination).
    pub security: bool,
    /// Owner (usually requires administrator / backup privilege).
    pub owner: bool,
}

impl Default for CopyFlags {
    fn default() -> Self {
        CopyFlags { data: true, attributes: true, timestamps: true, security: false, owner: false }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Filters {
    /// Wildcard patterns of files to include (empty = all).
    pub include_files: Vec<String>,
    pub exclude_files: Vec<String>,
    pub exclude_dirs: Vec<String>,
    pub min_size: Option<u64>,
    pub max_size: Option<u64>,
    /// Skip files older than N days (by modification time).
    pub max_age_days: Option<u32>,
    /// Skip files newer than N days.
    pub min_age_days: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyOptions {
    pub threads: u32,
    pub retries: u32,
    pub retry_wait_secs: u32,
    /// Restartable mode: interrupted large files resume mid-file (slower).
    pub restartable: bool,
    /// Unbuffered I/O, recommended for very large files.
    pub unbuffered: bool,
    pub overwrite: OverwritePolicy,
    pub flags: CopyFlags,
    pub copy_dir_timestamps: bool,
    pub include_empty_dirs: bool,
    /// Mirror: destination becomes identical to source, DELETING extra files.
    /// Requires a dry-run approval.
    pub mirror: bool,
    /// Never descend into junctions / symlinked dirs (default true).
    pub exclude_junctions: bool,
    pub filters: Filters,
    pub verify: VerifyLevel,
    /// Write one log line per copied file (detailed progress). Off for very
    /// large migrations to keep logs bounded; progress then uses I/O counters.
    pub list_files_in_log: bool,
}

impl Default for CopyOptions {
    fn default() -> Self {
        CopyProfile::Safe.options()
    }
}

/// One copy task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CopyJob {
    pub source: PathBuf,
    pub dest: PathBuf,
    pub profile: CopyProfile,
    pub options: CopyOptions,
    /// Optional individual file selection (relative to `source`). When empty,
    /// the whole directory (with filters) is copied.
    pub files: Vec<PathBuf>,
    /// Required when `options.mirror`: number of deletions the user approved
    /// after seeing a dry-run. The engine re-checks before running.
    pub approved_deletions: Option<u64>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum JobError {
    #[error("{0}")]
    Path(#[from] PathError),
    #[error("número de threads fora do intervalo 1..=128: {0}")]
    Threads(u32),
    #[error("tentativas fora do intervalo 0..=1000: {0}")]
    Retries(u32),
    #[error("seleção individual excede o limite de {MAX_INDIVIDUAL_FILES} arquivos ({0})")]
    TooManyFiles(usize),
    #[error("seleção individual abrange {0} diretórios (limite {MAX_FILE_GROUPS})")]
    TooManyGroups(usize),
    #[error("arquivo selecionado inválido (precisa ser relativo à origem, sem '..'): {0}")]
    BadSelection(String),
    #[error("espelhamento não pode ser combinado com seleção individual de arquivos")]
    MirrorWithSelection,
    #[error("padrão de filtro inválido: {0}")]
    BadPattern(String),
}

impl CopyJob {
    pub fn new(source: PathBuf, dest: PathBuf, profile: CopyProfile) -> Self {
        CopyJob { source, dest, profile, options: profile.options(), files: Vec::new(), approved_deletions: None }
    }

    /// Static validation (no I/O). Run before persisting and before running.
    pub fn validate(&self) -> Result<(), JobError> {
        validate_process_arg_path(&self.source)?;
        validate_process_arg_path(&self.dest)?;
        check_no_overlap(&self.source, &self.dest)?;
        let o = &self.options;
        if !(1..=128).contains(&o.threads) {
            return Err(JobError::Threads(o.threads));
        }
        if o.retries > 1000 {
            return Err(JobError::Retries(o.retries));
        }
        for p in o.filters.include_files.iter().chain(&o.filters.exclude_files).chain(&o.filters.exclude_dirs) {
            if p.is_empty() || p.chars().any(|c| c.is_control() || c == '"' || c == '|' || c == '<' || c == '>') || p.starts_with('/') {
                return Err(JobError::BadPattern(p.clone()));
            }
        }
        if !self.files.is_empty() {
            if o.mirror {
                return Err(JobError::MirrorWithSelection);
            }
            if self.files.len() > MAX_INDIVIDUAL_FILES {
                return Err(JobError::TooManyFiles(self.files.len()));
            }
            for f in &self.files {
                let s = f.to_string_lossy();
                if f.is_absolute()
                    || f.components().any(|c| !matches!(c, std::path::Component::Normal(_)))
                    || s.contains(['"', '*', '?', '|', '<', '>'])
                {
                    return Err(JobError::BadSelection(s.into_owned()));
                }
            }
            let groups = self.file_groups().len();
            if groups > MAX_FILE_GROUPS {
                return Err(JobError::TooManyGroups(groups));
            }
        }
        Ok(())
    }

    /// Individual selection grouped by parent directory (relative), so each
    /// group is one engine run: (relative dir, file names).
    pub fn file_groups(&self) -> BTreeMap<PathBuf, Vec<String>> {
        let mut m: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
        for f in &self.files {
            let parent = f.parent().map(Path::to_path_buf).unwrap_or_default();
            if let Some(name) = f.file_name() {
                m.entry(parent).or_default().push(name.to_string_lossy().into_owned());
            }
        }
        m
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    fn job() -> CopyJob {
        CopyJob::new(PathBuf::from(r"C:\src"), PathBuf::from(r"D:\dst"), CopyProfile::Safe)
    }

    #[test]
    fn validates() {
        assert_eq!(job().validate(), Ok(()));
        let mut j = job();
        j.dest = PathBuf::from(r"C:\src\inner");
        assert!(matches!(j.validate(), Err(JobError::Path(PathError::Overlap(_)))));
        let mut j = job();
        j.options.threads = 0;
        assert_eq!(j.validate(), Err(JobError::Threads(0)));
        let mut j = job();
        j.files = vec![PathBuf::from(r"..\evil.txt")];
        assert!(matches!(j.validate(), Err(JobError::BadSelection(_))));
        let mut j = job();
        j.options.filters.exclude_files = vec!["/PURGE".into()];
        assert!(matches!(j.validate(), Err(JobError::BadPattern(_))));
        let mut j = job();
        j.files = (0..501).map(|i| PathBuf::from(format!("{i}.txt"))).collect();
        assert_eq!(j.validate(), Err(JobError::TooManyFiles(501)));
    }

    #[test]
    fn groups_by_parent() {
        let mut j = job();
        j.files = vec![PathBuf::from(r"a\1.txt"), PathBuf::from(r"a\2.txt"), PathBuf::from("root.txt")];
        let g = j.file_groups();
        assert_eq!(g.len(), 2);
        assert_eq!(g[&PathBuf::from("a")], vec!["1.txt", "2.txt"]);
    }
}
