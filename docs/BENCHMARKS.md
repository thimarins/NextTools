# Benchmarks — Fase 0 (medições reais)

Data: 2026-11-17. Dados brutos: [`benchmarks/results-2026-11-17.jsonl`](../benchmarks/results-2026-11-17.jsonl), uma linha JSON por execução. Nenhum número aqui foi estimado: tudo saiu de execuções do `nexttool` em *release*.

## 1. Ambiente

| Item | Valor |
|---|---|
| CPU | Intel Core i5-10500T @ 2,30 GHz, 6 núcleos / 12 threads |
| RAM | 15,7 GB |
| Disco | ADATA IM2P33F3A 512 GB NVMe SSD, NTFS |
| SO | Windows 11 Pro 10.0.28000 (idioma de instalação 1046 = pt-BR; **Robocopy exibe saída em inglês** nesta máquina) |
| Antivírus | Microsoft Defender, proteção em tempo real **ativa** |
| Build | `cargo build --release` (Rust 1.100, LTO thin, codegen-units 1) |
| Privilégios | usuário comum, **sem administrador** |
| Modo | compatível (`FindFirstFileExW`). O modo NTFS/MFT ainda não existe. |

**Temperatura de cache:** salvo indicação, as execuções são **quentes** (metadados já no cache do Windows). A primeira execução sobre `C:\` é a única **fria** medida.

## 2. Conjuntos de dados

| Conjunto | Arquivos | Diretórios | Bytes | Origem |
|---|---|---|---|---|
| synthetic-100k | 100.000 | 3.992 | 25,6 MB | `nexttool-cli size gen` (≤ 512 B/arquivo, residente na MFT) |
| synthetic-1M | 1.000.000 | 39.992 | 256,0 MB | idem (geração: 477 s) |
| real-C-drive | ≈ 2.607.000 | 339.532 | ≈ 372 GB | volume de sistema **real**, em uso (os números variam alguns arquivos entre execuções) |

Os dados sintéticos não substituem sistemas reais: os nomes são curtos e a árvore é regular.

## 3. NextSize — varredura

Colunas: **varr.** = enumeração + persistência; **final.** = índices + roll-up + agregados; **RAM** = pico do working set do processo inteiro; **CPU** = núcleos médios usados durante a execução.

### 3.1 synthetic-1M

| Backend | Perfil | Threads | Persistir | Varr. | Final. | Arq/s (varr.) | RAM pico | Privado pico | CPU | Índice |
|---|---|---|---|---|---|---|---|---|---|---|
| win32-find | Equilibrado | 8 | sim | 4,63 s | 4,47 s | 215.889 | 75,0 MB | 87,6 MB | 1,80 | 103,8 MB |
| win32-find | Equilibrado | 8 | sim (2ª) | 3,52 s | 4,32 s | 283.768 | 76,5 MB | 89,8 MB | 1,91 | 104,8 MB |
| win32-find | Equilibrado | 8 | **não** | 1,51 s | — | **662.252** | 6,4 MB | 3,7 MB | 6,76 | — |
| std-readdir | Equilibrado | 8 | sim | 5,59 s | 6,01 s | 179.019 | 76,1 MB | 88,9 MB | 1,43 | 104,1 MB |
| win32-find | Econômico | 2 | sim | 8,50 s | 4,05 s | 117.661 | **26,0 MB** | 24,1 MB | 1,07 | 100,5 MB |
| win32-find | Rápido | 24 | sim | 3,53 s | 3,89 s | 283.527 | 158,1 MB | 175,2 MB | 2,44 | 105,3 MB |

### 3.2 real-C-drive (≈ 2,6 M arquivos, 340 mil diretórios, 372 GB, 231 diretórios sem permissão)

| Backend | Perfil | Threads | Persistir | Cache | Varr. | Final. | Arq/s | RAM pico | Privado pico | CPU | Índice | Pendentes máx. | Spill máx. |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| win32-find | Equilibrado | 8 | sim | **frio** | 87,6 s | 12,0 s | 29.762 | 79,8 MB | 94,9 MB | 1,28 | 367,8 MB | 44.014 | 11.343 |
| win32-find | Equilibrado | 8 | sim | quente | 19,2 s | 11,3 s | 135.618 | 81,9 MB | 95,0 MB | 3,38 | 367,6 MB | 49.084 | 16.560 |
| std-readdir | Equilibrado | 8 | sim | quente | 16,9 s | 11,4 s | 154.191 | 82,2 MB | 97,1 MB | 3,72 | 366,5 MB | 47.599 | 16.164 |
| win32-find | Equilibrado | 8 | **não** | quente | 12,3 s | — | 211.332 | 22,3 MB | 19,9 MB | 6,92 | — | 37.793 | 7.054 |
| win32-find | Econômico | 2 | sim | quente | 59,9 s | 10,4 s | 43.506 | **27,5 MB** | 26,8 MB | 1,19 | 360,1 MB | 90.062 | 82.772 |
| win32-find | Rápido | 24 | sim | quente | 15,1 s | 10,6 s | 172.101 | 281,1 MB | 346,9 MB | 4,29 | 371,6 MB | 49.022 | 0 |

### 3.3 Consultas após a varredura (snapshot finalizado)

| Operação | 100k | 1M | real C:\ (2,6M) |
|---|---|---|---|
| Abrir + resumo | 2,0–2,5 ms | 2,1–2,4 ms | 2,1–2,7 ms |
| Top 100 maiores arquivos | 3,5–4,7 ms | 4,2–5,8 ms | 6,0–6,5 ms |
| Consulta filtrada (`.bak`, ≥ 100 B, ordenada por tamanho, 100 linhas) | 3,9–4,9 ms | 5,1–9,1 ms | 6,7–7,3 ms |
| Listar raiz (100 linhas) | 0,3–0,6 ms | 0,3–0,7 ms | 0,2–0,4 ms |

A meta de "visão inicial em menos de 1 s" é atendida com folga: cerca de 2 ms, independentemente do tamanho.

## 4. Conclusões

1. **A memória não cresce com o número de arquivos.**
   - Equilibrado: 75 MB com 1 M arquivos e 82 MB com 2,6 M arquivos (2,6× mais arquivos, +9% de RAM).
   - Econômico: 26–27,5 MB em ambos.
   - O platô corresponde ao cache do SQLite mais os chunks em trânsito, conforme o orçamento do perfil.
   - Sem persistência, a varredura de 2,6 M arquivos usa 22 MB.
2. **O spill da fronteira funcionou em dados reais:** até 82.772 diretórios em disco no perfil Econômico, com resultado idêntico.
3. **O gargalo com persistência é o escritor SQLite único.**
   - A enumeração pura chega a 662 mil arquivos/s (1 M, quente) e 211 mil/s (C:\ real, quente).
   - Com persistência caem para 284 mil/s e 136–172 mil/s.
   - O perfil Rápido (24 threads, 281 MB) ganhou pouco sobre o Equilibrado: mais threads não resolvem o gargalo de escrita.
   - **Prioridade da Fase 2:** otimizar o escritor (INSERT de múltiplas linhas, *prepared statements* por lote, avaliar `synchronous=OFF` durante a ingestão com checkpoint explícito).
4. **A finalização custa de 37% a 100% do tempo de varredura quente** (11 s para C:\). Precisa ser otimizada e medida em 10 M+ antes de afirmar escalabilidade para 100 M.
5. **Índice:** 104 B/arquivo no sintético e ≈ 125 B por entrada no real (368 MB ÷ 2,95 M entradas). Projeção linear: 100 M ≈ 12–13 GB; 1 B ≈ 125 GB, o que exige particionamento (ADR-005). **A projeção não foi validada em escala.**
6. **O backend nativo não foi mais rápido que o `std`** nesta máquina: o `std` foi 12% mais rápido no C:\ quente e o nativo 20% mais rápido no 1M. A diferença está dentro da variação esperada.
   - O nativo continua o padrão porque dá acesso direto à *reparse tag*, aos atributos de nuvem e aos caminhos `\\?\`, e é a base do modo NTFS. Será reavaliado com mais repetições.
   - **Não há base, por enquanto, para afirmar ganho do backend nativo.**
7. **A primeira varredura fria do C:\ levou 87,6 s.** É o número que um usuário vê na primeira vez. Ferramentas que leem a MFT (WizTree) são, por natureza, muito mais rápidas nesse cenário. A comparação com o WizTree está **pendente**: o WizTree não está instalado e o modo MFT é da Fase 5.

## 5. Recuperação após encerramento forçado

O processo `nexttool-cli size scan` foi encerrado com `Stop-Process -Force` (equivalente a um crash) durante a varredura do synthetic-1M, perfil Econômico.

| Etapa | Resultado |
|---|---|
| Snapshot após o kill (1,2 s) | estado `scanning`, exibido como **⚠ DADOS PARCIAIS**: 297.600 arquivos e 11.856 diretórios gravados |
| Próxima execução da CLI | tarefa detectada como `interrupted`; alerta "Tarefa interrompida inesperadamente" emitido |
| `tasks resume 1` | 49 diretórios pendentes retomados; varredura concluída em 2,5 s + 3,4 s de finalização |
| Resultado final | **1.000.000 arquivos · 255.961.589 bytes**, idêntico à saída do gerador |
| `size check` | snapshot íntegro (`quick_check`, órfãos, contadores, roll-up) |

Um segundo teste, com kill aos 4 s, deu o mesmo resultado exato. O cenário com chunks parciais de um diretório grande é coberto pelo teste automatizado `cancel_then_resume_gives_exact_totals_without_duplicates`.

## 6. NextCopy × Robocopy direto

Cópia de synthetic-100k (100 mil arquivos pequenos, 25,6 MB) para o mesmo SSD. Mesmas opções: `/E /COPY:DAT /DCOPY:DAT /R:0 /W:0 /MT:8 /XJ /FP /BYTES /NP /NJH /NDL /UNILOG`.

| Ordem | Robocopy direto | NextCopy (`copy run --profile safe --threads 8 --verify basic`) |
|---|---|---|
| Robocopy primeiro | 50,6 s | 59,5 s |
| NextCopy primeiro | 56,6 s | 54,8 s |

**Conclusão:** a variação entre execuções (±10%, Defender e cache) é maior que qualquer overhead do NextCopy. Não há overhead mensurável com este método. São necessárias mais repetições, arquivos grandes, SMB e NAS (Fase 3).

| Verificação (100 mil arquivos) | Tempo |
|---|---|
| Padrão (existência + tamanho + data) | 8,3 s |
| Rigorosa (BLAKE3 de origem e destino) | 173,1 s |

A verificação rigorosa é dominada pela abertura de 200 mil arquivos pequenos (com o Defender) e hoje roda em **uma única thread**. Paralelizar com limite de I/O fica para a Fase 3.

## 7. Não medido ainda

10 M, 100 M e 1 B arquivos · SMB e NAS reais · rede lenta ou instável · destino sem espaço · CPU e I/O de outros processos durante a varredura (impacto na origem) · IOPS · comparação com WizTree · Linux. Ver [TEST_PLAN.md](TEST_PLAN.md) §4.
