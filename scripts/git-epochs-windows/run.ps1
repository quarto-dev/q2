<#
.SYNOPSIS
  GE-15, the Windows run A: runs the new Rust and hub tests of the git epochs epic and writes one results file.
  Temporary tooling on windows/git-epochs; it never lands on feature/git-epochs.

.DESCRIPTION
  PowerShell 5.1 compatible. Needs git (Git for Windows), Rust (MSVC host), cargo-nextest and uv; the preflight
  prints an install command for anything missing. Every item is independent: one failure never stops the others.
  Items a1 to a5 come from items.json (the same file check_filters.sh reads on macOS); a0 is the build and a6 is
  the epic's own command (clippy, then nextest) for the four crates.
  Writes scripts/git-epochs-windows/results/gea-<time>-<host>.json (machine-readable) and .md (human summary).
  Full logs go under results/logs/ (not committed).

.PARAMETER Only
  Run only these item ids (for example -Only a1,a5). Default: all.
.PARAMETER WithTools
  Download pandoc and typst (the versions CI pins in .github/workflows/test-suite.yml) as portable zips into
  results/tools and put them on PATH for this run. Without them, tests that need pandoc or typst fail, which says
  nothing about Windows (this matters for a6).
.PARAMETER Publish
  After the run, commit results/*.json and results/*.md and push the current branch to origin as windows/git-epochs.
  Without it the script prints the commands.
.PARAMETER SkipBuild
  Skip the up-front build item a0 (use when you have just built).
#>
[CmdletBinding()]
param(
    [string[]]$Only,
    [switch]$WithTools,
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

$crates  = @('quarto-git-epochs', 'quarto-hub', 'quarto-preview', 'quarto')
$results = New-Object System.Collections.ArrayList
$script:buildFailed = $false

# --------------------------------------------------------------------------------------------
# helpers
# --------------------------------------------------------------------------------------------

function Invoke-Logged {
    # Runs a command line through cmd.exe with stdout+stderr to a log file. Returns exit code and log path.
    param([string]$Name, [string]$CmdLine)
    $log = Join-Path $logDir ($Name + '.log')
    $bat = Join-Path $logDir ($Name + '.cmd')
    Set-Content -Path $bat -Encoding ASCII -Value @(
        '@echo off',
        ($CmdLine + ' > "' + $log + '" 2>&1'),
        'exit /b %ERRORLEVEL%')
    # Not `& $bat`: that captures cmd's stdout through a pipe and waits for EOF, which a leaked grandchild can
    # hold open for hours. WaitForExit() on the one cmd process has no pipe to wait on.
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $env:ComSpec
    $psi.Arguments = '/c "' + $bat + '"'
    $psi.UseShellExecute = $false
    $p = [System.Diagnostics.Process]::Start($psi)
    $p.WaitForExit()
    return @{ Exit = $p.ExitCode; Log = $log }
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
          [string]$Caveat = '', [bool]$Known = $false, [string]$Log = '', [string]$Catalog = '')
    $o = New-Object PSObject -Property ([ordered]@{
        id = $Id; catalog = $Catalog; title = $Title; status = $Status; reason = $Reason
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
    # One nextest invocation as one item. $Filter is a nextest filterset.
    param($Item, [string]$Caveat = '')
    $id = $Item.id
    if (-not (Test-Selected $id)) { return }
    if ($script:buildFailed) {
        Add-Result $id $Item.title 'SKIP' 'the up-front build (a0) failed; see its log' $Item.proves $Item.does_not_prove '' '' $false '' $Item.catalog
        return
    }
    $cmd = 'cargo nextest run ' + $Item.packages + ' --no-fail-fast -E "' + $Item.filter + '"'
    $r = Invoke-Logged $id $cmd
    $text = Get-LogText $r.Log
    $n = Get-NextestCount $text
    if ($r.Exit -ne 0) {
        Add-Result $id $Item.title 'FAIL' ('exit code ' + $r.Exit) $Item.proves $Item.does_not_prove (Get-Tail $r.Log 60) $Caveat $false $r.Log $Item.catalog
    } elseif ($null -eq $n -or $n -lt [int]$Item.min_tests) {
        Add-Result $id $Item.title 'FAIL' ('expected at least ' + $Item.min_tests + ' tests, nextest ran ' + $n + ' (a filter matched too little)') $Item.proves $Item.does_not_prove (Get-Tail $r.Log 30) $Caveat $false $r.Log $Item.catalog
    } else {
        Add-Result $id $Item.title 'PASS' ($n.ToString() + ' tests ran and passed') $Item.proves $Item.does_not_prove '' $Caveat $false $r.Log $Item.catalog
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
$tools += Test-Tool 'git'    'yes' 'winget install Git.Git   (Git for Windows; the probe is tested against it)'
$tools += Test-Tool 'rustc'  'yes' 'winget install Rustlang.Rustup   (then: rustup default stable-x86_64-pc-windows-msvc)'
$tools += Test-Tool 'cargo'  'yes' 'comes with rustup'
$tools += Test-Tool 'cargo-nextest' 'yes' 'cargo install cargo-nextest --locked'
$tools += Test-Tool 'uv'     'yes' 'winget install astral-sh.uv   (runs make_large_tree.py and extract_failures.py)'
$tools += Test-Tool 'nasm'   'yes' 'winget install NASM.NASM   (the aws-lc-sys crates in quarto-hub need it)'
$tools += Test-Tool 'cmake'  'yes' 'winget install Kitware.CMake   (the aws-lc-sys crates in quarto-hub need it)'
$tools += Test-Tool 'link'   'no'  'Visual Studio Build Tools, "Desktop development with C++" (cargo finds link.exe itself, so absent from PATH is normal; install only if the build says "linker not found")'
$tools += Test-Tool 'node'   'no'  'fnm/nvm; Node major per .nvmrc (only some quarto tests use it)'

$hostTriple = ''
if (Get-Command rustc -ErrorAction SilentlyContinue) {
    $hostTriple = ((& rustc -vV) | Where-Object { $_ -like 'host:*' }) -replace 'host:\s*', ''
    Write-Host ('  rust host: ' + $hostTriple)
    if ($hostTriple -notlike '*windows-msvc') {
        Write-Host '  WARNING: host is not *-windows-msvc; run: rustup default stable-x86_64-pc-windows-msvc'
    }
}
$gitVersion = (& git --version)
Write-Host ('  ' + $gitVersion)
$autocrlf = (& git config core.autocrlf)
if (-not $autocrlf) { $autocrlf = '(unset)' }
$longpaths = (& git config core.longpaths)
if (-not $longpaths) { $longpaths = '(unset)' }
Write-Host ('  git core.autocrlf = ' + $autocrlf + '  (item a3 is only a real CRLF test when this is "true")')
Write-Host ('  git core.longpaths = ' + $longpaths + '  (item a4 wants "true": git config --global core.longpaths true)')
$branch = (& git rev-parse --abbrev-ref HEAD).Trim()
$commit = (& git rev-parse HEAD).Trim()
Write-Host ('  checkout: ' + $branch + ' @ ' + $commit.Substring(0, 9))

# The run records its base commit and checks that it is the current tip of the tooling branch. The RFU-6
# first two runs used a stale base, so this is loud.
$baseNote = 'not checked (no network or no remote)'
& git fetch origin windows/git-epochs 2>&1 | Out-Null
if ($LASTEXITCODE -eq 0) {
    $tip = (& git rev-parse FETCH_HEAD).Trim()
    if ($tip -eq $commit) {
        $baseNote = 'current tip of origin/windows/git-epochs'
    } else {
        $baseNote = 'STALE: origin/windows/git-epochs is at ' + $tip.Substring(0, 9) + ', this checkout is at ' + $commit.Substring(0, 9)
        Write-Host ('  WARNING: ' + $baseNote + '. Run: git pull origin windows/git-epochs') -ForegroundColor Red
    }
}
Write-Host ('  base: ' + $baseNote)

$missing = @($tools | Where-Object { $_.required -eq 'yes' -and -not $_.found })
if ($missing.Count -gt 0) {
    Write-Host ''
    Write-Host 'Required tools are missing; nothing can run. Install them (commands above) and rerun.' -ForegroundColor Red
    $pre = New-Object PSObject -Property ([ordered]@{ run_id = $runId; error = 'missing required tools'; missing = @($missing | ForEach-Object { $_.tool }); tools = $tools })
    $pre | ConvertTo-Json -Depth 5 | Set-Content -Encoding UTF8 (Join-Path $resultsDir ('gea-' + $runId + '.json'))
    exit 3
}

if ($WithTools) {
    Write-Host '== pandoc and typst (portable zips, versions from CI) =='
    $wf = Get-Content -Raw (Join-Path $repo '.github/workflows/test-suite.yml')
    $pv = '3.12.1'; $tv = '0.15.1'
    if ($wf -match 'PANDOC_VERSION:\s*"?([0-9.]+)') { $pv = $Matches[1] }
    if ($wf -match 'TYPST_VERSION:\s*"?([0-9.]+)') { $tv = $Matches[1] }
    $toolsDir = Join-Path $resultsDir 'tools'
    New-Item -ItemType Directory -Force -Path $toolsDir | Out-Null
    $ProgressPreference = 'SilentlyContinue'
    $arm = ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64')
    $typstArch = if ($arm) { 'aarch64' } else { 'x86_64' }
    $dl = @(
        @{ Name = 'pandoc'; Exe = 'pandoc.exe'; Url = "https://github.com/jgm/pandoc/releases/download/$pv/pandoc-$pv-windows-x86_64.zip" },
        @{ Name = 'typst';  Exe = 'typst.exe';  Url = "https://github.com/typst/typst/releases/download/v$tv/typst-$typstArch-pc-windows-msvc.zip" }
    )
    foreach ($t in $dl) {
        $dir = Join-Path $toolsDir $t.Name
        if (-not (Get-ChildItem -Path $dir -Filter $t.Exe -Recurse -ErrorAction SilentlyContinue)) {
            $zip = Join-Path $toolsDir ($t.Name + '.zip')
            Invoke-WebRequest -UseBasicParsing -Uri $t.Url -OutFile $zip
            Expand-Archive -Force -Path $zip -DestinationPath $dir
        }
        $exe = Get-ChildItem -Path $dir -Filter $t.Exe -Recurse | Select-Object -First 1
        $env:PATH = $exe.DirectoryName + ';' + $env:PATH
        Write-Host ('  ' + $t.Name + ': ' + (& $exe.FullName --version | Select-Object -First 1))
    }
}

# --------------------------------------------------------------------------------------------
# a0: build everything the items need, once
# --------------------------------------------------------------------------------------------

$pkgs = ($crates | ForEach-Object { '-p ' + $_ }) -join ' '
if (-not $SkipBuild) {
    Write-Host '== a0: build test binaries (the first build is slow) =='
    $r = Invoke-Logged 'a0-build' ('cargo nextest run ' + $pkgs + ' --no-run')
    if ($r.Exit -ne 0) {
        $script:buildFailed = $true
        Add-Result 'a0' 'build the test binaries for the four crates' 'FAIL' ('exit code ' + $r.Exit + '. If the error is in aws-lc-sys, check NASM and CMake (see the toolchain list) and rerun.') '' '' (Get-Tail $r.Log 60) '' $false $r.Log
    } else {
        Add-Result 'a0' 'build the test binaries for the four crates' 'PASS' 'built' '' '' '' '' $false $r.Log
    }
}

Write-Host '== Items =='

$items = Get-Content -Raw -Encoding UTF8 (Join-Path $PSScriptRoot 'items.json') | ConvertFrom-Json
foreach ($it in $items) {
    $caveat = ''
    if ($it.PSObject.Properties.Name -contains 'needs_autocrlf' -and $it.needs_autocrlf -and $autocrlf -ne 'true') {
        $caveat = 'core.autocrlf is ' + $autocrlf + ', not true: the CRLF-checkout case was not exercised from the checkout. The tests set autocrlf=true in their own temp repos, so they still check the comparison, but reclone with: git clone -c core.autocrlf=true ... for the full item.'
    }
    if ($it.id -eq 'a4' -and $longpaths -ne 'true') {
        $caveat = 'core.longpaths is ' + $longpaths + ', not true: git operations on paths over 260 characters may fail for reasons unrelated to the code.'
    }
    if ($it.PSObject.Properties.Name -contains 'large_tree' -and $it.large_tree -and (Test-Selected $it.id)) {
        # a2: build the deterministic repo with uv and point the test at it.
        $ltDir = Join-Path $resultsDir ('large-tree-' + $runId)
        $lr = Invoke-Logged 'a2-make-tree' ('uv run "' + (Join-Path $PSScriptRoot 'make_large_tree.py') + '" "' + $ltDir + '"')
        if ($lr.Exit -eq 0) {
            $env:GIT_EPOCHS_LARGE_TREE_REPO = $ltDir
        } else {
            $caveat = 'make_large_tree.py failed (see a2-make-tree.log); the test generated its own tree instead.'
        }
    }
    Invoke-Nextest $it $caveat
    if ($it.PSObject.Properties.Name -contains 'large_tree' -and $it.large_tree) {
        Remove-Item Env:\GIT_EPOCHS_LARGE_TREE_REPO -ErrorAction SilentlyContinue
    }
}

# --------------------------------------------------------------------------------------------
# a6: the epic's own gate on the Windows host target
# --------------------------------------------------------------------------------------------
if (Test-Selected 'a6') {
    if ($script:buildFailed) {
        Add-Result 'a6' 'clippy then nextest for the four crates' 'SKIP' 'the up-front build (a0) failed; see its log' '' '' '' '' $false '' 'W10'
    } else {
        $a6Proves = 'The epic''s own gate is green on the Windows host target.'
        $a6Not = 'Tests that need pandoc or typst unless -WithTools was given. Failures that also appear in the macOS baseline are listed as known unrelated in the plan notes.'
        Write-Host '== a6: clippy (-D warnings) =='
        $rc = Invoke-Logged 'a6-clippy' ('cargo clippy ' + $pkgs + ' --all-targets -- -D warnings')
        if ($rc.Exit -eq 0) {
            Add-Result 'a6-clippy' 'clippy -D warnings, four crates' 'PASS' 'clean' $a6Proves $a6Not '' '' $false $rc.Log 'W10'
        } else {
            $ctext = Get-LogText $rc.Log
            $locs = @([regex]::Matches($ctext, '-->\s+(\S+?:\d+):\d+') | ForEach-Object { $_.Groups[1].Value })
            Add-Result 'a6-clippy' 'clippy -D warnings, four crates' 'FAIL' ('exit ' + $rc.Exit + '; locations: ' + ($locs -join ', ')) $a6Proves $a6Not (Get-Tail $rc.Log 50) '' $false $rc.Log 'W10'
        }

        Write-Host '== a6: nextest (several minutes) =='
        $rn = Invoke-Logged 'a6-nextest' ('cargo nextest run ' + $pkgs + ' --no-fail-fast')
        $ntext = Get-LogText $rn.Log
        $fails = @([regex]::Matches($ntext, '(?m)^\s+(?:FAIL|TIMEOUT|SIGABRT)\s+\[[^\]]*\]\s+\(\s*\d+/\d+\)\s+(\S+\s+\S+)') | ForEach-Object { $_.Groups[1].Value } | Sort-Object -Unique)
        $cnt = Get-NextestCount $ntext
        if ($rn.Exit -eq 0) {
            Add-Result 'a6-nextest' 'nextest, four crates' 'PASS' ($cnt.ToString() + ' tests ran and passed') $a6Proves $a6Not '' '' $false $rn.Log 'W10'
        } else {
            Add-Result 'a6-nextest' 'nextest, four crates' 'FAIL' ($fails.Count.ToString() + ' failing: ' + ($fails -join ', ')) $a6Proves $a6Not (Get-Tail $rn.Log 80) '' $false $rn.Log 'W10'
        }
    }
}

# --------------------------------------------------------------------------------------------
# write results
# --------------------------------------------------------------------------------------------
$out = New-Object PSObject -Property ([ordered]@{
    run_id = $runId; branch = $branch; commit = $commit; base = $baseNote; host_triple = $hostTriple
    os = (Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue).Caption
    git = $gitVersion; autocrlf = $autocrlf; longpaths = $longpaths
    tools = $tools; items = @($results) })
$jsonPath = Join-Path $resultsDir ('gea-' + $runId + '.json')
$out | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 $jsonPath

$md = New-Object System.Collections.ArrayList
[void]$md.Add('# GE-15 Windows run ' + $runId)
[void]$md.Add('')
[void]$md.Add('Branch ' + $branch + ' at ' + $commit + ' (' + $baseNote + '); host ' + $hostTriple + '; ' + $gitVersion + '; autocrlf ' + $autocrlf + '; longpaths ' + $longpaths + '.')
[void]$md.Add('')
[void]$md.Add('| item | catalog | status | reason |')
[void]$md.Add('|---|---|---|---|')
foreach ($i in $results) {
    $rs = ($i.reason -replace '\|', '/') -replace "`r?`n", ' '
    $st = $i.status; if ($i.known_unrelated) { $st += ' (known unrelated)' }; if ($i.caveat) { $st += ' (caveat)' }
    [void]$md.Add('| ' + $i.id + ' | ' + $i.catalog + ' | ' + $st + ' | ' + $rs + ' |')
}
foreach ($i in ($results | Where-Object { $_.status -eq 'FAIL' -or $_.caveat })) {
    [void]$md.Add('')
    [void]$md.Add('## ' + $i.id + ': ' + $i.title)
    if ($i.caveat) { [void]$md.Add('Caveat: ' + $i.caveat) }
    if ($i.output_tail) { [void]$md.Add('```text'); [void]$md.Add($i.output_tail); [void]$md.Add('```') }
}
$mdPath = Join-Path $resultsDir ('gea-' + $runId + '.md')
$md | Set-Content -Encoding UTF8 $mdPath

$nf = @($results | Where-Object { $_.status -eq 'FAIL' }).Count
Write-Host ''
Write-Host ('Results: ' + $jsonPath)
Write-Host ('Summary: ' + $mdPath)
Write-Host ('{0} items, {1} FAIL' -f $results.Count, $nf)
if ($nf -gt 0) {
    Write-Host 'To explain the failures: uv run scripts/git-epochs-windows/extract_failures.py   (then commit its output with the results)'
}

$pushCmd = 'git add scripts/git-epochs-windows/results/*.json scripts/git-epochs-windows/results/*.md; git commit -m "GE-15: Windows results"; git push origin HEAD:windows/git-epochs'
if ($Publish) {
    & git add $jsonPath $mdPath
    & git commit -m ('GE-15: Windows results ' + $runId)
    & git push origin 'HEAD:windows/git-epochs'
    Write-Host 'Pushed. Tell the macOS agent: results are pushed.'
} else {
    Write-Host 'To send the results back:'
    Write-Host ('  ' + $pushCmd)
}
if ($nf -gt 0) { exit 1 } else { exit 0 }
