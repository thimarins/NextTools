import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { ArrowRight, ChevronDown, ClipboardList, Copy, Eye, ShieldAlert, Trash2 } from "lucide-react";
import { api, ApiError } from "@/lib/api";
import { useAsync, useThrottledEffect } from "@/lib/hooks";
import { copyProfiles, verifyLevels } from "@/lib/i18n";
import { useLive } from "@/lib/live";
import type { CopyRequest, DryRunReport, TaskRecord } from "@/lib/types";
import { formatBytes, formatCount } from "@/lib/utils";
import { PathInput } from "@/components/shell";
import { ErrorNote, Loading } from "@/components/states";
import { TaskDetailDialog, TaskRow } from "@/components/tasks";
import { Button } from "@/components/ui/button";
import { Card, CardBody, CardHeader, Empty, Field, Input, PageHeader, Select } from "@/components/ui/primitives";

const splitList = (s: string) => s.split(/[,;\n]/).map((x) => x.trim()).filter(Boolean);

export default function NextCopyPage() {
  const { version } = useLive();
  const [source, setSource] = useState("");
  const [dest, setDest] = useState("");
  const [profile, setProfile] = useState("safe");
  const [verify, setVerify] = useState("");
  const [threads, setThreads] = useState("");
  const [excludeFiles, setExcludeFiles] = useState("");
  const [excludeDirs, setExcludeDirs] = useState("");
  const [advanced, setAdvanced] = useState(false);
  const [plan, setPlan] = useState<DryRunReport | null>(null);
  const [approved, setApproved] = useState(false);
  const [busy, setBusy] = useState<"plan" | "run" | null>(null);
  const [open, setOpen] = useState<TaskRecord | null>(null);

  const mirror = profile === "mirror";
  const req: CopyRequest = useMemo(
    () => ({
      source,
      dest,
      profile,
      verify: verify || null,
      threads: threads ? Number(threads) : null,
      exclude_files: splitList(excludeFiles),
      exclude_dirs: splitList(excludeDirs),
    }),
    [source, dest, profile, verify, threads, excludeFiles, excludeDirs],
  );

  // Any change invalidates a previous preview and its approval.
  useEffect(() => {
    setPlan(null);
    setApproved(false);
  }, [req]);

  const tasks = useAsync(() => api.tasksList(undefined, 300), []);
  useThrottledEffect(tasks.reload, version, 1500);
  const copies = useMemo(() => (tasks.data ?? []).filter((t) => t.kind === "copy").slice(0, 30), [tasks.data]);

  const deletions = plan ? plan.would_delete_files + plan.would_delete_dirs : 0;

  const doPlan = async () => {
    setBusy("plan");
    try {
      setPlan(await api.copyPlan(req));
    } catch (e) {
      toast.error("Falha na prévia", { description: e instanceof ApiError ? e.message : String(e) });
    } finally {
      setBusy(null);
    }
  };

  const doRun = async () => {
    setBusy("run");
    try {
      const id = await api.copySubmit({ ...req, approved_deletions: mirror ? deletions : null });
      toast.success(`Cópia #${id} criada`, { description: "Ela roda em segundo plano; acompanhe abaixo." });
      setPlan(null);
      setApproved(false);
      tasks.reload();
    } catch (e) {
      toast.error("Não foi possível iniciar a cópia", { description: e instanceof ApiError ? e.message : String(e) });
    } finally {
      setBusy(null);
    }
  };

  const canRun = source.trim() && dest.trim() && (!mirror || (plan && (deletions === 0 || approved)));

  return (
    <>
      <PageHeader title="NextCopy" description="Cópia, sincronização e espelhamento com Robocopy, verificação e retomada." />
      <div className="grid gap-5 [&>*]:min-w-0 lg:grid-cols-[minmax(340px,460px)_1fr]">
        <Card className="self-start">
          <CardHeader icon={<Copy />} title="Nova cópia" description="Escolha origem e destino. As opções seguras já vêm selecionadas." />
          <CardBody className="grid gap-4">
            <Field label="Origem" htmlFor="copy-src">
              <PathInput id="copy-src" value={source} onChange={setSource} placeholder="D:\Dados" />
            </Field>
            <Field label="Destino" htmlFor="copy-dst">
              <PathInput id="copy-dst" value={dest} onChange={setDest} placeholder="\\nas\backup\Dados" />
            </Field>
            <Field label="Perfil" hint={copyProfiles.find((p) => p.value === profile)?.hint} htmlFor="copy-profile">
              <Select id="copy-profile" value={profile} onChange={(e) => setProfile(e.target.value)}>
                {copyProfiles.map((p) => (
                  <option key={p.value} value={p.value}>
                    {p.label}
                  </option>
                ))}
              </Select>
            </Field>

            <button className="flex cursor-default items-center gap-1.5 text-sm font-medium text-muted-foreground hover:text-foreground" onClick={() => setAdvanced((v) => !v)}>
              <ChevronDown className={`size-4 transition-transform ${advanced ? "" : "-rotate-90"}`} /> Opções avançadas
            </button>
            {advanced && (
              <div className="grid gap-4 rounded-lg border bg-muted/30 p-3">
                <Field label="Verificação" hint={verifyLevels.find((v) => v.value === verify)?.hint ?? "Padrão do perfil."} htmlFor="copy-verify">
                  <Select id="copy-verify" value={verify} onChange={(e) => setVerify(e.target.value)}>
                    <option value="">Padrão do perfil</option>
                    {verifyLevels.map((v) => (
                      <option key={v.value} value={v.value}>
                        {v.label}
                      </option>
                    ))}
                  </Select>
                </Field>
                <Field label="Threads" hint="1–128. Vazio = valor do perfil. Em NAS, mais threads nem sempre é mais rápido." htmlFor="copy-threads">
                  <Input id="copy-threads" inputMode="numeric" value={threads} onChange={(e) => setThreads(e.target.value.replace(/\D/g, ""))} placeholder="perfil" />
                </Field>
                <Field label="Excluir arquivos" hint="Padrões separados por vírgula, ex.: *.tmp, ~$*" htmlFor="copy-xf">
                  <Input id="copy-xf" value={excludeFiles} onChange={(e) => setExcludeFiles(e.target.value)} placeholder="*.tmp, ~$*" />
                </Field>
                <Field label="Excluir pastas" hint="Nomes de pastas, ex.: $RECYCLE.BIN, node_modules" htmlFor="copy-xd">
                  <Input id="copy-xd" value={excludeDirs} onChange={(e) => setExcludeDirs(e.target.value)} placeholder="$RECYCLE.BIN" />
                </Field>
              </div>
            )}

            {plan && (
              <div className={`rounded-lg border p-3 text-sm ${mirror && deletions > 0 ? "border-destructive/40 bg-destructive/6" : "bg-muted/30"}`}>
                <p className="flex items-center gap-2 font-medium">
                  <ClipboardList className="size-4" /> Prévia — nada foi alterado
                </p>
                <p className="mt-1.5">
                  Seriam copiados <strong>{formatCount(plan.would_copy_files)}</strong> arquivos ({formatBytes(plan.would_copy_bytes)}).
                </p>
                {mirror && (
                  <>
                    <p className={`mt-1 ${deletions > 0 ? "font-medium text-destructive" : ""}`}>
                      <Trash2 className="mr-1 inline size-3.5 align-[-2px]" />
                      Seriam EXCLUÍDOS no destino: {formatCount(plan.would_delete_files)} arquivos e {formatCount(plan.would_delete_dirs)} pastas.
                    </p>
                    {plan.delete_sample.length > 0 && (
                      <ul className="mt-2 max-h-32 overflow-y-auto rounded border bg-card p-2 font-mono text-xs">
                        {plan.delete_sample.slice(0, 50).map((p) => (
                          <li key={p} className="truncate" title={p}>{p}</li>
                        ))}
                      </ul>
                    )}
                    {deletions > 0 && (
                      <label className="mt-3 flex items-start gap-2">
                        <input type="checkbox" className="mt-0.5 size-4 accent-[var(--destructive)]" checked={approved} onChange={(e) => setApproved(e.target.checked)} />
                        <span>
                          Aprovo a exclusão de <strong>{formatCount(deletions)}</strong> itens no destino. Se o destino mudar antes da cópia, a operação será bloqueada.
                        </span>
                      </label>
                    )}
                  </>
                )}
                {plan.errors > 0 && <p className="mt-1 text-warning">{plan.errors} erros durante a prévia (ver log).</p>}
              </div>
            )}

            {mirror && !plan && (
              <p className="flex items-start gap-2 rounded-lg border border-warning/40 bg-warning/10 p-3 text-sm">
                <ShieldAlert className="mt-0.5 size-4 shrink-0 text-warning" /> O espelhamento exclui arquivos no destino. Gere a prévia para ver e aprovar as exclusões.
              </p>
            )}

            <div className="flex gap-2">
              <Button variant="outline" className="flex-1" onClick={doPlan} disabled={!source.trim() || !dest.trim() || busy !== null}>
                <Eye /> {busy === "plan" ? "Gerando prévia…" : "Prévia"}
              </Button>
              <Button className="flex-1" variant={mirror && deletions > 0 ? "destructive" : "default"} onClick={doRun} disabled={!canRun || busy !== null}>
                <ArrowRight /> {busy === "run" ? "Criando…" : mirror ? "Espelhar" : "Copiar"}
              </Button>
            </div>
          </CardBody>
        </Card>

        <Card className="self-start">
          <CardHeader icon={<ClipboardList />} title="Transferências" description="Pausar encerra o Robocopy com segurança; retomar recopia só o que falta." />
          {tasks.loading && !tasks.data ? (
            <Loading />
          ) : tasks.error ? (
            <ErrorNote error={tasks.error} onRetry={tasks.reload} />
          ) : copies.length ? (
            <div className="border-t">
              {copies.map((t) => (
                <TaskRow key={t.id} task={t} onOpen={setOpen} onChanged={tasks.reload} />
              ))}
            </div>
          ) : (
            <Empty icon={<Copy />} title="Nenhuma transferência ainda" />
          )}
        </Card>
      </div>
      <TaskDetailDialog task={open} onClose={() => setOpen(null)} />
    </>
  );
}
