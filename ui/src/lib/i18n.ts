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

/** Plain-language explanation of each error class (ErrorKind in Rust). */
export const errorPlain: Record<string, { title: string; what: string; todo: string }> = {
  access_denied: {
    title: "Sem permissão",
    what: "O usuário do Windows que executa o NextTool não tem permissão para ler a origem ou gravar no destino.",
    todo: "Peça acesso ao responsável pela pasta ou execute com outro usuário e retome a tarefa.",
  },
  not_found: {
    title: "Caminho não encontrado",
    what: "A pasta ou o arquivo não existe mais (ou foi renomeado/movido durante a operação).",
    todo: "Confira o caminho e crie a tarefa novamente se necessário.",
  },
  path_too_long: {
    title: "Caminho longo demais",
    what: "Algum caminho ultrapassa o limite aceito pelo sistema de destino.",
    todo: "Encurte nomes de pastas no destino ou copie para um nível mais próximo da raiz.",
  },
  network: {
    title: "Problema de rede",
    what: "A conexão com o servidor, NAS ou compartilhamento caiu ou ficou indisponível.",
    todo: "Verifique a rede e o servidor. Tarefas com falha de rede são repetidas automaticamente algumas vezes.",
  },
  timeout: {
    title: "Tempo esgotado",
    what: "O servidor ou disco demorou demais para responder.",
    todo: "Verifique a carga do servidor/NAS. A tarefa é repetida automaticamente algumas vezes.",
  },
  authentication: {
    title: "Falha de autenticação",
    what: "As credenciais da sessão do Windows não foram aceitas pelo servidor.",
    todo: "Conecte-se ao compartilhamento pelo Windows (Explorador de Arquivos ou Gerenciador de Credenciais) e retome. O NextTool não guarda senhas.",
  },
  disk_full: {
    title: "Sem espaço em disco",
    what: "O destino (ou o disco onde o NextTool grava seus dados) ficou sem espaço.",
    todo: "Libere espaço e retome a tarefa. O que já foi gravado foi preservado.",
  },
  locked: {
    title: "Arquivo em uso",
    what: "Um ou mais arquivos estavam abertos/bloqueados por outro programa.",
    todo: "Feche o programa que usa os arquivos e retome. Arquivos já copiados não serão copiados de novo.",
  },
  invalid_name: {
    title: "Nome ou caminho inválido",
    what: "Algum caminho não é válido para o Windows ou não passou na validação de segurança.",
    todo: "Revise origem e destino.",
  },
  integrity: {
    title: "Divergência na verificação",
    what: "Depois da cópia, a verificação encontrou arquivos ausentes ou diferentes no destino.",
    todo: "Não use o destino antes de revisar as divergências nos detalhes técnicos. Retomar a tarefa recopia o que falta.",
  },
  corrupt_state: {
    title: "Estado interno corrompido",
    what: "Um arquivo de estado do NextTool estava danificado. Ele foi preservado para análise.",
    todo: "Revise as tarefas recentes.",
  },
  io: { title: "Erro de leitura ou gravação", what: "O sistema operacional informou um erro de entrada/saída.", todo: "Verifique o disco e os detalhes técnicos." },
  internal: { title: "Erro interno", what: "Ocorreu uma falha inesperada no NextTool.", todo: "Consulte os detalhes técnicos e o log. A tarefa pode ser retomada." },
  cancelled: { title: "Cancelada", what: "A operação foi cancelada.", todo: "Crie a tarefa novamente se necessário." },
};

export const plainError = (kind: string | null | undefined) => errorPlain[kind ?? ""] ?? errorPlain.internal;

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
