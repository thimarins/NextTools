# Relatório da Fase 2 — NextSize avançado (parcial)

Data: 2026-11-18 · Base: `0531cd0`

## 1. Implementado

| Item | Descrição | Teste |
|---|---|---|
| Índices de filtro adiados (ADR-030) | O snapshot fica utilizável antes dos índices `ext`/`mtime`; os filtros funcionam enquanto isso (varredura com tempo limite); recuperação por botão ou `build_filter_indexes` | `filter_indexes_are_deferred_and_recoverable` |
| Gestão de snapshots (ADR-031) | Catálogo, rótulo, exportação (`VACUUM INTO`), importação validada, exclusão com nome digitado e restrita à pasta do app | `catalog_label_and_import_validation`, `snapshot_management_guards` |
| Comparação (ADR-032) | Por caminho, um nível por vez, ordenada pela maior variação; situações nova, removida, cresceu, diminuiu, igual; detalhamento por pasta | `compare_two_real_scans_by_path` + app real |
| Treemap (ADR-033) | Canvas *squarified*, ≤ 300 itens + "Outros", cores do tema e por extensão, clique para entrar | app real |
| NextSize → NextCopy (ADR-034) | "Copiar com NextCopy" leva a pasta como origem, com aviso de revalidação | app real |
| Concorrência adaptativa (ADR-035) | Origens remotas: sobe e desce por medição, corta pela metade com erros de rede, respeita o teto e o escritor saturado; threads ativas mostradas no progresso | 2 testes de lógica + `adaptive_engine_backs_off_from_oversubscription_and_stays_exact` |

IPC: 31 comandos, todos na allowlist, com o teste de consistência passando.

## 2. Testes

- `cargo test --workspace`: **73 passando**, 4 testes de campo preparados (dependem de infraestrutura), 0 falhas.
- TypeScript estrito sem erros.
- App real (release, via CDP em localhost):
  - aba Snapshots salvos;
  - comparação de dois snapshots reais de `copy-src`, antes e depois de mudanças de verdade: `videos` nova +3 MiB, `fotos` removida −1,05 MiB, `docs` +97,7 KiB, somando exatamente +2,05 MiB;
  - treemap do snapshot de 1 M arquivos, com clique para entrar em `dir_000`;
  - "Copiar com NextCopy" preenchendo a origem com o aviso.

## 3. Medições

- Utilizável ~33% mais cedo a 11,1 M (BENCHMARKS §9).
- O controlador adaptativo recuou de 8 threads em direção ao ótimo de um servidor simulado, com totais exatos.

## 4. Problemas encontrados e corrigidos durante a fase

- **O controlador adaptativo só testava aumentos.** Se começasse acima do ótimo do servidor, nunca descia. O teste de convergência mostrou o problema (k=2 terminava em 5). Agora ele testa nos dois sentidos e, com o mesmo rendimento, prefere menos threads.

## 5. Limitações / pendências

- Concorrência adaptativa **não validada em SMB real** (precisa de compartilhamento).
- Comparação limitada a 20.000 subpastas por nível (aviso exibido).
- O treemap mostra um nível por vez; não há treemap aninhado de vários níveis.
- Ainda não implementados:
  - dono/SID sob demanda;
  - categorias de arquivo;
  - paginação *keyset* para páginas muito profundas.

## 6. Próximos passos

- Fase 2 (restante): dono/SID sob demanda com cache, categorias e paginação *keyset*.
- Fase 3: agendamento com o app aberto, prioridade de tarefas na UI, verificação rigorosa paralela.
- Fase 4: diagnóstico e recomendações do NextCopy a partir das métricas.
- Fase 6: empacotamento (NSIS/MSI e Portátil com WebView2 Fixed). Exige baixar ferramentas de build (WiX/NSIS) e o runtime Fixed Version.

---

# Atualização — Fase 2 concluída e início da Fase 3 (2026-11-18)

## Novos itens

| Item | Fase | Teste |
|---|---|---|
| Categorias de arquivo (ranking "Por categoria", filtro por categoria) — ADR-036 | 2 | `categories_from_extensions`, unicidade das extensões |
| Proprietário sob demanda (página atual, ≤ 200, cache de SID) — ADR-037 | 2 | `own_file_has_current_user_as_owner`, `owners_lookup_is_bounded_and_reports_errors` |
| Agendamento com o app aberto (única, diária, semanal; execuções perdidas → alerta) — ADR-038 | 3 | `daily_weekly_once`, `validation`, `schedules_run_when_due_and_missed_runs_are_not_executed_late` + app real |
| Verificação rigorosa em paralelo (173 s → 51 s em 100 mil arquivos) — ADR-039 | 3 | testes de verificação existentes + benchmark |

**Totais:** 80 testes passando, 4 de campo preparados, 0 falhas. IPC com 36 comandos, todos na allowlist.

## Verificação no app real

Agendamento criado pelo formulário do NextSize ("Agendar…", uma vez, daqui a ~1 min). Na hora marcada, o laço de eventos criou a tarefa #8, que terminou concluída; o agendamento passou para "sem próxima execução".

## Pendências que dependem de você

- SMB/NAS real (concorrência adaptativa, queda de rede), volume pequeno (destino sem espaço), log do Robocopy em pt-BR, VMs Windows Server.
- **Empacotamento (Fase 6):** gerar instaladores MSI/NSIS e a versão Portátil exige **baixar** ferramentas (WiX/NSIS, via Tauri) e o WebView2 Fixed Version (Microsoft). Vou pedir sua autorização antes desses downloads.

## Fase 4 (início)

- **Recomendações do NextCopy** (ADR-040): 7 regras com evidência e confiança, exibidas nos detalhes da tarefa; teste `rules_fire_only_with_evidence` e validação com cópia real de 100 mil arquivos.
- **Totais:** 81 testes passando, 4 de campo preparados, 0 falhas.

## O que falta e por quê

| Item | Bloqueio |
|---|---|
| Modo NTFS rápido (MFT/USN) — Fase 5 | Exige ler o volume com **privilégio de administrador**; o app nunca se eleva sozinho. Preciso de autorização para testar numa sessão elevada (ou numa VM). |
| Scanner Linux / NFS — Fase 5 | Precisa de uma máquina Linux (ou WSL) e baixar o alvo de compilação Linux do Rust. |
| Instaladores MSI/NSIS e versão Portátil — Fase 6 | Exige **baixar** ferramentas de build (WiX/NSIS) e o WebView2 Fixed Version. |
| Testes de campo | SMB/NAS, volume pequeno, Windows pt-BR, VMs Windows Server. |
