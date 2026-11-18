use serde::{Deserialize, Serialize};

use crate::job::{CopyFlags, CopyOptions, Filters, OverwritePolicy, VerifyLevel};

/// Predefined, documented option sets. Values are conservative starting
/// points; the diagnostics module may *recommend* changes but never applies
/// critical ones silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CopyProfile {
    /// Local disks: more threads, basic verification.
    Quick,
    /// Default: moderate threads, retries, standard verification.
    #[default]
    Safe,
    /// Server migration: ACLs + owner, restartable, standard verification.
    ServerMigration,
    /// Incremental backup: only new/changed, never deletes.
    Incremental,
    /// One-way sync without deletion: replace only older destination files.
    Sync,
    /// Mirror: deletes extras in destination. Requires dry-run + approval.
    Mirror,
    Custom,
}

impl CopyProfile {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "quick" | "rapida" => CopyProfile::Quick,
            "safe" | "segura" => CopyProfile::Safe,
            "migration" | "server_migration" | "migracao" => CopyProfile::ServerMigration,
            "incremental" => CopyProfile::Incremental,
            "sync" | "sincronizacao" => CopyProfile::Sync,
            "mirror" | "espelhamento" => CopyProfile::Mirror,
            "custom" | "personalizado" => CopyProfile::Custom,
            _ => return None,
        })
    }

    pub fn options(self) -> CopyOptions {
        let base = CopyOptions {
            threads: 8,
            retries: 3,
            retry_wait_secs: 10,
            restartable: false,
            unbuffered: false,
            overwrite: OverwritePolicy::ReplaceChanged,
            flags: CopyFlags::default(),
            copy_dir_timestamps: true,
            include_empty_dirs: true,
            mirror: false,
            exclude_junctions: true,
            filters: Filters::default(),
            verify: VerifyLevel::Standard,
            list_files_in_log: true,
        };
        match self {
            CopyProfile::Quick => CopyOptions { threads: 16, retries: 1, retry_wait_secs: 5, verify: VerifyLevel::Basic, ..base },
            CopyProfile::Safe | CopyProfile::Custom => base,
            CopyProfile::ServerMigration => CopyOptions {
                threads: 16,
                retries: 5,
                retry_wait_secs: 15,
                restartable: true,
                flags: CopyFlags { security: true, owner: true, ..CopyFlags::default() },
                list_files_in_log: false,
                ..base
            },
            CopyProfile::Incremental => CopyOptions { overwrite: OverwritePolicy::ReplaceChanged, ..base },
            CopyProfile::Sync => CopyOptions { overwrite: OverwritePolicy::ReplaceOlderOnly, ..base },
            CopyProfile::Mirror => CopyOptions { mirror: true, ..base },
        }
    }
}
