import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  ArrowLeft,
  BarChart3,
  ChevronLeft,
  ChevronRight,
  File,
  Filter,
  Folder,
  FolderOpen,
  FolderTree,
  Loader2,
  RefreshCw,
  Search,
  Square,
  TriangleAlert,
} from "lucide-react";
import { api, ApiError, nextRequestId } from "@/lib/api";
import { useAsync } from "@/lib/hooks";
import type { DirRow, FileQuery, FileRow, Paged, Summary } from "@/lib/types";
import { cn, formatBytes, formatCount, formatDate, formatDuration } from "@/lib/utils";
import { useSettings } from "@/lib/settings";
import { ErrorNote, Loading } from "@/components/states";
import { Button } from "@/components/ui/button";
import { Badge, Card, Field, Input, Select, Stat } from "@/components/ui/primitives";
import { Tabs, TabsContent, TabsList, TabsTrigger, Tooltip } from "@/components/ui/overlay";

const F_ENUM_ERROR = 1 << 14;
const F_NOT_FOLLOWED = 1 << 11;
const LIST_PAGE = 200;
const TREE_PAGE = 300;

/* ------------------------------------------------------------------ shell */

export function SnapshotExplorer({ path, onClose }: { path: string; onClose: () => void }) {
  const summary = useAsync(() => api.snapshotSummary(path), [path]);
  const s = summary.data;
  const [tab, setTab] = useState("explorer");
  const [dir, setDir] = useState(1);
  const openDir = (id: number) => {
    setDir(id);
    setTab("explorer");
  };

  return (
    <div className="grid gap-4">
      <div className="flex flex-wrap items-center gap-3">
        <Button variant="ghost" size="sm" onClick={onClose}>
          <ArrowLeft /> Análises
        </Button>
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-xl font-semibold tracking-tight">{s?.root ?? "Snapshot"}</h1>
          <p className="truncate font-mono text-xs text-muted-foreground">{path}</p>
        </div>
        <Tooltip content="Recarregar (para análises em andamento)">
          <Button variant="outline" size="icon" onClick={summary.reload} aria-label="Recarregar">
            <RefreshCw />
          </Button>
        </Tooltip>
      </div>
      {summary.error && (
        <Card>
          <ErrorNote error={summary.error} onRetry={summary.reload} />
        </Card>
      )}
      {!s && !summary.error && (
        <Card>
          <Loading />
        </Card>
      )}
      {s && (
        <>
          <SummaryStrip s={s} />
          <Tabs value={tab} onValueChange={setTab}>
            <TabsList>
              <TabsTrigger value="explorer">
                <FolderTree /> Explorador
              </TabsTrigger>
              <TabsTrigger value="filters">
                <Filter /> Filtros
              </TabsTrigger>
              <TabsTrigger value="rankings">
                <BarChart3 /> Rankings e distribuição
              </TabsTrigger>
            </TabsList>
            <TabsContent value="explorer" className="mt-3">
              <ExplorerPane path={path} summary={s} dir={dir} onDir={setDir} key={`${path}-${summary.data?.files}`} />
            </TabsContent>
            <TabsContent value="filters" className="mt-3" forceMount hidden={tab !== "filters"}>
              <FiltersPane path={path} onOpenDir={openDir} />
            </TabsContent>
            <TabsContent value="rankings" className="mt-3">
              <Rankings path={path} total={s.bytes} onOpenDir={openDir} finalized={s.finalized} />
            </TabsContent>
          </Tabs>
        </>
      )}
    </div>
  );
}

function SummaryStrip({ s }: { s: Summary }) {
  return (
    <Card className="p-[var(--card-pad)]">
      {!s.complete && (
        <div className="mb-4 flex items-start gap-2 rounded-lg border border-warning/40 bg-warning/12 px-3 py-2 text-sm">
          <TriangleAlert className="mt-0.5 size-4 shrink-0 text-warning" />
          <span>
            <strong>Dados parciais</strong> — estado "{s.state}". Os números cobrem só o que já foi lido e gravado. Tamanhos de pastas mostram apenas o conteúdo direto até a finalização; consultas podem ser mais lentas (limite de 20 s).
          </span>
        </div>
      )}
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 xl:grid-cols-6">
        <Stat label="Tamanho lógico" value={formatBytes(s.bytes)} sub={`${formatCount(s.bytes)} bytes`} />
        <Stat label="Tamanho alocado" value={s.alloc_bytes != null ? formatBytes(s.alloc_bytes) : "Não disponível"} sub={s.alloc_bytes == null ? "não informado neste modo" : undefined} />
        <Stat label="Arquivos" value={formatCount(s.files)} />
        <Stat label="Pastas" value={formatCount(s.dirs)} />
        <Stat label="Ocorrências" value={formatCount(s.issues)} sub={s.issues ? "pastas sem acesso etc." : "nenhuma"} />
        <Stat
          label="Analisado em"
          value={formatDate(s.finished_at ?? s.started_at)}
          sub={`${formatDuration((s.scan_ms ?? 0) + (s.finalize_ms ?? 0))} · ${s.source_kind === "remote" ? "rede" : "local"}${s.resumed_count ? ` · retomada ${s.resumed_count}×` : ""}`}
        />
      </div>
    </Card>
  );
}

/* ------------------------------------------------------------- explorer */

type TreeNode = {
  id: number;
  name: string;
  size: number;
  files: number;
  dirs: number;
  flags: number;
  depth: number;
  children?: number[];
  more?: boolean;
  loading?: boolean;
  error?: string;
};

function ExplorerPane({ path, summary, dir, onDir }: { path: string; summary: Summary; dir: number; onDir: (id: number) => void }) {
  return (
    <div className="grid gap-4 [&>*]:min-w-0 lg:grid-cols-[minmax(260px,360px)_1fr]">
      <TreePane path={path} summary={summary} selected={dir} onSelect={onDir} />
      <ListPane path={path} dirId={dir} onOpenDir={onDir} />
    </div>
  );
}

/** Lazy, virtualized directory tree. Children are fetched page by page only
 *  when a node is expanded; nothing large ever crosses the IPC. */
function TreePane({ path, summary, selected, onSelect }: { path: string; summary: Summary; selected: number; onSelect: (id: number) => void }) {
  const [nodes, setNodes] = useState<Map<number, TreeNode>>(
    () => new Map([[1, { id: 1, name: summary.root, size: summary.bytes, files: summary.files, dirs: 1, flags: 0, depth: 0 }]]),
  );
  const [expanded, setExpanded] = useState<Set<number>>(new Set());

  const load = useCallback(
    async (id: number, offset: number) => {
      setNodes((m) => new Map(m).set(id, { ...m.get(id)!, loading: true, error: undefined }));
      try {
        const page = await api.snapshotListSubdirs(path, id, offset, TREE_PAGE, nextRequestId("tree"));
        setNodes((m) => {
          const next = new Map(m);
          const parent = next.get(id)!;
          const kids = page.rows.map((r) => {
            next.set(r.id, { id: r.id, name: r.name, size: r.size, files: r.files, dirs: r.dirs, flags: r.flags, depth: parent.depth + 1 });
            return r.id;
          });
          next.set(id, { ...parent, loading: false, children: [...(offset ? parent.children ?? [] : []), ...kids], more: page.rows.length === TREE_PAGE });
          return next;
        });
      } catch (e) {
        setNodes((m) => new Map(m).set(id, { ...m.get(id)!, loading: false, error: e instanceof ApiError ? e.message : String(e) }));
      }
    },
    [path],
  );

  useEffect(() => {
    setExpanded(new Set([1]));
    load(1, 0);
  }, [load]);

  const toggle = (id: number) => {
    const n = nodes.get(id);
    const open = !expanded.has(id);
    setExpanded((s) => {
      const x = new Set(s);
      if (open) x.add(id);
      else x.delete(id);
      return x;
    });
    if (open && n && !n.children && !n.loading) load(id, 0);
  };

  // Flatten visible nodes (+ "load more" rows).
  type Row = { kind: "node"; node: TreeNode; parentSize: number } | { kind: "more"; parent: TreeNode } | { kind: "loading"; depth: number };
  const rows = useMemo(() => {
    const out: Row[] = [];
    const walk = (id: number, parentSize: number) => {
      const n = nodes.get(id);
      if (!n) return;
      out.push({ kind: "node", node: n, parentSize });
      if (!expanded.has(id)) return;
      if (n.loading && !n.children) out.push({ kind: "loading", depth: n.depth + 1 });
      for (const c of n.children ?? []) walk(c, n.size);
      if (n.more) out.push({ kind: "more", parent: n });
    };
    walk(1, summary.bytes);
    return out;
  }, [nodes, expanded, summary.bytes]);

  const parentRef = useRef<HTMLDivElement>(null);
  const virt = useVirtualizer({ count: rows.length, getScrollElement: () => parentRef.current, estimateSize: () => 30, overscan: 20 });

  return (
    <Card className="overflow-hidden">
      <div className="flex items-center gap-2 border-b px-3 py-2 text-xs font-medium text-muted-foreground">
        <FolderTree className="size-3.5" /> Pastas (maiores primeiro)
      </div>
      <div ref={parentRef} className="h-[calc(100vh-380px)] min-h-[320px] overflow-auto">
        <div style={{ height: virt.getTotalSize(), position: "relative", minWidth: "100%" }}>
          {virt.getVirtualItems().map((vi) => {
            const r = rows[vi.index];
            const style = { top: vi.start, height: vi.size } as const;
            if (r.kind === "loading")
              return (
                <div key={vi.key} className="absolute inset-x-0 flex items-center gap-2 text-xs text-muted-foreground" style={{ ...style, paddingLeft: 12 + r.depth * 14 }}>
                  <Loader2 className="size-3.5 animate-spin" /> carregando…
                </div>
              );
            if (r.kind === "more")
              return (
                <button
                  key={vi.key}
                  className="absolute inset-x-0 cursor-default text-left text-xs text-primary hover:underline"
                  style={{ ...style, paddingLeft: 30 + (r.parent.depth + 1) * 14 }}
                  onClick={() => load(r.parent.id, r.parent.children?.length ?? 0)}
                >
                  carregar mais pastas…
                </button>
              );
            const n = r.node;
            const pct = r.parentSize > 0 ? n.size / r.parentSize : 0;
            const canExpand = n.id === 1 || n.dirs > 0;
            return (
              <div
                key={vi.key}
                className={cn("absolute inset-x-0 flex cursor-default items-center gap-1 pr-2 text-sm hover:bg-muted/50", selected === n.id && "bg-accent text-accent-foreground")}
                style={{ ...style, paddingLeft: 6 + n.depth * 14 }}
                onClick={() => onSelect(n.id)}
              >
                <button
                  className={cn("grid size-5 shrink-0 place-items-center rounded text-muted-foreground hover:bg-muted", !canExpand && "invisible")}
                  aria-label={expanded.has(n.id) ? "Recolher" : "Expandir"}
                  onClick={(e) => {
                    e.stopPropagation();
                    toggle(n.id);
                  }}
                >
                  {n.loading ? <Loader2 className="size-3 animate-spin" /> : <ChevronRight className={cn("size-3.5 transition-transform", expanded.has(n.id) && "rotate-90")} />}
                </button>
                {expanded.has(n.id) ? <FolderOpen className="size-4 shrink-0 text-primary" /> : <Folder className="size-4 shrink-0 text-primary/80" />}
                <span className="min-w-0 flex-1 truncate" title={n.name}>
                  {n.name}
                </span>
                {n.flags & F_ENUM_ERROR ? <TriangleAlert className="size-3.5 shrink-0 text-warning" /> : null}
                <span className="w-16 shrink-0 text-right text-xs tabular-nums text-muted-foreground">{formatBytes(n.size)}</span>
                <span className="h-1.5 w-10 shrink-0 overflow-hidden rounded-full bg-muted">
                  <span className="block h-full rounded-full bg-primary/70" style={{ width: `${Math.min(100, pct * 100)}%` }} />
                </span>
              </div>
            );
          })}
        </div>
        {rows.some((r) => r.kind === "node" && r.node.error) && <p className="px-3 py-2 text-xs text-destructive">{[...nodes.values()].find((n) => n.error)?.error}</p>}
      </div>
    </Card>
  );
}

/** Paginated listing of one directory (folders and files, largest first). */
function ListPane({ path, dirId, onOpenDir }: { path: string; dirId: number; onOpenDir: (id: number) => void }) {
  const [page, setPage] = useState(0);
  const [data, setData] = useState<Paged<DirRow> | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [loading, setLoading] = useState(false);
  const latest = useRef("");
  const info = useAsync(() => api.snapshotDirInfo(path, dirId), [path, dirId]);

  useEffect(() => setPage(0), [dirId]);
  useEffect(() => {
    const rid = nextRequestId("list");
    latest.current = rid;
    setLoading(true);
    api
      .snapshotListDir(path, dirId, page * LIST_PAGE, LIST_PAGE, rid)
      .then((d) => {
        if (d.request_id !== latest.current) return; // stale response
        setData(d);
        setError(null);
      })
      .catch((e) => latest.current === rid && setError(e instanceof ApiError ? e : new ApiError("internal", String(e))))
      .finally(() => latest.current === rid && setLoading(false));
  }, [path, dirId, page]);

  const total = info.data ? info.data.subdirs + info.data.files : null;
  const dirSize = data?.rows.reduce((a, r) => a + r.size, 0) ?? 0;
  const chain = info.data?.chain ?? [];

  return (
    <Card className="flex min-h-0 flex-col overflow-hidden">
      <div className="flex flex-wrap items-center gap-1 border-b px-3 py-2 text-sm">
        {chain.map(([id, name], i) => (
          <span key={id} className="flex min-w-0 items-center gap-1">
            {i > 0 && <ChevronRight className="size-3.5 shrink-0 text-muted-foreground" />}
            <button className={cn("max-w-[16rem] cursor-default truncate rounded px-1 hover:bg-muted", i === chain.length - 1 && "font-semibold")} title={name} onClick={() => onOpenDir(id)}>
              {name}
            </button>
          </span>
        ))}
        {loading && <Loader2 className="ml-2 size-3.5 animate-spin text-muted-foreground" />}
      </div>
      {error ? (
        <ErrorNote error={error} />
      ) : !data ? (
        <Loading />
      ) : data.rows.length === 0 ? (
        <p className="px-4 py-8 text-center text-sm text-muted-foreground">Pasta vazia.</p>
      ) : (
        <div className="h-[calc(100vh-430px)] min-h-[280px] overflow-auto">
          <table className="w-full table-fixed text-sm">
            <colgroup>
              <col />
              <col className="w-28" />
              <col className="w-24" />
              <col className="w-28" />
              <col className="w-40" />
            </colgroup>
            <thead className="sticky top-0 z-10 bg-card text-xs text-muted-foreground shadow-[0_1px_0_var(--border)]">
              <tr className="h-9">
                <th className="px-3 text-left font-medium">Nome</th>
                <th className="px-3 text-right font-medium">Tamanho</th>
                <th className="px-3 text-right font-medium">% da página</th>
                <th className="px-3 text-right font-medium">Arquivos</th>
                <th className="px-3 text-right font-medium">Modificado</th>
              </tr>
            </thead>
            <tbody>
              {data.rows.map((r) => (
                <tr
                  key={(r.is_dir ? "d" : "f") + r.id}
                  className={cn("h-[var(--row-h)] border-t hover:bg-muted/40", r.is_dir && !(r.flags & F_NOT_FOLLOWED) && "cursor-pointer")}
                  onDoubleClick={() => r.is_dir && !(r.flags & F_NOT_FOLLOWED) && onOpenDir(r.id)}
                  onClick={() => r.is_dir && !(r.flags & F_NOT_FOLLOWED) && onOpenDir(r.id)}
                >
                  <td className="truncate px-3" title={r.name}>
                    <span className="flex items-center gap-2">
                      {r.is_dir ? <Folder className="size-4 shrink-0 text-primary/80" /> : <File className="size-4 shrink-0 text-muted-foreground" />}
                      <span className="truncate">{r.name}</span>
                      {r.flags & F_NOT_FOLLOWED ? <Badge>link não seguido</Badge> : null}
                      {r.flags & F_ENUM_ERROR ? <Badge tone="warning">leitura incompleta</Badge> : null}
                    </span>
                  </td>
                  <td className="px-3 text-right tabular-nums whitespace-nowrap">{formatBytes(r.size)}</td>
                  <td className="px-3 text-right text-xs tabular-nums text-muted-foreground">{dirSize ? ((r.size / dirSize) * 100).toLocaleString("pt-BR", { maximumFractionDigits: 1 }) + "%" : "—"}</td>
                  <td className="px-3 text-right tabular-nums text-muted-foreground">{r.is_dir ? formatCount(r.files) : ""}</td>
                  <td className="px-3 text-right whitespace-nowrap text-muted-foreground">{formatDate(r.mtime)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <Pager page={page} pageSize={LIST_PAGE} shown={data?.rows.length ?? 0} total={total} onPage={setPage} elapsed={data?.elapsed_ms} />
    </Card>
  );
}

function Pager({ page, pageSize, shown, total, onPage, elapsed }: { page: number; pageSize: number; shown: number; total: number | null; onPage: (p: number) => void; elapsed?: number }) {
  const from = page * pageSize + (shown ? 1 : 0);
  const to = page * pageSize + shown;
  const hasNext = total != null ? to < total : shown === pageSize;
  return (
    <div className="flex items-center justify-between gap-2 border-t px-3 py-1.5 text-xs text-muted-foreground">
      <span>
        {formatCount(from)}–{formatCount(to)}
        {total != null && ` de ${formatCount(total)}`}
        {elapsed != null && ` · ${elapsed} ms`}
      </span>
      <div className="flex gap-1">
        <Button size="sm" variant="ghost" disabled={page === 0} onClick={() => onPage(page - 1)}>
          <ChevronLeft /> Anterior
        </Button>
        <Button size="sm" variant="ghost" disabled={!hasNext} onClick={() => onPage(page + 1)}>
          Próxima <ChevronRight />
        </Button>
      </div>
    </div>
  );
}

/* --------------------------------------------------------------- filters */

const UNITS = [
  { v: "kb", label: "KB", pow: 1 },
  { v: "mb", label: "MB", pow: 2 },
  { v: "gb", label: "GB", pow: 3 },
  { v: "tb", label: "TB", pow: 4 },
];
const QUERY_PAGE = 100;

function FiltersPane({ path, onOpenDir }: { path: string; onOpenDir: (id: number) => void }) {
  const { settings } = useSettings();
  const base = settings.units === "decimal" ? 1000 : 1024;
  const [ext, setExt] = useState("");
  const [name, setName] = useState("");
  const [minV, setMinV] = useState("");
  const [minU, setMinU] = useState("gb");
  const [maxV, setMaxV] = useState("");
  const [maxU, setMaxU] = useState("gb");
  const [before, setBefore] = useState("");
  const [after, setAfter] = useState("");
  const [year, setYear] = useState("");
  const [page, setPage] = useState(0);
  const [res, setRes] = useState<Paged<FileRow> | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [running, setRunning] = useState<string | null>(null);
  const latest = useRef("");

  const bytes = (v: string, u: string) => (v.trim() ? Math.round(Number(v.replace(",", ".")) * base ** (UNITS.find((x) => x.v === u)?.pow ?? 3)) : null);
  const date = (d: string) => (d ? Math.floor(new Date(d + "T00:00:00").getTime() / 1000) : null);

  const run = async (p: number) => {
    const q: FileQuery = {
      ext: ext.trim() || null,
      name_contains: name.trim() || null,
      min_size: bytes(minV, minU),
      max_size: bytes(maxV, maxU),
      modified_before: date(before),
      modified_after: date(after),
      year: year ? Number(year) : null,
      offset: p * QUERY_PAGE,
      limit: QUERY_PAGE,
    };
    if (running) await api.snapshotCancelQuery(running).catch(() => {});
    const rid = nextRequestId("query");
    latest.current = rid;
    setRunning(rid);
    setPage(p);
    setError(null);
    try {
      const r = await api.snapshotQuery(path, q, rid);
      if (r.request_id === latest.current) setRes(r);
    } catch (e) {
      if (latest.current === rid) setError(e instanceof ApiError ? e : new ApiError("internal", String(e)));
    } finally {
      if (latest.current === rid) setRunning(null);
    }
  };

  const cancel = async () => {
    if (running) await api.snapshotCancelQuery(running).catch(() => {});
  };

  const errMsg = error
    ? error.code === "time_limit"
      ? "A consulta excedeu o tempo limite. Combine mais filtros (por exemplo, extensão + tamanho) para usar os índices."
      : error.code === "cancelled"
        ? "Consulta cancelada."
        : error.message
    : null;

  return (
    <div className="grid gap-4 [&>*]:min-w-0 lg:grid-cols-[minmax(280px,340px)_1fr]">
      <Card className="self-start p-[var(--card-pad)]">
        <form
          className="grid gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            run(0);
          }}
        >
          <Field label="Extensão" hint="Ex.: bak, mp4, pst">
            <Input value={ext} onChange={(e) => setExt(e.target.value)} placeholder="qualquer" />
          </Field>
          <Field label="Nome contém">
            <Input value={name} onChange={(e) => setName(e.target.value)} placeholder="qualquer" />
          </Field>
          <Field label="Tamanho mínimo">
            <div className="flex gap-2">
              <Input inputMode="decimal" value={minV} onChange={(e) => setMinV(e.target.value)} placeholder="—" />
              <Select value={minU} onChange={(e) => setMinU(e.target.value)} className="w-20">
                {UNITS.map((u) => (
                  <option key={u.v} value={u.v}>
                    {u.label}
                  </option>
                ))}
              </Select>
            </div>
          </Field>
          <Field label="Tamanho máximo">
            <div className="flex gap-2">
              <Input inputMode="decimal" value={maxV} onChange={(e) => setMaxV(e.target.value)} placeholder="—" />
              <Select value={maxU} onChange={(e) => setMaxU(e.target.value)} className="w-20">
                {UNITS.map((u) => (
                  <option key={u.v} value={u.v}>
                    {u.label}
                  </option>
                ))}
              </Select>
            </div>
          </Field>
          <div className="grid grid-cols-2 gap-2">
            <Field label="Modificado depois de">
              <Input type="date" value={after} onChange={(e) => setAfter(e.target.value)} />
            </Field>
            <Field label="Modificado antes de">
              <Input type="date" value={before} onChange={(e) => setBefore(e.target.value)} />
            </Field>
          </div>
          <Field label="Ano de modificação">
            <Input inputMode="numeric" value={year} onChange={(e) => setYear(e.target.value.replace(/\D/g, "").slice(0, 4))} placeholder="ex.: 2019" />
          </Field>
          <div className="flex gap-2">
            {running ? (
              <Button type="button" variant="outline" className="flex-1" onClick={cancel}>
                <Square /> Cancelar
              </Button>
            ) : (
              <Button type="submit" className="flex-1">
                <Search /> Buscar
              </Button>
            )}
          </div>
          <p className="text-xs text-muted-foreground">Resultados ordenados do maior para o menor, 100 por página. Tamanhos em {settings.units === "decimal" ? "base 1000" : "base 1024"}.</p>
        </form>
      </Card>
      <Card className="flex min-h-0 flex-col overflow-hidden">
        {running && (
          <div className="flex items-center gap-2 border-b px-3 py-2 text-sm text-muted-foreground">
            <Loader2 className="size-4 animate-spin" /> Consultando…
          </div>
        )}
        {errMsg && <p className="border-b px-3 py-2 text-sm text-destructive">{errMsg}</p>}
        {!res && !running && !errMsg && <p className="px-4 py-10 text-center text-sm text-muted-foreground">Defina os filtros e clique em Buscar.</p>}
        {res && (
          <>
            <div className="h-[calc(100vh-380px)] min-h-[300px] overflow-auto">
              <table className="w-full table-fixed text-sm">
                <colgroup>
                  <col />
                  <col className="w-28" />
                  <col className="w-40" />
                  <col className="w-12" />
                </colgroup>
                <thead className="sticky top-0 z-10 bg-card text-xs text-muted-foreground shadow-[0_1px_0_var(--border)]">
                  <tr className="h-9">
                    <th className="px-3 text-left font-medium">Arquivo</th>
                    <th className="px-3 text-right font-medium">Tamanho</th>
                    <th className="px-3 text-right font-medium">Modificado</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {res.rows.map((r) => (
                    <tr key={r.id} className="h-[var(--row-h)] border-t hover:bg-muted/40">
                      <td className="truncate px-3" title={r.path}>
                        <FileName path={r.path} />
                      </td>
                      <td className="px-3 text-right tabular-nums whitespace-nowrap">{formatBytes(r.size)}</td>
                      <td className="px-3 text-right whitespace-nowrap text-muted-foreground">{formatDate(r.mtime)}</td>
                      <td className="pr-2 text-right">
                        <Tooltip content="Abrir a pasta no explorador">
                          <Button size="icon" variant="ghost" aria-label="Abrir pasta" onClick={() => onOpenDir(r.parent_id)}>
                            <FolderOpen />
                          </Button>
                        </Tooltip>
                      </td>
                    </tr>
                  ))}
                  {res.rows.length === 0 && (
                    <tr>
                      <td colSpan={4} className="px-4 py-10 text-center text-muted-foreground">
                        Nenhum arquivo corresponde aos filtros.
                      </td>
                    </tr>
                  )}
                </tbody>
              </table>
            </div>
            <Pager page={page} pageSize={QUERY_PAGE} shown={res.rows.length} total={null} onPage={(p) => run(p)} elapsed={res.elapsed_ms} />
          </>
        )}
      </Card>
    </div>
  );
}

/** File name first (always visible), folder after it, muted. */
function FileName({ path }: { path: string }) {
  const i = path.lastIndexOf("\\");
  const name = i >= 0 ? path.slice(i + 1) : path;
  const dir = i >= 0 ? path.slice(0, i) : "";
  return (
    <span className="flex min-w-0 items-baseline gap-2">
      <span className="shrink-0 font-medium">{name}</span>
      <span className="truncate font-mono text-[11.5px] text-muted-foreground">{dir}</span>
    </span>
  );
}

/* -------------------------------------------------------------- rankings */

function Rankings({ path, total, onOpenDir, finalized }: { path: string; total: number; onOpenDir: (id: number) => void; finalized: boolean }) {
  return (
    <div className="grid gap-4 [&>*]:min-w-0 xl:grid-cols-2">
      <Card>
        <h3 className="px-[var(--card-pad)] pt-[var(--card-pad)] pb-3 text-sm font-semibold">Maiores arquivos</h3>
        <TopFiles path={path} onOpenDir={onOpenDir} />
      </Card>
      <Card>
        <h3 className="px-[var(--card-pad)] pt-[var(--card-pad)] pb-3 text-sm font-semibold">
          Maiores pastas {!finalized && <span className="font-normal text-muted-foreground">(conteúdo direto — dados parciais)</span>}
        </h3>
        <TopDirs path={path} onOpenDir={onOpenDir} />
      </Card>
      <Card>
        <h3 className="px-[var(--card-pad)] pt-[var(--card-pad)] pb-3 text-sm font-semibold">Por extensão</h3>
        {finalized ? <StatsTable path={path} kind="ext" total={total} /> : <p className="px-[var(--card-pad)] pb-[var(--card-pad)] text-sm text-muted-foreground">Disponível após a finalização da análise.</p>}
      </Card>
      <Card>
        <h3 className="px-[var(--card-pad)] pt-[var(--card-pad)] pb-3 text-sm font-semibold">Por ano de modificação</h3>
        {finalized ? <StatsTable path={path} kind="years" total={total} /> : <p className="px-[var(--card-pad)] pb-[var(--card-pad)] text-sm text-muted-foreground">Disponível após a finalização da análise.</p>}
      </Card>
    </div>
  );
}

function Table({ head, children }: { head: ReactNode[]; children: ReactNode }) {
  return (
    <div className="overflow-hidden border-t">
      <table className="w-full table-fixed text-sm">
        <colgroup>
          {head.map((_, i) => (
            <col key={i} className={i === 0 ? "" : i === head.length - 1 && head.length === 4 ? "w-40" : "w-28"} />
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

function TopFiles({ path, onOpenDir }: { path: string; onOpenDir: (id: number) => void }) {
  const { data, error } = useAsync(() => api.snapshotTopFiles(path, 50), [path]);
  if (error) return <ErrorNote error={error} />;
  if (!data) return <Loading />;
  return (
    <Table head={["Arquivo", "Tamanho", "Modificado"]}>
      {data.map((r) => (
        <tr key={r.id} className="cursor-pointer" onClick={() => onOpenDir(r.parent_id)}>
          <td className="truncate px-3" title={r.path}>
            <FileName path={r.path} />
          </td>
          <td className="px-3 text-right whitespace-nowrap tabular-nums">{formatBytes(r.size)}</td>
          <td className="px-3 text-right whitespace-nowrap text-muted-foreground">{formatDate(r.mtime)}</td>
        </tr>
      ))}
    </Table>
  );
}

function TopDirs({ path, onOpenDir }: { path: string; onOpenDir: (id: number) => void }) {
  const { data, error } = useAsync(() => api.snapshotTopDirs(path, 50), [path]);
  if (error) return <ErrorNote error={error} />;
  if (!data) return <Loading />;
  return (
    <Table head={["Pasta", "Tamanho", "Arquivos"]}>
      {data.map((r) => (
        <tr key={r.id} className="cursor-pointer" onClick={() => onOpenDir(r.id)}>
          <td className="truncate px-3 font-mono text-[12.5px]" title={r.path ?? r.name}>
            {r.path ?? r.name}
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
  const rows =
    kind === "ext"
      ? data.extensions.map((e) => ({ k: e.ext === "(sem extensão)" ? e.ext : `.${e.ext}`, files: e.files, size: e.size }))
      : data.years.map((y) => ({ k: y.year == null ? "(sem data)" : String(y.year), files: y.files, size: y.size })).reverse();
  return (
    <Table head={[kind === "ext" ? "Extensão" : "Ano", "Arquivos", "Tamanho", "Participação"]}>
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
