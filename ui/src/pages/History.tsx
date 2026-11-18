import { useEffect, useMemo, useRef, useState } from "react";
import { createColumnHelper, flexRender, getCoreRowModel, getSortedRowModel, useReactTable, type SortingState } from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowDownUp, History as HistoryIcon, RefreshCw } from "lucide-react";
import { api, ApiError } from "@/lib/api";
import { useThrottledEffect } from "@/lib/hooks";
import { taskKindLabel } from "@/lib/i18n";
import { useLive } from "@/lib/live";
import type { TaskRecord, TaskState } from "@/lib/types";
import { formatDate, formatDuration } from "@/lib/utils";
import { ErrorNote, Loading } from "@/components/states";
import { KindIcon, TaskDetailDialog, TaskStateBadge } from "@/components/tasks";
import { Button } from "@/components/ui/button";
import { Card, Empty, PageHeader, Select } from "@/components/ui/primitives";

const PAGE = 500;

const groups: Record<string, TaskState[] | undefined> = {
  all: undefined,
  active: ["queued", "running", "cancelling", "waiting_retry"],
  attention: ["paused", "interrupted", "failed"],
  done: ["completed", "completed_with_warnings"],
  cancelled: ["cancelled"],
};

const col = createColumnHelper<TaskRecord>();

export default function HistoryPage() {
  const { tasks: live, version } = useLive();
  const [group, setGroup] = useState("all");
  const [kind, setKind] = useState("all");
  const [rows, setRows] = useState<TaskRecord[] | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState<ApiError | null>(null);
  const [sorting, setSorting] = useState<SortingState>([]);
  const [open, setOpen] = useState<TaskRecord | null>(null);
  const seq = useRef(0);

  const load = (append: boolean) => {
    const my = ++seq.current;
    const offset = append ? rows?.length ?? 0 : 0;
    api
      .tasksList(groups[group], PAGE, offset)
      .then((page) => {
        if (my !== seq.current) return; // stale response
        setRows((r) => (append && r ? [...r, ...page] : page));
        setHasMore(page.length === PAGE);
        setError(null);
      })
      .catch((e) => my === seq.current && setError(e instanceof ApiError ? e : new ApiError("internal", String(e))));
  };

  useEffect(() => {
    setRows(null);
    load(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [group]);
  useThrottledEffect(() => load(false), version, 2000);

  const data = useMemo(() => {
    const merged = (rows ?? []).map((r) => {
      const l = live.get(r.id);
      return l && l.updated_at >= r.updated_at ? l : r;
    });
    return kind === "all" ? merged : merged.filter((r) => r.kind === kind);
  }, [rows, live, kind]);

  const columns = useMemo(
    () => [
      col.accessor("id", { header: "#", size: 64, cell: (c) => <span className="tabular-nums text-muted-foreground">{c.getValue()}</span> }),
      col.accessor("kind", {
        header: "Tipo",
        size: 120,
        cell: (c) => (
          <span className="flex items-center gap-2">
            <KindIcon t={c.row.original} /> {taskKindLabel[c.getValue()]}
          </span>
        ),
      }),
      col.accessor("title", { header: "Tarefa", size: 520, cell: (c) => <span className="block truncate" title={c.getValue()}>{c.getValue()}</span> }),
      col.accessor("state", { header: "Estado", size: 200, cell: (c) => <TaskStateBadge state={c.getValue()} /> }),
      col.accessor((r) => r.started_at ?? r.created_at, { id: "start", header: "Início", size: 140, cell: (c) => <span className="text-muted-foreground">{formatDate(c.getValue())}</span> }),
      col.accessor((r) => (r.started_at && r.finished_at ? (r.finished_at - r.started_at) * 1000 : null), {
        id: "dur",
        header: "Duração",
        size: 110,
        cell: (c) => <span className="tabular-nums text-muted-foreground">{formatDuration(c.getValue())}</span>,
      }),
    ],
    [],
  );

  const table = useReactTable({ data, columns, state: { sorting }, onSortingChange: setSorting, getCoreRowModel: getCoreRowModel(), getSortedRowModel: getSortedRowModel() });
  const parent = useRef<HTMLDivElement>(null);
  const rowModel = table.getRowModel().rows;
  const virt = useVirtualizer({ count: rowModel.length, getScrollElement: () => parent.current, estimateSize: () => 44, overscan: 12 });
  const template = table.getVisibleLeafColumns().map((c) => (c.id === "title" ? "minmax(200px,1fr)" : `${c.getSize()}px`)).join(" ");

  return (
    <>
      <PageHeader
        title="Histórico"
        description="Todas as tarefas do NextSize e do NextCopy, deste computador."
        actions={
          <>
            <Select value={kind} onChange={(e) => setKind(e.target.value)} className="w-40">
              <option value="all">Todos os tipos</option>
              <option value="size_scan">Análises</option>
              <option value="copy">Cópias</option>
            </Select>
            <Select value={group} onChange={(e) => setGroup(e.target.value)} className="w-52">
              <option value="all">Todos os estados</option>
              <option value="active">Ativas</option>
              <option value="attention">Requerem atenção</option>
              <option value="done">Concluídas</option>
              <option value="cancelled">Canceladas</option>
            </Select>
            <Button variant="outline" size="icon" onClick={() => load(false)} aria-label="Atualizar">
              <RefreshCw />
            </Button>
          </>
        }
      />
      <Card className="overflow-hidden">
        {error ? (
          <ErrorNote error={error} onRetry={() => load(false)} />
        ) : !rows ? (
          <Loading />
        ) : data.length === 0 ? (
          <Empty icon={<HistoryIcon />} title="Nenhuma tarefa encontrada" />
        ) : (
          <>
            <div className="grid border-b bg-muted/60 text-xs font-medium text-muted-foreground" style={{ gridTemplateColumns: template }}>
              {table.getHeaderGroups()[0].headers.map((h) => (
                <button key={h.id} className="flex h-9 cursor-default items-center gap-1 px-3 text-left hover:text-foreground" onClick={h.column.getToggleSortingHandler()}>
                  {flexRender(h.column.columnDef.header, h.getContext())}
                  {h.column.getIsSorted() && <ArrowDownUp className="size-3" />}
                </button>
              ))}
            </div>
            <div ref={parent} className="max-h-[calc(100vh-260px)] overflow-y-auto">
              <div style={{ height: virt.getTotalSize(), position: "relative" }}>
                {virt.getVirtualItems().map((vi) => {
                  const row = rowModel[vi.index];
                  return (
                    <div
                      key={row.id}
                      onClick={() => setOpen(row.original)}
                      className="absolute inset-x-0 grid items-center border-b text-sm hover:bg-muted/40"
                      style={{ top: vi.start, height: vi.size, gridTemplateColumns: template }}
                    >
                      {row.getVisibleCells().map((c) => (
                        <div key={c.id} className="min-w-0 px-3">
                          {flexRender(c.column.columnDef.cell, c.getContext())}
                        </div>
                      ))}
                    </div>
                  );
                })}
              </div>
            </div>
            {hasMore && (
              <div className="border-t p-2 text-center">
                <Button variant="ghost" size="sm" onClick={() => load(true)}>
                  Carregar mais
                </Button>
              </div>
            )}
          </>
        )}
      </Card>
      <TaskDetailDialog task={open} onClose={() => setOpen(null)} />
    </>
  );
}
