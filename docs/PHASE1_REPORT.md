# Relatório da Fase 1 — Fundação desktop

Data: 2026-11-18 · Base: Fase 0 (`4fd5245`)

## 1. O que foi implementado

**App desktop `NextTool.exe` (Tauri 2 + React 19)**

| Área | Entregue |
|---|---|
| Casca | Janela única com barra superior: Início, NextSize, NextCopy, Histórico, Alertas (com contador) e Configurações. Ícones Lucide, componentes no padrão shadcn/ui (Radix + Tailwind 4) |
| Temas | Claro, escuro e automático (segue o Windows, inclusive ao vivo). 8 cores principais (matiz OKLCH). Densidade confortável/compacta. Fonte de 90 a 120%. Unidades binárias ou decimais. Idioma pt-BR. Aplicado globalmente, persistido no `app.db`, sem *flash* na abertura |
| Início | Indicadores (em execução, pausadas/interrompidas, falhas, alertas), tarefas ativas com progresso ao vivo e ações, falhas recentes, alertas recentes, últimas análises e transferências, atalhos |
| NextSize | Formulário de análise (seletor nativo de pasta, perfis com explicação), lista de análises ao vivo, visualização do snapshot: resumo; tamanho alocado "Não disponível" quando não informado; aviso de **dados parciais**; maiores arquivos e pastas; distribuição por extensão e por ano |
| NextCopy | Formulário com perfis e opções avançadas (verificação, threads, exclusões). **Prévia** com contagem e amostra das exclusões. **Espelhamento só com aprovação explícita** de N exclusões; qualquer mudança invalida a prévia |
| Tarefas | Pausar (parada segura), retomar, cancelar, detalhes (eventos, erro, resultado, verificação). Progresso indeterminado com contadores reais, **sem porcentagem falsa** |
| Histórico | Tabela virtualizada (TanStack Table + Virtual), paginação no backend (500 por página), filtros por tipo e estado, ordenação |
| Alertas | Central com severidade, módulo, tarefa, ocorrências agrupadas, causa provável (só com evidência), consequência, ação recomendada, detalhes técnicos e botão "Reconhecer". Notificação só na 1ª ocorrência |
| Erros | *Error boundary* global e por página (as tarefas continuam no motor), erros de IPC com código e mensagem, notificações |
| Fechamento | Com tarefas em execução, pede confirmação e **pausa com segurança** antes de sair |
| Instância única | Uma segunda abertura foca a janela existente |

**Backend (`crates/nexttool-desktop`)**
- 22 comandos IPC com **allowlist explícita** (app manifest + capability + teste de consistência).
- CSP restritiva e `freezePrototype`.
- Laço de eventos (agendador + `task://update`, `alert://raised`, até 4 Hz).
- Toda a lógica em `ipc.rs`, testável sem janela.

**Mudanças em crates existentes**
- `nexttool-store`:
  - `try_claim`: reivindicação atômica de tarefa;
  - allowlist de configurações compartilhada (`SETTINGS`, `validate_setting`).
- `nexttool-tasks`:
  - `tick_only` (a CLI agenda só as próprias tarefas);
  - mensagem de falha do Robocopy sem formatação de depuração.
- `nexttool-cli`:
  - binário renomeado para **`nexttool-cli.exe`** (ADR-019);
  - novo comando `settings list|set`.

**Ferramentas instaladas (com sua autorização):** Node.js 26.13.1 LTS via winget. WebView2 Evergreen 158 já estava presente.

## 2. Testes executados

- `cargo test --workspace`: **58/58** passando (51 da Fase 0 + 7 novos; ver TEST_PLAN §2b).
- `tsc -b` (TypeScript estrito): sem erros. `vite build`: ok.
- **Ponta a ponta no app real** (release), conduzido via WebView2 DevTools (CDP, só em localhost, só durante o teste):
  - todas as telas, sem estouro horizontal;
  - análise de 1 M arquivos iniciada pelo formulário → progresso ao vivo (70.600 arquivos no 1º segundo) → **Pausar** na UI → "Pausada" → **Retomar** → "Concluída" com **exatamente 1.000.000 arquivos**;
  - abrir o snapshot;
  - trocar tema, cor e densidade, e confirmar a persistência pela CLI;
  - histórico e alertas compartilhados entre CLI e app no modo portátil.

## 3. Medições (detalhes em DISTRIBUTION §5)

| Item | Valor |
|---|---|
| Janela visível após iniciar | 192 ms |
| `NextTool.exe` | 13,2 MB em disco; 29 MB working set |
| WebView2 | 6 processos; **164 MB privados** |
| UI | 572 KB |

## 4. Problemas encontrados e corrigidos

1. **CLI e desktop com o mesmo nome de arquivo** (`nexttool.exe` = `NextTool.exe` no Windows): o build do app sobrescrevia a CLI. A CLI agora é `nexttool-cli.exe`.
2. **Risco de dois agendadores executarem a mesma tarefa** (app + CLI). Corrigido com reivindicação atômica e `tick_only`, com teste.
3. **`std::path::absolute` resolvia `..`** silenciosamente nas entradas da UI. Agora caminhos relativos ou com `..` são recusados (ADR-021).
4. **IDs HTML duplicados:** o campo "Origem" usava `id="root"`, o mesmo do contêiner do React, e o rótulo apontava para o elemento errado. Os IDs agora têm prefixo por página.
5. **Estouro horizontal** na tela Início: grid sem `minmax(0, …)`.
6. **Coluna de caminho** truncada cedo demais (tabela com layout automático).
7. Mensagem de falha do Robocopy exibia `[Some(9)]`.

**Observação:** durante os testes, havia uma janela do app aberta por você (você trocou o tema e analisou `C:\`). Na primeira vez eu a encerrei à força, sem perda: a análise já tinha terminado. Depois disso, só fechei pelo caminho normal e com sua autorização. Suas preferências (tema escuro) foram restauradas após os testes de tema.

## 5. Limitações

Lista completa em KNOWN_ISSUES §Desktop. Principais:
- Explorador em árvore, treemap, filtros e comparação de snapshots na UI ainda não existem (Fases 2 e 4).
- Agendamento e seleção NextSize → NextCopy: Fases 3 e 4.
- Sem testes automatizados de componentes de UI (Playwright/Vitest: Fase 2).
- Memória do WebView2 relevante em servidores pequenos (use a CLI).
- Empacotamento (MSI/NSIS/Portable com Fixed Version) e testes em Windows Server: Fase 6. **Preciso das VMs ou servidores.**

## 6. Próximos passos — Fase 2 (NextSize)

1. Escritor SQLite otimizado e finalização mais barata (meta: ≥ 2× a taxa atual com persistência), com benchmark de 10 M arquivos.
2. Explorador: árvore de diretórios + lista virtualizada (`snapshot_list_dir` já existe) + painel de filtros na UI (`snapshot_query` já existe).
3. Paralelismo adaptativo para SMB.
4. Dono/SID sob demanda, categorias, keyset pagination.
5. Snapshots: comparar, renomear, excluir com confirmação, exportar/importar.
6. Testes de UI automatizados (Playwright contra o WebView2 via CDP, reaproveitando o roteiro desta fase).
