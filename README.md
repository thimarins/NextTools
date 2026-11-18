# NextTool

Aplicativo desktop (Windows primeiro) com dois módulos:

- **NextSize**: análise de armazenamento em grande escala (somente leitura).
- **NextCopy**: cópia, sincronização e espelhamento seguros (motor Robocopy na v1).

Estado: **Fase 1 concluída** (motores, persistência, tarefas, recuperação, CLI e app desktop). Veja [docs/PHASE1_REPORT.md](docs/PHASE1_REPORT.md) e [docs/PHASE0_REPORT.md](docs/PHASE0_REPORT.md).

## Requisitos de desenvolvimento

Rust estável (MSVC), Node.js LTS, Visual Studio Build Tools (C++), WebView2 (já incluído no Windows 10/11).

## Compilar e testar

```
cargo test --workspace
cd ui && npm install && npm run build && cd ..
cargo build --release -p nexttool-desktop -p nexttool-cli
```

Gera `target\release\NextTool.exe` (app) e `target\release\nexttool-cli.exe` (linha de comando).
Para o modo portátil, crie um arquivo vazio `portable.flag` ao lado dos executáveis: os dados ficam em `.\data\`.

Desenvolvimento da UI com recarga automática: `cd ui && npm run dev` (no navegador, os motores não estão disponíveis e a UI indica isso; nenhum dado é simulado).

## Exemplos (CLI)

```
nexttool-cli size scan D:\Dados --out D:\analises\dados.nxs --profile balanced
nexttool-cli size info D:\analises\dados.nxs
nexttool-cli size top D:\analises\dados.nxs --n 50
nexttool-cli size query D:\analises\dados.nxs --ext bak --before-year 2021 --min-size 1000000000
nexttool-cli copy plan --src D:\Dados --dst \\nas\backup\dados --profile mirror
nexttool-cli copy run  --src D:\Dados --dst \\nas\backup\dados --profile mirror --approve-deletions 12 --verify strict
nexttool-cli tasks list
nexttool-cli alerts list
nexttool-cli settings set theme dark
```

Documentação: [arquitetura](docs/ARCHITECTURE.md) · [decisões](docs/DECISIONS.md) · [testes](docs/TEST_PLAN.md) · [benchmarks](docs/BENCHMARKS.md) · [distribuição](docs/DISTRIBUTION.md) · [limitações](docs/KNOWN_ISSUES.md)
