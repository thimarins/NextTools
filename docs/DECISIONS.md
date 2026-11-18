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

**Não adicionados (ainda):** `tokio` (no Tauri, na Fase 1), `rayon` (não houve agregação na CPU que justificasse), `zstd` (exportação de snapshot, Fase 2), `windows-rs` (o `windows-sys` basta). Auditoria `cargo audit` e `cargo deny` na Fase 6.
