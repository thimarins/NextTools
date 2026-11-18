import { createContext, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { toast } from "sonner";
import { api, isDesktop, on } from "./api";
import type { Alert, TaskRecord } from "./types";

/** Live state pushed by the backend (task://update, alert://raised). */
type Live = {
  /** Latest record of every task seen through events, by id. */
  tasks: Map<number, TaskRecord>;
  /** Bumped on every task event so views can refetch lists. */
  version: number;
  openAlerts: number;
  refreshAlerts: () => void;
};

const LiveContext = createContext<Live | null>(null);

export function LiveProvider({ children }: { children: ReactNode }) {
  const [tasks, setTasks] = useState<Map<number, TaskRecord>>(new Map());
  const [version, setVersion] = useState(0);
  const [openAlerts, setOpenAlerts] = useState(0);
  const lastToast = useRef<Map<number, number>>(new Map());

  const refreshAlerts = () => {
    if (!isDesktop) return;
    api
      .alertsList(false, 1000)
      .then((a) => setOpenAlerts(a.length))
      .catch(() => {});
  };

  useEffect(() => {
    refreshAlerts();
    const subs = [
      on<TaskRecord>("task://update", (t) => {
        setTasks((m) => new Map(m).set(t.id, t));
        setVersion((v) => v + 1);
      }),
      on<Alert>("alert://raised", (a) => {
        refreshAlerts();
        // Grouped alerts update silently; a toast only for the first
        // occurrence (or a severity increase), to avoid repetitive notifications.
        const prev = lastToast.current.get(a.id);
        if (prev !== undefined && a.count > 1) return;
        lastToast.current.set(a.id, a.count);
        const fn = a.draft.severity === "critical" || a.draft.severity === "error" ? toast.error : a.draft.severity === "warning" ? toast.warning : toast.info;
        fn(a.draft.title, { description: a.draft.problem });
      }),
    ];
    return () => {
      subs.forEach((p) => p.then((u) => u()));
    };
  }, []);

  const value = useMemo(() => ({ tasks, version, openAlerts, refreshAlerts }), [tasks, version, openAlerts]);
  return <LiveContext.Provider value={value}>{children}</LiveContext.Provider>;
}

export function useLive() {
  const c = useContext(LiveContext);
  if (!c) throw new Error("useLive outside LiveProvider");
  return c;
}

/** Merges a fetched task with the most recent event for it. */
export function useLatest(t: TaskRecord): TaskRecord {
  return useMaybeLatest(t) as TaskRecord;
}

export function useMaybeLatest(t: TaskRecord | null): TaskRecord | null {
  const { tasks } = useLive();
  if (!t) return null;
  const l = tasks.get(t.id);
  return l && l.updated_at >= t.updated_at ? l : t;
}
