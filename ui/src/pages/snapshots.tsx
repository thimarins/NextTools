import { useState } from "react";
import { toast } from "sonner";
import { ArrowLeft, ChevronRight, Download, FileSearch, GitCompareArrows, Pencil, Trash2, TriangleAlert, Upload } from "lucide-react";
import { api, ApiError, pickPath, pickSavePath } from "@/lib/api";
import { useAsync } from "@/lib/hooks";
import type { CompareRow, SnapshotEntry } from "@/lib/types";
import { cn, formatBytes, formatCount, formatDate } from "@/lib/utils";
import { ErrorNote, Loading } from "@/components/states";
import { Button } from "@/components/ui/button";
import { Badge, Card, Empty, Input } from "@/components/ui/primitives";
import { Dialog, DialogContent, Tooltip } from "@/components/ui/overlay";

const err = (e: unknown) => (e instanceof ApiError ? e.message : String(e));

/** Catalog of saved snapshots (app's snapshots folder). */
export function SnapshotsPanel({ onOpen, onCompare }: { onOpen: (path: string) => void; onCompare: (a: string, b: string) => void }) {
  const list = useAsync(() => api.snapshotsList(), []);
  const [sel, setSel] = useState<string[]>([]);
  const [label, setLabel] = useState<SnapshotEntry | null>(null);
  const [del, setDel] = useState<SnapshotEntry | null>(null);

  const toggle = (p: string) => setSel((s) => (s.includes(p) ? s.filter((x) => x !== p) : [...s.slice(-1), p]));
  const compare = () => {
    const items = (list.data ?? []).filter((e) => sel.includes(e.path)).sort((a, b) => (a.started_at ?? a.modified) - (b.started_at ?? b.modified));
    if (items.length === 2) onCompare(items[0].path, items[1].path);
  };

  return (
    <>
      <div className="flex flex-wrap items-center gap-2 border-t px-[var(--card-pad)] py-2">
        <Button size="sm" variant="outline" disabled={sel.length !== 2} onClick={compare}>
          <GitCompareArrows /> Comparar selecionados ({sel.length}/2)
        </Button>
        <Button
          size="sm"
          variant="ghost"
          onClick={async () => {
            const src = await pickPath({ directory: false, title: "Importar snapshot", filters: [{ name: "Snapshot NextSize", extensions: ["nxs"] }] });
            if (!src) return;
            try {
              await api.snapshotImport(src);
              toast.success("Snapshot importado e validado");
              list.reload();
            } catch (e) {
              toast.error("Importação recusada", { description: err(e) });
            }
          }}
        >
          <Upload /> Importar…
        </Button>
      </div>
      {list.loading && !list.data ? (
        <Loading />
      ) : list.error ? (
        <ErrorNote error={list.error} onRetry={list.reload} />
      ) : !list.data?.length ? (
        <Empty icon={<FileSearch />} title="Nenhum snapshot salvo">Os resultados das análises aparecem aqui.</Empty>
      ) : (
        <div className="border-t">
          {list.data.map((e) => (
            <div key={e.path} className="flex items-center gap-3 border-b px-[var(--card-pad)] py-2 last:border-b-0 hover:bg-muted/40">
              <input type="checkbox" className="size-4 accent-[var(--primary)]" checked={sel.includes(e.path)} disabled={!!e.invalid} onChange={() => toggle(e.path)} aria-label="Selecionar para comparar" />
              <button className="min-w-0 flex-1 cursor-default text-left" disabled={!!e.invalid} onClick={() => onOpen(e.path)}>
                <p className="truncate text-sm font-medium">
                  {e.label ? `${e.label} · ` : ""}
                  {e.root ?? e.file_name}
                </p>
                <p className="truncate text-xs text-muted-foreground">
                  {e.invalid ? (
                    <span className="text-destructive">inválido: {e.invalid}</span>
                  ) : (
                    <>
                      {formatDate(e.started_at ?? e.modified)} · {formatCount(e.files)} arquivos · {formatBytes(e.bytes)} · arquivo {formatBytes(e.size_bytes)}
                    </>
                  )}
                </p>
              </button>
              {e.state && e.state !== "completed" && e.state !== "completed_with_warnings" && <Badge tone="warning">parcial</Badge>}
              <Tooltip content="Rótulo">
                <Button size="icon" variant="ghost" aria-label="Rótulo" disabled={!!e.invalid} onClick={() => setLabel(e)}>
                  <Pencil />
                </Button>
              </Tooltip>
              <Tooltip content="Exportar (cópia compacta e consistente)">
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label="Exportar"
                  disabled={!!e.invalid}
                  onClick={async () => {
                    const dest = await pickSavePath(e.file_name, [{ name: "Snapshot NextSize", extensions: ["nxs"] }]);
                    if (!dest) return;
                    try {
                      await api.snapshotExport(e.path, dest);
                      toast.success("Snapshot exportado", { description: dest });
                    } catch (x) {
                      toast.error("Falha ao exportar", { description: err(x) });
                    }
                  }}
                >
                  <Download />
                </Button>
              </Tooltip>
              <Tooltip content="Excluir">
                <Button size="icon" variant="ghost" aria-label="Excluir" onClick={() => setDel(e)}>
                  <Trash2 />
                </Button>
              </Tooltip>
            </div>
          ))}
        </div>
      )}
      <LabelDialog entry={label} onClose={() => setLabel(null)} onDone={list.reload} />
      <DeleteDialog entry={del} onClose={() => setDel(null)} onDone={() => { setSel((s) => s.filter((p) => p !== del?.path)); list.reload(); }} />
    </>
  );
}

function LabelDialog({ entry, onClose, onDone }: { entry: SnapshotEntry | null; onClose: () => void; onDone: () => void }) {
  const [v, setV] = useState("");
  return (
    <Dialog open={!!entry} onOpenChange={(o) => (o ? setV(entry?.label ?? "") : onClose())}>
      {entry && (
        <DialogContent title="Rótulo do snapshot" description={entry.root ?? entry.file_name}>
          <form
            className="grid gap-3"
            onSubmit={async (ev) => {
              ev.preventDefault();
              try {
                await api.snapshotSetLabel(entry.path, v);
                onDone();
                onClose();
              } catch (e) {
                toast.error("Falha ao salvar rótulo", { description: err(e) });
              }
            }}
          >
            <Input autoFocus value={v} maxLength={120} onChange={(e) => setV(e.target.value)} placeholder="Ex.: Antes da migração" />
            <div className="flex justify-end gap-2">
              <Button variant="outline" onClick={onClose}>
                Cancelar
              </Button>
              <Button type="submit">Salvar</Button>
            </div>
          </form>
        </DialogContent>
      )}
    </Dialog>
  );
}

/** Permanent deletion: the user must type the file name. */
function DeleteDialog({ entry, onClose, onDone }: { entry: SnapshotEntry | null; onClose: () => void; onDone: () => void }) {
  const [v, setV] = useState("");
  const [busy, setBusy] = useState(false);
  return (
    <Dialog open={!!entry} onOpenChange={(o) => (o ? setV("") : onClose())}>
      {entry && (
        <DialogContent title="Excluir snapshot?" description="A exclusão é permanente. Os arquivos analisados não são afetados — apenas o resultado da análise.">
          <p className="text-sm">
            Para confirmar, digite <strong className="font-mono">{entry.file_name}</strong>:
          </p>
          <Input className="mt-2 font-mono" value={v} onChange={(e) => setV(e.target.value)} />
          <div className="mt-4 flex justify-end gap-2">
            <Button variant="outline" onClick={onClose}>
              Cancelar
            </Button>
            <Button
              variant="destructive"
              disabled={v.trim() !== entry.file_name || busy}
              onClick={async () => {
                setBusy(true);
                try {
                  await api.snapshotDelete(entry.path, v.trim());
                  toast.success("Snapshot excluído");
                  onDone();
                  onClose();
                } catch (e) {
                  toast.error("Não foi possível excluir", { description: err(e) });
                } finally {
                  setBusy(false);
                }
              }}
            >
              <Trash2 /> Excluir definitivamente
            </Button>
          </div>
        </DialogContent>
      )}
    </Dialog>
  );
}

/* ---------------------------------------------------------- comparison */

const statusLabel: Record<CompareRow["status"], { t: string; tone: "success" | "danger" | "warning" | "info" | "neutral" }> = {
  added: { t: "nova", tone: "info" },
  removed: { t: "removida", tone: "neutral" },
  grew: { t: "cresceu", tone: "warning" },
  shrank: { t: "diminuiu", tone: "success" },
  same: { t: "igual", tone: "neutral" },
  files: { t: "arquivos", tone: "neutral" },
};

function signed(n: number) {
  return (n > 0 ? "+" : n < 0 ? "−" : "") + formatBytes(Math.abs(n));
}

export function CompareView({ a, b, onClose }: { a: string; b: string; onClose: () => void }) {
  const [rel, setRel] = useState<string[]>([]);
  const sa = useAsync(() => api.snapshotSummary(a), [a]);
  const sb = useAsync(() => api.snapshotSummary(b), [b]);
  const lvl = useAsync(() => api.snapshotCompare(a, b, rel, 500), [a, b, rel.join("\u0000")]);
  const d = lvl.data;
  const partial = (sa.data && !sa.data.complete) || (sb.data && !sb.data.complete);

  return (
    <div className="grid gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <Button variant="ghost" size="sm" onClick={onClose}>
          <ArrowLeft /> Análises
        </Button>
        <h1 className="text-xl font-semibold tracking-tight">Comparação de snapshots</h1>
      </div>
      <Card className="grid gap-4 p-[var(--card-pad)] sm:grid-cols-3">
        <div className="min-w-0">
          <p className="text-xs font-medium text-muted-foreground">A (mais antigo)</p>
          <p className="truncate font-medium">{sa.data?.label ?? sa.data?.root}</p>
          <p className="text-xs text-muted-foreground">
            {formatDate(sa.data?.started_at)} · {formatBytes(sa.data?.bytes)}
          </p>
        </div>
        <div className="min-w-0">
          <p className="text-xs font-medium text-muted-foreground">B (mais recente)</p>
          <p className="truncate font-medium">{sb.data?.label ?? sb.data?.root}</p>
          <p className="text-xs text-muted-foreground">
            {formatDate(sb.data?.started_at)} · {formatBytes(sb.data?.bytes)}
          </p>
        </div>
        <div>
          <p className="text-xs font-medium text-muted-foreground">Variação nesta pasta</p>
          <p className={cn("text-xl font-semibold", d && (d.b_total ?? 0) - (d.a_total ?? 0) > 0 ? "text-warning" : "")}>{d ? signed((d.b_total ?? 0) - (d.a_total ?? 0)) : "—"}</p>
          {d?.a_total ? <p className="text-xs text-muted-foreground">{((((d.b_total ?? 0) - d.a_total) / d.a_total) * 100).toLocaleString("pt-BR", { maximumFractionDigits: 1 })}% em relação a A</p> : null}
        </div>
      </Card>
      {sa.data && sb.data && sa.data.root !== sb.data.root && (
        <p className="flex items-center gap-2 text-sm text-warning">
          <TriangleAlert className="size-4" /> As origens são diferentes ({sa.data.root} × {sb.data.root}); as pastas são comparadas pelo nome relativo.
        </p>
      )}
      {partial && (
        <p className="flex items-center gap-2 text-sm text-warning">
          <TriangleAlert className="size-4" /> Um dos snapshots é parcial: os tamanhos de pastas podem estar incompletos.
        </p>
      )}
      <Card className="overflow-hidden">
        <div className="flex flex-wrap items-center gap-1 border-b px-3 py-2 text-sm">
          <button className="cursor-default rounded px-1 hover:bg-muted" onClick={() => setRel([])}>
            {sb.data?.root ?? "raiz"}
          </button>
          {rel.map((n, i) => (
            <span key={i} className="flex items-center gap-1">
              <ChevronRight className="size-3.5 text-muted-foreground" />
              <button className={cn("cursor-default rounded px-1 hover:bg-muted", i === rel.length - 1 && "font-semibold")} onClick={() => setRel(rel.slice(0, i + 1))}>
                {n}
              </button>
            </span>
          ))}
        </div>
        {lvl.error ? (
          <ErrorNote error={lvl.error} onRetry={lvl.reload} />
        ) : !d ? (
          <Loading />
        ) : (
          <>
            {d.truncated && <p className="border-b px-3 py-2 text-xs text-warning">Pasta com mais de 20.000 subpastas: comparação parcial neste nível.</p>}
            {!d.a_found && <p className="border-b px-3 py-2 text-xs text-muted-foreground">Esta pasta não existe em A (nova).</p>}
            {!d.b_found && <p className="border-b px-3 py-2 text-xs text-muted-foreground">Esta pasta não existe mais em B (removida).</p>}
            <div className="max-h-[calc(100vh-360px)] overflow-auto">
              <table className="w-full table-fixed text-sm">
                <colgroup>
                  <col />
                  <col className="w-28" />
                  <col className="w-28" />
                  <col className="w-28" />
                  <col className="w-32" />
                  <col className="w-24" />
                </colgroup>
                <thead className="sticky top-0 bg-card text-xs text-muted-foreground shadow-[0_1px_0_var(--border)]">
                  <tr className="h-9">
                    <th className="px-3 text-left font-medium">Pasta</th>
                    <th className="px-3 text-left font-medium">Situação</th>
                    <th className="px-3 text-right font-medium">A</th>
                    <th className="px-3 text-right font-medium">B</th>
                    <th className="px-3 text-right font-medium">Variação</th>
                    <th className="px-3 text-right font-medium">%</th>
                  </tr>
                </thead>
                <tbody>
                  {d.rows.map((r) => (
                    <tr key={r.name} className={cn("h-[var(--row-h)] border-t hover:bg-muted/40", r.has_children && "cursor-pointer")} onClick={() => r.has_children && setRel([...rel, r.name])}>
                      <td className="truncate px-3" title={r.name}>
                        {r.name}
                        {r.has_children && <ChevronRight className="ml-1 inline size-3.5 text-muted-foreground" />}
                      </td>
                      <td className="px-3">
                        <Badge tone={statusLabel[r.status].tone}>{statusLabel[r.status].t}</Badge>
                      </td>
                      <td className="px-3 text-right tabular-nums text-muted-foreground">{formatBytes(r.a_size)}</td>
                      <td className="px-3 text-right tabular-nums">{formatBytes(r.b_size)}</td>
                      <td className={cn("px-3 text-right font-medium tabular-nums", r.delta > 0 ? "text-warning" : r.delta < 0 ? "text-success" : "text-muted-foreground")}>{signed(r.delta)}</td>
                      <td className="px-3 text-right text-xs tabular-nums text-muted-foreground">{r.pct == null ? "—" : `${r.pct > 0 ? "+" : ""}${r.pct.toLocaleString("pt-BR", { maximumFractionDigits: 1 })}%`}</td>
                    </tr>
                  ))}
                  {d.rows.length === 0 && (
                    <tr>
                      <td colSpan={6} className="px-4 py-8 text-center text-muted-foreground">
                        Nada a comparar neste nível.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
          </>
        )}
      </Card>
    </div>
  );
}
