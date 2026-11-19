<#
.SYNOPSIS
  GE-15, GE-16, and GE-17, the Windows runs A, B, and C: runs the new Rust and hub tests (items a0 to a6) and the new TypeScript
  and extension tests (items b1 to b7) of the git epochs epic and writes one results file.
  Temporary tooling on windows/git-epochs; it never lands on feature/git-epochs.

.DESCRIPTION
  PowerShell 5.1 compatible. Needs git (Git for Windows), Rust (MSVC host), cargo-nextest and uv; the preflight
  prints an install command for anything missing. Every item is independent: one failure never stops the others.
  Items a1 to a5 come from items.json (the same file check_filters.sh reads on macOS); a0 is the build and a6 is
  the epic's own command (clippy, then nextest) for the four crates.
  Writes scripts/git-epochs-windows/results/gea-<time>-<host>.json (machine-readable) and .md (human summary).
  Full logs go under results/logs/ (not committed).

.PARAMETER Only
  Run only these item ids (for example -Only a1,a5 or -Only b1,b2,b3,b4,b5,b6,b7). Default: all. A b-only run needs
  no Rust toolchain, NASM, or CMake; an a-only run needs no Node.
.PARAMETER WithTools
  Download pandoc and typst (the versions CI pins in .github/workflows/test-suite.yml) as portable zips into
  results/tools and put them on PATH for this run. Without them, tests that need pandoc or typst fail, which says
  nothing about Windows (this matters for a6).
.PARAMETER Publish
  After the run, commit results/*.json and results/*.md and push the current branch to origin as windows/git-epochs.
  Without it the script prints the commands.
.PARAMETER BuildEmbed
  Item c2 only: run `cargo xtask build-hub-client-embed` and rebuild q2 so the cross-client Playwright scenarios
  have the real editor embed. Needs the WASM toolchain (see instructions page); without it c2 skips that step.
.PARAMETER SkipBuild
  Skip the up-front build item a0 (use when you have just built).
#>
[CmdletBinding()]
param(
    [string[]]$Only,
    [switch]$WithTools,
    [switch]$Publish,
    [switch]$SkipBuild,
    [switch]$BuildEmbed
)

$ErrorActionPreference = 'Continue'   # native stderr must not abort the script (PS 5.1)
Set-StrictMode -Version 2

# `powershell -File run.ps1 -Only b1,b2` delivers one string 'b1,b2' (Windows PowerShell 5.1), so split on commas.
if ($Only) { $Only = @($Only | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ }) }

# c3 means "every a and b item on the final code": expand it so one invocation runs them all.
if ($Only -and ($Only -contains 'c3')) {
    $Only = @($Only + @('a0','a1','a2','a3','a4','a5','a6','b1','b2','b3','b4','b5','b6','b7') | Select-Object -Unique)
}
$wantC = (-not $Only) -or (@($Only | Where-Object { $_ -like 'c*' }).Count -gt 0)

$wantA = (-not $Only) -or (@($Only | Where-Object { $_ -like 'a*' }).Count -gt 0)
$wantB = (-not $Only) -or (@($Only | Where-Object { $_ -like 'b*' }).Count -gt 0)
$ra = if ($wantA) { 'yes' } else { 'no' }
$rb = if ($wantB) { 'yes' } else { 'no' }

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

# winget installs NASM (and sometimes CMake) without putting them on PATH, and a terminal opened before the
# install never sees PATH changes. Look in the usual folders and add what exists for this run only.
foreach ($cand in @(
        @{ Exe = 'nasm.exe';  Dirs = @((Join-Path $env:ProgramFiles 'NASM'), (Join-Path ${env:ProgramFiles(x86)} 'NASM'), (Join-Path $env:LOCALAPPDATA 'bin\NASM')) },
        @{ Exe = 'cmake.exe'; Dirs = @((Join-Path $env:ProgramFiles 'CMake\bin'), (Join-Path ${env:ProgramFiles(x86)} 'CMake\bin')) })) {
    if (-not (Get-Command $cand.Exe -ErrorAction SilentlyContinue)) {
        foreach ($d in $cand.Dirs) {
            if ($d -and (Test-Path (Join-Path $d $cand.Exe))) {
                $env:PATH = $d + ';' + $env:PATH
                Write-Host ('  added to PATH for this run: ' + $d)
                break
            }
        }
    }
}

Write-Host "== Toolchain ($runId) =="
$tools = @()
$tools += Test-Tool 'git'    'yes' 'winget install Git.Git   (Git for Windows; the probe is tested against it)'
$tools += Test-Tool 'rustc'  $ra 'winget install Rustlang.Rustup   (then: rustup default stable-x86_64-pc-windows-msvc)'
$tools += Test-Tool 'cargo'  $ra 'comes with rustup'
$tools += Test-Tool 'cargo-nextest' $ra 'cargo install cargo-nextest --locked'
$tools += Test-Tool 'uv'     $ra 'winget install astral-sh.uv   (runs make_large_tree.py and extract_failures.py)'
$tools += Test-Tool 'nasm'   $ra 'winget install NASM.NASM   (the aws-lc-sys crates in quarto-hub need it)'
$tools += Test-Tool 'cmake'  $ra 'winget install Kitware.CMake   (the aws-lc-sys crates in quarto-hub need it)'
$tools += Test-Tool 'link'   'no'  'Visual Studio Build Tools, "Desktop development with C++" (cargo finds link.exe itself, so absent from PATH is normal; install only if the build says "linker not found")'
$tools += Test-Tool 'node'   $rb 'fnm/nvm; Node major per .nvmrc (items b1 to b7)'
$tools += Test-Tool 'npm'    $rb 'comes with Node'
$tools += Test-Tool 'jj'     'no'  'optional: winget install jj-vcs.jj (the jj scenarios of b1 skip without it)'

$nodeVersion = ''
$nodeCaveat = ''
if (Get-Command node -ErrorAction SilentlyContinue) {
    $nodeVersion = (& node --version).Trim()
    $want = ''
    $nvmrc = Join-Path $repo '.nvmrc'
    if (Test-Path $nvmrc) { $want = ((Get-Content -Path $nvmrc -TotalCount 1) -replace '[^0-9.]', '').Split('.')[0] }
    $have = ($nodeVersion -replace '^v', '').Split('.')[0]
    Write-Host ('  node ' + $nodeVersion + '  (.nvmrc major: ' + $want + ')')
    if ($want -and $have -ne $want) {
        $nodeCaveat = 'Node ' + $nodeVersion + ' is not the .nvmrc major ' + $want + '; npm ci refuses other majors (engine-strict), so b7 and everything after it will fail for that reason alone.'
        Write-Host ('  WARNING: ' + $nodeCaveat) -ForegroundColor Red
    }
}

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
if ($wantA -and -not $SkipBuild) {
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
# b1 to b7 (GE-16): the TypeScript package and the VS Code extension's vscode-free core
# --------------------------------------------------------------------------------------------

function Get-VitestCounts {
    # Sums the "Tests  N failed | N passed | N skipped (N)" line of one vitest run. $null when there is none.
    param([string]$Text)
    $clean = $Text -replace '\x1b\[[0-9;]*m', ''
    $m = [regex]::Match($clean, '(?m)^\s*Tests\s+(.+)$')
    if (-not $m.Success) { return $null }
    $line = $m.Groups[1].Value
    $c = @{ failed = 0; passed = 0; skipped = 0 }
    foreach ($k in @('failed', 'passed', 'skipped')) {
        $mm = [regex]::Match($line, '(\d+) ' + $k)
        if ($mm.Success) { $c[$k] = [int]$mm.Groups[1].Value }
    }
    return $c
}

function Invoke-BItem {
    # One item = one or more steps (each a command line, run through Invoke-Logged). The item passes when every
    # step exits 0 with no failed test and the passed total reaches $MinTests (0 = no count guard, for npm ci).
    param([string]$Id, [string]$Catalog, [string]$Title, $Steps, [int]$MinTests,
          [string]$Proves, [string]$NotProves, [string]$Caveat = '')
    if (-not (Test-Selected $Id)) { return }
    if (-not $script:npmReady -and $Id -ne 'b7') {
        Add-Result $Id $Title 'SKIP' 'npm ci did not succeed (see b7 or b0); the TypeScript items need node_modules' $Proves $NotProves '' '' $false '' $Catalog
        return
    }
    $passed = 0; $skipped = 0; $failedTests = 0; $badStep = ''; $lastLog = ''; $tail = ''
    $n = 0
    foreach ($st in $Steps) {
        $n++
        $r = Invoke-Logged ($Id + '-' + $st.Name) $st.Cmd
        $lastLog = $r.Log
        $text = Get-LogText $r.Log
        $cnt = Get-VitestCounts $text
        if ($st.Counts -and $null -eq $cnt -and $r.Exit -eq 0) { $badStep = $st.Name + ': no vitest summary line'; $tail = Get-Tail $r.Log 30; continue }
        if ($cnt) { $passed += $cnt.passed; $skipped += $cnt.skipped; $failedTests += $cnt.failed }
        if ($r.Exit -ne 0 -or ($cnt -and $cnt.failed -gt 0)) {
            if (-not $badStep) { $badStep = $st.Name + ': exit code ' + $r.Exit; $tail = Get-Tail $r.Log 60 }
        }
    }
    $summary = $passed.ToString() + ' tests passed, ' + $skipped.ToString() + ' skipped'
    if ($badStep) {
        Add-Result $Id $Title 'FAIL' ($badStep + '; ' + $summary + ', ' + $failedTests.ToString() + ' failed') $Proves $NotProves $tail $Caveat $false $lastLog $Catalog
    } elseif ($passed -lt $MinTests) {
        Add-Result $Id $Title 'FAIL' ('expected at least ' + $MinTests + ' passed tests, saw ' + $passed + ' (a path or filter matched too little)') $Proves $NotProves $tail $Caveat $false $lastLog $Catalog
    } else {
        Add-Result $Id $Title 'PASS' $summary $Proves $NotProves '' $Caveat $false $lastLog $Catalog
    }
}

$script:npmReady = $false
if ($wantB) {
    Write-Host '== b: TypeScript and extension items =='
    $crlfCaveat = ''
    if ($autocrlf -ne 'true') {
        $crlfCaveat = 'core.autocrlf is ' + $autocrlf + ', not true: the CRLF-checkout case was not exercised from the checkout. The tests set autocrlf=true in their own temp repos, so they still check the comparison, but clone with: git clone -c core.autocrlf=true ... for the full item.'
    }
    $pkgGE = '@quarto/quarto-git-epochs'
    $pkgX  = 'quarto-hub-vscode'
    $vt = { param($pkg, $files) 'npm exec -w ' + $pkg + ' -- vitest run ' + $files }

    # b7 first: the clean install the other items stand on.
    if ($true) {
        $tsWs = @(Get-ChildItem -Directory (Join-Path $repo 'ts-packages') | Where-Object { Test-Path (Join-Path $_.FullName 'package.json') } | Sort-Object Name | ForEach-Object { '-w ts-packages/' + $_.Name }) -join ' '
        $installOk = $true
        $ri = $null
        $needInstall = (Test-Selected 'b7') -or -not (Test-Path (Join-Path $repo 'node_modules'))
        if ($needInstall) {
            Write-Host '== b7: npm ci (root) =='
            $ri = Invoke-Logged 'b7-npm-ci' 'npm ci'
            $installOk = ($ri.Exit -eq 0)
        }
        $script:npmReady = $installOk
        if (Test-Selected 'b7') {
            $steps = @()
            if ($installOk) {
                $steps += @{ Name = 'typecheck'; Cmd = 'npm run typecheck -w ' + $pkgX; Counts = $false }
                $steps += @{ Name = 'build-extension'; Cmd = 'npm run build -w ' + $pkgX; Counts = $false }
                $steps += @{ Name = 'build-ts-packages'; Cmd = 'npm run build --if-present ' + $tsWs; Counts = $false }
            }
            if (-not $installOk) {
                Add-Result 'b7' 'clean root install, extension typecheck and bundle, ts-packages builds' 'FAIL' ('npm ci exit code ' + $ri.Exit) 'The extension joined the workspaces cleanly and its bundle builds on Windows.' 'vsce package; installing a .vsix; the hub-client and preview-SPA builds.' (Get-Tail $ri.Log 40) $nodeCaveat $false $ri.Log ''
            } else {
                Invoke-BItem 'b7' '' 'clean root install, extension typecheck and bundle, ts-packages builds' $steps 0 'npm ci succeeded at the pinned Node; the extension typechecks (including e2e), its esbuild bundle builds, and every ts-packages workspace builds on Windows.' 'vsce package; installing a .vsix; the hub-client and preview-SPA builds (they need the WASM and a pandoc download).' $nodeCaveat
            }
        }
    }

    Invoke-BItem 'b1' 'W6' 'git probe, controller, and scenarios of the TypeScript package on Git for Windows' @(
        @{ Name = 'package'; Cmd = 'npm test -w ' + $pkgGE; Counts = $true }) 150 `
        'The probe parses Git for Windows output (C:/... from --git-path, ls-tree -z quoting, reflog); the controller sees real index.lock and rebase-merge markers; the shared scenario step lists run against real repositories in %TEMP%. jj scenarios skip when jj is absent.' `
        'Other Git builds; jj on Windows unless it is installed.'

    Invoke-BItem 'b2' 'W4' 'keys and paths: registry keys, ignore rules, safeJoin NTFS forms, junction, long path' @(
        @{ Name = 'package'; Cmd = (& $vt $pkgGE 'src/paths.test.ts src/registry.test.ts'); Counts = $true },
        @{ Name = 'extension'; Cmd = (& $vt $pkgX 'src/core/safe-path.test.ts src/core/ignore.test.ts'); Counts = $true }) 80 `
        'The TypeScript key normalization, ignore rules, and safeJoin hold for Windows path forms: / keys, drive letters and UNC prefixes, case collisions, nested .quarto-hub and .git segments, .git. and .git::$INDEX_ALLOCATION, a directory junction pointing outside the root, a path over 260 characters, and (where the volume has 8.3 names) GIT~1.' `
        'Filesystems other than NTFS; Dev Drive or OneDrive behavior. The 8.3 row skips on a volume with short names disabled.'

    Invoke-BItem 'b3' 'W3' 'core.autocrlf=true: content rule, arrival, and the dirty check' @(
        @{ Name = 'package'; Cmd = (& $vt $pkgGE 'src/arrival.test.ts src/classify.test.ts src/paths.test.ts src/scenarios.test.ts'); Counts = $true },
        @{ Name = 'extension'; Cmd = (& $vt $pkgX 'src/core/clone/arrival.test.ts src/core/origination/dirty.test.ts'); Counts = $true }) 70 `
        'A CRLF checkout does not look dirty, and comparisons normalize as the macOS tests assume (the autocrlf-checkout scenario, arrival step 3, the origination dirty check).' `
        '.gitattributes filters (LFS, ident); autocrlf=input.' $crlfCaveat

    Invoke-BItem 'b4' '' 'extension core engine and headless peer: watcher, disk writes, rebind, atomic record write' @(
        @{ Name = 'core'; Cmd = (& $vt $pkgX 'src/core/follower.test.ts src/core/clone src/core/project-sync.test.ts src/core/text-binding.test.ts src/core/presence.test.ts src/core/meta.test.ts src/core/editor-only.test.ts src/core/git-exclude.test.ts src/core/no-vscode.test.ts src/headless/peer.test.ts src/package-manifest.test.ts'); Counts = $true }) 100 `
        'The engine runs on Windows: the injected watcher, writes to disk, rebind deleting the Automerge store after the repo is shut down (no EBUSY), and the atomic write of git-epochs.json retrying a rename refused with EPERM or EBUSY.' `
        'VS Code''s own file watcher; a real scanner or watcher holding the file (the EPERM test injects the error).'

    Invoke-BItem 'b5' 'W7' 'Join: q2 spawn through a stub q2.cmd, readiness, stdin close, tree kill, ticket hygiene' @(
        @{ Name = 'join'; Cmd = (& $vt $pkgX 'src/core/join.test.ts src/core/join-proxy.test.ts'); Counts = $true }) 45 `
        'Spawn and kill semantics on Windows: the ticket in argv, a free port, the readiness poll of the stub /health, stdin close ending the stub, kill of the whole process tree on Leave, stderr surfaced, no ticket in any log; .cmd is spawned through a shell.' `
        'The iroh tunnel across real networks. The optional real q2 preview --share host and --join guest is not scripted: run it by hand if the network allows.'

    Invoke-BItem 'b6' '' 'origination: offline publish, resume, second peer reads every file' @(
        @{ Name = 'origination'; Cmd = (& $vt $pkgX 'src/headless/owner.test.ts src/headless/publish-git.test.ts src/core/origination'); Counts = $true }) 50 `
        'Publish order and resume on NTFS paths and Node file handles: docs and registry entries exist before the connect, the share link only after the heads are confirmed, an interrupted publish resumes from local storage, and a second peer reads every file.' `
        'An out-of-process q2 hub (these tests run an in-process hub); the public server; very large uploads.'
}

# --------------------------------------------------------------------------------------------
# c1 to c3 (GE-17): the scripted scenario, the Playwright subset, and the whole set together
# --------------------------------------------------------------------------------------------

function Get-PlaywrightCounts {
    # "  3 passed (2.4m)" / "  1 failed" / "  2 skipped" lines of one Playwright list-reporter run.
    param([string]$Text)
    $clean = $Text -replace '\x1b\[[0-9;]*m', ''
    $c = @{ passed = 0; failed = 0; skipped = 0 }
    foreach ($k in @('passed', 'failed', 'skipped', 'flaky')) {
        $m = [regex]::Matches($clean, '(?m)^\s+(\d+) ' + $k + '\b')
        if ($m.Count -gt 0 -and $c.ContainsKey($k)) { $c[$k] = [int]$m[$m.Count - 1].Groups[1].Value }
    }
    return $c
}

if ($wantC -and ((Test-Selected 'c1') -or (Test-Selected 'c2'))) {
    Write-Host '== c: scenario and Playwright items =='
    $cReady = $true
    if (-not (Test-Path (Join-Path $repo 'node_modules'))) {
        $ri = Invoke-Logged 'c-npm-ci' 'npm ci'
        $cReady = ($ri.Exit -eq 0)
    }
    $q2Exe = Join-Path $repo 'target\debug\q2.exe'
    $q2Ok = $false
    if ($cReady) {
        $rb = Invoke-Logged 'c-build-q2' 'cargo build --bin q2'
        $q2Ok = ($rb.Exit -eq 0) -and (Test-Path $q2Exe)
    }
    $pkgX = 'quarto-hub-vscode'

    if (Test-Selected 'c1') {
        $c1Proves = 'The whole feature works end to end on Windows through the real q2.exe and git.exe on a generated repo in %TEMP% (about 4,000 tracked files): the gate pauses during a real git switch, the classifier treats a commit as a move-forward and a switch as a flip, a changed file flips, a removed file is unlisted, an added file is listed, a binary changes, arrival re-attaches the earlier docs without a fork, reset --hard keeps the dirty text in the retired doc, and a hub stopped during a switch skips the flush and leaves no conflict markers.'
        $c1Not = 'jj on Windows; other Git builds; a long soak; the browser (c2 covers the editor surfaces).'
        if (-not $q2Ok) {
            Add-Result 'c1' 'scripted end-to-end scenario against q2.exe and git.exe' 'FAIL' 'npm ci or cargo build --bin q2 failed (see c-npm-ci.log and c-build-q2.log)' $c1Proves $c1Not '' '' $false '' 'W9'
        } else {
            $transcript = Join-Path $resultsDir ('c1-transcript-' + $runId + '.json')
            $env:C1_OUT = $transcript
            $env:Q2_BINARY = $q2Exe
            $r = Invoke-Logged 'c1-scenario' ('node "' + (Join-Path $PSScriptRoot 'c1\run.mjs') + '"')
            Remove-Item Env:\C1_OUT -ErrorAction SilentlyContinue
            # Keep the full log with the results (the logs directory is not committed): the transcript is small, the log is not.
            Copy-Item -Path $r.Log -Destination (Join-Path $resultsDir ('c1-scenario-' + $runId + '.log')) -ErrorAction SilentlyContinue
            Remove-Item Env:\Q2_BINARY -ErrorAction SilentlyContinue
            $cnt = Get-PlaywrightCounts (Get-LogText $r.Log)
            $text = (Get-LogText $r.Log) -replace '\x1b\[[0-9;]*m', ''
            $step9 = ''
            if ($text -match 'step 9 not run') { $step9 = 'Step 9 was NOT run on this machine: Ctrl+C could not be delivered to the hub (see the transcript); the skipped-flush behavior is then covered only by the GE-3 hub test.' }
            if ($r.Exit -eq 0 -and $cnt.failed -eq 0 -and $cnt.passed -ge 1) {
                Add-Result 'c1' 'scripted end-to-end scenario against q2.exe and git.exe' 'PASS' ($cnt.passed.ToString() + ' passed, ' + $cnt.skipped.ToString() + ' skipped; transcript ' + (Split-Path $transcript -Leaf)) $c1Proves $c1Not '' $step9 $false $r.Log 'W9'
            } else {
                Add-Result 'c1' 'scripted end-to-end scenario against q2.exe and git.exe' 'FAIL' ('exit code ' + $r.Exit + '; ' + $cnt.passed + ' passed, ' + $cnt.failed + ' failed; the transcript names the failing step') $c1Proves $c1Not (Get-Tail $r.Log 60) $step9 $false $r.Log 'W9'
            }
        }
    }

    if (Test-Selected 'c2') {
        $c2Proves = 'The extension''s VS Code surfaces (commands, notifications, status bar, buffers) work with the pinned Code.exe the harness downloads; the permanent cross-client scenarios (H1 to H7, L1 to L4, the registry check) pass on Windows; editor-only''s quiet window is measured against a real Windows buffer reload (a gap above 1 s is a finding for the experimental fallback in G20, not a failure).'
        $c2Not = 'VS Code Insiders or other versions; the unmarked exploratory scenarios.'
        if (-not $cReady) {
            Add-Result 'c2' 'Playwright subset with the pinned VS Code build and the timing test' 'FAIL' 'npm ci failed (see c-npm-ci.log)' $c2Proves $c2Not '' '' $false '' 'W8'
        } else {
            $failed = @(); $passed = 0; $skippedN = 0; $lastLog = ''; $tail = ''; $caveat = ''
            # The e2e harness loads vscode-sync-experiment\dist\extension.js; a fresh checkout has none (commands are then 'not found').
            $rx = Invoke-Logged 'c2-build-extension' ('npm run build -w ' + $pkgX)
            if ($rx.Exit -ne 0) { $failed += 'extension build (c2-build-extension.log)'; $tail = Get-Tail $rx.Log 40 }
            $steps = @(
                @{ Name = 'extension-flows'; Cmd = 'npm run test:e2e -w ' + $pkgX },
                @{ Name = 'timing'; Cmd = 'npm run test:e2e:timing -w ' + $pkgX; Timing = $true })
            if ($BuildEmbed) {
                $re = Invoke-Logged 'c2-build-embed' 'cargo xtask build-hub-client-embed'
                $rq = Invoke-Logged 'c2-rebuild-q2' 'cargo build --bin q2'
                if ($re.Exit -ne 0 -or $rq.Exit -ne 0) {
                    $failed += 'embed build (c2-build-embed.log, c2-rebuild-q2.log)'
                    $tail = Get-Tail $re.Log 40
                } else {
                    $steps = @(@{ Name = 'extension-flows'; Cmd = 'npm run test:e2e -w ' + $pkgX },
                               @{ Name = 'cross-client'; Cmd = 'npm run test:e2e:cross -w ' + $pkgX },
                               @{ Name = 'timing'; Cmd = 'npm run test:e2e:timing -w ' + $pkgX; Timing = $true })
                }
            } else {
                $caveat = 'The cross-client Playwright scenarios were NOT run: they need the hub-client embed in q2.exe (rerun with -BuildEmbed on a machine with the WASM toolchain; see the instructions page).'
            }
            $gap = ''
            foreach ($st in $steps) {
                $r = Invoke-Logged ('c2-' + $st.Name) $st.Cmd
                $lastLog = $r.Log
                $cnt = Get-PlaywrightCounts (Get-LogText $r.Log)
                $passed += $cnt.passed; $skippedN += $cnt.skipped
                $gm = [regex]::Match((Get-LogText $r.Log), 'lock-release to last-reload gap: (\d+) ms')
                if ($gm.Success) { $gap = $gm.Groups[1].Value + ' ms' }
                if ($st.ContainsKey('Timing') -and $st.Timing) {
                    # A timing gap above the window is a finding (G20), not a failed run; any other failure still counts.
                    $txt = Get-LogText $r.Log
                    if ($r.Exit -ne 0 -and $gm.Success -and $txt -match 'toBeLessThanOrEqual') {
                        $caveat += ' Timing test: the measured gap of ' + $gap + ' is above the 1000 ms window: a finding for the experimental fallback in G20.'
                    } elseif ($r.Exit -ne 0) { $failed += $st.Name; $tail = Get-Tail $r.Log 60 }
                } elseif ($r.Exit -ne 0 -or $cnt.failed -gt 0) {
                    $failed += $st.Name; if (-not $tail) { $tail = Get-Tail $r.Log 60 }
                }
            }
            if ($gap) { $caveat += ' Measured lock-release to last-reload gap: ' + $gap + '.' }
            $caveat = $caveat.Trim()
            if ($failed.Count -eq 0) {
                Add-Result 'c2' 'Playwright subset with the pinned VS Code build and the timing test' 'PASS' ($passed.ToString() + ' passed, ' + $skippedN.ToString() + ' skipped') $c2Proves $c2Not '' $caveat $false $lastLog 'W8'
            } else {
                Add-Result 'c2' 'Playwright subset with the pinned VS Code build and the timing test' 'FAIL' ('failed: ' + ($failed -join ', ') + '; ' + $passed + ' passed') $c2Proves $c2Not $tail $caveat $false $lastLog 'W8'
            }
        }
    }
}

if (Test-Selected 'c3') {
    # c3: the a and b items of this same invocation, all green together on the final code.
    $ab = @($results | Where-Object { $_.id -like 'a*' -or $_.id -like 'b*' })
    $expected = @('a0','a1','a2','a3','a4','a5','a6','b1','b2','b3','b4','b5','b6','b7')
    $missingIds = @($expected | Where-Object { $id = $_; -not ($ab | Where-Object { $_.id -eq $id }) })
    $bad = @($ab | Where-Object { $_.status -ne 'PASS' -and -not $_.known_unrelated })
    $c3Proves = 'Nothing regressed on Windows after the fix plans: every a and b item ran in one invocation on the final code and is green together.'
    $c3Not = 'Anything the items did not already cover.'
    if ($missingIds.Count -gt 0) {
        Add-Result 'c3' 'every a and b item together on the final code' 'FAIL' ('items did not run: ' + ($missingIds -join ', ') + ' (with -SkipBuild a0 is absent)') $c3Proves $c3Not '' '' $false '' ''
    } elseif ($bad.Count -gt 0) {
        Add-Result 'c3' 'every a and b item together on the final code' 'FAIL' ('not passing: ' + (($bad | ForEach-Object { $_.id + '=' + $_.status }) -join ', ')) $c3Proves $c3Not '' '' $false '' ''
    } else {
        Add-Result 'c3' 'every a and b item together on the final code' 'PASS' (($ab.Count).ToString() + ' items green together') $c3Proves $c3Not '' '' $false '' ''
    }
}

# --------------------------------------------------------------------------------------------
# write results
# --------------------------------------------------------------------------------------------
$out = New-Object PSObject -Property ([ordered]@{
    run_id = $runId; branch = $branch; commit = $commit; base = $baseNote; host_triple = $hostTriple
    os = (Get-CimInstance Win32_OperatingSystem -ErrorAction SilentlyContinue).Caption
    git = $gitVersion; node = $nodeVersion; autocrlf = $autocrlf; longpaths = $longpaths
    tools = $tools; items = @($results) })
$jsonPath = Join-Path $resultsDir ('gea-' + $runId + '.json')
$out | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 $jsonPath

$md = New-Object System.Collections.ArrayList
[void]$md.Add('# GE-15, GE-16, and GE-17 Windows run ' + $runId)
[void]$md.Add('')
[void]$md.Add('Branch ' + $branch + ' at ' + $commit + ' (' + $baseNote + '); host ' + $hostTriple + '; ' + $gitVersion + '; node ' + $nodeVersion + '; autocrlf ' + $autocrlf + '; longpaths ' + $longpaths + '.')
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

$pushCmd = 'git add scripts/git-epochs-windows/results/*.json scripts/git-epochs-windows/results/*.md; git commit -m "GE-15/GE-16: Windows results"; git push origin HEAD:windows/git-epochs'
if ($Publish) {
    & git add $jsonPath $mdPath
    Get-ChildItem -Path $resultsDir -Filter ('c1-*' + $runId + '*') -ErrorAction SilentlyContinue | ForEach-Object { & git add $_.FullName }
    & git commit -m ('GE-15/GE-16: Windows results ' + $runId)
    & git push origin 'HEAD:windows/git-epochs'
    Write-Host 'Pushed. Tell the macOS agent: results are pushed.'
} else {
    Write-Host 'To send the results back:'
    Write-Host ('  ' + $pushCmd)
}
if ($nf -gt 0) { exit 1 } else { exit 0 }
