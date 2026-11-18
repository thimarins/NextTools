import { Component, useEffect, useState, type ErrorInfo, type ReactNode } from "react";
import { Bell, Copy, FolderOpen, HardDrive, History, Home, Monitor, Moon, Settings as SettingsIcon, Sun, TriangleAlert } from "lucide-react";
import { isDesktop, pickPath } from "@/lib/api";
import { useLive } from "@/lib/live";
import { useSettings } from "@/lib/settings";
import { cn } from "@/lib/utils";
import { Button } from "./ui/button";
import { Input } from "./ui/primitives";
import { Tooltip } from "./ui/overlay";

export type Route = "home" | "nextsize" | "nextcopy" | "history" | "alerts" | "settings";

export const ROUTES: { id: Route; label: string; icon: ReactNode }[] = [
  { id: "home", label: "Início", icon: <Home /> },
  { id: "nextsize", label: "NextSize", icon: <HardDrive /> },
  { id: "nextcopy", label: "NextCopy", icon: <Copy /> },
  { id: "history", label: "Histórico", icon: <History /> },
  { id: "alerts", label: "Alertas", icon: <Bell /> },
  { id: "settings", label: "Configurações", icon: <SettingsIcon /> },
];

export function useRoute(): [Route, (r: Route) => void] {
  const parse = (): Route => {
    const h = window.location.hash.replace(/^#\/?/, "") as Route;
    return ROUTES.some((r) => r.id === h) ? h : "home";
  };
  const [route, setRoute] = useState<Route>(parse);
  useEffect(() => {
    const h = () => setRoute(parse());
    window.addEventListener("hashchange", h);
    return () => window.removeEventListener("hashchange", h);
  }, []);
  return [route, (r) => (window.location.hash = `/${r}`)];
}

function Logo() {
  return (
    <div className="flex items-center gap-2.5">
      <div className="grid size-7 place-items-center rounded-lg bg-gradient-to-br from-primary to-[oklch(0.55_0.2_calc(var(--hue)+40))] shadow-sm">
        <div className="grid w-3.5 gap-[3px]">
          <span className="h-[3px] rounded-full bg-white" />
          <span className="h-[3px] w-2.5 rounded-full bg-white" />
          <span className="h-[3px] w-1.5 rounded-full bg-white/60" />
        </div>
      </div>
      <span className="text-[15px] font-semibold tracking-tight">NextTool</span>
    </div>
  );
}

function ThemeToggle() {
  const { settings, update } = useSettings();
  const next = settings.theme === "light" ? "dark" : settings.theme === "dark" ? "system" : "light";
  const icon = settings.theme === "light" ? <Sun /> : settings.theme === "dark" ? <Moon /> : <Monitor />;
  const label = settings.theme === "light" ? "Tema claro" : settings.theme === "dark" ? "Tema escuro" : "Tema do sistema";
  return (
    <Tooltip content={`${label} — clique para alternar`} side="bottom">
      <Button variant="ghost" size="icon" onClick={() => update("theme", next)} aria-label={label}>
        {icon}
      </Button>
    </Tooltip>
  );
}

export function AppShell({ route, go, children }: { route: Route; go: (r: Route) => void; children: ReactNode }) {
  const { openAlerts } = useLive();
  return (
    <div className="flex h-full flex-col">
      <header className="sticky top-0 z-30 border-b bg-card/85 backdrop-blur-md">
        <div className="mx-auto flex h-14 max-w-[1400px] items-center gap-6 px-5">
          <Logo />
          <nav className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto" aria-label="Principal">
            {ROUTES.map((r) => (
              <button
                key={r.id}
                onClick={() => go(r.id)}
                aria-current={route === r.id ? "page" : undefined}
                className={cn(
                  "relative inline-flex h-9 cursor-default items-center gap-2 rounded-md px-3 text-sm font-medium whitespace-nowrap text-muted-foreground transition-colors hover:bg-accent hover:text-accent-foreground [&_svg]:size-4",
                  route === r.id && "bg-accent text-accent-foreground",
                )}
              >
                {r.icon}
                {r.label}
                {r.id === "alerts" && openAlerts > 0 && (
                  <span className="ml-0.5 rounded-full bg-destructive px-1.5 text-[11px] leading-[18px] font-semibold text-white">{openAlerts > 99 ? "99+" : openAlerts}</span>
                )}
              </button>
            ))}
          </nav>
          <ThemeToggle />
        </div>
      </header>
      {!isDesktop && (
        <div className="border-b border-warning/30 bg-warning/12 px-5 py-2 text-center text-sm">
          <TriangleAlert className="mr-1.5 inline size-4 align-[-3px] text-warning" />
          Pré-visualização no navegador: os motores só estão disponíveis no aplicativo desktop. Nenhum dado é simulado.
        </div>
      )}
      <main className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto max-w-[1400px] px-5 py-7">{children}</div>
      </main>
    </div>
  );
}

export class ErrorBoundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("UI error", error, info.componentStack);
  }
  render() {
    if (!this.state.error) return this.props.children;
    return (
      <div className="mx-auto mt-16 max-w-lg rounded-xl border border-destructive/30 bg-card p-6">
        <p className="font-semibold text-destructive">Ocorreu um erro na interface</p>
        <p className="mt-2 text-sm text-muted-foreground">As tarefas em execução não foram afetadas: elas rodam no motor, fora da interface.</p>
        <pre className="mt-3 max-h-40 overflow-auto rounded-md bg-muted p-3 text-xs">{this.state.error.message}</pre>
        <Button className="mt-4" onClick={() => this.setState({ error: null })}>
          Tentar novamente
        </Button>
      </div>
    );
  }
}

export function PathInput({ value, onChange, placeholder, id, file }: { value: string; onChange: (v: string) => void; placeholder?: string; id?: string; file?: { name: string; extensions: string[] } }) {
  return (
    <div className="flex gap-2">
      <Input id={id} value={value} onChange={(e) => onChange(e.target.value)} placeholder={placeholder} spellCheck={false} className="font-mono text-[13px]" />
      <Tooltip content={file ? "Escolher arquivo" : "Escolher pasta"}>
        <Button
          variant="outline"
          size="icon"
          disabled={!isDesktop}
          aria-label="Procurar"
          onClick={async () => {
            const p = await pickPath({ directory: !file, filters: file ? [file] : undefined });
            if (p) onChange(p);
          }}
        >
          <FolderOpen />
        </Button>
      </Tooltip>
    </div>
  );
}
