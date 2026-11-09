# Tool setup for the Windows run (companion of windows-riu-knitr-exr-tests.ps1).
#
# TEMPORARY. Lives only on the branch windows/riu-knitr-exr-run; never merge it.
# Installs, with no admin rights, everything the test run needs that Windows lacks:
#
#   pandoc   the version CI pins (PANDOC_VERSION in .github/workflows/test-suite.yml)
#   typst    the version CI pins (TYPST_VERSION)
#   uv       the version CI pins (setup-uv `version:`)
#   a uv venv (Python 3.12) with the Jupyter runtime, an ipykernel kernelspec named python3,
#   and the widget/dataframe packages the jupyter-gated tests use (CI's package list).
#
# Marimo needs no setup here: the marimo tests need node and uv on PATH, and uv fetches
# marimo itself when they run.
#
# Everything goes under target\windows-run\tools (inside the checkout, deleted by
# `cargo clean`). The test script prepends these directories to PATH for its own run when
# the tool is not already on PATH, so running this once is enough. To also put them on your
# PATH for every new shell (the way you would normally want pandoc and typst), pass
# -AddToUserPath, then open a new terminal.
#
# Usage (PowerShell 7 recommended; 5.1 works), from anywhere inside the checkout:
#
#   .\scripts\windows-riu-knitr-exr-setup.ps1
#   .\scripts\windows-riu-knitr-exr-setup.ps1 -AddToUserPath
#   .\scripts\windows-riu-knitr-exr-setup.ps1 -Force       # re-download and rebuild everything
#
# If PowerShell refuses to run it, use:
#   powershell -ExecutionPolicy Bypass -File .\scripts\windows-riu-knitr-exr-setup.ps1
#
# Idempotent: a tool already present under the tools directory is left alone unless -Force.
# Still required from you: Rust (MSVC), Visual Studio Build Tools, cargo-nextest, and node
# (see .nvmrc). This script does not touch them.

[CmdletBinding()]
param(
    [switch]$AddToUserPath,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is very slow in 5.1 with the bar
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$repo = (& git rev-parse --show-toplevel).Trim()
Set-Location $repo
$tools = Join-Path $repo 'target\windows-run\tools'
New-Item -ItemType Directory -Force -Path $tools | Out-Null

# Versions come from the CI workflow so this tracks it; the literals are fallbacks.
$wf = Get-Content -Raw (Join-Path $repo '.github\workflows\test-suite.yml')
function Pin($pattern, $fallback) {
    $m = [regex]::Match($wf, $pattern)
    if ($m.Success) { return $m.Groups[1].Value } else { return $fallback }
}
$pandocVer = Pin 'PANDOC_VERSION:\s*"([^"]+)"' '3.12.1'
$typstVer  = Pin 'TYPST_VERSION:\s*"([^"]+)"' '0.15.1'
$uvVer     = Pin '(?s)setup-uv.*?version:\s*"([^"]+)"' '0.13.7'
$pythonVer = '3.12'
$pyPackages = @('jupyter-core', 'ipykernel', 'ipywidgets', 'anywidget', 'plotly', 'great-tables', 'polars', 'pandas')

$arm = ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64')
$typstArch = if ($arm) { 'aarch64' } else { 'x86_64' }
$uvArch = if ($arm) { 'aarch64' } else { 'x86_64' }

function Download-Zip($url, $destDir) {
    if ((Test-Path $destDir) -and -not $Force) { return $false }
    if (Test-Path $destDir) { Remove-Item -Recurse -Force $destDir }
    $zip = Join-Path $tools ([IO.Path]::GetFileName($url))
    Write-Host "  downloading $url"
    Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
    Expand-Archive -Path $zip -DestinationPath $destDir -Force
    Remove-Item -Force $zip
    return $true
}
# Directory (under $root) that holds $exeName, whatever the archive's folder layout was.
function Find-ExeDir($root, $exeName) {
    $hit = Get-ChildItem -Path $root -Recurse -Filter $exeName -File | Select-Object -First 1
    if (-not $hit) { throw "$exeName not found under $root after extraction" }
    return $hit.DirectoryName
}

Write-Host "== pandoc $pandocVer" -ForegroundColor Cyan
# Pandoc ships no native Windows ARM64 zip; the x64 build runs under emulation there.
[void](Download-Zip "https://github.com/jgm/pandoc/releases/download/$pandocVer/pandoc-$pandocVer-windows-x86_64.zip" (Join-Path $tools 'pandoc'))
$pandocDir = Find-ExeDir (Join-Path $tools 'pandoc') 'pandoc.exe'

Write-Host "== typst $typstVer" -ForegroundColor Cyan
[void](Download-Zip "https://github.com/typst/typst/releases/download/v$typstVer/typst-$typstArch-pc-windows-msvc.zip" (Join-Path $tools 'typst'))
$typstDir = Find-ExeDir (Join-Path $tools 'typst') 'typst.exe'

Write-Host "== uv $uvVer" -ForegroundColor Cyan
[void](Download-Zip "https://github.com/astral-sh/uv/releases/download/$uvVer/uv-$uvArch-pc-windows-msvc.zip" (Join-Path $tools 'uv'))
$uvDir = Find-ExeDir (Join-Path $tools 'uv') 'uv.exe'
$uv = Join-Path $uvDir 'uv.exe'

Write-Host "== Python $pythonVer venv with Jupyter" -ForegroundColor Cyan
$venv = Join-Path $tools 'jupyter-venv'
$venvPython = Join-Path $venv 'Scripts\python.exe'
if ($Force -and (Test-Path $venv)) { Remove-Item -Recurse -Force $venv }
if (-not (Test-Path $venvPython)) {
    & $uv venv $venv --python $pythonVer
    if ($LASTEXITCODE -ne 0) { throw "uv venv failed (exit $LASTEXITCODE)" }
}
& $uv pip install --python $venvPython @pyPackages
if ($LASTEXITCODE -ne 0) { throw "uv pip install failed (exit $LASTEXITCODE)" }
& $venvPython -m ipykernel install --user --name python3
if ($LASTEXITCODE -ne 0) { throw "ipykernel install failed (exit $LASTEXITCODE)" }
$venvScripts = Join-Path $venv 'Scripts'

# The test script reads this file to build its PATH. One directory per line.
$dirs = @($pandocDir, $typstDir, $uvDir, $venvScripts)
$dirs | Out-File -Encoding ascii (Join-Path $tools 'paths.txt')

if ($AddToUserPath) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $have = @(($userPath -split ';') | Where-Object { $_ })
    $add = @($dirs | Where-Object { $have -notcontains $_ })
    if ($add.Count -gt 0) {
        [Environment]::SetEnvironmentVariable('Path', (($add + $have) -join ';'), 'User')
        Write-Host "Added to your user PATH (open a new terminal to see it): $($add -join '; ')"
    } else { Write-Host 'User PATH already has all of them.' }
}

# Verify with the PATH the test script will use.
$env:Path = (($dirs -join ';') + ';' + $env:Path)
Write-Host "`n== Verify" -ForegroundColor Cyan
foreach ($t in 'pandoc --version', 'typst --version', 'uv --version', 'python --version', 'jupyter --version', 'jupyter kernelspec list') {
    Write-Host "> $t"
    & cmd /c "$t 2>&1" | Select-Object -First 4 | ForEach-Object { Write-Host "  $_" }
}
Write-Host "`nDone. Now run: .\scripts\windows-riu-knitr-exr-tests.ps1" -ForegroundColor Green
