# Plano de testes

Como executar:

```
cargo test --workspace            # unitários + integração (inclui Robocopy real no Windows)
cargo build --release -p nexttool-cli
target\release\nexttool size bench <raiz> --json-out bench.jsonl
```

Legenda: ✅ automatizado e passando · 🧪 executado manualmente na Fase 0 (resultado em BENCHMARKS/PHASE0_REPORT) · ⏳ pendente (fase indicada)

## 1. Unitários (40)

| Área | Testes | Status |
|---|---|---|
| Classificação de erros Win32/errno, retry/needs_user | `nexttool-core::error` | ✅ |
| Validação de caminhos (absoluto, `..`, `\\.\`, aspas) e sobreposição origem/destino | `security` | ✅ |
| Prefixo `\\?\` / `\\?\UNC\`, caminhos > 260, Unicode | `fs::win::path`, `fs::win::find` | ✅ |
| Identidade de processo (pid + início), leitura de RAM/CPU | `process` | ✅ |
| FILETIME → Unix, extensões, unidades | `time`, `ext`, `units` | ✅ |
| Spill LIFO (ida e volta, Unicode, remoção do arquivo) | `nextsize-scanner::spill` | ✅ |
| Fronteira: spill além do limite, drenagem completa sem duplicatas | `frontier` | ✅ |
| Pausa bloqueia, cancelamento libera, worker limitado sai ao fim | `control` | ✅ |
| Datas civis → época (filtro por ano) | `nextsize-index::reader` | ✅ |
| Validação de CopyJob (threads, seleção, padrões que injetam opções, limite de 500) | `nextcopy-core::job` | ✅ |
| Argumentos do Robocopy (perfis, `/MIR /L /X`, filtros, raiz de drive `C:\.`) | `nextcopy-robocopy::args` | ✅ |
| Bitmask de saída → resultado | `exit` | ✅ |
| Log en-US e pt-BR → resultado idêntico; decodificador UTF-16 com leituras partidas e surrogates; UNC | `log` | ✅ (*a fixture pt-BR foi construída, não capturada*) |
| Wildcards `/XF` `/XD` | `nextcopy-verify::wildcard` | ✅ |
| Rotação de logs limita o disco | `nexttool-store::logs` | ✅ |
| Ciclo de vida da tarefa, recuperação após crash, filho vivo detectado | `store` | ✅ |
| Agrupamento de alertas e elevação de severidade | `store` | ✅ |
| Banco corrompido é preservado e recriado | `store` | ✅ |
| Backoff limitado, só erros transitórios | `nexttool-tasks::retry` | ✅ |
| Monitor: ativo → sem progresso → ativo (só I/O); lentidão persistente × queda curta; processo encerrado | `monitor` | ✅ |
| Agendador: mesmo recurso em série, limite de concorrência, retentativa até desistir, alerta agrupado, pânico contido, cancelamento | `manager` | ✅ |

## 2. Integração (11) — arquivos reais

| Cenário | Teste | Status |
|---|---|---|
| Varredura = caminhada de referência independente (arquivos e bytes), backends native e portable, com Unicode, diretórios vazios, caminho > 260, chunks múltiplos por diretório | `scan_matches_reference_walk_for_both_backends` | ✅ |
| Integridade do snapshot (`quick_check`, órfãos, contadores, roll-up), rankings, filtros, listagem | idem | ✅ |
| Fronteira com spill em disco (3.000 diretórios, limite 64) | `spill_to_disk_keeps_results_identical` | ✅ |
| Cancelar no meio → snapshot "interrompido" (não completo) → retomar → totais exatos, sem duplicatas; snapshot completo não é retomável | `cancel_then_resume_gives_exact_totals_without_duplicates` | ✅ |
| Loop de junction não seguido; diretório com acesso negado registrado sem abortar | `junction_loop_is_not_followed_and_access_denied_is_reported` | ✅ |
| Robocopy: cópia completa, idempotente (2ª execução código 0) e verificação rigorosa | `copy_is_complete_idempotent_and_verifies` | ✅ |
| Espelhamento: prévia não altera nada; sem aprovação → bloqueado; aprovação desatualizada → bloqueado; aprovação correta → exclui e verifica | `mirror_requires_matching_approval` | ✅ |
| Arquivo bloqueado → falha parcial, código 32 → `Locked` | `locked_file_is_partial_failure_with_classified_error` | ✅ |
| Origem inexistente → falha fatal `NotFound` | `missing_source_is_fatal` | ✅ |
| Pausar durante a cópia (processo encerrado) → retomar → conteúdo idêntico (BLAKE3) | `pause_stops_process_and_rerun_completes_with_identical_content` | ✅ |
| Seleção individual: um processo por diretório, verificação só dos selecionados | `individual_selection_runs_one_process_per_directory` | ✅ |

## 3. Manuais / escala (Fase 0)

| Cenário | Status |
|---|---|
| 100 mil e 1 milhão de arquivos sintéticos: tempo, RAM, CPU, índice, consultas | 🧪 |
| Volume real C:\ (sem admin): native × portable, com e sem persistência | 🧪 |
| Processo encerrado à força (`Stop-Process`) durante a varredura → recuperação → retomada → integridade | 🧪 |

## 4. Pendentes

| Cenário | Fase |
|---|---|
| 10 M, 100 M arquivos (tempo de finalização e tamanho do índice) | 2–5 |
| 1 B (sintético particionado e real, quando houver ambiente) | 5 |
| SMB real: latência, desconexão no meio da varredura e da cópia (desconectar a rede da VM) | 2–3 |
| NAS lento (simulado com limitação de banda/latência) | 3 |
| Destino sem espaço (VHD pequeno; requer admin para montar) | 3 |
| Disco do cache quase cheio durante a varredura (VHD) | 2 |
| Log Robocopy capturado em **Windows pt-BR real** (substituir a fixture construída) | 3 |
| Hardlinks (contagem por caminho documentada); arquivos esparsos e comprimidos | 2 |
| Placeholders OneDrive (não hidratar na varredura nem na verificação rigorosa) | 2–3 |
| Diretório com 1 M+ entradas | 2 |
| Alterações concorrentes durante a varredura e a cópia | 2–3 |
| Comparação com WizTree e com Robocopy direto | 3–5 |
| Compatibilidade Windows Server 2016/2019/2022/2025 ([DISTRIBUTION.md](DISTRIBUTION.md)) | 6 |
| Testes de interface (Tauri) | 1+ |
| Linux: scanner nativo, NFS | 5 |
| Fuzzing do parser de log e da importação de snapshot | 6 |
| `cargo audit` / `cargo deny` | 6 |

**Ressalva:** testes sintéticos não equivalem a testes reais de alta escala. Os resultados sintéticos estão sempre rotulados como tal.
