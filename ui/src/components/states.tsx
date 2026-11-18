import { Loader2, MonitorX, TriangleAlert } from "lucide-react";
import type { ApiError } from "@/lib/api";
import { Button } from "./ui/button";

export function ErrorNote({ error, onRetry }: { error: ApiError; onRetry?: () => void }) {
  if (error.code === "desktop_only") {
    return (
      <div className="flex items-center gap-2 px-[var(--card-pad)] py-6 text-sm text-muted-foreground">
        <MonitorX className="size-4" /> Disponível apenas no aplicativo desktop.
      </div>
    );
  }
  return (
    <div className="flex items-start gap-3 px-[var(--card-pad)] py-4 text-sm">
      <TriangleAlert className="mt-0.5 size-4 shrink-0 text-destructive" />
      <div className="min-w-0 flex-1">
        <p className="font-medium text-destructive">Não foi possível carregar</p>
        <p className="mt-0.5 break-words text-muted-foreground">{error.message}</p>
      </div>
      {onRetry && (
        <Button size="sm" variant="outline" onClick={onRetry}>
          Tentar novamente
        </Button>
      )}
    </div>
  );
}

export function Loading({ label = "Carregando…" }: { label?: string }) {
  return (
    <div className="flex items-center gap-2 px-[var(--card-pad)] py-6 text-sm text-muted-foreground">
      <Loader2 className="size-4 animate-spin" /> {label}
    </div>
  );
}
