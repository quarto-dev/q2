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
# Writes target\windows-run\failure-reasons.txt: one block per failing test with the first
# lines of its output (the panic message). Send that file back. Also prints the most common
# messages. Nextest prints each failure twice (live, then in the final recap); only the first
# is kept per test and log.

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
