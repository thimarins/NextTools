# Distribuição e runtime WebView2

Status: **planejado** (empacotamento na Fase 6; a casca Tauri começa na Fase 1). Nada aqui foi medido ainda. Os números marcados como *estimativa* serão substituídos por medições reais.

## 1. Duas distribuições

| | **Standard** | **Offline / Portable** |
|---|---|---|
| Formato | Instalador MSI (WiX) e EXE (NSIS) gerados pelo Tauri | ZIP com `NextTool.exe`, `portable.flag`, pasta `webview2\` |
| WebView2 | *Evergreen*, instalado no sistema (vem com Windows 10/11 e é atualizado pelo Windows Update e Edge) | *Fixed Version* embarcado em `webview2\` e referenciado pelo Tauri (`webviewInstallMode: fixedRuntime`) |
| Se o WebView2 estiver ausente | O instalador usa o *bootstrapper* (precisa de internet) ou o instalador offline do Evergreen, se for incluído | Não depende do sistema |
| Dados | `%LOCALAPPDATA%\NextTool` | `.\data\` ao lado do exe (detectado por `portable.flag`) |
| Privilégio | Instalação por usuário (sem admin) ou por máquina | Nenhum |
| Tamanho | *estimativa*: app ~8–15 MB | *estimativa*: app + runtime ~180–200 MB descompactado (o runtime Fixed x64 tem ~150–170 MB conforme a versão) |
| Atualização de segurança do runtime | Automática (Evergreen) | **Manual**: cada release do NextTool precisa embarcar um runtime Fixed recente |

A CLI (`nexttool.exe`) não usa WebView2 e funciona em qualquer Windows Server x64 suportado pelo Rust (Windows 10 / Server 2016+).

## 2. Manutenção de segurança do runtime Fixed Version

- O *Fixed Version* **não se atualiza sozinho**. Ele recebe as correções de segurança do Chromium só quando o NextTool é reempacotado.
- Processo obrigatório a cada release Portable:
  1. Baixar o Fixed Version mais recente (canal Stable) do site oficial da Microsoft.
  2. Verificar a assinatura digital do pacote.
  3. Registrar a versão em `webview2\VERSION.txt` e nas notas de versão.
  4. Executar os testes de interface.
- Política proposta: **uma nova release Portable pelo menos a cada 6 semanas**, ou em até 7 dias após uma correção crítica do Chromium/Edge.
- O app Portable deve exibir em "Sobre" a versão e a data do runtime embarcado e avisar quando ele tiver mais de 90 dias.
- O webview carrega **somente** o conteúdo local empacotado. Nenhuma navegação externa é permitida (CSP restritiva e allowlist de comandos do Tauri), o que reduz a exposição a falhas do motor web.

## 3. Requisitos

| Sistema | CLI | Standard | Portable |
|---|---|---|---|
| Windows 11 / 10 (21H2+) x64 | ✔ | ✔ (WebView2 já presente) | ✔ |
| Windows Server 2025 | ✔ esperado | WebView2 presente por padrão? **verificar** | ✔ esperado |
| Windows Server 2022 | ✔ esperado | WebView2 geralmente ausente na instalação Core/sem Edge — **verificar** | ✔ esperado |
| Windows Server 2019 | ✔ esperado | idem — **verificar** | ✔ esperado |
| Windows Server 2016 | ✔ esperado | idem — **verificar** | ✔ esperado (o WebView2 suporta 2016+) |
| Server Core (sem desktop) | ✔ | ✗ (sem GUI) | ✗ (sem GUI) |

✔ esperado = ainda não testado num sistema real. O único sistema disponível na Fase 0 é **Windows 11 Pro (build 28000)**.

## 4. Plano de teste de compatibilidade (pendente)

Para cada sistema disponível (2016, 2019, 2022, 2025, 10, 11), em VM limpa:
1. CLI: `size scan` de um volume, `copy run` local e para SMB, `tasks list`. Comparar com o Windows 11.
2. Standard: instalar sem internet, verificar a detecção do WebView2 e o comportamento quando ele está ausente.
3. Portable: executar a partir de um compartilhamento e de um pendrive, sem instalar nada, e verificar que nada é gravado fora de `.\data`.
4. Registrar build do SO, versão do WebView2, resultado e problemas em `docs/COMPAT.md`.

**Preciso que os sistemas (VMs ou servidores) sejam disponibilizados para executar este plano.**
