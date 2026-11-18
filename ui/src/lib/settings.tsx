import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { api, isDesktop } from "./api";
import type { Settings } from "./types";
import { setUnits } from "./utils";

export const DEFAULT_SETTINGS: Settings = {
  theme: "system",
  accent: 255,
  density: "comfortable",
  font_scale: 100,
  units: "binary",
  language: "pt-BR",
  max_running: 2,
};

export const ACCENTS = [
  { hue: 255, name: "Azul" },
  { hue: 285, name: "Violeta" },
  { hue: 200, name: "Ciano" },
  { hue: 165, name: "Verde-água" },
  { hue: 145, name: "Verde" },
  { hue: 60, name: "Âmbar" },
  { hue: 25, name: "Laranja" },
  { hue: 350, name: "Rosa" },
];

const LS_KEY = "nexttool.settings";

type Ctx = {
  settings: Settings;
  resolvedDark: boolean;
  update: <K extends keyof Settings>(key: K, value: Settings[K]) => Promise<void>;
};

const SettingsContext = createContext<Ctx | null>(null);

function readLocal(): Partial<Settings> {
  try {
    return JSON.parse(localStorage.getItem(LS_KEY) || "{}");
  } catch {
    return {};
  }
}

function writeLocal(s: Settings) {
  try {
    localStorage.setItem(LS_KEY, JSON.stringify(s));
  } catch {
    /* storage unavailable */
  }
}

function fromBackend(raw: Record<string, string>): Partial<Settings> {
  const out: Partial<Settings> = {};
  if (raw.theme) out.theme = raw.theme as Settings["theme"];
  if (raw.accent) out.accent = Number(raw.accent);
  if (raw.density) out.density = raw.density as Settings["density"];
  if (raw.font_scale) out.font_scale = Number(raw.font_scale) as Settings["font_scale"];
  if (raw.units) out.units = raw.units as Settings["units"];
  if (raw.max_running) out.max_running = Number(raw.max_running);
  return out;
}

export function SettingsProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState<Settings>({ ...DEFAULT_SETTINGS, ...readLocal() });
  const [systemDark, setSystemDark] = useState(() => window.matchMedia("(prefers-color-scheme: dark)").matches);

  // The app database is the source of truth; localStorage only avoids a flash on start.
  useEffect(() => {
    if (!isDesktop) return;
    api
      .settingsGet()
      .then((raw) => setSettings((s) => ({ ...s, ...fromBackend(raw) })))
      .catch(() => {});
  }, []);

  useEffect(() => {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const h = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    mq.addEventListener("change", h);
    return () => mq.removeEventListener("change", h);
  }, []);

  const resolvedDark = settings.theme === "dark" || (settings.theme === "system" && systemDark);

  useEffect(() => {
    const el = document.documentElement;
    el.classList.toggle("dark", resolvedDark);
    el.style.setProperty("--hue", String(settings.accent));
    el.setAttribute("data-density", settings.density);
    el.style.fontSize = `${settings.font_scale}%`;
    setUnits(settings.units);
    writeLocal(settings);
  }, [settings, resolvedDark]);

  const update = useCallback(async <K extends keyof Settings>(key: K, value: Settings[K]) => {
    setSettings((s) => ({ ...s, [key]: value }));
    if (isDesktop) await api.settingsSet(key, String(value));
  }, []);

  const value = useMemo(() => ({ settings, resolvedDark, update }), [settings, resolvedDark, update]);
  return <SettingsContext.Provider value={value}>{children}</SettingsContext.Provider>;
}

export function useSettings() {
  const c = useContext(SettingsContext);
  if (!c) throw new Error("useSettings outside SettingsProvider");
  return c;
}
