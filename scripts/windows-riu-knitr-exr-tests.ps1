# Windows run for the execution-records (EXR), knitr and records-in-use (RIU) epics.
#
# TEMPORARY. Lives only on the branch windows/riu-knitr-exr-run; never merge it.
# It runs the checks those epics deferred because Windows is not in CI and the
# aws-lc-sys C build blocks a cross-check from macOS (see RIU epic "Exit criteria",
# the "Windows testing" box, and RFU-6).
#
# Usage (PowerShell 7 recommended; 5.1 works), from anywhere inside the checkout:
#
#   .\scripts\windows-riu-knitr-exr-tests.ps1                 # everything
#   .\scripts\windows-riu-knitr-exr-tests.ps1 -SkipClippy     # tests only
#   .\scripts\windows-riu-knitr-exr-tests.ps1 -SkipPampa      # drop the big pampa suite
#   .\scripts\windows-riu-knitr-exr-tests.ps1 -SkipNode       # drop the TypeScript suite
#   .\scripts\windows-riu-knitr-exr-tests.ps1 -OnlyCrate quarto-core
#
# Prerequisites: Rust (rustup, MSVC toolchain), Visual Studio Build Tools with the C++
# workload, cargo-nextest (cargo install cargo-nextest --locked). aws-lc-sys may also want
# CMake and NASM on PATH. Optional, each unlocks more tests, and tests skip cleanly without
# them: Rscript with knitr, python with ipykernel, julia, node (see .nvmrc).
#
# It never stops at the first failure: every stage runs, then a summary table prints and the
# exit code is 1 if anything failed. Full logs go to target\windows-run\ (one file per stage).
# Send back that directory's *.log files, or at least the summary and the failing test names.

[CmdletBinding()]
param(
    [switch]$SkipClippy,
    [switch]$SkipPampa,
    [switch]$SkipNode,
    [string]$OnlyCrate = ''
)

$ErrorActionPreference = 'Continue'
Set-StrictMode -Version Latest

$repo = (& git rev-parse --show-toplevel).Trim()
Set-Location $repo
$logDir = Join-Path $repo 'target\windows-run'
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

# Crates touched by the three epics (RIU gate list from the epic, plus the error catalog the
# new Q-18 codes live in, plus pampa for the knitr filter and writer changes).
$crates = @(
    'quarto-core',            # presenter, records, splice, knitr engine, Positron locator
    'quarto-preview',         # preview records, --records, stamps, render scripts
    'quarto',                 # q2 binary: preview, provide-hub, record export-positron, render
    'quarto-hub-provider',    # record sources in the provider walk
    'quarto-hub',             # hub resource changes
    'quarto-ast-reconcile',   # remap for stamped spliced output
    'quarto-error-catalog'    # error catalog entries
)
if (-not $SkipPampa) { $crates += 'pampa' }
if ($OnlyCrate) { $crates = @($crates | Where-Object { $_ -eq $OnlyCrate }) }
if ($crates.Count -eq 0) { Write-Error "no such crate in the list: $OnlyCrate"; exit 2 }

$results = New-Object System.Collections.Generic.List[object]
function Add-Result($stage, $status, $detail, $log) {
    $results.Add([pscustomobject]@{ Stage = $stage; Status = $status; Detail = $detail; Log = $log })
}
function Have($cmd) { [bool](Get-Command $cmd -ErrorAction SilentlyContinue) }

# Run a native command with all output redirected into a log (no pipe, so nothing buffers or
# hangs), then return its exit code.
function Run-Logged($log, $exe, $argList) {
    # cmd.exe does the redirection so the bytes land raw: PowerShell 5.1's own `>>` writes
    # UTF-16 and would corrupt a log that also holds our ASCII header and footer.
    "> $exe $($argList -join ' ')" | Out-File -Encoding ascii $log
    $sw = [Diagnostics.Stopwatch]::StartNew()
    & cmd /c "$exe $($argList -join ' ') >> `"$log`" 2>&1"
    $code = $LASTEXITCODE
    $sw.Stop()
    "`n[exit code $code after $([int]$sw.Elapsed.TotalSeconds)s]" | Out-File -Encoding ascii -Append $log
    return $code
}

Write-Host "== Environment" -ForegroundColor Cyan
$envLog = Join-Path $logDir '00-environment.log'
$lines = @()
$lines += "date:    $(Get-Date -Format s)"
$lines += "os:      $([System.Environment]::OSVersion.VersionString)"
$lines += "ps:      $($PSVersionTable.PSVersion)"
$lines += "commit:  $((& git rev-parse --short HEAD).Trim()) on $((& git rev-parse --abbrev-ref HEAD).Trim())"
foreach ($t in 'rustc --version', 'cargo --version', 'cargo nextest --version', 'node --version', 'python --version', 'Rscript --version', 'julia --version') {
    $exe = $t.Split(' ')[0]
    if (Have $exe) { $v = (& cmd /c "$t 2>&1" | Select-Object -First 1); $lines += ("{0,-28} {1}" -f $t, $v) }
    else { $lines += ("{0,-28} NOT FOUND (tests needing it will skip or be absent)" -f $t) }
}
$lines | Tee-Object -FilePath $envLog | Write-Host
if (-not (Have 'cargo')) { Write-Error 'cargo not found'; exit 2 }
& cargo nextest --version *> $null
if ($LASTEXITCODE -ne 0) { Write-Error 'cargo-nextest not installed: cargo install cargo-nextest --locked'; exit 2 }

# Stage 1: clippy, the cross-check the plans deferred. -D warnings catches cfg-gated dead code.
if (-not $SkipClippy) {
    Write-Host "`n== Stage 1: clippy -D warnings (all targets), $($crates -join ', ')" -ForegroundColor Cyan
    $log = Join-Path $logDir '01-clippy.log'
    $a = @('clippy') + ($crates | ForEach-Object { '-p'; $_ }) + @('--all-targets', '--', '-D', 'warnings')
    $code = Run-Logged $log 'cargo' $a
    if ($code -eq 0) { Add-Result 'clippy' 'PASS' '' $log } else { Add-Result 'clippy' 'FAIL' "exit $code" $log }
}

# Stage 2: nextest per crate, --no-fail-fast so one failure does not hide the rest.
$i = 2
foreach ($c in $crates) {
    Write-Host "`n== Stage ${i}: nextest -p $c" -ForegroundColor Cyan
    $log = Join-Path $logDir ("{0:d2}-nextest-{1}.log" -f $i, $c)
    $code = Run-Logged $log 'cargo' @('nextest', 'run', '-p', $c, '--no-fail-fast')
    $summary = (Select-String -Path $log -Pattern 'Summary \[' | Select-Object -Last 1)
    $detail = if ($summary) { $summary.Line.Trim() } else { "exit $code (no nextest summary; probably a build failure)" }
    if ($code -eq 0) { Add-Result "nextest $c" 'PASS' $detail $log } else { Add-Result "nextest $c" 'FAIL' $detail $log }
    $i++
}

# Stage 3: the TypeScript preview-renderer suite (the RIU stamp work touched it).
if (-not $SkipNode -and -not $OnlyCrate) {
    Write-Host "`n== Stage ${i}: ts-packages/preview-renderer (vitest)" -ForegroundColor Cyan
    $log = Join-Path $logDir ("{0:d2}-vitest-preview-renderer.log" -f $i)
    if (-not (Have 'npm')) { Add-Result 'vitest preview-renderer' 'SKIP' 'npm not found' $log }
    else {
        $code = Run-Logged $log 'npm' @('ci')
        if ($code -ne 0) { Add-Result 'npm ci' 'FAIL' "exit $code (needs the Node major in .nvmrc)" $log }
        else {
            $code = Run-Logged $log 'npm' @('test', '-w', 'ts-packages/preview-renderer')
            if ($code -eq 0) { Add-Result 'vitest preview-renderer' 'PASS' '' $log } else { Add-Result 'vitest preview-renderer' 'FAIL' "exit $code" $log }
        }
    }
}

Write-Host "`n== Summary" -ForegroundColor Cyan
$results | Format-Table Stage, Status, Detail -AutoSize -Wrap | Out-String -Width 200 | Write-Host
$failed = @($results | Where-Object { $_.Status -eq 'FAIL' })
if ($failed.Count -gt 0) {
    Write-Host "Failing tests (from the logs):" -ForegroundColor Yellow
    foreach ($f in $failed) {
        Write-Host "--- $($f.Stage)  [$($f.Log)]"
        Select-String -Path $f.Log -Pattern '^\s+(FAIL|SIGABRT|SIGSEGV|TIMEOUT)\s|^error(\[|:)|^warning: unused|^\s+--> ' |
            Select-Object -First 40 | ForEach-Object { Write-Host "  $($_.Line.Trim())" }
    }
    Write-Host "`nLogs: $logDir" -ForegroundColor Yellow
    exit 1
}
Write-Host "All stages passed. Logs: $logDir" -ForegroundColor Green
exit 0
