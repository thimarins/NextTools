import { useState, type ReactNode } from "react";
import { Activity, ArrowRight, Bell, CircleCheck, Copy, HardDrive, OctagonX } from "lucide-react";
import { api } from "@/lib/api";
import { useAsync, useThrottledEffect } from "@/lib/hooks";
import { useLive } from "@/lib/live";
import type { TaskRecord } from "@/lib/types";
import { relTime } from "@/lib/utils";
import { severityLabel } from "@/lib/i18n";
import type { Route } from "@/components/shell";
import { TaskDetailDialog, TaskRow } from "@/components/tasks";
import { ErrorNote, Loading } from "@/components/states";
import { Button } from "@/components/ui/button";
import { Badge, Card, CardHeader, Empty, PageHeader } from "@/components/ui/primitives";

const sevTone = { info: "info", warning: "warning", error: "danger", critical: "danger" } as const;

export default function HomePage({ go }: { go: (r: Route) => void }) {
  const { version } = useLive();
  const { data, error, loading, reload } = useAsync(() => api.overview(), []);
  useThrottledEffect(reload, version, 1500);
  const [open, setOpen] = useState<TaskRecord | null>(null);

  const running = data?.active.filter((t) => t.state === "running" || t.state === "cancelling").length ?? 0;
  const waiting = data?.active.filter((t) => t.state === "queued" || t.state === "waiting_retry").length ?? 0;
  const stopped = data?.active.filter((t) => t.state === "paused" || t.state === "interrupted").length ?? 0;

  return (
    <>
      <PageHeader
        title="Início"
        description="Tarefas em andamento, falhas e alertas que precisam de atenção."
        actions={
          <>
            <Button variant="outline" onClick={() => go("nextsize")}>
              <HardDrive /> Nova análise
            </Button>
            <Button onClick={() => go("nextcopy")}>
              <Copy /> Nova cópia
            </Button>
          </>
        }
      />

      <div className="mb-5 grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
        <Kpi icon={<Activity />} label="Em execução" value={data ? running : "—"} sub={waiting ? `${waiting} na fila` : "nenhuma na fila"} />
        <Kpi icon={<OctagonX />} label="Pausadas ou interrompidas" value={data ? stopped : "—"} sub="podem ser retomadas" tone={stopped ? "warning" : undefined} />
        <Kpi icon={<OctagonX />} label="Falhas recentes" value={data ? data.failed_recent.length : "—"} sub="últimas 5" tone={data?.failed_recent.length ? "danger" : undefined} />
        <Kpi icon={<Bell />} label="Alertas abertos" value={data ? data.open_alert_count : "—"} sub="não reconhecidos" tone={data?.open_alert_count ? "danger" : undefined} />
      </div>

      <div className="grid gap-5 [&>*]:min-w-0 lg:grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)]">
        <div className="grid min-w-0 grid-cols-1 content-start gap-5">
          <Card>
            <CardHeader icon={<Activity />} title="Tarefas ativas" description="Execução, fila, pausadas e interrompidas" />
            {loading && !data ? (
              <Loading />
            ) : error ? (
              <ErrorNote error={error} onRetry={reload} />
            ) : data && data.active.length > 0 ? (
              <div className="border-t">{data.active.map((t) => <TaskRow key={t.id} task={t} onOpen={setOpen} onChanged={reload} />)}</div>
            ) : (
              <Empty icon={<CircleCheck />} title="Nenhuma tarefa ativa">
                Inicie uma análise no NextSize ou uma cópia no NextCopy.
              </Empty>
            )}
          </Card>
          {data && data.failed_recent.length > 0 && (
            <Card>
              <CardHeader icon={<OctagonX />} title="Falhas recentes" actions={<Button variant="ghost" size="sm" onClick={() => go("history")}>Histórico <ArrowRight /></Button>} />
              <div className="border-t">{data.failed_recent.map((t) => <TaskRow key={t.id} task={t} onOpen={setOpen} onChanged={reload} />)}</div>
            </Card>
          )}
        </div>
        <div className="grid min-w-0 grid-cols-1 content-start gap-5">
          <Card>
            <CardHeader icon={<Bell />} title="Alertas recentes" actions={<Button variant="ghost" size="sm" onClick={() => go("alerts")}>Ver todos <ArrowRight /></Button>} />
            {data && data.open_alerts.length > 0 ? (
              <div className="border-t">
                {data.open_alerts.map((a) => (
                  <button key={a.id} onClick={() => go("alerts")} className="flex w-full cursor-default items-start gap-3 border-b px-[var(--card-pad)] py-2.5 text-left last:border-b-0 hover:bg-muted/40">
                    <Badge tone={sevTone[a.draft.severity]}>{severityLabel[a.draft.severity]}</Badge>
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm font-medium">{a.draft.title}{a.count > 1 && <span className="text-muted-foreground"> ×{a.count}</span>}</p>
                      <p className="truncate text-xs text-muted-foreground">{a.draft.problem}</p>
                    </div>
                    <span className="shrink-0 text-xs text-muted-foreground">{relTime(a.last_at)}</span>
                  </button>
                ))}
              </div>
            ) : (
              !error && data && <Empty icon={<Bell />} title="Nenhum alerta aberto" />
            )}
            {error && <ErrorNote error={error} />}
          </Card>
          <RecentList title="Últimas análises" icon={<HardDrive />} items={data?.recent_scans} onOpen={setOpen} empty="Nenhuma análise ainda." />
          <RecentList title="Transferências recentes" icon={<Copy />} items={data?.recent_copies} onOpen={setOpen} empty="Nenhuma cópia ainda." />
        </div>
      </div>
      <TaskDetailDialog task={open} onClose={() => setOpen(null)} />
    </>
  );
}

function Kpi({ icon, label, value, sub, tone }: { icon: ReactNode; label: string; value: ReactNode; sub: string; tone?: "warning" | "danger" }) {
  return (
    <Card className="flex items-center gap-4 p-[var(--card-pad)]">
      <div className={`grid size-10 shrink-0 place-items-center rounded-lg [&_svg]:size-5 ${tone === "danger" ? "bg-destructive/12 text-destructive" : tone === "warning" ? "bg-warning/18 text-warning" : "bg-accent text-accent-foreground"}`}>{icon}</div>
      <div className="min-w-0">
        <p className="text-xs font-medium text-muted-foreground">{label}</p>
        <p className="text-2xl font-semibold leading-tight tracking-tight">{value}</p>
        <p className="truncate text-xs text-muted-foreground">{sub}</p>
      </div>
    </Card>
  );
}

function RecentList({ title, icon, items, onOpen, empty }: { title: string; icon: ReactNode; items?: TaskRecord[]; onOpen: (t: TaskRecord) => void; empty: string }) {
  if (!items) return null;
  return (
    <Card>
      <CardHeader icon={icon} title={title} />
      {items.length ? <div className="border-t">{items.map((t) => <TaskRow key={t.id} task={t} onOpen={onOpen} />)}</div> : <p className="px-[var(--card-pad)] pb-[var(--card-pad)] text-sm text-muted-foreground">{empty}</p>}
    </Card>
  );
}
