# NextTool

Aplicativo desktop (Windows primeiro) com dois módulos:

- **NextSize**: análise de armazenamento em grande escala (somente leitura).
- **NextCopy**: cópia, sincronização e espelhamento seguros (motor Robocopy na v1).

Estado: **Fase 0 concluída** (motores, persistência, tarefas, recuperação, CLI). A interface desktop começa na Fase 1. Veja [docs/PHASE0_REPORT.md](docs/PHASE0_REPORT.md).

## Compilar e testar

```
cargo test --workspace
cargo build --release -p nexttool-cli
```

## Exemplos (CLI)

```
nexttool size scan D:\Dados --out D:\analises\dados.nxs --profile balanced
nexttool size info D:\analises\dados.nxs
nexttool size top D:\analises\dados.nxs --n 50
nexttool size query D:\analises\dados.nxs --ext bak --before-year 2021 --min-size 1000000000
nexttool copy plan --src D:\Dados --dst \\nas\backup\dados --profile mirror
nexttool copy run  --src D:\Dados --dst \\nas\backup\dados --profile mirror --approve-deletions 12 --verify strict
nexttool tasks list
nexttool alerts list
```

Documentação: [arquitetura](docs/ARCHITECTURE.md) · [decisões](docs/DECISIONS.md) · [testes](docs/TEST_PLAN.md) · [benchmarks](docs/BENCHMARKS.md) · [distribuição](docs/DISTRIBUTION.md) · [limitações](docs/KNOWN_ISSUES.md)
