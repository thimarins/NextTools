<#
  Captura um log REAL do Robocopy com as mesmas opções usadas pelo NextCopy,
  para validar o parser em outro idioma (ex.: Windows em pt-BR).

  Uso (em um Windows com interface em pt-BR):
    powershell -ExecutionPolicy Bypass -File scripts\capture-robocopy-log.ps1 -Out C:\temp\robocopy-ptbr.log

  Depois, nesta máquina de desenvolvimento:
    $env:NEXTTOOL_TEST_PTBR_LOG = "C:\temp\robocopy-ptbr.log"
    $env:NEXTTOOL_TEST_PTBR_DEST = "<o caminho de destino impresso abaixo>"
    cargo test -p nexttool-tasks --test field_scenarios real_ptbr -- --ignored

  O script cria e apaga apenas pastas temporárias próprias. Um arquivo é
  mantido aberto de propósito para gerar uma linha de erro real.
#>
param([Parameter(Mandatory = $true)][string]$Out)

$base = Join-Path $env:TEMP ("nexttool-ptbr-" + [guid]::NewGuid().ToString("N").Substring(0, 8))
$src = Join-Path $base "origem"; $dst = Join-Path $base "destino"
New-Item -ItemType Directory -Force "$src\sub" | Out-Null
1..20 | ForEach-Object { Set-Content -Path "$src\arquivo_$_.txt" -Value ("conteúdo " * (100 * $_)) }
1..5 | ForEach-Object { [IO.File]::WriteAllBytes("$src\sub\dados_$_.bin", (New-Object byte[] (50000 * $_))) }
$lock = [IO.File]::Open("$src\arquivo_3.txt", 'Open', 'Read', 'None')
try {
  robocopy $src $dst /E /COPY:DAT /DCOPY:DAT /R:0 /W:0 /MT:8 /XJ /FP /BYTES /NP /NJH /NDL "/UNILOG:$Out" | Out-Null
  "Código de saída do Robocopy: $LASTEXITCODE (esperado 9 = copiados + 1 falha)"
} finally { $lock.Close() }
"Idioma da interface: $((Get-UICulture).Name)"
"Destino usado (NEXTTOOL_TEST_PTBR_DEST): $dst"
"Log salvo em: $Out"
Remove-Item -Recurse -Force $base
