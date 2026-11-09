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
foreach ($f in $files) {
    # A finished stage's .log supersedes the same stage's .tmp, which is deleted on completion
    # anyway; reading both only matters while a stage is running.
    $L = @(Get-Content -LiteralPath $f.FullName)
    for ($i = 0; $i -lt $L.Count; $i++) {
        if ($L[$i] -match '^\s*--- STD(ERR|OUT):\s+(.+?)\s+---\s*$') {
            $test = $Matches[2]
            $key = ($f.Name -replace '\.stderr\.tmp$', '.log') + '|' + $test
            if ($seen.ContainsKey($key)) { continue }
            $seen[$key] = 1
            $out.Add("=== $($f.Name): $test")
            $j = $i + 1
            $n = 0
            while ($j -lt $L.Count -and $n -lt 8 -and $L[$j] -notmatch '^\s*--- STD') {
                if ($L[$j].Trim()) { $out.Add('    ' + $L[$j].TrimEnd()); $n++ }
                $j++
            }
        }
    }
}

$out | Out-File -Encoding ascii -FilePath $dest
Write-Host "$($seen.Count) failing tests -> $dest ($($out.Count) lines)"
if ($seen.Count -eq 0) {
    Write-Host 'No "--- STDERR:" blocks found; send the output of:'
    Write-Host "  Select-String -Path $dir\02-nextest-quarto-core.log -Pattern 'FAIL' -Context 0,10 | Select-Object -First 1"
    exit 0
}

Write-Host "`nMost common messages:" -ForegroundColor Cyan
$out | Where-Object { $_ -match 'panicked at' -or $_ -match '^\s+(Error|assertion|called|pandoc|No such|The system)' } |
    ForEach-Object { ($_ -replace '^\s+', '' -replace "thread '[^']*' ", '' -replace '[A-Za-z]:\\[^ :]*', '<path>' -replace ':\d+:\d+', '') } |
    Group-Object | Sort-Object Count -Descending | Select-Object -First 25 |
    Format-Table Count, Name -AutoSize -Wrap | Out-String -Width 200 | Write-Host
