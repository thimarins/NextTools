import { useState } from "react";
import { toast } from "sonner";
import { CalendarClock, Trash2 } from "lucide-react";
import { api, ApiError } from "@/lib/api";
import { useAsync } from "@/lib/hooks";
import type { Recurrence } from "@/lib/types";
import { cn, formatDate, relTime } from "@/lib/utils";
import { ErrorNote, Loading } from "./states";
import { Button } from "./ui/button";
import { Badge, Card, Empty, Field, Input, Select } from "./ui/primitives";
import { Dialog, DialogContent, Switch } from "./ui/overlay";

const DAYS = ["Seg", "Ter", "Qua", "Qui", "Sex", "Sáb", "Dom"];

/** "Agendar…" dialog shared by NextSize and NextCopy. */
export function ScheduleDialog({
  open,
  onClose,
  kind,
  request,
  defaultTitle,
  note,
}: {
  open: boolean;
  onClose: () => void;
  kind: "scan" | "copy";
  request: unknown;
  defaultTitle: string;
  note?: string;
}) {
  const [title, setTitle] = useState("");
  const [freq, setFreq] = useState<"once" | "daily" | "weekly">("daily");
  const [time, setTime] = useState("02:00");
  const [date, setDate] = useState("");
  const [days, setDays] = useState<number[]>([0, 1, 2, 3, 4]);
  const [busy, setBusy] = useState(false);

  const build = (): Recurrence | null => {
    const [h, m] = time.split(":").map(Number);
    if (freq === "once") {
      if (!date) return null;
      return { type: "once", at: Math.floor(new Date(`${date}T${time}:00`).getTime() / 1000) };
    }
    if (freq === "daily") return { type: "daily", hour: h, minute: m };
    return { type: "weekly", days, hour: h, minute: m };
  };

  const save = async () => {
    const rec = build();
    if (!rec) return;
    setBusy(true);
    try {
      await api.scheduleCreate(title || defaultTitle, kind, request, rec);
      toast.success("Agendamento criado", { description: "Ele roda enquanto o NextTool estiver aberto. Veja em Histórico › Agendamentos." });
      onClose();
    } catch (e) {
      toast.error("Não foi possível agendar", { description: e instanceof ApiError ? e.message : String(e) });
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={(o) => !o && onClose()}>
      {open && (
        <DialogContent title="Agendar" description="Na versão 1, o agendamento só é executado com o aplicativo aberto. Execuções perdidas geram um alerta e não são feitas depois automaticamente.">
          <div className="grid gap-3">
            {note && <p className="rounded-md border border-warning/40 bg-warning/10 p-2 text-sm">{note}</p>}
            <Field label="Nome">
              <Input value={title} onChange={(e) => setTitle(e.target.value)} placeholder={defaultTitle} maxLength={120} />
            </Field>
            <Field label="Frequência">
              <Select value={freq} onChange={(e) => setFreq(e.target.value as typeof freq)}>
                <option value="once">Uma vez</option>
                <option value="daily">Diariamente</option>
                <option value="weekly">Semanalmente</option>
              </Select>
            </Field>
            {freq === "once" && (
              <Field label="Data">
                <Input type="date" value={date} onChange={(e) => setDate(e.target.value)} />
              </Field>
            )}
            {freq === "weekly" && (
              <Field label="Dias">
                <div className="flex flex-wrap gap-1">
                  {DAYS.map((d, i) => (
                    <button
                      key={d}
                      type="button"
                      className={cn("h-8 w-11 cursor-default rounded-md border text-sm", days.includes(i) ? "border-primary bg-primary text-primary-foreground" : "hover:bg-muted")}
                      onClick={() => setDays((s) => (s.includes(i) ? s.filter((x) => x !== i) : [...s, i]))}
                    >
                      {d}
                    </button>
                  ))}
                </div>
              </Field>
            )}
            <Field label="Horário (local)">
              <Input type="time" value={time} onChange={(e) => setTime(e.target.value)} />
            </Field>
            <div className="mt-2 flex justify-end gap-2">
              <Button variant="outline" onClick={onClose}>
                Cancelar
              </Button>
              <Button onClick={save} disabled={busy || !build() || (freq === "weekly" && days.length === 0)}>
                <CalendarClock /> Agendar
              </Button>
            </div>
          </div>
        </DialogContent>
      )}
    </Dialog>
  );
}

/** List of schedules (History › Agendamentos). */
export function SchedulesPanel() {
  const list = useAsync(() => api.schedulesList(), []);
  const act = async (fn: () => Promise<unknown>) => {
    try {
      await fn();
      list.reload();
    } catch (e) {
      toast.error("Falha", { description: e instanceof ApiError ? e.message : String(e) });
    }
  };
  if (list.error) return <Card><ErrorNote error={list.error} onRetry={list.reload} /></Card>;
  if (!list.data) return <Card><Loading /></Card>;
  if (!list.data.length)
    return (
      <Card>
        <Empty icon={<CalendarClock />} title="Nenhum agendamento">
          Use "Agendar…" nos formulários do NextSize ou do NextCopy.
        </Empty>
      </Card>
    );
  return (
    <Card>
      {list.data.map((s) => (
        <div key={s.id} className="flex items-center gap-4 border-b px-[var(--card-pad)] py-3 last:border-b-0">
          <Switch checked={s.enabled} onCheckedChange={(v) => act(() => api.scheduleSetEnabled(s.id, v))} aria-label="Ativo" />
          <div className="min-w-0 flex-1">
            <p className="truncate text-sm font-medium">
              {s.title} <Badge className="ml-1">{s.kind === "scan" ? "Análise" : "Cópia"}</Badge>
            </p>
            <p className="truncate text-xs text-muted-foreground">
              {s.description}
              {s.enabled && s.next_run_at ? ` · próxima: ${formatDate(s.next_run_at)}` : s.enabled ? " · sem próxima execução" : " · desativado"}
              {s.last_run_at ? ` · última: ${relTime(s.last_run_at)} (${s.last_result})` : s.last_result ? ` · ${s.last_result}` : ""}
            </p>
          </div>
          <Button size="icon" variant="ghost" aria-label="Excluir agendamento" onClick={() => act(() => api.scheduleDelete(s.id))}>
            <Trash2 />
          </Button>
        </div>
      ))}
    </Card>
  );
}
