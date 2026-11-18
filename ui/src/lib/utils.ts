import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";
import type { CopyProgress, ScanProgress, TaskRecord } from "./types";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

let unitBase: 1024 | 1000 = 1024;
export const setUnits = (u: "binary" | "decimal") => {
  unitBase = u === "decimal" ? 1000 : 1024;
};

export function formatBytes(b: number | null | undefined): string {
  if (b == null) return "—";
  const units = unitBase === 1024 ? ["B", "KiB", "MiB", "GiB", "TiB", "PiB"] : ["B", "KB", "MB", "GB", "TB", "PB"];
  let v = b;
  let i = 0;
  while (v >= unitBase && i < units.length - 1) {
    v /= unitBase;
    i++;
  }
  return i === 0 ? `${v} B` : `${v.toLocaleString("pt-BR", { maximumFractionDigits: v < 10 ? 2 : 1 })} ${units[i]}`;
}

export const formatCount = (n: number | null | undefined) => (n == null ? "—" : n.toLocaleString("pt-BR"));

export function formatDate(unix: number | null | undefined): string {
  if (!unix) return "—";
  return new Date(unix * 1000).toLocaleString("pt-BR", { dateStyle: "short", timeStyle: "short" });
}

export function relTime(unix: number | null | undefined): string {
  if (!unix) return "—";
  const d = Date.now() / 1000 - unix;
  if (d < 60) return "agora";
  if (d < 3600) return `há ${Math.floor(d / 60)} min`;
  if (d < 86400) return `há ${Math.floor(d / 3600)} h`;
  return formatDate(unix);
}

export function formatDuration(ms: number | null | undefined): string {
  if (ms == null) return "—";
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} min ${s % 60} s`;
  return `${Math.floor(m / 60)} h ${m % 60} min`;
}

export function parseJson<T>(s: string | null | undefined): T | null {
  if (!s) return null;
  try {
    return JSON.parse(s) as T;
  } catch {
    return null;
  }
}

export const scanProgress = (t: TaskRecord) => (t.kind === "size_scan" ? parseJson<ScanProgress>(t.progress_json) : null);
export const copyProgress = (t: TaskRecord) => (t.kind === "copy" ? parseJson<CopyProgress>(t.progress_json) : null);

/** Snapshot path recorded by a NextSize task. */
export const snapshotOf = (t: TaskRecord) => parseJson<{ snapshot: string }>(t.checkpoint_json)?.snapshot ?? null;

export const isActive = (s: TaskRecord["state"]) => s === "running" || s === "queued" || s === "cancelling" || s === "waiting_retry";
export const isResumable = (s: TaskRecord["state"]) => s === "paused" || s === "interrupted" || s === "failed";
