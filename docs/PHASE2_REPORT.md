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
