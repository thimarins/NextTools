# Problemas conhecidos e limitações (Fase 0)

## NextSize
1. **Tamanho alocado indisponível** no modo compatível do Windows (ADR-007).
2. **Hardlinks contados por caminho.** A política `per_path` fica registrada no snapshot. A deduplicação virá com o modo NTFS rápido e o scanner Linux.
3. **Nomes não-Unicode** (UTF-16 inválido no Windows, bytes no Linux) são gravados com caracteres de substituição e a flag `NAME_LOSSY`. Se um diretório desses estiver pendente numa *retomada*, o caminho reconstruído fica errado: a listagem falha e é registrada como ocorrência, nunca descartada em silêncio. Correção prevista: gravar o nome bruto (BLOB) quando houver perda.
4. **Chamadas bloqueadas do SO** (um `FindNextFileW` num compartilhamento travado) não podem ser canceladas instantaneamente (ADR-009).
5. **Paralelismo adaptativo ainda não implementado.** A concorrência é fixa por perfil (o remoto começa conservador: 4 threads no Equilibrado). `ScanControl::set_concurrency` já permite o ajuste em tempo real; o controlador AIMD é da Fase 2.
6. A finalização (índices, roll-up, agregados) acontece depois da varredura e **pode levar uma fração relevante do tempo total** em índices grandes (ver BENCHMARKS). Durante esse tempo o estado é `finalizing`.
7. Paginação por `OFFSET`: páginas profundas de consultas grandes ficam mais lentas. Paginação *keyset* na Fase 2.
8. Dono/SID, categorias, diretórios vazios como filtro e comparação de snapshots ainda não estão implementados (Fase 2).
9. O atime no Windows geralmente está desativado ou impreciso no NTFS; ele é exibido como foi informado.
10. `days_from_year` e os filtros por data usam UTC.

## NextCopy
1. **Windows somente** (Robocopy). Não há segundo motor (decisão do projeto).
2. Sem `/Z`, um arquivo interrompido é recopiado do início ao retomar.
3. A amostra de erros contém erros **observados**: um arquivo pode ter dado erro e funcionado numa retentativa. O número autoritativo de falhas vem do resumo do Robocopy.
4. A fixture pt-BR do parser foi **construída** a partir do formato documentado, não capturada de um Windows pt-BR. A validação real está pendente.
5. Com `/NFL` (perfil Migração), o progresso por arquivo não existe durante a execução: o monitoramento usa os contadores de I/O do processo e o resumo final.
6. A verificação padrão compara `mtime` com tolerância de 2 s. Destinos FAT/exFAT ou alguns NAS podem arredondar de forma diferente.
7. A pilha de diretórios da verificação cresce com a largura da árvore. Diretórios com milhões de subdiretórios podem usar dezenas de MB. Reutilizar a fronteira com spill do NextSize está previsto.
8. Agendamento: só com o app aberto (v1). O Agendador de Tarefas do Windows será opcional e com autorização explícita.
9. Origem ou destino sem espaço, autenticação expirada e desconexão SMB **ainda não foram testados** em ambiente real (só a classificação por código).

## Desktop (Fase 1)
1. **Memória do WebView2:** cerca de 164 MB privados em 6 processos, além dos 29 MB do app. Para servidores com pouca RAM, use a CLI.
2. **Explorador em árvore, treemap, filtros na UI e comparação de snapshots** ainda não existem na interface (Fase 2/4). A UI de NextSize mostra resumo, maiores arquivos e pastas, e distribuição por extensão e por ano.
3. **Agendamento** e **seleção NextSize → NextCopy** ainda não estão na UI (Fases 3 e 4).
4. Mudar "Tarefas simultâneas" só vale após reiniciar o app.
5. A UI ainda não tem testes automatizados de componentes. A verificação da Fase 1 foi feita de ponta a ponta no app real via CDP (ver PHASE1_REPORT §2). Playwright/Vitest ficam para a Fase 2.
6. Somente pt-BR. A estrutura de rótulos (`ui/src/lib/i18n.ts`) cobre estados, tipos e perfis; textos de página ainda estão nos componentes.
7. Ao trocar o tema, a primeira pintura usa a preferência salva localmente. Se o `app.db` tiver outro valor (por exemplo, alterado pela CLI), a janela troca de tema na inicialização (poucos ms).
8. A pré-visualização no navegador (fora do app) não acessa os motores, por projeto: mostra "Disponível apenas no aplicativo desktop".

## Plataforma / distribuição
1. Testado apenas em **Windows 11 Pro build 28000**, en-US, CPU i5-10500T, 16 GB, SSD.
2. App desktop testado só com WebView2 Evergreen 158 (Windows 11). Fixed Version e Windows Server: pendentes (DISTRIBUTION.md).
3. O build Linux compila o código portátil, mas **não foi compilado nem testado** num Linux nesta fase.
