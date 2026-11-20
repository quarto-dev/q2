# Installs a current binaryen (wasm-opt.exe) for the c2 -BuildEmbed run.
#
# Why: hub-client/scripts/build-wasm.js looks up wasm-opt with spawnSync and no shell, so on Windows it cannot
# launch npm's wasm-opt.cmd shim, and `cargo install wasm-opt` only gives binaryen 116, which rejects the
# wasm-bindgen output. A real wasm-opt.exe from a current binaryen release works.
#
# Usage (PowerShell 5.1 or 7):  powershell -ExecutionPolicy Bypass -File .\install-binaryen.ps1
# Safe to rerun. Touches only %USERPROFILE%\.binaryen and your user PATH.
$ErrorActionPreference = 'Stop'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$ProgressPreference = 'SilentlyContinue'   # makes Invoke-WebRequest much faster on 5.1

$root = Join-Path $env:USERPROFILE '.binaryen'
New-Item -ItemType Directory -Force -Path $root | Out-Null

Write-Host 'Looking up the latest binaryen release...'
$rel = Invoke-RestMethod -Uri 'https://api.github.com/repos/WebAssembly/binaryen/releases/latest' -Headers @{ 'User-Agent' = 'install-binaryen' }
$tag = $rel.tag_name
$asset = $rel.assets | Where-Object { $_.name -match 'x86_64-windows\.tar\.gz$' } | Select-Object -First 1
if (-not $asset) { throw "No x86_64-windows tar.gz asset in release $tag" }
$sumAsset = $rel.assets | Where-Object { $_.name -eq ($asset.name + '.sha256') } | Select-Object -First 1
Write-Host "Release $tag, asset $($asset.name)"

$dest = Join-Path $root $tag
$exe = Get-ChildItem -Path $dest -Recurse -Filter wasm-opt.exe -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $exe) {
  $tmp = Join-Path $env:TEMP ("binaryen-" + [guid]::NewGuid().ToString('N'))
  New-Item -ItemType Directory -Force -Path $tmp | Out-Null
  $tgz = Join-Path $tmp $asset.name
  Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $tgz -UseBasicParsing
  if ($sumAsset) {
    $want = ((Invoke-RestMethod -Uri $sumAsset.browser_download_url -Headers @{ 'User-Agent' = 'install-binaryen' }) -split '\s+')[0].ToLower()
    $got = (Get-FileHash -Algorithm SHA256 -Path $tgz).Hash.ToLower()
    if ($want -ne $got) { throw "SHA-256 mismatch: expected $want, got $got" }
    Write-Host 'SHA-256 verified.'
  } else {
    Write-Host 'No .sha256 asset published; skipping checksum.'
  }
  New-Item -ItemType Directory -Force -Path $dest | Out-Null
  & tar.exe -xzf $tgz -C $dest          # tar.exe ships with Windows 10 1803+
  if ($LASTEXITCODE -ne 0) { throw "tar failed with exit code $LASTEXITCODE" }
  Remove-Item -Recurse -Force $tmp
  $exe = Get-ChildItem -Path $dest -Recurse -Filter wasm-opt.exe | Select-Object -First 1
  if (-not $exe) { throw "wasm-opt.exe not found under $dest" }
}
$bin = $exe.DirectoryName
Write-Host "wasm-opt.exe is in $bin"

# Remove the binaryen 116 build from `cargo install`, if present.
if (Get-Command cargo -ErrorAction SilentlyContinue) {
  $installed = & cargo install --list 2>$null | Select-String '^wasm-opt v'
  if ($installed) { Write-Host 'Uninstalling cargo wasm-opt (binaryen 116)...'; & cargo uninstall wasm-opt }
}

# Put the new bin first on the user PATH (drop older entries of ours) and on this session's PATH.
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$parts = @($userPath -split ';' | Where-Object { $_ -and ($_ -notlike (Join-Path $root '*')) })
[Environment]::SetEnvironmentVariable('Path', (@($bin) + $parts) -join ';', 'User')
$env:Path = $bin + ';' + (($env:Path -split ';' | Where-Object { $_ -and ($_ -notlike (Join-Path $root '*')) }) -join ';')

Write-Host ''
Write-Host 'where.exe wasm-opt:'
& where.exe wasm-opt
Write-Host -NoNewline 'wasm-opt --version: '
& wasm-opt --version
Write-Host ''
Write-Host 'First "where" line must be the .binaryen path above, and the version must be well above 116.'
Write-Host 'Open a NEW terminal before running run.ps1 (this window already has the new PATH).'
