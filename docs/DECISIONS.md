# Registro de decisões técnicas (ADR)

Formato: contexto → decisão → consequências. Uma decisão só muda com evidência (benchmark, teste ou incidente) registrada aqui.

---

### ADR-001 — Um aplicativo, dois módulos, núcleo pequeno
**Contexto:** NextSize e NextCopy precisam compartilhar tarefas, alertas, histórico e segurança sem acoplar os motores.
**Decisão:** workspace com 10 crates (mais `nexttool-desktop` na Fase 1), consolidado a partir dos 20 sugeridos na especificação. As responsabilidades estão em ARCHITECTURE §1.1. Monitor, alertas e histórico ficam em `nexttool-tasks` e `nexttool-store`, e não em crates próprios. Os motores não dependem de `nexttool-tasks`.
**Consequências:** sem ciclos (o Cargo garante). O ponto de integração é `nexttool-tasks::runners`. Desmembrar um crate depois é barato.

### ADR-002 — Motores antes da interface
**Decisão:** a Fase 0 valida motores, persistência, recuperação e CLI. O desktop (Fase 1) usa as mesmas APIs que a CLI usa.
**Consequências:** todo comportamento crítico tem teste sem interface.

### ADR-003 — Pipeline de varredura limitado com spill em disco
**Contexto:** milhões ou bilhões de arquivos não cabem na RAM. Uma fila de diretórios pendentes pode crescer muito em árvores largas.
**Decisão:** canal limitado (backpressure) entre os workers e um único escritor; fronteira LIFO limitada (`frontier_mem_cap`), com o excedente num arquivo LIFO temporário; chunks de no máximo `chunk_size`.
**Consequências:** a memória do motor não cresce com o número de arquivos. O teste `spill_to_disk_keeps_results_identical` comprova: mais de 2.900 diretórios foram para o spill e o resultado ficou idêntico. Custo: I/O extra apenas quando a fronteira excede o limite.

### ADR-004 — SQLite por snapshot e índices só na finalização
**Contexto:** a especificação pede SQLite inicialmente, e manter índices durante a ingestão reduz muito a taxa de inserção.
**Decisão:** um arquivo `.nxs` (SQLite, WAL, `synchronous=NORMAL`, página de 8 KB) por snapshot. As tabelas são criadas sem índices secundários; os índices, o roll-up e os agregados são criados na finalização. Os commits ocorrem a cada N linhas ou T ms e cada um é um checkpoint.
**Consequências:** a ingestão é rápida, mas há uma fase de finalização (medida no benchmark). Snapshots interrompidos não têm índices: consultas recusam com `NotFinalized`, e a retomada cria apenas os índices de que precisa.

### ADR-005 — Particionamento = snapshot
**Decisão:** não existe banco monolítico. A partição é o snapshot (uma raiz: volume, compartilhamento ou subárvore). Consultas entre partições, na Fase 5, fazem fan-out paginado + k-way merge.
**Consequências:** excluir ou arquivar é apagar um arquivo. Não há limite global de linhas. Agregar vários snapshots custa O(k × página) de memória.

### ADR-006 — Modo NTFS rápido adiado para a Fase 5, com avaliação de bibliotecas
**Contexto:** a especificação pede avaliar `ntfs-reader`, `mft` e `ntfs-core`, e as APIs `FSCTL_ENUM_USN_DATA`, `FSCTL_READ_USN_JOURNAL` e `FSCTL_QUERY_USN_JOURNAL`, além de não criar um parser próprio sem necessidade.
**Decisão:** a Fase 0 usa só o modo compatível. Avaliação planejada para a Fase 5, nesta ordem:
1. `FSCTL_ENUM_USN_DATA` — API oficial; enumera registros (FRN, FRN pai, nome, atributos) **sem tamanhos**. Ainda exigiria uma leitura de tamanhos.
2. Leitura de `$MFT` via `ntfs-reader` — tamanhos e datas num único passe; exige administrador e leitura do volume bruto.
3. O USN Journal só serve para **incremental** (`FSCTL_READ_USN_JOURNAL` + validação de `UsnJournalID` e da continuidade do `NextUsn`; lacuna → nova varredura completa).

Antes de escolher, verificar manutenção, licença, compatibilidade e benchmark contra o WizTree.
**Consequências:** sem privilégio elevado na v1. Comparação com o WizTree pendente.

### ADR-007 — Tamanho alocado "não disponível" no modo compatível
**Contexto:** `FindFirstFileExW` não retorna o tamanho alocado. `GetCompressedFileSizeW` custaria uma chamada por arquivo (lenta em SMB).
**Decisão:** gravar `alloc = NULL` e exibir "não disponível neste modo". No Unix usar `st_blocks × 512`. O modo NTFS rápido trará o tamanho alocado real.
**Consequências:** nenhum valor inventado.

### ADR-008 — Dono/SID sob demanda
**Decisão:** não resolver o dono durante a primeira varredura. A Fase 2 implementa enriquecimento opcional (por subárvore ou resultado filtrado) com cache de SID→nome; no Linux, UID/GID via `statx` sem resolução de nomes durante a varredura.
**Consequências:** a varredura padrão continua no custo de uma chamada por diretório. Filtros de dono mostram "requer enriquecimento" quando o dado não foi coletado.

### ADR-009 — Pausa, cancelamento e chamadas bloqueadas
**Decisão:** pausar para de entregar novos diretórios aos workers; cancelar sinaliza e cada worker para na próxima entrada. Uma chamada do SO já bloqueada (por exemplo, `FindNextFileW` num compartilhamento travado) não pode ser interrompida; isso está documentado e o estado mostra "Cancelando". No nível de tarefa, "pausar" é uma parada segura e retomável (snapshot interrompido + retomada).
**Consequências:** nenhuma promessa de cancelamento instantâneo de I/O do SO.

### ADR-010 — Robocopy interpretado sem depender do idioma
**Decisão:** a ordem de autoridade é código de saída (bitmask) → resumo por posição (linhas com seis inteiros: Diretórios, Arquivos, Bytes) → linhas de arquivo (número + caminho) → erros (`(0x########)`). Rótulos traduzidos nunca são interpretados. O log usa `/UNILOG` (UTF-16).
**Consequências:** o mesmo resultado em en-US e pt-BR (teste com fixture pt-BR *construída*; validação em um Windows pt-BR real está pendente). Sem resumo, o resultado não é declarado limpo.

### ADR-011 — Pausar o Robocopy = encerrar; retomar = executar de novo
**Contexto:** o Robocopy não tem pausa. Suspender o processo com handles SMB abertos é arriscado.
**Decisão:** encerrar o processo (`TerminateProcess`), estado `Paused`. Retomar executa de novo: arquivos idênticos são pulados e o arquivo interrompido é recopiado (ou continuado com `/Z`). Não se promete retomada exata em bytes sem `/Z`.
**Consequências:** validado em teste: pausar, retomar e verificar por BLAKE3 dá conteúdo idêntico.

### ADR-012 — Seleção individual limitada
**Decisão:** até 500 arquivos, agrupados por diretório pai, com no máximo 50 grupos. Cada grupo é uma execução sequencial do Robocopy. Seleções maiores usam diretório + filtros.
**Consequências:** nunca milhares de processos. A linha de comando fica abaixo do limite de 32 K caracteres.

### ADR-013 — Diagnóstico por evidência, nunca encerramento automático
**Decisão:** `ActivityMonitor` classifica a tarefa como Ativo, Lento, Sem progresso observável ou Encerrado, sempre com confiança e evidências. A comparação é com a própria média da tarefa (sem números mágicos absolutos). Nada é encerrado automaticamente.
**Consequências:** poucos falsos positivos (média longa de 10 min, persistência de 3 min). Uma queda curta não gera alerta (testado).

### ADR-014 — Identidade de processo por (pid, horário de início)
**Decisão:** as tarefas guardam `owner_pid + owner_start` e `child_pid + child_start`. Na recuperação, um processo só é considerado vivo se o pid **e** o horário de início coincidirem.
**Consequências:** um pid reciclado não é confundido com o Robocopy antigo.

### ADR-015 — Sem credenciais armazenadas
**Decisão:** usar a sessão do Windows e o Gerenciador de Credenciais. Falha de autenticação → alerta pedindo para conectar pelo Windows.

### ADR-016 — Distribuição: Standard e Offline/Portable
Ver [DISTRIBUTION.md](DISTRIBUTION.md). Standard usa o WebView2 *Evergreen* instalado; Offline/Portable embarca o *Fixed Version*.

### ADR-017 — Dependências da Fase 0
| Crate | Uso | Motivo |
|---|---|---|
| `windows-sys` 0.59 | FFI Win32 | bindings oficiais da Microsoft, sem runtime |
| `rusqlite` 0.32 (bundled, hooks) | SQLite | SQLite embutido, sem DLL externa; `hooks` para o progress handler (cancelamento e tempo limite) |
| `crossbeam-channel` | canal limitado com timeout | padrão de mercado |
| `serde`, `serde_json` | specs e IPC | — |
| `thiserror`, `tracing`, `tracing-subscriber` | erros e logs | — |
| `blake3` | verificação rigorosa | rápido, sem dependências nativas |
| `clap`, `ctrlc` | CLI | — |
| `tempfile` (dev) | testes | — |

**Fase 1 (desktop):** `tauri` 2.x (estável; a 3.0 ainda é alpha e foi descartada), `tauri-build`, `tauri-plugin-dialog` (seletor nativo de pasta/arquivo), `tauri-plugin-single-instance` (ADR-020). UI: React 19, Vite 8, TypeScript **5.9** (a 7.0, com o novo compilador nativo, fica para depois), Tailwind 4, Radix (dialog, tooltip, tabs, switch), `lucide-react`, TanStack Table **8** e Virtual 3, `sonner` (notificações), `clsx` + `tailwind-merge` + `class-variance-authority` (padrão shadcn/ui).

**Não adicionados (ainda):** `tokio` (no Tauri, na Fase 1), `rayon` (não houve agregação na CPU que justificasse), `zstd` (exportação de snapshot, Fase 2), `windows-rs` (o `windows-sys` basta). Auditoria `cargo audit` e `cargo deny` na Fase 6.

---

## Fase 1 — desktop

### ADR-018 — IPC com allowlist explícita
**Contexto:** a especificação pede IPC com comandos e permissões restritos.
**Decisão:** o `build.rs` registra cada comando no *app manifest* do Tauri (`AppManifest::commands`). Com isso, cada comando vira uma permissão `allow-<comando>` que precisa constar em `capabilities/default.json`. A capability concede apenas: os 22 comandos do NextTool, eventos, janela e o diálogo de abrir/salvar. Não há plugin de shell, de sistema de arquivos ou HTTP. A CSP restringe tudo a `'self'` + `ipc:`, e `freezePrototype` está ativo. O teste `command_list_matches_capability` falha se um comando existir sem permissão ou se uma permissão existir sem comando.
**Consequências:** um comando novo exige uma mudança explícita em três lugares (lista, handler e capability), verificada por teste.

### ADR-019 — Nomes dos executáveis
**Contexto:** `nexttool.exe` (CLI) e `NextTool.exe` (desktop) são **o mesmo arquivo** no Windows, que não diferencia maiúsculas. O build do desktop sobrescreveu a CLI, e isso foi encontrado durante os testes da Fase 1.
**Decisão:** o desktop é `NextTool.exe`; a CLI passa a ser `nexttool-cli.exe`.

### ADR-020 — Instância única e reivindicação atômica de tarefas
**Contexto:** duas instâncias do app, ou o app junto com a CLI, poderiam executar o mesmo agendador sobre o mesmo `app.db`.
**Decisão:**
- O app usa `tauri-plugin-single-instance`: uma segunda abertura apenas foca a janela existente.
- Toda execução de tarefa começa com `UPDATE … SET state='running' WHERE id=? AND state='queued'` (`try_claim`). Só quem altera a linha executa.
- A CLI agenda **somente as próprias tarefas** (`tick_only`), então uma execução curta da CLI nunca deixa órfãs tarefas criadas pelo app.

Teste: `two_schedulers_never_run_the_same_task`.

### ADR-021 — Entradas da UI não são normalizadas
**Decisão:** caminhos vindos da UI precisam chegar absolutos e sem `.`/`..`. Eles não são resolvidos contra o diretório de trabalho do processo, o que esconderia *traversal*. Caminhos de snapshot precisam ter extensão `.nxs` e existir. Configurações passam pela allowlist `nexttool_store::SETTINGS`, a mesma usada pela CLI.

### ADR-022 — UI: tema, offline e pré-visualização sem dados simulados
**Decisão:**
- Tokens de cor em OKLCH com matiz configurável (`--hue`); claro, escuro e automático; densidade e escala de fonte via variáveis CSS.
- Fontes do sistema (Segoe UI Variable / Cascadia), sem fontes remotas, porque o app funciona offline.
- Fora do app desktop (navegador), a API devolve `desktop_only` e a UI mostra uma faixa de aviso. **Nenhum dado é simulado.**
- Sem porcentagem falsa: o progresso é indeterminado e mostra contadores reais enquanto o total é desconhecido.

### ADR-023 — Fechar com tarefas em execução
**Decisão:** o fechamento da janela é interceptado. Se há tarefas em execução, a UI pede confirmação; ao confirmar, as tarefas são **pausadas com segurança** (`TaskManager::shutdown`) antes de sair, e ficam retomáveis. Sem tarefas, fecha normalmente.

---

## Fase 1 — complemento (prioridades de 2026-11-18)

### ADR-024 — SQLite mantido; otimizar antes de trocar
**Contexto:** a ingestão + finalização era o gargalo (BENCHMARKS §8). A diretriz é não trocar o SQLite sem benchmark.
**Decisão:** manter o SQLite. Otimizações medidas e adotadas:
- agregados durante a ingestão;
- INSERT multi-linha com *binding* direto;
- ordenação paralela limitada na finalização;
- ANALYZE aproximado.

**Resultado:** −49% no total e −58% na finalização a 11,1 M; ingestão linear até 111 M com 95 MB de RAM.
**Revisar quando:** a finalização a 100 M+ for inaceitável mesmo com índices de filtro adiados, ou quando o particionamento não bastar para 1 B.

### ADR-025 — Agregados calculados durante a ingestão
Por extensão (≤ 200 mil chaves) e por ano: memória limitada.
- São válidos apenas para snapshots gravados numa única sessão.
- Após uma retomada, são recalculados por SQL na finalização.
- Equivalência garantida por teste.

### ADR-026 — Memória da finalização dentro do orçamento do perfil
`PRAGMA threads` × cache por thread ≤ cache SQLite do perfil (Econômico 1 × 8 MB, Equilibrado 2 × 16 MB, Rápido 4 × 16 MB). A configuração 4 × 128 MB, embora mais rápida que a original, levou o pico a 484 MB e foi **rejeitada**. Variáveis `NEXTTOOL_FIN_THREADS` / `NEXTTOOL_FIN_CACHE_KIB` existem só para benchmark.

### ADR-027 — Snapshots parciais consultáveis; finalização recuperável
- Consultas não exigem mais finalização.
- Sem índices, usam varredura de tabela com **tempo limite de 20 s** e caem para tamanhos diretos (`d_size`) enquanto não há roll-up. A UI rotula como "dados parciais".
- O estado `finalizing` é gravado antes da finalização; a retomada aceita esse estado e refaz a finalização, que é idempotente (`CREATE INDEX IF NOT EXISTS`, roll-up que recomeça de `d_*`, agregados com *delete + insert*).

### ADR-028 — Robocopy: erros do log podem rebaixar o veredito do código de saída (achado R-1)
**Contexto:** verificado nesta máquina: com o destino negando escrita, o Robocopy imprime `ERROR 5 … Accessing Destination Directory`, termina com **código 0** e mostra um resumo zerado.
**Decisão** (`exit::reconcile`):
- Erro de **diretório** observado → falha parcial, qualquer que seja o código.
- Qualquer erro observado → nunca "Concluída", no máximo "Concluída com avisos".

**Consequências:** um falso alarme é possível se um diretório falhar e depois funcionar numa retentativa (direção segura). Nunca há falso sucesso. Teste com a saída real capturada.

### ADR-029 — Árvore e listas no desktop sem transferir índices
- A árvore carrega subpastas sob demanda (`snapshot_list_subdirs`, 300 por página, as maiores primeiro) e é virtualizada.
- A lista da pasta é paginada (200 por página) com contagem total (`snapshot_dir_info`).
- Filtros: 100 resultados por página, cancelamento por `request_id` e respostas obsoletas descartadas.
- Nenhuma lista completa cruza o IPC.

---

## Fase 2 — NextSize avançado

### ADR-030 — Índices de filtro adiados
**Contexto:** a 111 M registros, os índices `ext` e `mtime` respondiam por 57% da finalização.
**Decisão:** a finalização tem duas fases.
1. **Essencial** (índices de pastas, `parent` e `size`, roll-up, agregados, ANALYZE) → o snapshot é marcado como concluído e utilizável.
2. **Filtros** (`ext`, `mtime`) → `meta.filter_indexes = ready`.

Enquanto o marcador está `pending`, consultas por extensão ou data fazem varredura com tempo limite. Se o processo parar na fase 2, `build_filter_indexes` (botão na UI, idempotente) completa depois.
**Medido:** a 11,1 M, o snapshot fica utilizável ~33% mais cedo (24–26 s contra 36–38 s).

### ADR-031 — Gestão de snapshots
- Catálogo da pasta de snapshots do app, lendo só o `meta`.
- Rótulo gravado no `meta` (o arquivo não é renomeado, então as referências de tarefas continuam válidas).
- Exportação via `VACUUM INTO`: cópia consistente mesmo com WAL, compacta, sem sobrescrever.
- Importação só após validar assinatura, schema e `quick_check`.
- **Exclusão permanente** apenas de `.nxs` válidos dentro da pasta do app, não usados por tarefas ativas, com o nome do arquivo digitado pelo usuário. Os arquivos analisados nunca são tocados.

### ADR-032 — Comparação preguiçosa por caminho
**Contexto:** os IDs dos nós diferem entre snapshots.
**Decisão:**
- As pastas são casadas por nome sob o mesmo caminho relativo, um nível por vez, como na árvore.
- Cada nível lê no máximo 20.000 subpastas por lado; acima disso, mostra o aviso "comparação parcial".
- O resultado vem ordenado pela maior variação absoluta, e o conteúdo direto aparece como a linha "(arquivos nesta pasta)".

Memória O(subpastas do nível), nunca O(snapshot).

### ADR-033 — Treemap agregado
Canvas com layout *squarified*. Mostra as até 300 maiores entradas da pasta atual; o restante vira "Outros", usando o total real da pasta. Os dados vêm de `snapshot_list_dir` (paginado), nunca do índice inteiro. Clicar numa pasta entra nela.

### ADR-034 — NextSize → NextCopy por referência
"Copiar com NextCopy" envia **apenas o caminho da pasta** (referência), nunca uma lista de arquivos. O NextCopy avisa que o conteúdo pode ter mudado desde a análise e orienta a gerar a Prévia, que enumera o estado atual. A validação normal do `CopyJob` se aplica.

### ADR-035 — Concorrência adaptativa em origens de rede
- Ativa apenas para origens remotas (UNC ou unidade mapeada).
- Começa conservadora (Econômico 2, Equilibrado 4, Rápido 8) com teto de 4, 16 e 32 threads.
- A cada intervalo (2 s), o controlador:
  - testa ±1 thread e mantém o aumento só se a taxa de pastas/s subir ≥ 5%;
  - mantém a redução se a taxa cair no máximo 3% (mesmo resultado com menos carga no servidor);
  - **corta pela metade** ao ver erros de rede ou timeout;
  - não cresce enquanto o escritor estiver saturado.
- Discos locais mantêm concorrência fixa.

Testes: lógica pura (converge para o ótimo de um servidor simulado, a partir de cima e de baixo) e motor com servidor simulado (recua de 8 threads, totais exatos). **Ainda não validado num SMB real.**

### ADR-036 — Categorias por extensão
Doze categorias fixas e documentadas (`nextsize_core::category`), sem inspecionar o conteúdo dos arquivos. Cada extensão pertence a no máximo uma categoria (teste); `ts` fica em Código-fonte, por ambiguidade com vídeo MPEG-TS. As estatísticas por categoria são derivadas dos agregados por extensão, sem nova passada sobre os arquivos. O filtro por categoria vira `ext IN (…)`.

### ADR-037 — Proprietário sob demanda
- O dono não é coletado na varredura (ADR-008).
- A UI consulta o proprietário **atual** (no disco, agora) apenas da página exibida, no máximo 200 caminhos por chamada.
- Usa `GetNamedSecurityInfoW(OWNER)` (somente leitura) e `LookupAccountSidW`, com cache de SID→nome limitado a 10 mil entradas.
- SIDs sem conta são exibidos como "conta inexistente".

## Fase 3 — NextCopy e tarefas

### ADR-038 — Agendamento (v1: com o app aberto)
- Agendamentos no `app.db`, com recorrência única, diária ou semanal em **horário local** (`chrono`; horários inexistentes no horário de verão são pulados, e horários ambíguos usam a primeira ocorrência).
- O laço de eventos verifica a cada 5 s e cria a tarefa pelo mesmo caminho e validação de uma execução imediata.
- **Execuções perdidas** (atraso > 2 min, ou seja, o app estava fechado) **não são executadas depois**: geram alerta e passam para a próxima ocorrência.
- Espelhamento agendado guarda as exclusões aprovadas; na execução, a reverificação bloqueia se houver mais.
- O Agendador de Tarefas do Windows continua fora da v1 (opcional futuro, com autorização explícita).

### ADR-039 — Verificação rigorosa em paralelo
Hash BLAKE3 num pool fixo de até 4 threads com fila limitada (256 tarefas); arquivos em processamento ≤ fila + threads. Resultados idênticos aos da versão sequencial (mesmos testes). Medido: 100 mil arquivos pequenos, 173 s → 51 s.
