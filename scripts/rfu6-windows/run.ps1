<#
.SYNOPSIS
  RFU-6, the Windows run: runs every automatable checklist item of
  claude-notes/plans/2026-11-08-rfu-6-windows-run.md and writes one results file.

.DESCRIPTION
  PowerShell 5.1 compatible. Needs git, rustup/cargo and cargo-nextest; see -Check output
  for the rest. Every item is independent: one failure never stops the others.
  Writes scripts/rfu6-windows/results/rfu6-<host>-<time>.json (machine-readable) and .md
  (human summary). Full logs go under results/logs/ (not committed).

.PARAMETER Only
  Run only these item ids (e.g. -Only i1,i5). Default: all.
.PARAMETER WithPython
  Use uv to build a venv with ipykernel, give it a python3.exe, register a `python3` kernelspec
  for the current user, and put it on PATH for this run, so the Python engine tests run
  instead of skipping.
.PARAMETER PositronDoc
  Optional path of a .qmd you have run in Positron on this machine: item i2b prints the cache
  file name q2 expects for it so you can look for it on disk.
.PARAMETER Publish
  After the run, commit results/*.json and results/*.md and push the current branch to
  origin under the same name. Without it the script prints the commands.
.PARAMETER SkipBuild
  Skip the up-front build item (use when you have just built).
#>
[CmdletBinding()]
param(
    [string[]]$Only,
    [switch]$WithPython,
    [string]$PositronDoc,
    [switch]$Publish,
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Continue'   # native stderr must not abort the script (PS 5.1)
Set-StrictMode -Version 2

$repo = (& git -C $PSScriptRoot rev-parse --show-toplevel).Trim()
if (-not $repo) { Write-Host 'Run this from inside the q2 checkout.'; exit 2 }
Set-Location $repo

$runId      = (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + $env:COMPUTERNAME
$resultsDir = Join-Path $PSScriptRoot 'results'
$logDir     = Join-Path (Join-Path $resultsDir 'logs') $runId
New-Item -ItemType Directory -Force -Path $logDir | Out-Null

$crates = @('quarto', 'quarto-core', 'quarto-preview', 'quarto-hub', 'quarto-hub-provider', 'quarto-ast-reconcile')
$results = New-Object System.Collections.ArrayList
$script:buildFailed = $false

# --------------------------------------------------------------------------------------------
# helpers
# --------------------------------------------------------------------------------------------

function Invoke-Logged {
    # Runs a command line through cmd.exe with stdout+stderr to a log file. Returns exit code.
    param([string]$Name, [string]$CmdLine)
    $log = Join-Path $logDir ($Name + '.log')
    $bat = Join-Path $logDir ($Name + '.cmd')
    Set-Content -Path $bat -Encoding ASCII -Value @(
        '@echo off',
        ($CmdLine + ' > "' + $log + '" 2>&1'),
        'exit /b %ERRORLEVEL%')
    & $bat
    return @{ Exit = $LASTEXITCODE; Log = $log }
}

function Get-Tail {
    param([string]$Path, [int]$Lines = 40)
    if (-not (Test-Path $Path)) { return '' }
    return ((Get-Content -Path $Path -Encoding UTF8 -Tail $Lines) -join "`n")
}

function Get-LogText {
    param([string]$Path)
    if (-not (Test-Path $Path)) { return '' }
    return (Get-Content -Path $Path -Encoding UTF8 -Raw)
}

function Get-NextestCount {
    # "Summary [..] 12 tests run: 12 passed, 3 skipped" -> 12 ; $null when absent
    param([string]$Text)
    $m = [regex]::Match($Text, 'Summary \[[^\]]*\]\s+(\d+) tests? run')
    if ($m.Success) { return [int]$m.Groups[1].Value }
    return $null
}

function Add-Result {
    param([string]$Id, [string]$Title, [string]$Status, [string]$Reason,
          [string]$Proves = '', [string]$NotProves = '', [string]$Tail = '',
          [string]$Caveat = '', [bool]$Known = $false, [string]$Log = '')
    $o = New-Object PSObject -Property ([ordered]@{
        id = $Id; title = $Title; status = $Status; reason = $Reason
        caveat = $Caveat; known_unrelated = $Known
        proves = $Proves; does_not_prove = $NotProves
        output_tail = $Tail; log = $Log })
    [void]$results.Add($o)
    $color = 'Yellow'
    if ($Status -eq 'PASS') { $color = 'Green' } elseif ($Status -eq 'FAIL') { $color = 'Red' }
    Write-Host ('[{0}] {1}  {2}' -f $Status, $Id, $Title) -ForegroundColor $color
    if ($Reason) { Write-Host ('       ' + $Reason) }
}

function Test-Selected {
    param([string]$Id)
    if (-not $Only -or $Only.Count -eq 0) { return $true }
    return ($Only -contains $Id)
}

function Invoke-Nextest {
    # One nextest invocation as one item. $Filter is a nextest filterset (or '' for none).
    param([string]$Id, [string]$Title, [string]$Packages, [string]$Filter, [int]$MinTests,
          [string]$Proves, [string]$NotProves, [string]$Extra = '', [string]$Pre = '')
    if (-not (Test-Selected $Id)) { return }
    if ($script:buildFailed) {
        Add-Result $Id $Title 'SKIP' 'the up-front build (b0) failed; see its log' $Proves $NotProves
        return
    }
    $cmd = 'cargo nextest run ' + $Packages + ' --no-fail-fast'
    if ($Filter) { $cmd += ' -E "' + $Filter + '"' }
    if ($Extra)  { $cmd += ' ' + $Extra }
    if ($Pre)    { $cmd = $Pre + ' && ' + $cmd }
    $r = Invoke-Logged $Id $cmd
    $text = Get-LogText $r.Log
    $n = Get-NextestCount $text
    $caveat = ''
    if ($text -match 'Skipping test: python3') {
        $caveat = 'Python engine tests skipped (python3 with ipykernel not on PATH); rerun with -WithPython'
    }
    if ($r.Exit -ne 0) {
        Add-Result $Id $Title 'FAIL' ('exit code ' + $r.Exit) $Proves $NotProves (Get-Tail $r.Log 60) $caveat $false $r.Log
    } elseif ($null -eq $n -or $n -lt $MinTests) {
        Add-Result $Id $Title 'FAIL' ('expected at least ' + $MinTests + ' tests, nextest ran ' + $n + ' (a filter matched too little)') $Proves $NotProves (Get-Tail $r.Log 30) $caveat $false $r.Log
    } else {
        Add-Result $Id $Title 'PASS' ($n.ToString() + ' tests ran and passed') $Proves $NotProves '' $caveat $false $r.Log
    }
}

# --------------------------------------------------------------------------------------------
# preflight: toolchain
# --------------------------------------------------------------------------------------------

function Test-Tool {
    param([string]$Name, [string]$Required, [string]$Install)
    $c = Get-Command $Name -ErrorAction SilentlyContinue
    $line = New-Object PSObject -Property ([ordered]@{ tool = $Name; required = $Required; found = [bool]$c; install = $Install })
    $tag = 'ok     '
    if (-not $c) { if ($Required -eq 'yes') { $tag = 'MISSING' } else { $tag = 'absent ' } }
    Write-Host ('  {0} {1}{2}' -f $tag, $Name, $(if (-not $c) { '  -> ' + $Install } else { '' }))
    return $line
}

Write-Host "== Toolchain ($runId) =="
$tools = @()
$tools += Test-Tool 'git'    'yes' 'winget install Git.Git'
$tools += Test-Tool 'rustc'  'yes' 'winget install Rustlang.Rustup   (then: rustup default stable-x86_64-pc-windows-msvc)'
$tools += Test-Tool 'cargo'  'yes' 'comes with rustup'
$tools += Test-Tool 'cargo-nextest' 'yes' 'cargo install cargo-nextest --locked'
$tools += Test-Tool 'link'   'no' 'Visual Studio Build Tools, "Desktop development with C++" (cargo finds link.exe itself, so absent from PATH is normal; install only if the build says "linker not found")'
$tools += Test-Tool 'nasm'   'maybe' 'winget install NASM.NASM   (aws-lc-sys may want it; install only if the build fails in aws-lc-sys)'
$tools += Test-Tool 'cmake'  'maybe' 'winget install Kitware.CMake  (same: only if aws-lc-sys fails)'
$tools += Test-Tool 'node'   'no'  'fnm/nvm; Node major per .nvmrc (only some engine tests use it)'
$tools += Test-Tool 'julia'  'no'  'optional: Julia tests skip cleanly without it'
$tools += Test-Tool 'Rscript' 'no' 'optional: knitr tests skip cleanly without it'
$tools += Test-Tool 'uv'     'no'  'only for -WithPython'

$hostTriple = ''
if (Get-Command rustc -ErrorAction SilentlyContinue) {
    $hostTriple = ((& rustc -vV) | Where-Object { $_ -like 'host:*' }) -replace 'host:\s*', ''
    Write-Host ('  rust host: ' + $hostTriple)
    if ($hostTriple -notlike '*windows-msvc') {
        Write-Host '  WARNING: host is not *-windows-msvc; run: rustup default stable-x86_64-pc-windows-msvc'
    }
}
$nvm = ''
if (Test-Path (Join-Path $repo '.nvmrc')) { $nvm = (Get-Content (Join-Path $repo '.nvmrc') -TotalCount 1).Trim() }
if ($nvm -and (Get-Command node -ErrorAction SilentlyContinue)) {
    $nodeMajor = ((& node -v) -replace '^v', '').Split('.')[0]
    Write-Host ('  node ' + $nodeMajor + ' (.nvmrc says ' + $nvm + ')')
}
$autocrlf = (& git config core.autocrlf)
if (-not $autocrlf) { $autocrlf = '(unset)' }
Write-Host ('  git core.autocrlf = ' + $autocrlf + '  (the CRLF item (i7) is only a real test when this is "true")')
$branch = (& git rev-parse --abbrev-ref HEAD).Trim()
$commit = (& git rev-parse HEAD).Trim()
Write-Host ('  checkout: ' + $branch + ' @ ' + $commit.Substring(0, 9))

$missing = @($tools | Where-Object { $_.required -eq 'yes' -and -not $_.found })
if ($missing.Count -gt 0) {
    Write-Host ''
    Write-Host 'Required tools are missing; nothing can run. Install them (commands above) and rerun.' -ForegroundColor Red
    $pre = New-Object PSObject -Property ([ordered]@{ run_id = $runId; error = 'missing required tools'; missing = @($missing | ForEach-Object { $_.tool }); tools = $tools })
    $pre | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 (Join-Path $resultsDir ('rfu6-' + $runId + '.json'))
    exit 3
}

# --------------------------------------------------------------------------------------------
# optional: a python3 with ipykernel, via uv (opt-in; registers a per-user kernelspec)
# --------------------------------------------------------------------------------------------

if ($WithPython) {
    Write-Host '== Python venv via uv =='
    $venv = Join-Path $resultsDir 'venv'
    $scripts = Join-Path $venv 'Scripts'
    & uv venv $venv 2>&1 | Out-Null
    & uv pip install --python (Join-Path $scripts 'python.exe') ipykernel 2>&1 | Out-Null
    Copy-Item -Force (Join-Path $scripts 'python.exe') (Join-Path $scripts 'python3.exe')
    $env:PATH = $scripts + ';' + $env:PATH
    & (Join-Path $scripts 'python3.exe') -m ipykernel install --user --name python3 2>&1 | Out-Null
}
$pyOk = $false
& python3 -I -c 'import ipykernel' 2>$null
if ($LASTEXITCODE -eq 0) { $pyOk = $true }
Write-Host ('  python3 with ipykernel on PATH: ' + $pyOk)

# --------------------------------------------------------------------------------------------
# b0: build everything the items need, once
# --------------------------------------------------------------------------------------------

$pkgs = ($crates | ForEach-Object { '-p ' + $_ }) -join ' '
if (-not $SkipBuild) {
    Write-Host '== b0: build test binaries (first build is slow) =='
    $r = Invoke-Logged 'b0-build' ('cargo nextest run ' + $pkgs + ' --no-run')
    if ($r.Exit -ne 0) {
        $script:buildFailed = $true
        Add-Result 'b0' 'build the test binaries for the six crates' 'FAIL' ('exit code ' + $r.Exit + '. If the error is in aws-lc-sys, install NASM and CMake (see toolchain list) and rerun.') '' '' (Get-Tail $r.Log 60) '' $false $r.Log
    } else {
        Add-Result 'b0' 'build the test binaries for the six crates' 'PASS' 'built' '' '' '' '' $false $r.Log
    }
}

Write-Host '== Items =='

# --------------------------------------------------------------------------------------------
# i1: canonicalize() verbatim path vs the "outside the document directory" comparison
# --------------------------------------------------------------------------------------------
Invoke-Nextest 'i1' 'execute.rs canonicalize(): verbatim \\?\ root vs capture-file prefix comparison' `
    '-p quarto-hub-provider -p quarto-core' `
    'test(a_figure_reported_under_the_materialized_root_is_captured) | test(provider_executes_an_allowed_request_and_writes_a_capture) | test(one_shot_executes_once_and_flushes_to_the_hub) | test(engine::capture_files::)' `
    28 `
    'A figure an engine reports under the canonicalized (on Windows: \\?\C:\...) materialization root is kept by collect_capture_files with a forward-slash relative path (the new probe, a_figure_reported_..., fails on macOS if canonicalize() is removed); a verbatim root works through ProjectContext::discover and the capture upload; the capture_files unit tests build their doc dir from canonicalize() too.' `
    'What a real engine (knitr, Jupyter) reports on Windows: knitr/R may print a plain C:\ path or reject a \\?\ path. If a real R run is wanted, render a knitr document by hand with q2 and look for dropped figures.'

# --------------------------------------------------------------------------------------------
# i2: positron_cache.rs vscode_file_uri and the %APPDATA% rule
# --------------------------------------------------------------------------------------------
Invoke-Nextest 'i2' 'positron_cache.rs: vscode_file_uri (drive letter, %3A, non-ASCII) and %APPDATA% rule' `
    '-p quarto-core' `
    'test(positron_cache::)' `
    16 `
    'The URI encoder and the directory rules are internally consistent for Windows inputs (HostOs::Windows is a parameter, so these unit tests ran on macOS too): drive letter lower-cased, colon as %3A, spaces and non-ASCII as upper-case UTF-8 escapes, the %APPDATA% then %USERPROFILE%\AppData\Roaming fallback, normalize_location on all spellings.' `
    'That REAL Positron on Windows hashes the same URI (the expected strings come from the same port of uri.ts). Step i2b checks that against a real Positron cache file when you give -PositronDoc.'

if (Test-Selected 'i2b') {
    if (-not $PositronDoc) {
        Add-Result 'i2b' 'real Positron cache file name for a document (optional)' 'SKIP' 'no -PositronDoc given' '' ''
    } else {
        # Same algorithm as positron_cache.rs: backslashes -> /, leading /, lower-case drive,
        # %XX for everything outside [A-Za-z0-9-._~/], then sha1(uri)[0:16].ipynb
        $full = (Resolve-Path -LiteralPath $PositronDoc -ErrorAction SilentlyContinue)
        if (-not $full) {
            Add-Result 'i2b' 'real Positron cache file name (optional)' 'SKIP' ('no such file: ' + $PositronDoc)
        } else {
            $p = $full.Path.Replace('\', '/')
            if (-not $p.StartsWith('/')) { $p = '/' + $p }
            if ($p -match '^/[A-Za-z]:') { $p = '/' + $p.Substring(1, 1).ToLower() + $p.Substring(2) }
            $sb = New-Object System.Text.StringBuilder
            foreach ($b in [System.Text.Encoding]::UTF8.GetBytes($p)) {
                $ch = [char]$b
                if (($b -ge 48 -and $b -le 57) -or ($b -ge 65 -and $b -le 90) -or ($b -ge 97 -and $b -le 122) -or $ch -eq '-' -or $ch -eq '.' -or $ch -eq '_' -or $ch -eq '~' -or $ch -eq '/') { [void]$sb.Append($ch) }
                else { [void]$sb.Append(('%{0:X2}' -f $b)) }
            }
            $uri = 'file://' + $sb.ToString()
            $sha = [System.Security.Cryptography.SHA1]::Create()
            $hex = ($sha.ComputeHash([System.Text.Encoding]::UTF8.GetBytes($uri)) | ForEach-Object { $_.ToString('x2') }) -join ''
            $name = $hex.Substring(0, 16) + '.ipynb'
            $roots = @()
            if ($env:QUARTO_POSITRON_CACHE_DIR) { $roots += $env:QUARTO_POSITRON_CACHE_DIR }
            if ($env:APPDATA) { $roots += (Join-Path $env:APPDATA 'Positron') }
            $hit = @()
            foreach ($rt in $roots) {
                if (Test-Path $rt) { $hit += @(Get-ChildItem -Path $rt -Recurse -Filter $name -ErrorAction SilentlyContinue | ForEach-Object { $_.FullName }) }
            }
            if ($hit.Count -gt 0) {
                Add-Result 'i2b' 'real Positron cache file name (optional)' 'PASS' ('expected ' + $name + ' for ' + $uri + ' exists: ' + ($hit -join '; ')) 'Real Positron wrote the file name q2 computes for this path.' ''
            } else {
                Add-Result 'i2b' 'real Positron cache file name (optional)' 'FAIL' ('expected ' + $name + ' for ' + $uri + ' not found under ' + ($roots -join ', ') + '. Run the document in Positron first, or the rule is wrong; list the cache folder and compare.') '' '' ''
            }
        }
    }
}

# --------------------------------------------------------------------------------------------
# i4: the CLI test file that replaced record_export_positron_cli.rs (python3, literal paths)
# --------------------------------------------------------------------------------------------
Invoke-Nextest 'i4' 'convert_positron_cli: python3 use and literal paths' `
    '-p quarto' `
    'test(convert_positron_cli::)' `
    20 `
    'The q2 convert CLI end to end on Windows: exit codes, -o handling, and (only if no caveat is reported) the Python-engine tests that need python3 with ipykernel on PATH. Reading the file found no literal /some/dir path (the old one at line ~113 is gone).' `
    'The engine tests are skipped, silently, unless python3.exe with ipykernel is on PATH (a venv has python.exe, not python3.exe; Microsoft Store python3 is a stub). A PASS with a caveat means they skipped: rerun with -WithPython.'

# --------------------------------------------------------------------------------------------
# i5: julia_engine_e2e kill/pgrep gates
# --------------------------------------------------------------------------------------------
if (Test-Selected 'i5') {
    if ($script:buildFailed) {
        Add-Result 'i5' 'julia_engine_e2e: kill/pgrep cannot run on Windows' 'SKIP' 'the up-front build (b0) failed; see its log'
    } else {
        # pc4a needs QUARTO_PC4A_LIVE=1 and --run-ignored to get past its first gate and reach
        # the unix gate this item is about.
        $r = Invoke-Logged 'i5' 'set "QUARTO_PC4A_LIVE=1" && cargo nextest run -p quarto-core --no-fail-fast --run-ignored all --no-capture -E "test(julia_engine_e2e::pc4a_abandoned_worker_close_busy) | test(julia_engine_e2e::j7_failed_run_does_not_leak_worker)"'
        $text = Get-LogText $r.Log
        $a = $text -match 'pc4a_abandoned_worker_close_busy cleanup requires unix process groups'
        $b = $text -match 'j7 worker-count cleanup requires unix process groups'
        $pr = 'Both tests that call kill/pgrep return at an `if !cfg!(unix)` guard before any process is started, so nothing is signalled on Windows (PASS means both SKIP lines were printed).'
        $np = 'The helpers (kill, pgrep) compile on Windows (b0 proves that) but are never executed here.'
        if ($r.Exit -eq 0 -and $a -and $b) {
            Add-Result 'i5' 'julia_engine_e2e: kill/pgrep cannot run on Windows' 'PASS' 'both unix-gate SKIP lines printed' $pr $np '' '' $false $r.Log
        } else {
            Add-Result 'i5' 'julia_engine_e2e: kill/pgrep cannot run on Windows' 'FAIL' ('exit ' + $r.Exit + '; pc4a SKIP line: ' + $a + '; j7 SKIP line: ' + $b) $pr $np (Get-Tail $r.Log 60) '' $false $r.Log
        }
    }
}

# --------------------------------------------------------------------------------------------
# i6: record/sources.rs display and capture_driver URI test
# --------------------------------------------------------------------------------------------
Invoke-Nextest 'i6' 'record sources: native-separator display and the capture_driver URI test' `
    '-p quarto-core -p quarto-preview' `
    'test(record::sources::) | test(capture_driver::)' `
    75 `
    'The record-source list parser, ~ expansion (accepting \ after ~), is-a-directory checks and the diagnostics run on Windows paths; capture_driver builds the cache file URI with the same vscode_file_uri it later matches against, so the import test is self-consistent on Windows.' `
    'That the printed path in a diagnostic reads well (inspect one: render with -M "execute.record-sources=C:\nope" and check the message shows C:\nope with native separators), and that the URI equals Positron''s (see i2b).'

# --------------------------------------------------------------------------------------------
# i7: CRLF
# --------------------------------------------------------------------------------------------
if (Test-Selected 'i7') {
    $fx = Join-Path $repo 'crates\quarto-core\tests\fixtures'
    $files = @(Get-ChildItem -Path (Join-Path $fx 'positron-conformance'), (Join-Path $fx 'positron-cache') -File -ErrorAction SilentlyContinue)
    $crCount = @()
    foreach ($f in $files) {
        $bytes = [System.IO.File]::ReadAllBytes($f.FullName)
        $cr = 0; foreach ($b in $bytes) { if ($b -eq 13) { $cr++ } }
        if ($cr -gt 0) { $crCount += ($f.Name + '=' + $cr) }
    }
    $note = 'git core.autocrlf=' + $autocrlf + '; fixtures containing CR bytes in this checkout: ' + $(if ($crCount.Count) { $crCount -join ', ' } else { 'none' })
    Write-Host ('       ' + $note)
    Set-Content -Path (Join-Path $logDir 'i7-crlf-bytes.txt') -Value $note
    Invoke-Nextest 'i7' 'CRLF: positron-conformance/crlf.qmd and positron-cache/*.ipynb under autocrlf' `
        '-p quarto-core' `
        'test(positron_conformance::) | test(positron_source::) | test(knitr_positron_import::)' `
        24 `
        ('The conformance, source and knitr-import tests hash the fixtures as checked out (crlf.qmd is a committed-CRLF case; the cache notebooks are LF in git). ' + $note) `
        'Anything under autocrlf=input/false: with autocrlf unset the LF fixtures stay LF and the checkout is not the stressing case. These fixtures are NOT pinned in .gitattributes (unlike pandoc-recordings), which is why this run matters.'
    # attach the CR note to the item just added
    $last = $results[$results.Count - 1]
    if ($last.id -eq 'i7' -and $autocrlf -ne 'true' -and $last.status -eq 'PASS') {
        $last.caveat = 'core.autocrlf is ' + $autocrlf + ', not true: the CRLF-checkout case was not exercised. Reclone with: git clone -c core.autocrlf=true ... and rerun -Only i7'
    }
}

# --------------------------------------------------------------------------------------------
# i8: the epic's command
# --------------------------------------------------------------------------------------------
if (Test-Selected 'i8') {
    if ($script:buildFailed) {
        Add-Result 'i8' 'epic command: clippy then nextest for the six crates' 'SKIP' 'the up-front build (b0) failed; see its log'
    } else {
        Write-Host '== i8: clippy (-D warnings) =='
        $rc = Invoke-Logged 'i8-clippy' ('cargo clippy ' + $pkgs + ' --all-targets -- -D warnings')
        $ctext = Get-LogText $rc.Log
        $locs = @([regex]::Matches($ctext, '-->\s+(\S+?):\d+:\d+') | ForEach-Object { $_.Groups[1].Value + ':' + ($_.Value -replace '.*:(\d+):\d+$', '$1') })
        $known = $false
        if ($rc.Exit -ne 0 -and $locs.Count -gt 0) {
            $unknownLocs = @($locs | Where-Object { $_ -notmatch 'positron_cache\.rs:360$' })
            if ($unknownLocs.Count -eq 0) { $known = $true }
        }
        if ($rc.Exit -eq 0) {
            Add-Result 'i8-clippy' 'clippy -D warnings, six crates' 'PASS' 'clean' '' '' '' '' $false $rc.Log
        } else {
            Add-Result 'i8-clippy' 'clippy -D warnings, six crates' 'FAIL' ('exit ' + $rc.Exit + '; locations: ' + ($locs -join ', ')) 'Compiles and lints on Windows under -D warnings.' '' (Get-Tail $rc.Log 50) '' $known $rc.Log
        }

        Write-Host '== i8: nextest (several minutes) =='
        $rn = Invoke-Logged 'i8-nextest' ('cargo nextest run ' + $pkgs + ' --no-fail-fast')
        $ntext = Get-LogText $rn.Log
        $fails = @([regex]::Matches($ntext, '(?m)^\s+(?:FAIL|TIMEOUT|SIGABRT)\s+\[[^\]]*\]\s+\(\s*\d+/\d+\)\s+(\S+\s+\S+)') | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
        $unknownFails = @($fails | Where-Object { $_ -notmatch 'marimo_records' })
        $cnt = Get-NextestCount $ntext
        $named = @{}
        foreach ($g in @('render_record_sources_cli::', 'convert_positron_cli::', 'record_gate::', 'execute::', 'record::sources::', 'capture_driver::')) {
            $p = [regex]::Matches($ntext, '(?m)^\s+PASS\s+\[[^\]]*\]\s+\(\s*\d+/\d+\)\s+\S+\s+\S*' + [regex]::Escape($g)).Count
            $f = @($fails | Where-Object { $_ -like ('*' + $g + '*') }).Count
            $named[$g] = ('' + $p + ' passed, ' + $f + ' failed')
        }
        $grp = ($named.GetEnumerator() | Sort-Object Name | ForEach-Object { $_.Name + ' ' + $_.Value }) -join '; '
        $kn = ($unknownFails.Count -eq 0 -and $fails.Count -gt 0)
        if ($rn.Exit -eq 0) {
            Add-Result 'i8-nextest' 'nextest, six crates' 'PASS' ($cnt.ToString() + ' tests; ' + $grp) '' '' '' '' $false $rn.Log
        } else {
            Add-Result 'i8-nextest' 'nextest, six crates' 'FAIL' ($fails.Count.ToString() + ' failing: ' + ($fails -join ', ') + '. Groups: ' + $grp) '' '' (Get-Tail $rn.Log 80) '' $kn $rn.Log
        }
        # The record-sources and convert integration tests the plan names, reported on their own.
        $tag = 'PASS'
        $fl = @($fails | Where-Object { $_ -match 'render_record_sources_cli::|convert_positron_cli::|record_gate::' })
        if ($fl.Count -gt 0) { $tag = 'FAIL' }
        Add-Result 'i8-named' 'record-sources (render, preview, provider) and convert integration tests' $tag $grp 'Which of the plan''s named groups passed on Windows (they build paths with Path::join).' '' $(if ($fl.Count) { $fl -join "`n" } else { '' }) '' $false $rn.Log
    }
}

# --------------------------------------------------------------------------------------------
# write results
# --------------------------------------------------------------------------------------------
$out = New-Object PSObject -Property ([ordered]@{
    run_id = $runId; branch = $branch; commit = $commit; host_triple = $hostTriple
    os = (Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue).Caption
    autocrlf = $autocrlf; python3_with_ipykernel = $pyOk
    tools = $tools; items = @($results) })
$jsonPath = Join-Path $resultsDir ('rfu6-' + $runId + '.json')
$out | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 $jsonPath

$md = New-Object System.Collections.ArrayList
[void]$md.Add('# RFU-6 Windows run ' + $runId)
[void]$md.Add('')
[void]$md.Add('Branch ' + $branch + ' at ' + $commit + '; host ' + $hostTriple + '; autocrlf ' + $autocrlf + '.')
[void]$md.Add('')
[void]$md.Add('| item | status | reason |')
[void]$md.Add('|---|---|---|')
foreach ($i in $results) {
    $rs = ($i.reason -replace '\|', '/') -replace "`r?`n", ' '
    $st = $i.status; if ($i.known_unrelated) { $st += ' (known unrelated)' }; if ($i.caveat) { $st += ' (caveat)' }
    [void]$md.Add('| ' + $i.id + ' | ' + $st + ' | ' + $rs + ' |')
}
foreach ($i in ($results | Where-Object { $_.status -eq 'FAIL' -or $_.caveat })) {
    [void]$md.Add('')
    [void]$md.Add('## ' + $i.id + ': ' + $i.title)
    if ($i.caveat) { [void]$md.Add('Caveat: ' + $i.caveat) }
    if ($i.output_tail) { [void]$md.Add('```text'); [void]$md.Add($i.output_tail); [void]$md.Add('```') }
}
$mdPath = Join-Path $resultsDir ('rfu6-' + $runId + '.md')
$md | Set-Content -Encoding UTF8 $mdPath

$nf = @($results | Where-Object { $_.status -eq 'FAIL' }).Count
Write-Host ''
Write-Host ('Results: ' + $jsonPath)
Write-Host ('Summary: ' + $mdPath)
Write-Host ('{0} items, {1} FAIL' -f $results.Count, $nf)

$pushCmd = 'git add scripts/rfu6-windows/results/*.json scripts/rfu6-windows/results/*.md; git commit -m "RFU-6: Windows results"; git push origin HEAD:' + $branch
if ($Publish) {
    & git add $jsonPath $mdPath
    & git commit -m ('RFU-6: Windows results ' + $runId)
    & git push origin ('HEAD:' + $branch)
    Write-Host 'Pushed. Tell the macOS agent to fetch.'
} else {
    Write-Host 'To send the results back:'
    Write-Host ('  ' + $pushCmd)
}
if ($nf -gt 0) { exit 1 } else { exit 0 }
