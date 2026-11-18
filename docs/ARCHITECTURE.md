# NextTool — Arquitetura

Versão do documento: Fase 1 (2026-11-18). Estado: os motores, a infraestrutura compartilhada e a fundação desktop (Tauri + React) estão implementados e testados.

Documentos relacionados: [DECISIONS.md](DECISIONS.md) (ADRs) · [TEST_PLAN.md](TEST_PLAN.md) · [BENCHMARKS.md](BENCHMARKS.md) · [DISTRIBUTION.md](DISTRIBUTION.md) · [KNOWN_ISSUES.md](KNOWN_ISSUES.md) · [PHASE0_REPORT.md](PHASE0_REPORT.md)

---

## 1. Visão geral

O NextTool é um aplicativo desktop único com dois módulos independentes:

| Módulo | Função | Escreve em disco do usuário? |
|---|---|---|
| **NextSize** | Análise de armazenamento: inventário, rankings, filtros, snapshots | **Nunca**. Somente leitura. |
| **NextCopy** | Cópia, sincronização, espelhamento, verificação | Somente no destino autorizado |

Os dois módulos compartilham um núcleo pequeno: erros, enumeração de diretórios, modelos de tarefa e alerta, validação de caminhos, persistência do app e orquestração de tarefas. Nenhum depende diretamente do outro.

```
                      nexttool-desktop (Fase 1: Tauri 2 + React)      nexttool-cli
                                       \                               /
                                        +------- nexttool-tasks ------+
                                        |  fila · concorrência · retry |
                                        |  recuperação · monitor       |
                                        +--+----------+-----------+---+
                                           |          |           |
                              nexttool-store   NextSize runners   NextCopy runners
                              (app.db, logs)        |                  |
                                           nextsize-scanner     nextcopy-robocopy
                                           nextsize-index       nextcopy-verify
                                           nextsize-core        nextcopy-core
                                                 \                  /
                                                  +-- nexttool-core +
                                                  erros · fs · tarefa · alerta · segurança · processo
```

### 1.1 Crates (11)

| Crate | Responsabilidade | Depende de |
|---|---|---|
| `nexttool-core` | `ErrorKind`/`Severity` e classificação de códigos Win32/errno; enumeração de diretórios somente leitura (`DirEnumerator`: Win32 `FindFirstFileExW` e `std`); modelos `Task*` e `Alert*`; validação de caminhos e sobreposição; identidade de processo (pid + horário de início) e leitura de recursos | — |
| `nexttool-store` | `app.db` (SQLite): tarefas, eventos, checkpoints, alertas agrupados, configurações; detecção de corrupção; logs rotativos limitados | core |
| `nexttool-tasks` | `TaskManager` (fila limitada, prioridades, concorrência por recurso, retentativas com backoff, contenção de pânico), recuperação na inicialização, `ActivityMonitor` (lentidão e ausência de progresso com evidência e confiança), textos de alerta, *runners* que adaptam os motores | store, motores |
| `nextsize-core` | Modelo de varredura (`EntryRecord`, `DirChunk`), contratos do pipeline (`ScanSink`), perfis/`Tuning`, progresso, extensões | core |
| `nextsize-scanner` | Motor: workers de enumeração, canal limitado (backpressure), fronteira LIFO limitada com *spill* em disco, pausa, cancelamento e controle de concorrência | nextsize-core |
| `nextsize-index` | Snapshot `.nxs` (SQLite): gravação em lotes (checkpoints), finalização (índices, roll-up, agregados), retomada, leitura paginada com tempo limite e cancelamento, verificação de integridade | nextsize-core, scanner (`DirTask`) |
| `nextcopy-core` | `CopyJob`, perfis, políticas, filtros, validação, contrato `CopyEngine`, resultados | core |
| `nextcopy-robocopy` | Motor v1: `Robocopy.exe` como processo filho, argumentos estruturados, log interpretado sem depender do idioma, código de saída como bitmask, prévia (dry-run) e aprovação de espelhamento | nextcopy-core |
| `nextcopy-verify` | Verificação padrão (existência, tamanho, data) e rigorosa (BLAKE3 completo) | nextcopy-core, core |
| `nexttool-cli` | Binário `nexttool`: os mesmos motores e o mesmo `TaskManager` do desktop, mais benchmarks e gerador de massa de teste | todos |
| `nexttool-desktop` | App Tauri 2 (`NextTool.exe`): IPC com allowlist, laço de eventos e agendador, instância única, fechamento seguro. A UI fica em `/ui` (React 19 + TS + Tailwind 4 + componentes estilo shadcn/ui + lucide + TanStack) | tasks, store, index, robocopy |

Regras verificadas: não há dependência circular (o Cargo recusaria) e NextSize ↔ NextCopy não se referenciam. A única dependência entre os motores e a orquestração é **tasks → motores**; os motores não conhecem tarefas.

### 1.2 Diretório de dados

```
%LOCALAPPDATA%\NextTool\         (ou <exe>\data\ quando existe portable.flag ao lado do exe)
  app.db                        tarefas, eventos, alertas, configurações (WAL, synchronous=FULL)
  logs\nexttool.log …5.log      log técnico rotativo (10 MB × 6 no máximo)
  logs\copy\<task>\robocopy-*.log   logs do Robocopy (UNILOG, UTF-16)
  tmp\                          arquivos de spill da fronteira (apagados ao terminar)
<qualquer lugar>\*.nxs           snapshots NextSize (escolhidos pelo usuário)
```

---

## 2. NextSize

### 2.1 Pipeline de varredura (streaming, memória limitada)

```
 fronteira LIFO (RAM ≤ frontier_mem_cap)  ⇄  spill em disco (arquivo LIFO)
        │ pop
        ▼
 N workers de I/O ── enumerate(dir) ──► chunks de ≤ chunk_size entradas
        │                                        │ send (bloqueia se o canal estiver cheio = backpressure)
        │ push dos subdiretórios (após enviar o chunk que os contém)
        ▼                                        ▼
                                   canal limitado (channel_cap chunks)
                                                 │
                                                 ▼
                                   1 thread escritora → ScanSink (IndexWriter)
                                     transações de commit_rows linhas ou commit_interval_ms
                                     (cada COMMIT é um checkpoint durável)
```

- **Sem árvore global na RAM.** Os nós vão para o SQLite assim que são lidos. O processo guarda apenas: chunks em trânsito (canal mais um por worker), a fronteira (limitada, com excedente em disco), acumuladores por diretório *em andamento* (cerca de um por worker) e o dicionário de extensões (limitado a 200 mil).
- **Backpressure.** Quando o SQLite não acompanha, `send` bloqueia e os workers desaceleram. Não existe fila sem limite.
- **Orçamento de memória** (`Tuning::queue_budget_bytes`): Econômico ≈ 12 MB, Equilibrado ≈ 59 MB, Rápido ≈ 216 MB, incluindo o cache do SQLite. O consumo medido do processo está em [BENCHMARKS.md](BENCHMARKS.md).
- **Ordem garantida.** Os subdiretórios só entram na fronteira depois que o chunk com a linha deles foi enviado. Assim, a linha de um diretório filho sempre chega ao escritor antes das linhas do conteúdo dele.
- **Links.** Junctions, symlinks e mount points (reparse points) **nunca são seguidos**: são gravados com `NOT_FOLLOWED`, o que evita loops. No Unix, outro `st_dev` recebe `OTHER_MOUNT` e não é atravessado, salvo `cross_mounts`.
- **Nuvem.** Placeholders do OneDrive e semelhantes são apenas listados. A enumeração não provoca download.

### 2.2 Backends de enumeração

| Backend | Plataforma | API | Privilégio |
|---|---|---|---|
| `win32-find` (Auto/Native) | Windows local e SMB | `FindFirstFileExW(FindExInfoBasic, FIND_FIRST_EX_LARGE_FETCH)` com prefixo `\\?\` / `\\?\UNC\` (sem limite de 260 caracteres) | nenhum |
| `std-readdir` (Portable) | todas | `std::fs::read_dir` (no Unix, um `lstat` por entrada) | nenhum |
| NTFS rápido (MFT/USN) | Windows NTFS | **Fase 5** — ver ADR-006 | administrador (opcional, isolado) |
| Linux nativo (`getdents64` + `statx`) | Linux | **Fase 5** | nenhum |

### 2.3 Modelo de persistência (snapshot `.nxs` = 1 arquivo SQLite)

```sql
meta(key, value)                          -- schema_version, app, root, state, contadores, opções, tempos
dirs(id PK, parent, depth, name, state, flags, mtime, ctime, atime,
     d_files, d_dirs, d_size, d_alloc,    -- conteúdo direto
     t_files, t_dirs, t_size, t_alloc)    -- totais recursivos (roll-up na finalização)
files(id PK, parent, name, ext, kind, size, alloc, mtime, ctime, atime, flags, file_id, nlink)
exts(id PK, ext UNIQUE)                   -- dicionário de extensões
issues(...)                               -- erros por diretório (código, tipo, severidade, recuperável)
ext_stats, year_stats                     -- agregados pré-calculados
```

- IDs são `u64` atribuídos pelo motor (contador atômico). Arquivos e diretórios ficam em tabelas separadas, e o caminho é reconstruído pela cadeia `parent` (só diretórios).
- **Índices secundários são criados na finalização**, não durante a ingestão (ADR-004): `dirs(parent)`, `dirs(depth,parent)`, `dirs(t_size)`, `files(parent)`, `files(size)`, `files(mtime)`, `files(ext,size)`.
- **Roll-up** por nível de profundidade, do mais profundo para a raiz, com `UPDATE … FROM (SELECT parent, SUM(…) … GROUP BY parent)`. A memória fica limitada porque tudo acontece dentro do SQLite.
- **Snapshots parciais consultáveis (ADR-027):** sem índices, as consultas usam varredura com tempo limite de 20 s e tamanhos diretos (`d_size`). O estado `finalizing` é persistido antes da finalização, que é idempotente e retomável.
- **Agregados** por extensão e ano são calculados durante a ingestão (ADR-025); a finalização usa ordenação paralela limitada ao orçamento do perfil (ADR-026).
- **Abertura rápida:** `Snapshot::open` + `summary()` leem só `meta` e a linha da raiz, independentemente do tamanho do snapshot.
- Tamanhos são `u64` no motor e `i64` no SQLite (saturação em 9,2 EB). As somas usam `saturating_add`.
- **Tamanho alocado:** o `FindFirstFileExW` não o informa. Ele é gravado como `NULL` e exibido como "não disponível neste modo" (ADR-007). No Unix vem de `st_blocks × 512`.
- **Hardlinks:** política `per_path` (cada caminho conta), registrada em `meta.hardlink_policy`. A deduplicação por `file_id` vem com o modo NTFS rápido e o scanner Linux.

#### Escala: 100 milhões e 1 bilhão de registros

Medido na Fase 0 ([BENCHMARKS.md](BENCHMARKS.md) §4): **104 B/arquivo** (sintético, nomes curtos) e **≈ 125 B por entrada** no volume real C:\ (368 MB para 2,95 M entradas, com todos os índices). A projeção abaixo é **linear e ainda não foi validada em escala**:

| Arquivos | Índice projetado | Estratégia |
|---|---|---|
| 1 M | 0,10 GB (medido) | 1 snapshot |
| 2,6 M (real) | 0,37 GB (medido) | 1 snapshot |
| 100 M | ~12–13 GB | 1 snapshot por volume ou compartilhamento; validar o tempo de finalização (hoje 37–100% do tempo de varredura) |
| 1 B | ~125 GB | **Particionamento obrigatório:** N snapshots (um por volume, compartilhamento ou subárvore de 1º nível), com consultas consolidadas por *fan-out* + *merge* paginado (Fase 5) |

Particionamento (ADR-005): **a partição é o snapshot**, e um snapshot cobre uma raiz. O catálogo (Fase 2) lista os snapshots por origem e data. Uma consulta consolidada executa a mesma consulta paginada em cada partição e faz *k-way merge* com memória O(k × página). A troca do SQLite por segmentos colunares imutáveis (zstd) **só será avaliada com benchmark** de 100 M+ em hardware real.

### 2.4 Retomada após falha (cancelamento, crash, queda de energia, disco cheio)

Invariante: o diretório com `state=1` tem **todas** as entradas diretas gravadas. Com `state=0`, pode ter chunks parciais e até descendentes gravados.

`open_for_resume`:
1. Valida o schema e o estado. Snapshots completos não são retomáveis.
2. Para cada diretório com `state=0`, remove **toda** a subárvore já gravada (CTE recursiva em tabela temporária em disco) e zera os contadores diretos.
3. Recalcula `next_id` e os contadores e incrementa `meta.resumed_count`.
4. Transmite os pendentes, com caminho reconstruído, para a fronteira (com spill).

Resultado: **nenhum registro perdido e nenhum duplicado**. Verificado em teste: os totais após cancelar e retomar são iguais aos de uma varredura direta, e `verify()` não encontra problemas. O custo é refazer só o trabalho que estava em andamento.

Estados do snapshot: `scanning` → `completed` | `completed_with_warnings` | `interrupted` | `failed`. Se o processo morre, o snapshot fica em `scanning` e é tratado como interrompido. **Um snapshot incompleto nunca é exibido como completo** (`Summary.complete = false` → "DADOS PARCIAIS").

### 2.5 Consultas

- Todas paginadas (`LIMIT/OFFSET`, máximo de 10.000 por página).
- Tempo limite e cancelamento com o *progress handler* do SQLite (verificado a cada 10 mil instruções da VM).
- Filtros combináveis: tamanho mín/máx, extensão, antes/depois de uma data, ano (convertido em intervalo de `mtime` para usar o índice), nome contém.
- Rankings: `top_files` (índice `size`), `top_dirs` (índice `t_size`), `list_dir` (filhos de um diretório, por tamanho).
- **Fase 2:** dono/SID (enriquecimento opcional sob demanda, ADR-008), categoria, comparação entre snapshots, paginação *keyset*.

---

## 3. NextCopy

### 3.1 Motor Robocopy (v1, Windows)

- Executável: `%SystemRoot%\System32\Robocopy.exe` por caminho absoluto. **Nunca o PATH, nunca um shell**. Os argumentos vão como vetor (`Command::args`) com `CREATE_NO_WINDOW`.
- Os caminhos passam por `validate_process_arg_path`: absoluto, sem `..`, sem `"`, `*`, `?`, `|`, `<` ou `>`. A origem não pode conter o destino nem vice-versa. Padrões de filtro não podem começar com `/`, o que impede injetar uma opção.
- Log: `/UNILOG:<arquivo>` (UTF-16, sem problema de *code page*) com `/FP /BYTES /NP /NJH`, e `/NDL` na cópia.

**Interpretação sem depender do idioma (ADR-010)**, em ordem de autoridade:

1. **Código de saída (bitmask):** 1 copiados · 2 extras · 4 divergentes · 8 falhas · 16 fatal. 0 a 3 é sucesso; o bit 4 é aviso; o bit 8 é falha parcial; o bit 16 é falha.
2. **Resumo do job, por posição:** as três linhas cujo texto após o primeiro `:` tem exatamente seis inteiros são Diretórios, Arquivos e Bytes (Total, Copiado, Ignorado, Divergente, FALHA, Extras). Os rótulos traduzidos são ignorados.
3. **Linhas de arquivo ou diretório:** último número antes do caminho completo, mais o caminho. Caminho dentro do destino = extra (candidato a exclusão no `/MIR`).
4. **Erros:** padrão `(0x########)` → código Win32 → `ErrorKind` (o mesmo classificador do NextSize).

O resumo só pode **piorar** o veredito, nunca melhorar. Sem resumo, uma execução "OK" vira `CompletedWithWarnings` porque os totais não podem ser confirmados.

**Pausar e retomar (ADR-011):** o Robocopy não tem pausa. Pausar encerra o processo; o resultado fica `Paused` e não `Completed`. Retomar executa de novo: o Robocopy pula arquivos idênticos (tamanho e data) e recopia o que estava incompleto. A retomada no meio de um arquivo só existe com `/Z` (perfil Migração). Isso foi verificado em teste: pausar, retomar e verificar por hash BLAKE3 dá conteúdo idêntico.

**Espelhamento (exclusões):**
1. `copy plan` executa `/L /X` (dry-run) e lista quantos arquivos e diretórios **seriam excluídos**, com amostra.
2. O usuário aprova N exclusões.
3. `run` executa **uma nova prévia** imediatamente antes da cópia. Se houver mais exclusões que as aprovadas, bloqueia com `MirrorApprovalMismatch` e nada é excluído.

**Seleção individual (ADR-012):** até 500 arquivos em até 50 diretórios-pai. Cada diretório-pai gera uma execução sequencial do Robocopy, nunca paralela. Seleções maiores devem usar diretório + filtros. O espelhamento não aceita seleção individual.

### 3.2 Perfis

| Perfil | Threads | Retentativas (R/W) | /Z | Cópia | Sobrescrita | Verificação |
|---|---|---|---|---|---|---|
| Rápida | 16 | 1/5 s | — | DAT | alterados | básica |
| Segura (padrão) | 8 | 3/10 s | — | DAT | alterados | padrão |
| Migração de servidor | 16 | 5/15 s | ✔ | DATSO (ACL + dono) | alterados | padrão (log sem lista de arquivos) |
| Backup incremental | 8 | 3/10 s | — | DAT | alterados | padrão |
| Sincronização | 8 | 3/10 s | — | DAT | só mais antigos (`/XO`) | padrão |
| Espelhamento | 8 | 3/10 s | — | DAT | alterados + **exclui extras** | padrão |

Todos usam `/XJ` (não seguir junctions), `/E` e `/DCOPY:DAT`.

### 3.3 Verificação

| Nível | O que confere | Custo |
|---|---|---|
| Básica | código de saída, resumo, erros | nenhum |
| Padrão | todo arquivo selecionado pelos filtros existe no destino com o mesmo tamanho e `mtime` (±2 s) | 1 `stat` por arquivo |
| Rigorosa | padrão + BLAKE3 completo da origem e do destino | lê tudo duas vezes |

- Divergências esperadas pela política de sobrescrita são classificadas como `kept_by_policy`, não como erro.
- Arquivos em nuvem na origem não são abertos (`not_verified`).
- **Hash parcial nunca é usado como prova.**
- Falha de verificação → tarefa `Failed` com `ErrorKind::Integrity` e alerta crítico.

---

## 4. Tarefas

### 4.1 Modelo

```
TaskRecord { id, kind (size_scan|copy|verify), module, title, spec_json, state, priority,
             resource_keys[], attempt, max_attempts, created/started/finished/updated_at,
             next_retry_at, progress_json, result_json, checkpoint_json,
             error_kind, error_message, owner_pid+owner_start, child_pid+child_start }
```

Máquina de estados:

```
queued ──► running ──► completed | completed_with_warnings
   ▲          │──────► failed ──(transitório e tentativas restantes)──► waiting_retry ──(vencido)──► queued
   │          │──────► paused  (parada segura; retomável)
   │          │──────► cancelling ──► cancelled
   │          └─(processo morreu)──► interrupted   (detectado na próxima inicialização)
   └──────────── resume (ação explícita do usuário): paused | interrupted | failed
```

- `interrupted` e `paused` **nunca** têm `finished_at` e nunca aparecem como concluídos.
- O executor (runner) devolve um `RunOutcome`; somente o manager grava os estados.
- Um pânico no runner é contido (`catch_unwind`) → `Failed(Internal)`.

### 4.2 Agendador

- **Fila limitada:** no máximo 1.000 tarefas em `queued` + `waiting_retry`.
- `max_running` limita as tarefas simultâneas (padrão 2, máximo 16).
- **Chaves de recurso:** `vol:c:` e `unc:\\servidor\share`. Duas tarefas que compartilham uma chave não rodam juntas, o que evita que duas cópias disputem o mesmo NAS.
- Prioridade (alta > normal > baixa), depois FIFO.

### 4.3 Retentativas

- Backoff exponencial limitado: 30 s, 60 s, 120 s… até 15 min, no máximo 5 tentativas.
- **Só para erros transitórios** (rede, timeout, arquivo bloqueado).
- Permissão, disco cheio, autenticação, integridade e estado corrompido não são repetidos: precisam de intervenção do usuário (`ErrorKind::needs_user`).
- O Robocopy já repete cada arquivo (`/R /W`). O nível de tarefa cobre a execução inteira, por exemplo quando o compartilhamento cai.

### 4.4 Recuperação na inicialização

`AppStore::recover_interrupted` roda sempre que o app ou a CLI abre o store:

1. Seleciona as tarefas em `running` ou `cancelling`.
2. Se o dono (pid + horário de início) **ainda está vivo** (outra instância), não mexe.
3. Senão, verifica se o **processo filho** (o Robocopy) ainda roda, também por pid + horário de início, o que evita confusão com pid reciclado.
4. Marca a tarefa como `interrupted`, registra um evento e emite um alerta. Se o filho ainda roda, o alerta pede para aguardar ou encerrar antes de retomar, para evitar cópia dupla.
5. Retomar é explícito: `tasks resume <id>`. Para NextSize, usa a retomada do snapshot (§2.4). Para NextCopy, executa de novo; o Robocopy pula o que já é idêntico.

### 4.5 Monitoramento e diagnóstico (ADR-013)

`ActivityMonitor` recebe amostras de contador de trabalho, bytes, contadores de I/O do processo (`GetProcessIoCounters`), tamanho do log e se o processo está vivo:

| Classificação | Regra | Confiança |
|---|---|---|
| Ativo | qualquer sinal mudou | alta |
| Lento | média móvel curta (τ=30 s) < 30% da média longa (τ=600 s) da **própria tarefa**, por ≥ 180 s, após aquecimento de 120 s | média |
| Sem progresso observável | nenhum sinal mudou por ≥ janela (copy 600 s, scan 300 s) | baixa → média (≥ 3× a janela) |
| Encerrado | processo terminou | alta |

- **Nunca encerra processos.**
- "Sem progresso" não é tratado como travamento definitivo: uma chamada do SO bloqueada num compartilhamento lento pode voltar.
- As causas listadas no alerta são "possíveis causas a verificar", nunca afirmações.

### 4.6 Alertas

- Severidades: info, atenção, erro, crítico.
- Campos: título, módulo, tarefa, horário (primeiro e último), problema, causa provável (**só com evidência**), consequência, ação recomendada, recuperável, detalhes técnicos.
- **Agrupamento:** um alerta aberto com a mesma *fingerprint* (`módulo.tipo.alvo`) visto há menos de 1 h incrementa `count` e eleva a severidade, sem duplicar.

---

## 5. Tratamento de erros

| Camada | Mecanismo |
|---|---|
| Arquivo ou diretório (scan) | `ScanIssue` gravado em `issues`. O diretório recebe `ENUM_ERROR`. A varredura continua e termina como `completed_with_warnings` |
| Persistência (scan) | Erro do SQLite → fatal → cancela os workers → snapshot fica `failed`, **retomável** (disco cheio: liberar espaço e retomar) |
| Spill (fila em disco) | Falha de I/O → fatal com a mesma política |
| Arquivo (copy) | Robocopy `/R /W`; erro classificado pelo código `0x…`; amostra limitada a 200 |
| Execução (copy) | Bitmask + resumo → `CopyOutcome`; erro dominante → política de retentativa |
| Tarefa | `RunOutcome` → manager → estado, evento e alerta |
| Pânico | Contido por thread (workers, writer, runner) → `Failed` |
| Estado do app corrompido | `quick_check` falha → arquivo movido para `app.db.corrupt-<ts>` → novo store + alerta crítico |
| Snapshot corrompido ou incompatível | `check_schema` (assinatura `app`, `schema_version`) → erro claro, sem tentativa de "consertar" dados |

Códigos de saída da CLI: 0 ok · 1 concluído com avisos · 2 falhou · 3 interrompido, pausado ou cancelado · 64 uso ou validação.

---

## 6. Contratos de IPC (Tauri 2 — implementados na Fase 1)

Código: `crates/nexttool-desktop/src/ipc.rs` (lógica testável, sem Tauri) e `commands.rs` (wrappers). Tipos TypeScript espelhados em `ui/src/lib/types.ts`.

Princípios:
- O frontend **nunca** recebe índices completos, apenas páginas. Toda lista é limitada a ≤ 1.000 linhas (padrão 100).
- Cada consulta de snapshot carrega um `request_id` que volta na resposta. A UI descarta respostas obsoletas, e `snapshot_cancel_query` cancela pelo id. Uma nova consulta com o mesmo id cancela a anterior.
- Trabalho pesado roda em `spawn_blocking`; o webview nunca espera por disco ou SQLite.
- Erros chegam como `{ code, message }`. Códigos estáveis: `invalid_path`, `not_found`, `invalid_argument`, `invalid_job`, `invalid_state`, `approval_required`, `cancelled`, `time_limit`, `not_finalized`, `store`, `snapshot`, `engine`, `internal`, `unsupported`.
- **Allowlist (ADR-018):** cada comando é uma permissão explícita na capability. Não há acesso genérico a arquivos, shell ou rede no webview.

### Comandos (36)

| Comando | Entrada | Saída |
|---|---|---|
| `app_info` | — | versão, diretório de dados, portátil?, caminho do Robocopy |
| `app_quit` | — | pausa as tarefas com segurança e encerra (ADR-023) |
| `overview` | — | ativas, falhas recentes, últimas análises e cópias, alertas abertos |
| `tasks_list` | `states?`, `limit`, `offset` | `TaskRecord[]` (página) |
| `task_get` | `id` | `TaskRecord` + 200 eventos mais recentes |
| `task_pause` / `task_resume` / `task_cancel` | `id` | `bool` / — / `bool` (retomar só a partir de pausada, interrompida ou falha) |
| `size_scan_submit` | `{ root, profile, snapshot? }` | `task_id` (snapshot padrão em `<dados>\snapshots\`) |
| `snapshot_summary` | `path` (.nxs) | `Summary` (lê só o `meta`; ~2 ms) |
| `snapshot_list_dir` | `path, dirId, offset, limit, requestId` | `{ request_id, rows: DirRow[], elapsed_ms }` |
| `snapshot_list_subdirs` | `path, dirId, offset, limit, requestId` | só subpastas (árvore), maiores primeiro, paginado |
| `snapshot_dir_info` | `path, dirId` | `{ chain: [id, nome][], subdirs, files, size }` (breadcrumb, total para paginação, tamanho para o treemap) |
| `snapshots_list` | — | catálogo da pasta de snapshots (só `meta` de cada arquivo, ≤ 500) |
| `snapshot_set_label` | `path, label` | rótulo (sem renomear o arquivo) |
| `snapshot_delete` | `path, confirmName` | exclusão permanente: só `.nxs` válidos da pasta do app, não usados por tarefa ativa, com o nome digitado |
| `snapshot_import` / `snapshot_export` | `src` / `path, dest` | importação validada (assinatura, schema, `quick_check`) / exportação consistente via `VACUUM INTO`, sem sobrescrever |
| `snapshot_build_indexes` | `path` | constrói índices de filtro pendentes |
| `snapshot_compare` | `a, b, rel[], limit` | comparação de um nível por caminho (ADR-032) |
| `owners_lookup` | `paths[]` (≤ 200) | proprietário atual de cada caminho (ADR-037) |
| `schedules_list` / `schedule_create` / `schedule_set_enabled` / `schedule_delete` | título, tipo (scan/copy), requisição, recorrência | agendamentos (ADR-038) |
| `snapshot_top_files` / `snapshot_top_dirs` | `path, limit` | `FileRow[]` / `DirRow[]` |
| `snapshot_query` | `path, query: { min_size, max_size, ext, modified_before/after, year, name_contains, offset, limit }, requestId` | `{ request_id, rows, elapsed_ms }`; tempo limite de 30 s |
| `snapshot_cancel_query` | `requestId` | `bool` |
| `snapshot_stats` | `path` | `{ extensions[30], years[] }` |
| `copy_plan` | `CopyRequest` | `DryRunReport` (Robocopy `/L`, nada é alterado) |
| `copy_submit` | `CopyRequest` (+ `approved_deletions` obrigatório no espelhamento) | `task_id` |
| `alerts_list` / `alerts_ack` | `includeAcknowledged, limit` / `id` | `Alert[]` / `bool` |
| `settings_get` / `settings_set` | — / `key, value` (allowlist) | mapa / — |

`CopyRequest = { source, dest, profile, threads?, verify?, exclude_files[], exclude_dirs[], approved_deletions? }`. O `CopyJob` completo é montado e validado no Rust a partir do perfil; a UI não define opções internas.

### Eventos (Rust → UI)

Enviados pelo laço `pump` (250 ms), que também executa o agendador:

| Evento | Carga | Frequência |
|---|---|---|
| `task://update` | `TaskRecord` | a cada mudança de estado ou progresso, ≤ 4 Hz por tarefa, mais um evento final ao sair do conjunto ativo |
| `alert://raised` | `Alert` | novo alerta ou incremento de agrupamento (a UI notifica só a 1ª ocorrência) |
| `app://close-requested` | nº de tarefas em execução | ao fechar a janela com tarefas ativas |

**Seleção NextSize → NextCopy (Fase 4):** a seleção trafegará como **referência** (`snapshot + dir_ids` ou `snapshot + FileQuery`), nunca como lista de caminhos. O Rust revalida contra o disco atual antes de criar a tarefa.

---
## 7. Segurança

- **NextSize somente leitura:** os enumeradores só listam. Nenhum `open` de conteúdo, nenhuma alteração de atributo ou timestamp (`FindFirstFileExW` não altera o atime).
- **Menor privilégio:** nada pede elevação. O modo NTFS rápido (Fase 5) será opcional, explicado e isolado.
- **Credenciais:** nenhuma armazenada. Usa a sessão do Windows e o Gerenciador de Credenciais. Os logs não contêm segredos porque o NextTool não os manipula.
- **Processos filhos:** caminho absoluto do sistema, argumentos estruturados, validação de caminhos e padrões (sem `"`, `*`, `?`, `|`, `<`, `>`; padrões não podem começar com `/`).
- **Operações destrutivas:** só no espelhamento, com prévia, aprovação numérica e reverificação imediatamente antes. Excluir a origem (mover) **não existe na v1**.
- **Snapshots** são tratados como sensíveis porque contêm nomes e estrutura interna. Ficam onde o usuário escolhe; a importação valida assinatura e schema e é aberta somente leitura.
- **Sem telemetria e sem rede própria.** O app não abre conexões.
- Nunca desativa antivírus, EDR, assinatura SMB nem criptografia.

---

## 8. Métricas de benchmark

Coletadas pelo `nexttool-cli size bench` (JSON por linha):

| Métrica | Fonte |
|---|---|
| Tempo de varredura e de finalização | relógio do motor |
| Arquivos/s, diretórios/s | contadores ÷ tempo de varredura |
| Pico de RAM (working set e privado) | `GetProcessMemoryInfo` (pico do processo inteiro) |
| CPU (usuário, kernel, núcleos médios) | `GetProcessTimes` |
| I/O lido e escrito do processo | `GetProcessIoCounters` |
| Tamanho do índice | arquivo `.nxs` |
| Abertura do resumo, top 100, consulta filtrada, listagem da raiz | cronometrado após a varredura |
| Pendentes máximos e spill máximo | progresso do motor |
| Hardware, SO, núcleos | ambiente |

Para NextCopy (Fase 3): tempo, MB/s, arquivos/s, RAM, CPU, comparação com o Robocopy direto nas mesmas condições.
