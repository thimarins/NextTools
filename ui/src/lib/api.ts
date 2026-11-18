import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Alert,
  AppInfo,
  CompareLevel,
  CopyRequest,
  OwnerInfo,
  Recurrence,
  ScheduleView,
  SnapshotEntry,
  DirInfo,
  DirRow,
  DryRunReport,
  FileQuery,
  FileRow,
  Overview,
  Paged,
  Stats,
  Summary,
  TaskDetail,
  TaskRecord,
} from "./types";

/** True inside the desktop app; false in a plain browser (dev preview). */
export const isDesktop = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export class ApiError extends Error {
  code: string;
  constructor(code: string, message: string) {
    super(message);
    this.code = code;
  }
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!isDesktop) {
    throw new ApiError("desktop_only", "Disponível apenas no aplicativo desktop NextTool.");
  }
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    if (e && typeof e === "object" && "code" in e && "message" in e) {
      const err = e as { code: string; message: string };
      throw new ApiError(err.code, err.message);
    }
    throw new ApiError("internal", String(e));
  }
}

let seq = 0;
/** Unique id per request; responses carry it back so stale ones are dropped. */
export const nextRequestId = (view: string) => `${view}-${++seq}`;

export const api = {
  appInfo: () => call<AppInfo>("app_info"),
  appQuit: () => call<void>("app_quit"),
  overview: () => call<Overview>("overview"),
  tasksList: (states?: string[], limit = 200, offset = 0) => call<TaskRecord[]>("tasks_list", { states, limit, offset }),
  taskGet: (id: number) => call<TaskDetail>("task_get", { id }),
  taskPause: (id: number) => call<boolean>("task_pause", { id }),
  taskResume: (id: number) => call<void>("task_resume", { id }),
  taskCancel: (id: number) => call<boolean>("task_cancel", { id }),
  sizeScanSubmit: (root: string, profile: string, snapshot?: string) =>
    call<number>("size_scan_submit", { req: { root, profile, snapshot: snapshot || null } }),
  snapshotSummary: (path: string) => call<Summary>("snapshot_summary", { path }),
  snapshotListDir: (path: string, dirId: number, offset: number, limit: number, requestId: string) =>
    call<Paged<DirRow>>("snapshot_list_dir", { path, dirId, offset, limit, requestId }),
  snapshotListSubdirs: (path: string, dirId: number, offset: number, limit: number, requestId: string) =>
    call<Paged<DirRow>>("snapshot_list_subdirs", { path, dirId, offset, limit, requestId }),
  snapshotDirInfo: (path: string, dirId: number) => call<DirInfo>("snapshot_dir_info", { path, dirId }),
  snapshotQuery: (path: string, query: FileQuery, requestId: string) => call<Paged<FileRow>>("snapshot_query", { path, query, requestId }),
  snapshotCancelQuery: (requestId: string) => call<boolean>("snapshot_cancel_query", { requestId }),
  snapshotsList: () => call<SnapshotEntry[]>("snapshots_list"),
  ownersLookup: (paths: string[]) => call<OwnerInfo[]>("owners_lookup", { paths }),
  schedulesList: () => call<ScheduleView[]>("schedules_list"),
  scheduleCreate: (title: string, kind: "scan" | "copy", request: unknown, recurrence: Recurrence) => call<number>("schedule_create", { title, kind, request, recurrence }),
  scheduleSetEnabled: (id: number, enabled: boolean) => call<boolean>("schedule_set_enabled", { id, enabled }),
  scheduleDelete: (id: number) => call<boolean>("schedule_delete", { id }),
  snapshotSetLabel: (path: string, label: string) => call<void>("snapshot_set_label", { path, label }),
  snapshotDelete: (path: string, confirmName: string) => call<void>("snapshot_delete", { path, confirmName }),
  snapshotImport: (src: string) => call<string>("snapshot_import", { src }),
  snapshotExport: (path: string, dest: string) => call<void>("snapshot_export", { path, dest }),
  snapshotBuildIndexes: (path: string) => call<number>("snapshot_build_indexes", { path }),
  snapshotCompare: (a: string, b: string, rel: string[], limit = 500) => call<CompareLevel>("snapshot_compare", { a, b, rel, limit }),
  snapshotTopFiles: (path: string, limit = 50) => call<FileRow[]>("snapshot_top_files", { path, limit }),
  snapshotTopDirs: (path: string, limit = 50) => call<DirRow[]>("snapshot_top_dirs", { path, limit }),
  snapshotStats: (path: string) => call<Stats>("snapshot_stats", { path }),
  copyPlan: (req: CopyRequest) => call<DryRunReport>("copy_plan", { req }),
  copySubmit: (req: CopyRequest) => call<number>("copy_submit", { req }),
  alertsList: (includeAcknowledged = false, limit = 200) => call<Alert[]>("alerts_list", { includeAcknowledged, limit }),
  alertsAck: (id: number) => call<boolean>("alerts_ack", { id }),
  settingsGet: () => call<Record<string, string>>("settings_get"),
  settingsSet: (key: string, value: string) => call<void>("settings_set", { key, value }),
};

/** Subscribes to a backend event; no-op outside the desktop app. */
export async function on<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  if (!isDesktop) return () => {};
  return listen<T>(event, (e) => cb(e.payload));
}

/** Native "save as" dialog (desktop only). */
export async function pickSavePath(defaultPath: string, filters: { name: string; extensions: string[] }[]): Promise<string | null> {
  if (!isDesktop) return null;
  const { save } = await import("@tauri-apps/plugin-dialog");
  return (await save({ defaultPath, filters })) ?? null;
}

/** One-shot hand-off from NextSize to NextCopy (a folder path, never a file list). */
let copyPrefill: { source: string; note: string } | null = null;
export const setCopyPrefill = (p: { source: string; note: string } | null) => {
  copyPrefill = p;
};
export const takeCopyPrefill = () => {
  const p = copyPrefill;
  copyPrefill = null;
  return p;
};

/** Native folder/file picker (desktop only). */
export async function pickPath(opts: { directory?: boolean; title?: string; filters?: { name: string; extensions: string[] }[] }): Promise<string | null> {
  if (!isDesktop) return null;
  const { open } = await import("@tauri-apps/plugin-dialog");
  const r = await open({ directory: opts.directory ?? true, multiple: false, title: opts.title, filters: opts.filters });
  return typeof r === "string" ? r : null;
}
