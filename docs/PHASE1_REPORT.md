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

---

# Complemento da Fase 1 — prioridades de 2026-11-18

## Situação por prioridade

| # | Prioridade | Situação |
|---|---|---|
| 1 | SQLite: medir cada etapa, otimizar, snapshots parciais consultáveis, finalização recuperável | ✅ Instrumentação por etapa (backpressure, espera do escritor, inserção, commits, cada índice, roll-up, agregados, ANALYZE). **Total −49% e finalização −58%** a 11,1 M; ingestão linear até **111 M com 95 MB de RAM**. SQLite mantido (ADR-024). Snapshots parciais consultáveis e queda durante a finalização recuperável, com testes |
| 2 | Desktop Tauri 2 + React + TS + Tailwind + shadcn/ui + Lucide, temas, só NextSize e NextCopy + áreas compartilhadas, sem dados simulados | ✅ (Fase 1 base). Sem dados simulados: fora do app a UI mostra "disponível apenas no desktop" |
| 3 | NextSize: árvore virtualizada, lista paginada, filtros, progresso real, cancelamento, snapshots, sem índices grandes no IPC | ✅ Explorador com árvore virtualizada sob demanda (300 por nível), lista paginada (200), breadcrumb, filtros combinados (extensão, nome, tamanho mín/máx, datas, ano) com cancelamento e paginação, rankings e distribuição, abertura de análises em andamento/interrompidas como dados parciais |
| 4 | NextCopy: simples e avançada, multitarefa, progresso, filas, recuperação, central de erros em linguagem simples com detalhes técnicos | ✅ Erros em linguagem simples ("o que aconteceu / o que fazer") com detalhes técnicos sob demanda (classe, mensagem, códigos de saída, arquivos com erro, log). Fila com posição e motivo da espera. Robocopy mantido como motor |
| 5 | Testes: rede interrompida, destino sem espaço, permissões negadas, Robocopy pt-BR real, SMB/NAS, escala progressiva | ✅ Permissão negada: automatizado (revelou o achado R-1). ✅ Log real do Robocopy validado (en-US) + script de captura pronto para pt-BR. ⏳ SMB, sem espaço e queda de rede: testes prontos, ativados por variável, **dependem de infraestrutura**. ✅ Escala 1,1 M → 11,1 M → 111 M |
| 6 | Qualidade: desempenho e segurança, 51 testes mantidos, cobertura ampliada | ✅ **66 testes passando** + 4 de campo preparados, 0 falhas. Uma regressão de memória detectada e rejeitada (484 MB, ADR-026) |
| 7 | Distribuição instalada e portátil, WebView2 no Windows Server | ✅ Arquitetura e medições (DISTRIBUTION.md). Portátil validado. ⏳ Empacotamento e testes em Windows Server: Fase 6, dependem das VMs |

## Achados importantes

- **R-1 — Robocopy retorna 0 com o destino inacessível.** Com o destino negando escrita, o Robocopy imprimiu `ERROR 5 … Accessing Destination Directory`, terminou com **código 0** e mostrou o resumo zerado. Com verificação básica, a cópia teria sido marcada como "Concluída" sem copiar nada. Corrigido (ADR-028): erros de diretório agora sempre resultam em falha parcial. Teste com a saída real.
- **Regressão de memória rejeitada:** a primeira otimização da finalização (4 threads × 128 MB) era mais rápida, mas levou o pico a 484 MB. A matriz de medições mostrou que 2 × 16 MB é mais rápido **e** respeita o orçamento.
- **Finalização superlinear a 111 M:** os índices de filtro são 57% do tempo. O próximo passo foi identificado e medido (adiar esses índices).

## Arquivos principais alterados ou criados

- `nextsize-index`:
  - `writer.rs`: agregados durante a ingestão, INSERT multi-linha, finalização com recursos limitados, estado `finalizing`, tempos por etapa;
  - `reader.rs`: consultas em snapshots parciais, `list_subdirs`, `dir_chain`, `child_counts`, `parent_id`;
  - `resume.rs`: aceita `finalizing`.
- `nextsize-scanner/engine.rs`, `nextsize-core/progress.rs`: métricas de backpressure e de espera do escritor.
- `nextcopy-robocopy`:
  - `exit.rs`: `reconcile`;
  - `log.rs`: contagem de erros de diretório.
- `nexttool-tasks`:
  - `tests/field_scenarios.rs` (novo);
  - deduplicação de chaves de recurso.
- `nexttool-desktop`: comandos `snapshot_list_subdirs` e `snapshot_dir_info` (24 no total, todos na allowlist).
- `nexttool-cli`:
  - `synth.rs` (novo): enumerador sintético em memória;
  - `bench --synthetic` e métricas por etapa.
- UI:
  - `pages/explorer.tsx` (novo): explorador, filtros e rankings;
  - `components/tasks.tsx`: erros em linguagem simples;
  - `lib/i18n.ts`: vocabulário de erros;
  - `pages/NextCopy.tsx`: fila.
- `scripts/capture-robocopy-log.ps1` (novo).
- Docs: BENCHMARKS §8, DECISIONS ADR-024 a 029, TEST_PLAN §2d/2e, ARCHITECTURE, KNOWN_ISSUES.

## Verificação no app real (release, via CDP em localhost)

- Explorador do snapshot de 1 M arquivos: árvore carregada sob demanda, expandir e navegar, breadcrumb, lista "1–200 de 208" em 6 ms.
- Filtros: `.bak` ≥ 0,49 KB → 100 resultados por página, ordenados por tamanho. "Abrir pasta" leva ao explorador na pasta do arquivo.
- NextCopy: falha por permissão mostrada como "Sem permissão — o que aconteceu / o que fazer" + detalhes técnicos.

## O que preciso de você para fechar os pendentes

1. Um **compartilhamento SMB/NAS** de teste (gravável), para os testes de SMB e de queda de rede.
2. Um **volume pequeno** (VHD de ~64 MB ou pendrive) para o teste de destino sem espaço. Montar um VHD exige administrador.
3. Rodar `scripts\capture-robocopy-log.ps1` num **Windows com interface em pt-BR** e me enviar o log.
4. **VMs Windows Server 2016/2019/2022/2025** para os testes de compatibilidade e WebView2.
