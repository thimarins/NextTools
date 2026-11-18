# Relatório da Fase 0 — Arquitetura e motores

Data: 2026-11-17 · Commit base: `834fff9` (checkpoint NextSize) → Fase 0

## 1. O que foi implementado

**Preservação e reorganização**
- O estado anterior foi preservado de três formas:
  - commit git `834fff9` no projeto original;
  - `C:\Projetos\NextSize-checkpoint-2026-11-17.zip`;
  - a pasta original `C:\Projetos\NextSize` mantida intacta.
- O projeto foi copiado para `C:\Projetos\NextTool`. O código NextSize existente (modelos e enumerador Windows) foi compilado e testado (6/6) **antes** da reorganização e depois incorporado com `git mv`, mantendo o histórico.
  - `nextsize-windows` → `nexttool-core::fs::win`
  - `enumerate.rs`, `time.rs`, `issue.rs` → `nexttool-core`

**Workspace (10 crates; o desktop entra na Fase 1)**

| Crate | Entregue |
|---|---|
| `nexttool-core` | classificação de erros Win32/errno; enumeração somente leitura (Win32 `FindFirstFileExW` com `\\?\` e `std`); modelos de tarefa e alerta; validação de caminhos e sobreposição; identidade de processo (pid + início); leitura de RAM, CPU e I/O |
| `nexttool-store` | `app.db`: tarefas, eventos (limitados por tarefa), checkpoints, alertas agrupados, configurações; recuperação de tarefas interrompidas; detecção de banco corrompido (preserva e recria); logs rotativos limitados |
| `nexttool-tasks` | agendador com fila limitada, prioridade, concorrência por volume/compartilhamento, retentativa com backoff só para erros transitórios, contenção de pânico; monitor de atividade com evidência e confiança; textos de alerta; runners NextSize e NextCopy |
| `nextsize-core` | modelo de varredura, contratos do pipeline, perfis Econômico/Equilibrado/Rápido |
| `nextsize-scanner` | motor com canal limitado (backpressure), fronteira LIFO limitada com spill em disco, pausa, cancelamento, concorrência ajustável |
| `nextsize-index` | snapshot SQLite com commits em lote (checkpoints), finalização (índices, roll-up, agregados), retomada sem perda nem duplicação, consultas paginadas com tempo limite e cancelamento, verificação de integridade |
| `nextcopy-core` | `CopyJob`, 7 perfis, políticas de sobrescrita, filtros, validação (anti-injeção de opções), contrato `CopyEngine` |
| `nextcopy-robocopy` | Robocopy por caminho absoluto e argumentos estruturados; interpretação independente de idioma (bitmask, resumo por posição, códigos `0x`); prévia, aprovação e reverificação de espelhamento; pausa/retomada; seleção individual limitada; contadores de I/O do processo |
| `nextcopy-verify` | verificação padrão e rigorosa (BLAKE3), respeitando filtros e política de sobrescrita, sem abrir arquivos em nuvem |
| `nexttool-cli` | `nexttool size …`, `copy …`, `tasks …`, `alerts …`, `sys`; benchmark e gerador de massa de teste |

**Documentação:** [ARCHITECTURE](ARCHITECTURE.md) (pipeline, persistência para 100 M e 1 B, retomada, particionamento, contratos de IPC, erros, segurança, métricas) · [DECISIONS](DECISIONS.md) (17 ADRs) · [TEST_PLAN](TEST_PLAN.md) · [BENCHMARKS](BENCHMARKS.md) · [DISTRIBUTION](DISTRIBUTION.md) (Standard × Offline/Portable, WebView2) · [KNOWN_ISSUES](KNOWN_ISSUES.md)

## 2. Testes executados

`cargo test --workspace`: **51 testes, 51 passando** (40 unitários + 11 de integração com arquivos reais e Robocopy real). Detalhe em [TEST_PLAN](TEST_PLAN.md).

Testes manuais em escala (release):
- 100 mil e 1 milhão de arquivos sintéticos e o volume real C:\ (2,6 M arquivos, 372 GB), em 3 perfis e 2 backends, com e sem persistência.
- Encerramento forçado no meio da varredura → recuperação → retomada: totais idênticos ao esperado, duas vezes.
- NextCopy × Robocopy direto em 100 mil arquivos, nas duas ordens.

## 3. Resultados principais (detalhes em [BENCHMARKS](BENCHMARKS.md))

- **Memória limitada e independente do número de arquivos:** Equilibrado 75 MB (1 M) → 82 MB (2,6 M); Econômico 26–27,5 MB.
- **C:\ real (2,6 M arquivos):**
  - quente: 19 s de varredura + 11 s de finalização;
  - frio: 88 s + 12 s;
  - sem persistência: 12 s.
- **Visão inicial de um snapshot:** cerca de 2 ms. Top 100 e consultas filtradas: ≤ 10 ms.
- **Recuperação após kill:** sem perda e sem duplicação (1.000.000 arquivos / 255.961.589 bytes exatos).
- **NextCopy:** sem overhead mensurável sobre o Robocopy direto (a variação de ±10% domina).

## 4. Problemas encontrados e corrigidos durante a fase

1. **Deadlock no motor.** Quando o `ScanControl` permitia menos workers que o configurado, os workers excedentes esperavam para sempre. O teste de spill detectou. Correção: os workers também saem quando a fronteira termina, e o motor não cria mais workers do que o controle permite. Há teste de regressão.
2. **Parser do Robocopy.** Nas linhas de diretório, o número fica no mesmo campo que o rótulo (`*EXTRA Dir   -1`). Correção: usar o último token do campo.
3. **Verificação de seleção individual** não conferia arquivos em subdiretórios. Correção: percorrer os diretórios-pai dos selecionados.
4. **Snapshot parcial exibia 0 arquivos.** Correção: os contadores são gravados no `meta` a cada commit, na mesma transação que as linhas.
5. Teste de pausa ajustado: a cópia a partir do cache do SO terminava antes do evento de pausa.

## 5. Limitações (resumo; lista completa em [KNOWN_ISSUES](KNOWN_ISSUES.md))

- O escritor SQLite único é o gargalo com persistência, e a finalização é cara. Ambos são a prioridade da Fase 2.
- Modo NTFS/MFT, USN incremental, scanner Linux e paralelismo adaptativo ainda não existem (o controle de concorrência em tempo real já existe).
- Dono/SID, categorias e comparação de snapshots: Fase 2.
- Tamanho alocado indisponível no modo compatível; hardlinks contados por caminho.
- Parser pt-BR do Robocopy validado só com fixture **construída**: o Windows desta máquina exibe o Robocopy em inglês.
- Testado em um único sistema (Windows 11, NVMe). Nenhum teste SMB/NAS real, de disco cheio ou de Windows Server.
- **Nenhuma escala acima de 2,6 M arquivos foi validada.** As capacidades para 100 M e 1 B são de projeto, não comprovadas.

## 6. Próximos passos

**Fase 1 — fundação desktop** (precisa de Node.js; autorização para instalar):
- Tauri 2 + React + TS + Tailwind + shadcn/ui + lucide.
- Temas claro, escuro e automático, com cor principal.
- Navegação: Início, NextSize, NextCopy, Histórico, Alertas, Configurações.
- Comandos de IPC conforme ARCHITECTURE §6.
- Central de alertas, lista de tarefas com progresso.

**Fase 2 — NextSize:**
- Escritor otimizado (meta: ≥ 2× a taxa atual com persistência).
- Finalização mais barata.
- Paralelismo adaptativo para SMB.
- Teste de 10 M arquivos.
- Filtros avançados, dono sob demanda, comparação de snapshots, keyset.

**Testes que dependem de você:**
- Compartilhamentos SMB/NAS e VMs Windows Server 2016/2019/2022/2025.
- Um Windows com interface em pt-BR (para capturar o log real do Robocopy).
- Permissão para instalar o WizTree, para a comparação.
