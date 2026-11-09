# Extract why each test failed from the logs of windows-riu-knitr-exr-tests.ps1.
#
# TEMPORARY. Lives only on the branch windows/riu-knitr-exr-run; never merge it.
# Safe to run while the test script is still going (it reads the finished logs and the
# in-progress *.stderr.tmp files).
#
# Usage, from anywhere inside the checkout:
#   .\scripts\windows-riu-knitr-exr-reasons.ps1
#   powershell -ExecutionPolicy Bypass -File .\scripts\windows-riu-knitr-exr-reasons.ps1
#
# Also writes target\windows-run\failure-details.txt: the full output (diffs included) of the tests
# named by -DetailPattern. Send both files back.
#
# Writes target\windows-run\failure-reasons.txt: one block per failing test with the first
# lines of its output (the panic message). Send that file back. Also prints the most common
# messages. Nextest prints each failure twice (live, then in the final recap); only the first
# is kept per test and log.

[CmdletBinding()]
param(
    # Tests whose whole failure output (panic, assertion diff, insta snapshot diff) is wanted,
    # not just the first lines. Matched as a substring of the nextest test name.
    [string[]]$DetailPattern = @('smoke_all', 'jupyter_markdown', 'a_record_only_cache_entry', 'book_refactor_guard', 'book_download'),
    [int]$DetailLines = 220
)

$ErrorActionPreference = 'Continue'
Set-StrictMode -Version Latest

$repo = (& git rev-parse --show-toplevel).Trim()
$dir = Join-Path $repo 'target\windows-run'
$dest = Join-Path $dir 'failure-reasons.txt'

$files = @(Get-ChildItem -Path $dir -File | Where-Object { $_.Name -match '^\d\d-nextest-.*\.(log|stderr\.tmp)$' })
if ($files.Count -eq 0) { Write-Host "no nextest logs in $dir"; exit 1 }

$out = New-Object System.Collections.Generic.List[string]
$seen = @{}
$diag = New-Object System.Collections.Generic.List[string]
foreach ($f in $files) {
    $L = @(Get-Content -LiteralPath $f.FullName)
    $nBlocks = @($L | Where-Object { $_ -match 'STD(ERR|OUT):' }).Count
    $nPanic = @($L | Where-Object { $_ -match 'panicked at' }).Count
    $nFail = @($L | Where-Object { $_ -match '^\s*(FAIL|SIGABRT|TIMEOUT)\b' }).Count
    $diag.Add(("{0,-40} lines={1} FAIL={2} STDERR/STDOUT-blocks={3} panicked={4}" -f $f.Name, $L.Count, $nFail, $nBlocks, $nPanic))
    # Pass 1: nextest failure blocks ("--- STDERR: crate test ---", any spacing or decoration).
    for ($i = 0; $i -lt $L.Count; $i++) {
        if ($L[$i] -match 'STD(ERR|OUT):\s+(\S+\s+\S+)') {
            $test = $Matches[2]
            $key = ($f.Name -replace '\.stderr\.tmp$', '.log') + '|' + $test
            if ($seen.ContainsKey($key)) { continue }
            $seen[$key] = 1
            $out.Add("=== $($f.Name): $test")
            $j = $i + 1
            $n = 0
            while ($j -lt $L.Count -and $n -lt 8 -and $L[$j] -notmatch 'STD(ERR|OUT):\s') {
                if ($L[$j].Trim()) { $out.Add('    ' + $L[$j].TrimEnd()); $n++ }
                $j++
            }
        }
    }
    # Pass 2 (fallback, always): every panic line with context.
    for ($i = 0; $i -lt $L.Count; $i++) {
        if ($L[$i] -match 'panicked at') {
            $key = $f.Name + '|panic|' + $L[$i].Trim()
            if ($seen.ContainsKey($key)) { continue }
            $seen[$key] = 1
            $out.Add("=== PANIC in $($f.Name) line $($i + 1)")
            for ($j = [Math]::Max(0, $i - 1); $j -lt [Math]::Min($L.Count, $i + 4); $j++) { $out.Add('    ' + $L[$j].TrimEnd()) }
        }
    }
}

# Full output for the tests named in -DetailPattern: the FAIL line of the live failure report and
# everything nextest printed under it (stdout/stderr), minus backtrace frames. Own file.
$detail = New-Object System.Collections.Generic.List[string]
$detailSeen = @{}
foreach ($f in @($files | Where-Object { $_.Name -match '\.log$' })) {
    $L = @(Get-Content -LiteralPath $f.FullName)
    for ($i = 0; $i -lt $L.Count; $i++) {
        if ($L[$i] -notmatch '^\s*(FAIL|SIGABRT|TIMEOUT)\s+\[') { continue }
        $hit = $false
        foreach ($pat in $DetailPattern) { if ($L[$i] -like "*$pat*") { $hit = $true } }
        if (-not $hit) { continue }
        $key = $f.Name + '|' + $L[$i].Trim()
        if ($detailSeen.ContainsKey($key)) { continue }
        $detailSeen[$key] = 1
        $detail.Add('=== ' + $f.Name + ' line ' + ($i + 1))
        $detail.Add($L[$i].TrimEnd())
        $blk = New-Object System.Collections.Generic.List[string]
        for ($j = $i + 1; $j -lt $L.Count; $j++) {
            if ($L[$j] -match '^\s*(FAIL|PASS|SLOW|SIGABRT|TIMEOUT)\s+\[') { break }
            if ($L[$j] -match '^\s+\d+:\s+0x[0-9a-fA-F]+\s' -or ($L[$j] -match '^\s+at\s' -and $L[$j] -notmatch 'crates[\\/]')) { continue }
            $t = $L[$j].TrimEnd()
            if ($t.Length -gt 400) { $t = $t.Substring(0, 400) + ' ...' }
            $blk.Add($t)
        }
        # Long blocks (pass lists, golden diffs) keep their head and their tail: the verdict is at the end.
        if ($blk.Count -le $DetailLines) {
            foreach ($t in $blk) { $detail.Add($t) }
        } else {
            $head = 30
            for ($k = 0; $k -lt $head; $k++) { $detail.Add($blk[$k]) }
            $detail.Add("... [$($blk.Count - $DetailLines) lines omitted] ...")
            for ($k = $blk.Count - ($DetailLines - $head); $k -lt $blk.Count; $k++) { $detail.Add($blk[$k]) }
        }
        $detail.Add('')
    }
}
# Vitest log (not nextest format): its failure lines and the last lines of output.
foreach ($f in @(Get-ChildItem -Path $dir -File | Where-Object { $_.Name -match '^\d\d-vitest-.*\.log$' })) {
    $V = @(Get-Content -LiteralPath $f.FullName)
    $detail.Add('=== ' + $f.Name + ' (' + $V.Count + ' lines): failure lines, then the last 60 lines')
    foreach ($t in @($V | Where-Object { $_ -match 'FAIL|AssertionError|Error:|expected|received' } | Select-Object -First 60)) { $detail.Add($t.TrimEnd()) }
    $detail.Add('--- tail')
    foreach ($t in @($V | Select-Object -Last 60)) { $detail.Add($t.TrimEnd()) }
    $detail.Add('')
}
$detailDest = Join-Path $dir 'failure-details.txt'
$detail | Out-File -Encoding ascii -FilePath $detailDest
Write-Host "$($detailSeen.Count) detailed failures ($($DetailPattern -join ', ')) -> $detailDest ($($detail.Count) lines)"

$hdr = @('# per-file diagnostics') + $diag + @('')
($hdr + $out) | Out-File -Encoding ascii -FilePath $dest
$diag | ForEach-Object { Write-Host $_ }
Write-Host "$($seen.Count) entries -> $dest ($($out.Count) lines)"
if ($out.Count -eq 0) {
    Write-Host 'Nothing matched. Send the diagnostics above plus the output of:'
    Write-Host "  Get-Content $dir\02-nextest-quarto-core.log | Select-Object -First 60"
    exit 0
}

Write-Host "`nMost common messages:" -ForegroundColor Cyan
$out | Where-Object { $_ -match 'panicked at' -or $_ -match '^\s+(Error|assertion|called|pandoc|No such|The system)' } |
    ForEach-Object { ($_ -replace '^\s+', '' -replace "thread '[^']*' ", '' -replace '[A-Za-z]:\\[^ :]*', '<path>' -replace ':\d+:\d+', '') } |
    Group-Object | Sort-Object Count -Descending | Select-Object -First 25 |
    Format-Table Count, Name -AutoSize -Wrap | Out-String -Width 200 | Write-Host
