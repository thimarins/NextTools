import { useMemo, useState, type ReactNode } from "react";
import { toast } from "sonner";
import { FileSearch, FolderTree, HardDrive, Play, TriangleAlert } from "lucide-react";
import { api, ApiError } from "@/lib/api";
import { useAsync, useThrottledEffect } from "@/lib/hooks";
import { scanProfiles } from "@/lib/i18n";
import { useLive } from "@/lib/live";
import type { TaskRecord } from "@/lib/types";
import { formatBytes, formatCount, formatDate, formatDuration, snapshotOf } from "@/lib/utils";
import { PathInput } from "@/components/shell";
import { ErrorNote, Loading } from "@/components/states";
import { TaskDetailDialog, TaskRow } from "@/components/tasks";
import { Button } from "@/components/ui/button";
import { Badge, Card, CardBody, CardHeader, Empty, Field, PageHeader, Select, Stat } from "@/components/ui/primitives";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/overlay";

export default function NextSizePage() {
  const { version } = useLive();
  const [root, setRoot] = useState("");
  const [profile, setProfile] = useState("balanced");
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState<TaskRecord | null>(null);
  const [snapshot, setSnapshot] = useState<string | null>(null);
  const [external, setExternal] = useState("");

  const tasks = useAsync(() => api.tasksList(undefined, 300), []);
  useThrottledEffect(tasks.reload, version, 1500);
  const scans = useMemo(() => (tasks.data ?? []).filter((t) => t.kind === "size_scan").slice(0, 30), [tasks.data]);

  const start = async () => {
    setBusy(true);
    try {
      const id = await api.sizeScanSubmit(root, profile);
      toast.success(`Análise #${id} criada`, { description: "Ela aparece abaixo e roda em segundo plano." });
      tasks.reload();
    } catch (e) {
      toast.error("Não foi possível iniciar a análise", { description: e instanceof ApiError ? e.message : String(e) });
    } finally {
      setBusy(false);
    }
  };

  const hint = scanProfiles.find((p) => p.value === profile)?.hint;

  return (
    <>
      <PageHeader title="NextSize" description="Análise de armazenamento somente leitura: nada é alterado na origem." />
      <div className="grid gap-5 [&>*]:min-w-0 lg:grid-cols-[minmax(320px,420px)_1fr]">
        <Card className="self-start">
          <CardHeader icon={<HardDrive />} title="Nova análise" description="Disco local, pasta, compartilhamento SMB (\\servidor\pasta) ou unidade de rede." />
          <CardBody className="grid gap-4">
            <Field label="Origem" htmlFor="scan-root">
              <PathInput id="scan-root" value={root} onChange={setRoot} placeholder="D:\Dados ou \\nas\compartilhamento" />
            </Field>
            <Field label="Perfil de desempenho" hint={hint} htmlFor="scan-profile">
              <Select id="scan-profile" value={profile} onChange={(e) => setProfile(e.target.value)}>
                {scanProfiles.map((p) => (
                  <option key={p.value} value={p.value}>
                    {p.label}
                  </option>
                ))}
              </Select>
            </Field>
            <Button onClick={start} disabled={busy || !root.trim()}>
              <Play /> {busy ? "Criando…" : "Iniciar análise"}
            </Button>
            <p className="text-xs text-muted-foreground">
              O resultado é salvo como snapshot (.nxs). Uma análise interrompida pode ser retomada sem perder o que já foi lido.
            </p>
          </CardBody>
        </Card>

        <Card className="self-start">
          <CardHeader
            icon={<FolderTree />}
            title="Análises"
            description="Clique em uma análise concluída para ver o resultado"
            actions={
              <div className="flex w-72 gap-2">
                <PathInput value={external} onChange={setExternal} placeholder="Abrir snapshot .nxs…" file={{ name: "Snapshot NextSize", extensions: ["nxs"] }} />
                <Button variant="outline" disabled={!external.trim()} onClick={() => setSnapshot(external.trim())}>
                  Abrir
                </Button>
              </div>
            }
          />
          {tasks.loading && !tasks.data ? (
            <Loading />
          ) : tasks.error ? (
            <ErrorNote error={tasks.error} onRetry={tasks.reload} />
          ) : scans.length ? (
            <div className="border-t">
              {scans.map((t) => (
                <TaskRow
                  key={t.id}
                  task={t}
                  onChanged={tasks.reload}
                  onOpen={(x) => {
                    const s = snapshotOf(x);
                    if (s && (x.state === "completed" || x.state === "completed_with_warnings")) setSnapshot(s);
                    else setOpen(x);
                  }}
                />
              ))}
            </div>
          ) : (
            <Empty icon={<HardDrive />} title="Nenhuma análise ainda">Escolha uma origem ao lado para começar.</Empty>
          )}
        </Card>
      </div>

      {snapshot && <SnapshotView path={snapshot} key={snapshot} onClose={() => setSnapshot(null)} />}
      <TaskDetailDialog task={open} onClose={() => setOpen(null)} />
    </>
  );
}

function SnapshotView({ path, onClose }: { path: string; onClose: () => void }) {
  const summary = useAsync(() => api.snapshotSummary(path), [path]);
  const s = summary.data;
  return (
    <Card className="mt-5">
      <CardHeader
        icon={<FileSearch />}
        title={s ? s.root : "Snapshot"}
        description={<span className="font-mono text-xs">{path}</span>}
        actions={
          <Button variant="ghost" size="sm" onClick={onClose}>
            Fechar
          </Button>
        }
      />
      {summary.error && <ErrorNote error={summary.error} onRetry={summary.reload} />}
      {!s && !summary.error && <Loading />}
      {s && (
        <CardBody>
          {!s.complete && (
            <div className="mb-4 flex items-center gap-2 rounded-lg border border-warning/40 bg-warning/12 px-3 py-2 text-sm">
              <TriangleAlert className="size-4 text-warning" />
              <span>
                <strong>Dados parciais</strong> — a análise está em "{s.state}". Os números abaixo cobrem apenas o que já foi lido.
              </span>
            </div>
          )}
          <div className="mb-5 grid grid-cols-2 gap-4 sm:grid-cols-3 xl:grid-cols-6">
            <Stat label="Tamanho lógico" value={formatBytes(s.bytes)} sub={`${formatCount(s.bytes)} bytes`} />
            <Stat label="Tamanho alocado" value={s.alloc_bytes != null ? formatBytes(s.alloc_bytes) : "Não disponível"} sub={s.alloc_bytes == null ? "não informado neste modo" : undefined} />
            <Stat label="Arquivos" value={formatCount(s.files)} />
            <Stat label="Pastas" value={formatCount(s.dirs)} />
            <Stat label="Ocorrências" value={formatCount(s.issues)} sub={s.issues ? "pastas sem acesso etc." : "nenhuma"} />
            <Stat label="Analisado em" value={formatDate(s.finished_at ?? s.started_at)} sub={`${formatDuration((s.scan_ms ?? 0) + (s.finalize_ms ?? 0))} · ${s.source_kind === "remote" ? "rede" : "local"}`} />
          </div>
          {s.complete ? (
            <Tabs defaultValue="files">
              <TabsList>
                <TabsTrigger value="files">Maiores arquivos</TabsTrigger>
                <TabsTrigger value="dirs">Maiores pastas</TabsTrigger>
                <TabsTrigger value="ext">Por extensão</TabsTrigger>
                <TabsTrigger value="years">Por ano</TabsTrigger>
              </TabsList>
              <TabsContent value="files" className="mt-3">
                <TopFiles path={path} />
              </TabsContent>
              <TabsContent value="dirs" className="mt-3">
                <TopDirs path={path} />
              </TabsContent>
              <TabsContent value="ext" className="mt-3">
                <StatsTable path={path} kind="ext" total={s.bytes} />
              </TabsContent>
              <TabsContent value="years" className="mt-3">
                <StatsTable path={path} kind="years" total={s.bytes} />
              </TabsContent>
            </Tabs>
          ) : (
            <p className="text-sm text-muted-foreground">Rankings e filtros ficam disponíveis quando a análise terminar.</p>
          )}
        </CardBody>
      )}
    </Card>
  );
}

function Table({ head, children }: { head: ReactNode[]; children: ReactNode }) {
  return (
    <div className="overflow-hidden rounded-lg border">
      <table className="w-full table-fixed text-sm">
        <colgroup>
          {head.map((_, i) => (
            <col key={i} className={i === 0 ? "" : i === head.length - 1 && head.length === 4 ? "w-44" : "w-36"} />
          ))}
        </colgroup>
        <thead className="bg-muted/60 text-xs text-muted-foreground">
          <tr>
            {head.map((h, i) => (
              <th key={i} className={`h-9 px-3 font-medium ${i === 0 ? "text-left" : "text-right"}`}>
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="[&_tr]:h-[var(--row-h)] [&_tr]:border-t [&_tr:hover]:bg-muted/40">{children}</tbody>
      </table>
    </div>
  );
}

function Bar({ v, total }: { v: number; total: number }) {
  const pct = total > 0 ? (v / total) * 100 : 0;
  return (
    <div className="ml-auto flex w-32 items-center gap-2">
      <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
        <div className="h-full rounded-full bg-primary" style={{ width: `${Math.min(100, pct)}%` }} />
      </div>
      <span className="w-10 text-right text-xs text-muted-foreground tabular-nums">{pct < 0.1 ? "<0,1" : pct.toLocaleString("pt-BR", { maximumFractionDigits: 1 })}%</span>
    </div>
  );
}

function TopFiles({ path }: { path: string }) {
  const { data, error } = useAsync(() => api.snapshotTopFiles(path, 50), [path]);
  if (error) return <ErrorNote error={error} />;
  if (!data) return <Loading />;
  return (
    <Table head={["Arquivo", "Tamanho", "Modificado"]}>
      {data.map((r) => (
        <tr key={r.id}>
          <td className="truncate px-3 font-mono text-[12.5px]" title={r.path}>{r.path}</td>
          <td className="px-3 text-right whitespace-nowrap tabular-nums">{formatBytes(r.size)}</td>
          <td className="px-3 text-right whitespace-nowrap text-muted-foreground">{formatDate(r.mtime)}</td>
        </tr>
      ))}
    </Table>
  );
}

function TopDirs({ path }: { path: string }) {
  const { data, error } = useAsync(() => api.snapshotTopDirs(path, 50), [path]);
  if (error) return <ErrorNote error={error} />;
  if (!data) return <Loading />;
  return (
    <Table head={["Pasta", "Tamanho total", "Arquivos"]}>
      {data.map((r) => (
        <tr key={r.id}>
          <td className="truncate px-3 font-mono text-[12.5px]" title={r.path ?? r.name}>
            {r.path ?? r.name}
            {r.flags & (1 << 14) ? <Badge tone="warning" className="ml-2">leitura incompleta</Badge> : null}
          </td>
          <td className="px-3 text-right whitespace-nowrap tabular-nums">{formatBytes(r.size)}</td>
          <td className="px-3 text-right whitespace-nowrap tabular-nums text-muted-foreground">{formatCount(r.files)}</td>
        </tr>
      ))}
    </Table>
  );
}

function StatsTable({ path, kind, total }: { path: string; kind: "ext" | "years"; total: number }) {
  const { data, error } = useAsync(() => api.snapshotStats(path), [path]);
  if (error) return <ErrorNote error={error} />;
  if (!data) return <Loading />;
  const rows = kind === "ext" ? data.extensions.map((e) => ({ k: e.ext === "(sem extensão)" ? e.ext : `.${e.ext}`, files: e.files, size: e.size })) : data.years.map((y) => ({ k: y.year == null ? "(sem data)" : String(y.year), files: y.files, size: y.size })).reverse();
  return (
    <Table head={[kind === "ext" ? "Extensão" : "Ano (modificação)", "Arquivos", "Tamanho", "Participação"]}>
      {rows.map((r) => (
        <tr key={r.k}>
          <td className="px-3 font-mono text-[12.5px]">{r.k}</td>
          <td className="px-3 text-right tabular-nums text-muted-foreground">{formatCount(r.files)}</td>
          <td className="px-3 text-right tabular-nums">{formatBytes(r.size)}</td>
          <td className="px-3">
            <Bar v={r.size} total={total} />
          </td>
        </tr>
      ))}
    </Table>
  );
}
