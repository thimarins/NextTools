// Mirrors the Rust types serialized by nexttool-desktop (serde). Keep in sync.

export type TaskState =
  | "queued"
  | "running"
  | "paused"
  | "waiting_retry"
  | "cancelling"
  | "interrupted"
  | "completed"
  | "completed_with_warnings"
  | "failed"
  | "cancelled";

export type TaskKind = "size_scan" | "copy" | "verify";
export type Module = "next_size" | "next_copy" | "system";
export type Severity = "info" | "warning" | "error" | "critical";

export interface TaskRecord {
  id: number;
  kind: TaskKind;
  module: Module;
  title: string;
  spec_json: string;
  state: TaskState;
  priority: "low" | "normal" | "high";
  resource_keys: string[];
  attempt: number;
  max_attempts: number;
  created_at: number;
  started_at: number | null;
  finished_at: number | null;
  updated_at: number;
  next_retry_at: number | null;
  progress_json: string | null;
  result_json: string | null;
  checkpoint_json: string | null;
  error_kind: string | null;
  error_message: string | null;
  child_pid: number | null;
}

export interface TaskDetail {
  task: TaskRecord;
  events: [number, string, string][];
}

export interface AlertDraft {
  module: Module;
  task_id: number | null;
  severity: Severity;
  fingerprint: string;
  title: string;
  problem: string;
  probable_cause: string | null;
  consequence: string;
  recommended_action: string;
  recoverable: boolean;
  details: string | null;
}

export interface Alert {
  id: number;
  draft: AlertDraft;
  first_at: number;
  last_at: number;
  count: number;
  acknowledged: boolean;
}

export interface Overview {
  active: TaskRecord[];
  failed_recent: TaskRecord[];
  recent_scans: TaskRecord[];
  recent_copies: TaskRecord[];
  open_alerts: Alert[];
  open_alert_count: number;
}

export interface AppInfo {
  version: string;
  data_dir: string;
  portable: boolean;
  platform: string;
  robocopy: string | null;
}

export interface Summary {
  root: string;
  state: string;
  complete: boolean;
  finalized: boolean;
  filter_indexes_ready: boolean;
  label: string | null;
  usable_ms: number | null;
  started_at: number | null;
  finished_at: number | null;
  files: number;
  dirs: number;
  bytes: number;
  alloc_bytes: number | null;
  issues: number;
  enumerator: string;
  source_kind: string;
  profile: string;
  resumed_count: number;
  hardlink_policy: string;
  scan_ms: number | null;
  finalize_ms: number | null;
}

export interface FileRow {
  id: number;
  parent_id: number;
  path: string;
  size: number;
  mtime: number | null;
  flags: number;
}

export interface DirRow {
  id: number;
  name: string;
  path: string | null;
  is_dir: boolean;
  size: number;
  files: number;
  dirs: number;
  mtime: number | null;
  flags: number;
}

export interface Paged<T> {
  request_id: string;
  rows: T[];
  elapsed_ms: number;
}

export interface SnapshotEntry {
  path: string;
  file_name: string;
  size_bytes: number;
  modified: number;
  root: string | null;
  state: string | null;
  files: number | null;
  bytes: number | null;
  started_at: number | null;
  label: string | null;
  invalid: string | null;
}

export interface CompareRow {
  name: string;
  status: "added" | "removed" | "grew" | "shrank" | "same" | "files";
  a_size: number | null;
  b_size: number | null;
  a_files: number | null;
  b_files: number | null;
  delta: number;
  pct: number | null;
  has_children: boolean;
}

export interface CompareLevel {
  path: string[];
  a_found: boolean;
  b_found: boolean;
  a_total: number | null;
  b_total: number | null;
  rows: CompareRow[];
  truncated: boolean;
}

export interface DirInfo {
  chain: [number, string][];
  subdirs: number;
  files: number;
  size: number;
}

export interface FileQuery {
  min_size?: number | null;
  max_size?: number | null;
  ext?: string | null;
  modified_before?: number | null;
  modified_after?: number | null;
  year?: number | null;
  name_contains?: string | null;
  offset: number;
  limit: number;
}

export interface Stats {
  extensions: { ext: string; files: number; size: number }[];
  years: { year: number | null; files: number; size: number }[];
}

export interface DryRunReport {
  would_copy_files: number;
  would_copy_bytes: number;
  would_delete_files: number;
  would_delete_dirs: number;
  delete_sample: string[];
  errors: number;
  exit_code: number | null;
  log_file: string;
}

export interface CopyRequest {
  source: string;
  dest: string;
  profile: string;
  threads?: number | null;
  verify?: string | null;
  exclude_files?: string[];
  exclude_dirs?: string[];
  approved_deletions?: number | null;
}

/** NextSize progress (ProgressSnapshot) */
export interface ScanProgress {
  state: string;
  files: number;
  dirs: number;
  bytes: number;
  issues: number;
  pending_dirs: number;
  spilled_dirs: number;
  rows_committed: number;
  concurrency?: number;
}

/** NextCopy progress */
export interface CopyProgress {
  phase: "copy" | "verify";
  files_done?: number;
  bytes_done?: number;
  errors?: number;
  io_write?: number;
  rate_mb_s?: number;
  activity?: "active" | "slow" | "no_observable_progress" | "exited";
}

export type Settings = {
  theme: "light" | "dark" | "system";
  accent: number;
  density: "comfortable" | "compact";
  font_scale: 90 | 100 | 110 | 120;
  units: "binary" | "decimal";
  language: "pt-BR";
  max_running: number;
};
