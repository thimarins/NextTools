import { useMemo, useState } from "react";
import { toast } from "sonner";
import { FolderTree, HardDrive, Play } from "lucide-react";
import { api, ApiError } from "@/lib/api";
import { useAsync, useThrottledEffect } from "@/lib/hooks";
import { scanProfiles } from "@/lib/i18n";
import { useLive } from "@/lib/live";
import type { TaskRecord } from "@/lib/types";
import { snapshotOf } from "@/lib/utils";
import { PathInput } from "@/components/shell";
import { ErrorNote, Loading } from "@/components/states";
import { TaskDetailDialog, TaskRow } from "@/components/tasks";
import { Button } from "@/components/ui/button";
import { Card, CardBody, CardHeader, Empty, Field, PageHeader, Select } from "@/components/ui/primitives";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/overlay";
import { SnapshotExplorer } from "./explorer";
import { CompareView, SnapshotsPanel } from "./snapshots";

export default function NextSizePage() {
  const { version } = useLive();
  const [root, setRoot] = useState("");
  const [profile, setProfile] = useState("balanced");
  const [busy, setBusy] = useState(false);
  const [open, setOpen] = useState<TaskRecord | null>(null);
  const [snapshot, setSnapshot] = useState<string | null>(null);
  const [external, setExternal] = useState("");
  const [tab, setTab] = useState("scans");
  const [compare, setCompare] = useState<[string, string] | null>(null);

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

  if (snapshot) {
    return <SnapshotExplorer path={snapshot} key={snapshot} onClose={() => setSnapshot(null)} />;
  }
  if (compare) {
    return <CompareView a={compare[0]} b={compare[1]} onClose={() => setCompare(null)} />;
  }

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
            title={
              <Tabs value={tab} onValueChange={setTab}>
                <TabsList>
                  <TabsTrigger value="scans">Análises</TabsTrigger>
                  <TabsTrigger value="snapshots">Snapshots salvos</TabsTrigger>
                </TabsList>
              </Tabs>
            }
            description={tab === "scans" ? "Clique para explorar. Análises em andamento ou interrompidas abrem como dados parciais." : "Abra, rotule, compare, exporte ou exclua resultados de análises."}
            actions={
              <div className="flex w-72 gap-2">
                <PathInput value={external} onChange={setExternal} placeholder="Abrir snapshot .nxs…" file={{ name: "Snapshot NextSize", extensions: ["nxs"] }} />
                <Button variant="outline" disabled={!external.trim()} onClick={() => setSnapshot(external.trim())}>
                  Abrir
                </Button>
              </div>
            }
          />
          {tab === "snapshots" ? (
            <SnapshotsPanel onOpen={setSnapshot} onCompare={(a, b) => setCompare([a, b])} />
          ) : tasks.loading && !tasks.data ? (
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
                    // Any snapshot that exists can be explored (partial ones are labeled).
                    if (s && x.state !== "queued") setSnapshot(s);
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

      <TaskDetailDialog task={open} onClose={() => setOpen(null)} />
    </>
  );
}
