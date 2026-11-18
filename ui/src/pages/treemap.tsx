import { useEffect, useMemo, useRef, useState } from "react";
import { api, ApiError, nextRequestId } from "@/lib/api";
import type { DirRow } from "@/lib/types";
import { formatBytes } from "@/lib/utils";
import { useSettings } from "@/lib/settings";
import { ErrorNote, Loading } from "@/components/states";

/** Max rectangles drawn per level; the rest is aggregated in "Outros". */
const MAX_ITEMS = 300;

type Item = { id: number; name: string; size: number; isDir: boolean; other?: boolean };
type Rect = Item & { x: number; y: number; w: number; h: number };

/** Squarified treemap layout (Bruls, Huizing, van Wijk). */
function squarify(items: Item[], x: number, y: number, w: number, h: number): Rect[] {
  const out: Rect[] = [];
  const total = items.reduce((a, i) => a + i.size, 0);
  if (total <= 0 || w <= 0 || h <= 0) return out;
  const scale = (w * h) / total;
  const areas = items.map((i) => ({ item: i, a: i.size * scale })).filter((v) => v.a > 0);
  let rx = x, ry = y, rw = w, rh = h;
  let row: { item: Item; a: number }[] = [];
  const worst = (r: { a: number }[], side: number) => {
    const s = r.reduce((t, v) => t + v.a, 0);
    const mx = Math.max(...r.map((v) => v.a));
    const mn = Math.min(...r.map((v) => v.a));
    return Math.max((side * side * mx) / (s * s), (s * s) / (side * side * mn));
  };
  const layout = (r: { item: Item; a: number }[]) => {
    const s = r.reduce((t, v) => t + v.a, 0);
    if (rw >= rh) {
      const cw = s / rh;
      let cy = ry;
      for (const v of r) {
        const ch = v.a / cw;
        out.push({ ...v.item, x: rx, y: cy, w: cw, h: ch });
        cy += ch;
      }
      rx += cw;
      rw -= cw;
    } else {
      const ch = s / rw;
      let cx = rx;
      for (const v of r) {
        const cw = v.a / ch;
        out.push({ ...v.item, x: cx, y: ry, w: cw, h: ch });
        cx += cw;
      }
      ry += ch;
      rh -= ch;
    }
  };
  for (const v of areas) {
    const side = Math.min(rw, rh);
    if (row.length === 0 || worst([...row, v], side) <= worst(row, side)) {
      row.push(v);
    } else {
      layout(row);
      row = [v];
    }
  }
  if (row.length) layout(row);
  return out;
}

function extHue(name: string) {
  const e = name.includes(".") ? name.slice(name.lastIndexOf(".") + 1).toLowerCase() : "";
  let h = 0;
  for (const c of e) h = (h * 31 + c.charCodeAt(0)) % 360;
  return h;
}

export function Treemap({ path, dirId, onOpenDir }: { path: string; dirId: number; onOpenDir: (id: number) => void }) {
  const { resolvedDark, settings } = useSettings();
  const wrap = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ w: 800, h: 420 });
  const [rows, setRows] = useState<DirRow[] | null>(null);
  const [total, setTotal] = useState(0);
  const [error, setError] = useState<ApiError | null>(null);
  const [hover, setHover] = useState<Rect | null>(null);
  const latest = useRef("");

  useEffect(() => {
    const rid = nextRequestId("treemap");
    latest.current = rid;
    setRows(null);
    Promise.all([api.snapshotListDir(path, dirId, 0, MAX_ITEMS, rid), api.snapshotDirInfo(path, dirId)])
      .then(([page, info]) => {
        if (latest.current !== rid) return;
        setRows(page.rows);
        setTotal(info.size);
        setError(null);
      })
      .catch((e) => latest.current === rid && setError(e instanceof ApiError ? e : new ApiError("internal", String(e))));
  }, [path, dirId]);

  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(([e]) => setSize({ w: Math.max(200, e.contentRect.width), h: Math.max(240, e.contentRect.height) }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const rects = useMemo(() => {
    if (!rows) return [];
    const items: Item[] = rows.filter((r) => r.size > 0).map((r) => ({ id: r.id, name: r.name, size: r.size, isDir: r.is_dir }));
    const shown = items.reduce((a, i) => a + i.size, 0);
    if (total > shown) items.push({ id: -1, name: "Outros (itens menores)", size: total - shown, isDir: false, other: true });
    return squarify(items, 0, 0, size.w, size.h);
  }, [rows, total, size]);

  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = size.w * dpr;
    c.height = size.h * dpr;
    const g = c.getContext("2d");
    if (!g) return;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, size.w, size.h);
    const border = resolvedDark ? "rgba(0,0,0,0.55)" : "rgba(255,255,255,0.9)";
    for (const r of rects) {
      const l = resolvedDark ? 0.42 : 0.72;
      g.fillStyle = r.other
        ? resolvedDark ? "oklch(0.32 0.01 260)" : "oklch(0.88 0.01 260)"
        : r.isDir
          ? `oklch(${l} 0.12 ${settings.accent})`
          : `oklch(${l + 0.04} 0.08 ${extHue(r.name)})`;
      g.fillRect(r.x, r.y, r.w, r.h);
      g.strokeStyle = border;
      g.lineWidth = 1;
      g.strokeRect(r.x + 0.5, r.y + 0.5, r.w - 1, r.h - 1);
      if (r.w > 60 && r.h > 22) {
        g.fillStyle = resolvedDark ? "rgba(255,255,255,0.92)" : "rgba(15,23,42,0.88)";
        g.font = `${r.isDir ? 600 : 400} 12px "Segoe UI", system-ui, sans-serif`;
        const label = r.name.length * 6.5 > r.w - 10 ? r.name.slice(0, Math.max(1, Math.floor((r.w - 16) / 6.5))) + "…" : r.name;
        g.fillText(label, r.x + 6, r.y + 15);
        if (r.h > 38) {
          g.font = '11px "Segoe UI", system-ui, sans-serif';
          g.fillStyle = resolvedDark ? "rgba(255,255,255,0.7)" : "rgba(15,23,42,0.6)";
          g.fillText(formatBytes(r.size), r.x + 6, r.y + 30);
        }
      }
    }
  }, [rects, size, resolvedDark, settings.accent]);

  const at = (e: React.MouseEvent) => {
    const b = canvas.current!.getBoundingClientRect();
    const x = e.clientX - b.left, y = e.clientY - b.top;
    return rects.find((r) => x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h) ?? null;
  };

  if (error) return <ErrorNote error={error} />;
  return (
    <div className="relative">
      <div ref={wrap} className="h-[calc(100vh-470px)] min-h-[300px] w-full">
        {!rows ? (
          <Loading />
        ) : (
          <canvas
            ref={canvas}
            style={{ width: size.w, height: size.h }}
            className={hover?.isDir ? "cursor-pointer" : ""}
            onMouseMove={(e) => setHover(at(e))}
            onMouseLeave={() => setHover(null)}
            onClick={(e) => {
              const r = at(e);
              if (r?.isDir && !r.other) onOpenDir(r.id);
            }}
            role="img"
            aria-label="Treemap da pasta atual"
          />
        )}
      </div>
      {hover && (
        <div className="pointer-events-none absolute top-2 right-2 rounded-md bg-foreground px-2.5 py-1.5 text-xs text-background shadow">
          <strong>{hover.name}</strong> · {formatBytes(hover.size)}
          {total > 0 && ` · ${((hover.size / total) * 100).toLocaleString("pt-BR", { maximumFractionDigits: 1 })}%`}
          {hover.isDir && !hover.other && " · clique para entrar"}
        </div>
      )}
      <p className="px-3 py-1.5 text-xs text-muted-foreground">
        Até {MAX_ITEMS} maiores itens desta pasta; o restante aparece agregado em "Outros". Pastas na cor principal; arquivos coloridos por extensão.
      </p>
    </div>
  );
}
