import { useEffect, useState } from "react";
import { Toaster, toast } from "sonner";
import { api, ApiError, on } from "@/lib/api";
import { LiveProvider } from "@/lib/live";
import { SettingsProvider, useSettings } from "@/lib/settings";
import { AppShell, ErrorBoundary, useRoute } from "@/components/shell";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, TooltipProvider } from "@/components/ui/overlay";
import HomePage from "@/pages/Home";
import NextSizePage from "@/pages/NextSize";
import NextCopyPage from "@/pages/NextCopy";
import HistoryPage from "@/pages/History";
import AlertsPage from "@/pages/Alerts";
import SettingsPage from "@/pages/Settings";

/** Closing with running tasks: confirm, then pause them safely and quit. */
function CloseGuard() {
  const [running, setRunning] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    const u = on<number>("app://close-requested", (n) => setRunning(n));
    return () => {
      u.then((f) => f());
    };
  }, []);
  return (
    <Dialog open={running !== null} onOpenChange={(o) => !o && setRunning(null)}>
      {running !== null && (
        <DialogContent title="Sair do NextTool?" description={`Há ${running} ${running === 1 ? "tarefa" : "tarefas"} em execução.`}>
          <p className="text-sm">
            Ao sair, as tarefas serão <strong>pausadas com segurança</strong> e poderão ser retomadas depois em Histórico. Nada será marcado como concluído.
          </p>
          <div className="mt-5 flex justify-end gap-2">
            <Button variant="outline" onClick={() => setRunning(null)} disabled={busy}>
              Continuar executando
            </Button>
            <Button
              variant="destructive"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                try {
                  await api.appQuit();
                } catch (e) {
                  setBusy(false);
                  toast.error("Falha ao sair", { description: e instanceof ApiError ? e.message : String(e) });
                }
              }}
            >
              {busy ? "Pausando tarefas…" : "Pausar tarefas e sair"}
            </Button>
          </div>
        </DialogContent>
      )}
    </Dialog>
  );
}

function Routed() {
  const [route, go] = useRoute();
  const { resolvedDark } = useSettings();
  const page = {
    home: <HomePage go={go} />,
    nextsize: <NextSizePage />,
    nextcopy: <NextCopyPage />,
    history: <HistoryPage />,
    alerts: <AlertsPage />,
    settings: <SettingsPage />,
  }[route];
  return (
    <>
      <AppShell route={route} go={go}>
        <ErrorBoundary key={route}>{page}</ErrorBoundary>
      </AppShell>
      <CloseGuard />
      <Toaster theme={resolvedDark ? "dark" : "light"} position="bottom-right" richColors closeButton />
    </>
  );
}

export default function App() {
  return (
    <ErrorBoundary>
      <SettingsProvider>
        <LiveProvider>
          <TooltipProvider>
            <Routed />
          </TooltipProvider>
        </LiveProvider>
      </SettingsProvider>
    </ErrorBoundary>
  );
}
