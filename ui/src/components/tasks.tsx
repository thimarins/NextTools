import { useState, type ReactElement, type ReactNode } from "react";
import { toast } from "sonner";
import {
  AlertTriangle,
  Ban,
  CheckCircle2,
  ChevronDown,
  CircleDashed,
  Clock,
  Copy as CopyIcon,
  HardDrive,
  Loader2,
  Pause,
  Play,
  RotateCcw,
  Square,
  XCircle,
} from "lucide-react";
import { api, ApiError } from "@/lib/api";
import { plainError, taskKindLabel, taskStateLabel } from "@/lib/i18n";
import { useLatest, useMaybeLatest } from "@/lib/live";
import { useAsync } from "@/lib/hooks";
import type { TaskRecord, TaskState } from "@/lib/types";
import { copyProgress, formatBytes, formatCount, formatDate, isActive, isResumable, parseJson, relTime, scanProgress } from "@/lib/utils";
import { Button } from "./ui/button";
import { Badge, Progress } from "./ui/primitives";
import { Dialog, DialogContent, Tooltip } from "./ui/overlay";

const stateTone: Record<TaskState, "neutral" | "primary" | "success" | "warning" | "danger" | "info"> = {
  queued: "neutral",
  running: "primary",
  paused: "warning",
  waiting_retry: "warning",
  cancelling: "neutral",
  interrupted: "warning",
  completed: "success",
  completed_with_warnings: "warning",
  failed: "danger",
  cancelled: "neutral",
};

const stateIcon: Record<TaskState, ReactElement> = {
  queued: <Clock />,
  running: <Loader2 className="animate-spin" />,
  paused: <Pause />,
  waiting_retry: <RotateCcw />,
  cancelling: <Loader2 className="animate-spin" />,
  interrupted: <AlertTriangle />,
  completed: <CheckCircle2 />,
  completed_with_warnings: <AlertTriangle />,
  failed: <XCircle />,
  cancelled: <Ban />,
};

export function TaskStateBadge({ state }: { state: TaskState }) {
  return (
    <Badge tone={stateTone[state]}>
      {stateIcon[state]}
      {taskStateLabel[state]}
    </Badge>
  );
}

export function KindIcon({ t }: { t: TaskRecord }) {
  return t.kind === "size_scan" ? <HardDrive className="size-4 text-muted-foreground" /> : <CopyIcon className="size-4 text-muted-foreground" />;
}

/** One-line live progress. No percentage: totals are unknown while running. */
export function TaskProgressText({ t }: { t: TaskRecord }) {
  const sp = scanProgress(t);
  if (sp) {
    return (
      <span>
        {formatCount(sp.files)} arquivos · {formatCount(sp.dirs)} pastas · {formatBytes(sp.bytes)}
        {sp.issues > 0 && <span className="text-warning"> · {sp.issues} ocorrências</span>}
      </span>
    );
  }
  const cp = copyProgress(t);
  if (cp) {
    if (cp.phase === "verify") return <span>Verificando destino…</span>;
    return (
      <span>
        {formatCount(cp.files_done ?? 0)} arquivos · {formatBytes(cp.io_write ?? 0)} gravados · {(cp.rate_mb_s ?? 0).toLocaleString("pt-BR", { maximumFractionDigits: 1 })} MB/s
        {(cp.errors ?? 0) > 0 && <span className="text-warning"> · {cp.errors} erros</span>}
        {cp.activity === "slow" && <span className="text-warning"> · abaixo do esperado</span>}
        {cp.activity === "no_observable_progress" && <span className="text-warning"> · sem progresso observável</span>}
      </span>
    );
  }
  return null;
}

export function TaskActions({ t, onChanged }: { t: TaskRecord; onChanged?: () => void }) {
  const [busy, setBusy] = useState(false);
  const act = async (fn: () => Promise<unknown>, ok: string) => {
    setBusy(true);
    try {
      await fn();
      toast.success(ok);
      onChanged?.();
    } catch (e) {
      toast.error("Não foi possível concluir a ação", { description: e instanceof ApiError ? e.message : String(e) });
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="flex items-center gap-1">
      {t.state === "running" && (
        <Tooltip content="Parar com segurança. A tarefa poderá ser retomada.">
          <Button size="icon" variant="ghost" disabled={busy} onClick={() => act(() => api.taskPause(t.id), "Pausa solicitada")} aria-label="Pausar">
            <Pause />
          </Button>
        </Tooltip>
      )}
      {isResumable(t.state) && (
        <Tooltip content="Retomar (recoloca na fila)">
          <Button size="icon" variant="ghost" disabled={busy} onClick={() => act(() => api.taskResume(t.id), "Tarefa recolocada na fila")} aria-label="Retomar">
            <Play />
          </Button>
        </Tooltip>
      )}
      {(isActive(t.state) || t.state === "paused" || t.state === "interrupted") && t.state !== "cancelling" && (
        <Tooltip content="Cancelar">
          <Button size="icon" variant="ghost" disabled={busy} onClick={() => act(() => api.taskCancel(t.id), "Cancelamento solicitado")} aria-label="Cancelar">
            <Square />
          </Button>
        </Tooltip>
      )}
    </div>
  );
}

export function TaskRow({ task, onOpen, onChanged, note }: { task: TaskRecord; onOpen?: (t: TaskRecord) => void; onChanged?: () => void; note?: string }) {
  const t = useLatest(task);
  const running = t.state === "running" || t.state === "cancelling";
  const showErr = t.error_message && (t.state === "failed" || t.state === "waiting_retry" || t.state === "interrupted");
  return (
    <div className="group flex items-center gap-3 border-b px-[var(--card-pad)] py-2.5 last:border-b-0 hover:bg-muted/40">
      <KindIcon t={t} />
      <button className="min-w-0 flex-1 cursor-default text-left" onClick={() => onOpen?.(t)}>
        <div className="flex items-center gap-2">
          <span className="truncate text-sm font-medium">{t.title}</span>
        </div>
        <div className="mt-0.5 truncate text-xs text-muted-foreground" title={t.error_message ?? undefined}>
          {running ? (
            <TaskProgressText t={t} />
          ) : showErr ? (
            <span className={t.state === "failed" ? "text-destructive" : "text-warning"}>
              {plainError(t.error_kind).title}
              {t.state === "waiting_retry" && t.next_retry_at ? ` — nova tentativa ${relTime(t.next_retry_at).replace("há ", "em ")}` : ""}
            </span>
          ) : note ? (
            note
          ) : (
            relTime(t.finished_at ?? t.updated_at)
          )}
        </div>
        {running && <Progress className="mt-1.5" />}
      </button>
      <TaskStateBadge state={t.state} />
      <TaskActions t={t} onChanged={onChanged} />
    </div>
  );
}

function KV({ k, v }: { k: string; v: ReactNode }) {
  return (
    <div className="grid grid-cols-[10rem_1fr] gap-3 py-1 text-sm">
      <dt className="text-muted-foreground">{k}</dt>
      <dd className="min-w-0 break-words">{v}</dd>
    </div>
  );
}

export function TaskDetailDialog({ task, onClose }: { task: TaskRecord | null; onClose: () => void }) {
  const live = useMaybeLatest(task);
  const { data } = useAsync(() => (task ? api.taskGet(task.id) : Promise.resolve(null)), [task?.id, live?.state, live?.updated_at]);
  const t = data?.task ?? live;
  const result = parseJson<Record<string, any>>(t?.result_json);
  return (
    <Dialog open={!!task} onOpenChange={(o) => !o && onClose()}>
      {t && (
        <DialogContent wide title={t.title} description={`${taskKindLabel[t.kind]} #${t.id}`}>
          <div className="mb-4 flex items-center justify-between">
            <TaskStateBadge state={t.state} />
            <TaskActions t={t} />
          </div>
          {(t.state === "running" || t.state === "cancelling") && (
            <div className="mb-4 rounded-lg border bg-muted/40 p-3 text-sm">
              <TaskProgressText t={t} />
              <Progress className="mt-2" />
            </div>
          )}
          {t.error_message && <PlainError t={t} result={result} />}
          <dl className="mb-4">
            <KV k="Criada" v={formatDate(t.created_at)} />
            <KV k="Iniciada" v={formatDate(t.started_at)} />
            <KV k="Finalizada" v={t.finished_at ? formatDate(t.finished_at) : "—"} />
            <KV k="Execuções" v={`${t.attempt} (retentativas automáticas até ${t.max_attempts})`} />
            <KV k="Recursos" v={t.resource_keys.join(", ") || "—"} />
            {t.child_pid && <KV k="Processo filho" v={`PID ${t.child_pid}`} />}
          </dl>
          {result && <ResultBlock result={result} />}
          <h4 className="mb-2 text-sm font-semibold">Eventos</h4>
          <div className="max-h-64 overflow-y-auto rounded-lg border font-mono text-xs">
            {(data?.events ?? []).map(([at, level, msg], i) => (
              <div key={i} className="flex gap-3 border-b px-3 py-1.5 last:border-b-0">
                <span className="shrink-0 text-muted-foreground">{formatDate(at)}</span>
                <span className={level === "error" ? "text-destructive" : level === "warn" ? "text-warning" : "text-muted-foreground"}>{level}</span>
                <span className="min-w-0 break-words">{msg}</span>
              </div>
            ))}
            {!data && <div className="flex items-center gap-2 px-3 py-2 text-muted-foreground"><CircleDashed className="size-3.5 animate-spin" />Carregando…</div>}
          </div>
        </DialogContent>
      )}
    </Dialog>
  );
}

/** Error in plain language first; technical details on demand. */
function PlainError({ t, result }: { t: TaskRecord; result: Record<string, any> | null }) {
  const [open, setOpen] = useState(false);
  const p = plainError(t.error_kind);
  const samples: { os_code: number; kind: string; path: string }[] = result?.errors_sample ?? [];
  const failed = t.state === "failed";
  return (
    <div className={`mb-4 rounded-lg border p-3 text-sm ${failed ? "border-destructive/30 bg-destructive/8" : "border-warning/40 bg-warning/10"}`}>
      <p className={`font-semibold ${failed ? "text-destructive" : ""}`}>{p.title}</p>
      <p className="mt-1">
        <span className="text-muted-foreground">O que aconteceu: </span>
        {p.what}
      </p>
      <p className="mt-1">
        <span className="text-muted-foreground">O que fazer: </span>
        {p.todo}
      </p>
      <button className="mt-2 flex cursor-default items-center gap-1 text-xs font-medium text-muted-foreground hover:text-foreground" onClick={() => setOpen((v) => !v)}>
        <ChevronDown className={`size-3.5 transition-transform ${open ? "" : "-rotate-90"}`} /> Detalhes técnicos
      </button>
      {open && (
        <div className="mt-2 grid gap-2 rounded-md bg-card p-2 font-mono text-xs">
          <p>
            classe: {t.error_kind ?? "—"}
            <br />
            mensagem: {t.error_message}
          </p>
          {result?.exit_codes && <p>códigos de saída do Robocopy: {JSON.stringify(result.exit_codes)}</p>}
          {samples.length > 0 && (
            <div>
              <p className="mb-1">erros observados (até 200; podem incluir arquivos que deram certo numa nova tentativa):</p>
              <ul className="max-h-40 overflow-y-auto">
                {samples.map((s, i) => (
                  <li key={i} className="truncate" title={s.path}>
                    0x{s.os_code.toString(16).padStart(8, "0")} {s.kind} — {s.path}
                  </li>
                ))}
              </ul>
            </div>
          )}
          {result && result.log_files?.length > 0 && <p className="break-all">log: {result.log_files.join(", ")}</p>}
        </div>
      )}
    </div>
  );
}

function ResultBlock({ result }: { result: Record<string, any> }) {
  if (result.stats) {
    const s = result.stats;
    const v = result.verify;
    return (
      <div className="mb-4 grid gap-2 rounded-lg border p-3 text-sm">
        <p className="font-medium">Resultado da cópia</p>
        <p>
          {formatCount(s.files_copied)} copiados · {formatCount(s.files_skipped)} ignorados (já idênticos) · <span className={s.files_failed > 0 ? "text-destructive" : ""}>{formatCount(s.files_failed)} falhas</span> · {formatCount(s.files_extra)} extras no destino
        </p>
        <p className="text-muted-foreground">{formatBytes(s.bytes_copied)} copiados · códigos de saída {JSON.stringify(result.exit_codes)}</p>
        {v && (
          <p className={v.missing + v.size_mismatch + v.time_mismatch + v.hash_mismatch + v.errors > 0 ? "text-destructive" : "text-success"}>
            Verificação {v.level}: {formatCount(v.files_ok)}/{formatCount(v.files_checked)} conferidos · ausentes {v.missing} · tamanho {v.size_mismatch} · data {v.time_mismatch} · hash {v.hash_mismatch} · erros {v.errors}
            {v.not_verified > 0 && ` · ${v.not_verified} não verificados (nuvem)`}
          </p>
        )}
      </div>
    );
  }
  if (result.outcome) {
    const o = result.outcome;
    return (
      <div className="mb-4 grid gap-1 rounded-lg border p-3 text-sm">
        <p className="font-medium">Resultado da análise</p>
        <p>
          {formatCount(o.files)} arquivos · {formatCount(o.dirs)} pastas · {formatBytes(o.bytes)} · {o.issues} ocorrências
        </p>
        <p className="text-muted-foreground">
          Varredura {(result.scan_ms / 1000).toFixed(1)} s · finalização {(result.finish_ms / 1000).toFixed(1)} s · {result.threads} threads · {result.enumerator}
        </p>
        {result.fatal_error && <p className="text-destructive">{result.fatal_error}</p>}
      </div>
    );
  }
  return null;
}
