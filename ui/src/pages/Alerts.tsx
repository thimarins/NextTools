import { useState, type ReactElement } from "react";
import { toast } from "sonner";
import { Bell, CheckCheck, ChevronDown, CircleAlert, Info, OctagonAlert, TriangleAlert } from "lucide-react";
import { api, ApiError } from "@/lib/api";
import { useAsync } from "@/lib/hooks";
import { moduleLabel, severityLabel } from "@/lib/i18n";
import { useLive } from "@/lib/live";
import type { Alert, Severity } from "@/lib/types";
import { formatDate, relTime } from "@/lib/utils";
import { ErrorNote, Loading } from "@/components/states";
import { Button } from "@/components/ui/button";
import { Badge, Card, Empty, PageHeader } from "@/components/ui/primitives";
import { Switch } from "@/components/ui/overlay";

const sevIcon: Record<Severity, ReactElement> = {
  info: <Info className="text-info" />,
  warning: <TriangleAlert className="text-warning" />,
  error: <CircleAlert className="text-destructive" />,
  critical: <OctagonAlert className="text-destructive" />,
};
const sevTone = { info: "info", warning: "warning", error: "danger", critical: "danger" } as const;

export default function AlertsPage() {
  const [all, setAll] = useState(false);
  const { version, refreshAlerts } = useLive();
  const { data, error, loading, reload } = useAsync(() => api.alertsList(all, 500), [all, version]);

  const ack = async (a: Alert) => {
    try {
      await api.alertsAck(a.id);
      reload();
      refreshAlerts();
    } catch (e) {
      toast.error("Falha ao reconhecer alerta", { description: e instanceof ApiError ? e.message : String(e) });
    }
  };

  return (
    <>
      <PageHeader
        title="Alertas"
        description="Central compartilhada do NextSize e do NextCopy. Eventos semelhantes são agrupados."
        actions={
          <label className="flex items-center gap-2 text-sm">
            <Switch checked={all} onCheckedChange={setAll} /> Mostrar reconhecidos
          </label>
        }
      />
      {loading && !data ? (
        <Card><Loading /></Card>
      ) : error ? (
        <Card><ErrorNote error={error} onRetry={reload} /></Card>
      ) : data && data.length ? (
        <div className="grid gap-3">
          {data.map((a) => (
            <AlertCard key={a.id} a={a} onAck={() => ack(a)} />
          ))}
        </div>
      ) : (
        <Card>
          <Empty icon={<Bell />} title={all ? "Nenhum alerta registrado" : "Nenhum alerta aberto"}>Tudo certo por aqui.</Empty>
        </Card>
      )}
    </>
  );
}

function AlertCard({ a, onAck }: { a: Alert; onAck: () => void }) {
  const [details, setDetails] = useState(false);
  const d = a.draft;
  return (
    <Card className={a.acknowledged ? "opacity-70" : ""}>
      <div className="flex gap-4 p-[var(--card-pad)]">
        <div className="mt-0.5 [&_svg]:size-5">{sevIcon[d.severity]}</div>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3 className="font-semibold">{d.title}</h3>
            <Badge tone={sevTone[d.severity]}>{severityLabel[d.severity]}</Badge>
            <Badge>{moduleLabel[d.module]}</Badge>
            {d.task_id ? <Badge>Tarefa #{d.task_id}</Badge> : null}
            {a.count > 1 && <Badge tone="primary">{a.count} ocorrências</Badge>}
            {d.recoverable && <Badge tone="success">Recuperável</Badge>}
          </div>
          <p className="mt-2 text-sm">{d.problem}</p>
          <dl className="mt-3 grid gap-1.5 text-sm sm:grid-cols-[9rem_1fr]">
            {d.probable_cause && (
              <>
                <dt className="text-muted-foreground">Causa provável</dt>
                <dd>{d.probable_cause}</dd>
              </>
            )}
            <dt className="text-muted-foreground">Consequência</dt>
            <dd>{d.consequence}</dd>
            <dt className="text-muted-foreground">Ação recomendada</dt>
            <dd className="font-medium">{d.recommended_action}</dd>
          </dl>
          {d.details && (
            <>
              <button className="mt-3 flex cursor-default items-center gap-1 text-xs font-medium text-muted-foreground hover:text-foreground" onClick={() => setDetails((v) => !v)}>
                <ChevronDown className={`size-3.5 transition-transform ${details ? "" : "-rotate-90"}`} /> Detalhes técnicos
              </button>
              {details && <pre className="mt-2 max-h-48 overflow-auto rounded-md bg-muted p-3 font-mono text-xs whitespace-pre-wrap">{d.details}</pre>}
            </>
          )}
        </div>
        <div className="flex shrink-0 flex-col items-end gap-2 text-xs text-muted-foreground">
          <span title={`Primeiro: ${formatDate(a.first_at)}`}>{relTime(a.last_at)}</span>
          {!a.acknowledged && (
            <Button size="sm" variant="outline" onClick={onAck}>
              <CheckCheck /> Reconhecer
            </Button>
          )}
        </div>
      </div>
    </Card>
  );
}
