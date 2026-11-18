import type { ReactNode } from "react";
import { toast } from "sonner";
import { Check, Info, Monitor, Moon, Palette, SlidersHorizontal, Sun } from "lucide-react";
import { api, ApiError, isDesktop } from "@/lib/api";
import { useAsync } from "@/lib/hooks";
import { ACCENTS, useSettings } from "@/lib/settings";
import type { Settings } from "@/lib/types";
import { cn } from "@/lib/utils";
import { Card, CardBody, CardHeader, PageHeader, Select } from "@/components/ui/primitives";
import { Tooltip } from "@/components/ui/overlay";

function Row({ label, hint, children }: { label: string; hint?: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-4 border-t py-3.5 first:border-t-0">
      <div className="min-w-0">
        <p className="text-sm font-medium">{label}</p>
        {hint && <p className="mt-0.5 text-xs text-muted-foreground">{hint}</p>}
      </div>
      <div className="flex items-center gap-2">{children}</div>
    </div>
  );
}

function Segmented<T extends string | number>({ value, options, onChange }: { value: T; options: { value: T; label: string; icon?: ReactNode }[]; onChange: (v: T) => void }) {
  return (
    <div className="inline-flex rounded-lg bg-muted p-1">
      {options.map((o) => (
        <button
          key={String(o.value)}
          onClick={() => onChange(o.value)}
          className={cn(
            "inline-flex h-7 cursor-default items-center gap-1.5 rounded-md px-3 text-sm font-medium text-muted-foreground [&_svg]:size-3.5",
            value === o.value && "bg-card text-foreground shadow-sm",
          )}
        >
          {o.icon}
          {o.label}
        </button>
      ))}
    </div>
  );
}

export default function SettingsPage() {
  const { settings, update } = useSettings();
  const info = useAsync(() => api.appInfo(), []);

  const set = async <K extends keyof Settings>(k: K, v: Settings[K]) => {
    try {
      await update(k, v);
    } catch (e) {
      toast.error("Não foi possível salvar", { description: e instanceof ApiError ? e.message : String(e) });
    }
  };

  return (
    <>
      <PageHeader title="Configurações" description="Aparência e comportamento. As preferências ficam salvas neste computador." />
      <div className="grid gap-5 [&>*]:min-w-0 lg:grid-cols-2">
        <Card>
          <CardHeader icon={<Palette />} title="Aparência" />
          <CardBody>
            <Row label="Tema" hint="Automático acompanha o Windows.">
              <Segmented
                value={settings.theme}
                onChange={(v) => set("theme", v)}
                options={[
                  { value: "light", label: "Claro", icon: <Sun /> },
                  { value: "dark", label: "Escuro", icon: <Moon /> },
                  { value: "system", label: "Automático", icon: <Monitor /> },
                ]}
              />
            </Row>
            <Row label="Cor principal">
              <div className="flex flex-wrap gap-1.5">
                {ACCENTS.map((a) => (
                  <Tooltip key={a.hue} content={a.name}>
                    <button
                      aria-label={a.name}
                      onClick={() => set("accent", a.hue)}
                      className="grid size-7 cursor-default place-items-center rounded-full ring-offset-2 ring-offset-card transition-shadow"
                      style={{ background: `oklch(0.6 0.18 ${a.hue})`, boxShadow: settings.accent === a.hue ? `0 0 0 2px var(--card), 0 0 0 4px oklch(0.6 0.18 ${a.hue})` : undefined }}
                    >
                      {settings.accent === a.hue && <Check className="size-3.5 text-white" />}
                    </button>
                  </Tooltip>
                ))}
              </div>
            </Row>
            <Row label="Densidade" hint="Compacta mostra mais linhas por tela.">
              <Segmented value={settings.density} onChange={(v) => set("density", v)} options={[{ value: "comfortable", label: "Confortável" }, { value: "compact", label: "Compacta" }]} />
            </Row>
            <Row label="Tamanho da fonte">
              <Segmented value={settings.font_scale} onChange={(v) => set("font_scale", v)} options={[90, 100, 110, 120].map((n) => ({ value: n as Settings["font_scale"], label: `${n}%` }))} />
            </Row>
            <Row label="Unidades" hint="Binárias: 1 GiB = 1024³ bytes (como o Windows). Decimais: 1 GB = 1000³ bytes (como fabricantes de disco).">
              <Segmented value={settings.units} onChange={(v) => set("units", v)} options={[{ value: "binary", label: "GiB (1024)" }, { value: "decimal", label: "GB (1000)" }]} />
            </Row>
            <Row label="Idioma" hint="Outros idiomas serão adicionados em versões futuras.">
              <Select value="pt-BR" disabled className="w-48">
                <option value="pt-BR">Português (Brasil)</option>
              </Select>
            </Row>
          </CardBody>
        </Card>

        <div className="grid min-w-0 content-start gap-5">
          <Card>
            <CardHeader icon={<SlidersHorizontal />} title="Tarefas" />
            <CardBody>
              <Row label="Tarefas simultâneas" hint="Máximo de análises e cópias ao mesmo tempo. Tarefas no mesmo disco ou compartilhamento sempre rodam uma de cada vez. Vale após reiniciar o aplicativo.">
                <Select value={String(settings.max_running)} onChange={(e) => set("max_running", Number(e.target.value))} className="w-24" disabled={!isDesktop}>
                  {[1, 2, 3, 4, 6, 8].map((n) => (
                    <option key={n} value={n}>
                      {n}
                    </option>
                  ))}
                </Select>
              </Row>
            </CardBody>
          </Card>
          <Card>
            <CardHeader icon={<Info />} title="Sobre" />
            <CardBody className="grid gap-2 text-sm">
              {info.data ? (
                <>
                  <p>
                    NextTool <strong>{info.data.version}</strong> · {info.data.portable ? "versão portátil" : "versão instalada"} · {info.data.platform}
                  </p>
                  <p className="text-muted-foreground">
                    Dados: <span className="font-mono text-xs">{info.data.data_dir}</span>
                  </p>
                  <p className="text-muted-foreground">
                    Robocopy: <span className="font-mono text-xs">{info.data.robocopy ?? "não encontrado — NextCopy indisponível"}</span>
                  </p>
                </>
              ) : (
                <p className="text-muted-foreground">{info.error?.code === "desktop_only" ? "Informações disponíveis no aplicativo desktop." : "Carregando…"}</p>
              )}
              <p className="mt-2 text-xs text-muted-foreground">Sem telemetria. O aplicativo funciona offline e não envia dados para nenhum serviço.</p>
            </CardBody>
          </Card>
        </div>
      </div>
    </>
  );
}
