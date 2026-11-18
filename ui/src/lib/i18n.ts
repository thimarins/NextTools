// pt-BR is the only language in v1. Labels for enums live here so other
// languages can be added without touching components.
import type { Module, Severity, TaskKind, TaskState } from "./types";

export const taskStateLabel: Record<TaskState, string> = {
  queued: "Na fila",
  running: "Em execução",
  paused: "Pausada",
  waiting_retry: "Aguardando nova tentativa",
  cancelling: "Cancelando",
  interrupted: "Interrompida",
  completed: "Concluída",
  completed_with_warnings: "Concluída com avisos",
  failed: "Falhou",
  cancelled: "Cancelada",
};

export const taskKindLabel: Record<TaskKind, string> = {
  size_scan: "Análise",
  copy: "Cópia",
  verify: "Verificação",
};

export const moduleLabel: Record<Module, string> = {
  next_size: "NextSize",
  next_copy: "NextCopy",
  system: "Sistema",
};

export const severityLabel: Record<Severity, string> = {
  info: "Informativo",
  warning: "Atenção",
  error: "Erro",
  critical: "Crítico",
};

export const scanProfiles = [
  { value: "economy", label: "Econômico", hint: "Menor impacto: 2 threads, prioridade baixa de CPU e disco. Indicado para servidores de produção." },
  { value: "balanced", label: "Equilibrado", hint: "Recomendado. Concorrência moderada; compartilhamentos de rede começam com 4 threads." },
  { value: "fast", label: "Alto desempenho", hint: "Usa mais CPU e memória. Para discos locais e janelas de manutenção." },
];

export const copyProfiles = [
  { value: "safe", label: "Cópia segura", hint: "Padrão. 8 threads, 3 tentativas, verificação de tamanho e data." },
  { value: "quick", label: "Cópia rápida", hint: "16 threads, 1 tentativa, verificação básica. Para discos locais." },
  { value: "migration", label: "Migração de servidor", hint: "Copia permissões (ACL) e proprietário, modo reiniciável. Pode exigir privilégios no destino." },
  { value: "incremental", label: "Backup incremental", hint: "Copia apenas novos e alterados. Nunca exclui." },
  { value: "sync", label: "Sincronização", hint: "Não substitui arquivos mais novos no destino. Nunca exclui." },
  { value: "mirror", label: "Espelhamento", hint: "Destino idêntico à origem: EXCLUI o que só existe no destino. Exige prévia e aprovação." },
];

export const verifyLevels = [
  { value: "basic", label: "Básica", hint: "Resultado e estatísticas do Robocopy." },
  { value: "standard", label: "Padrão", hint: "Existência, tamanho e data de cada arquivo no destino." },
  { value: "strict", label: "Rigorosa", hint: "Padrão + hash completo (BLAKE3) de origem e destino. Lê tudo duas vezes." },
];
