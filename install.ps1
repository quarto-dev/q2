#!/usr/bin/env pwsh
# q2 Windows installer — the PowerShell counterpart to install.sh.
#
#   irm https://raw.githubusercontent.com/quarto-dev/q2/main/install.ps1 | iex
#
# Downloads the published release zip for x86_64 Windows, verifies its
# SHA-256 against the published checksum, and installs q2.exe. Mirrors
# install.sh's contract (same artifact naming, same checksum file
# format). Signature verification is checksum-only on Windows for now —
# minisign has no ubiquitous Windows install path; noted in the plan
# (claude-notes/plans/2026-06-12-q2-github-releases-bundled-mcp.md).
#
# Note `q2 mcp` additionally needs Node.js 24+ at runtime.
#
# Flags (for testing / non-default installs):
#   -Version v0.1.0      install a specific tag instead of the latest
#   -Dest <dir>          install directory (default: %USERPROFILE%\.local\bin)
#   -ArtifactUrl <u>     override the download (a URL or a local path — the
#                        local path form is what the CI smoke test uses)
#   -Checksum <hex>      expected sha256; skips fetching the .sha256 file
#   -NoVerify            skip checksum verification (discouraged)
[CmdletBinding()]
param(
    [string]$Version,
    [string]$Dest,
    [string]$ArtifactUrl,
    [string]$Checksum,
    [switch]$NoVerify
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$Owner = 'quarto-dev'
$Repo = 'q2'
$Platform = 'windows_amd64'
$UA = @{ 'User-Agent' = 'q2-install' }

function Die([string]$msg) { throw "q2 install: $msg" }
function Step([string]$msg) { Write-Host "-> $msg" }

# q2 ships an x86_64 Windows binary; ARM64 Windows runs it under
# emulation, so we don't hard-block on architecture.
if (-not [System.Environment]::Is64BitOperatingSystem) {
    Die 'only 64-bit Windows is supported'
}

if (-not $Dest) { $Dest = Join-Path $env:USERPROFILE '.local\bin' }

# Fetch a URL or copy a local path (so -ArtifactUrl can be a file for
# offline testing) into $out.
function Get-Artifact([string]$src, [string]$out) {
    if (Test-Path -LiteralPath $src) {
        Copy-Item -LiteralPath $src -Destination $out -Force
    }
    else {
        Invoke-WebRequest -Uri $src -OutFile $out -Headers $UA
    }
}

if (-not $Version -and -not $ArtifactUrl) {
    Step 'resolving latest release...'
    $rel = Invoke-RestMethod -Uri "https://api.github.com/repos/$Owner/$Repo/releases/latest" -Headers $UA
    $Version = $rel.tag_name
    if (-not $Version) { Die 'could not determine the latest release; pass -Version vX.Y.Z' }
}
if ($Version) { Step "release: $Version" }

$bare = if ($Version) { $Version -replace '^v', '' } else { '' }
$archive = "q2-$bare-$Platform.zip"
if (-not $ArtifactUrl) {
    $ArtifactUrl = "https://github.com/$Owner/$Repo/releases/download/$Version/$archive"
}

$work = Join-Path ([System.IO.Path]::GetTempPath()) ("q2-install-" + [System.Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $work | Out-Null
try {
    $zip = Join-Path $work $archive
    Step "downloading $archive..."
    Get-Artifact $ArtifactUrl $zip

    if (-not $NoVerify) {
        if (-not $Checksum) {
            # The published "<hash>  <file>" line, same format as install.sh.
            $sumFile = Join-Path $work "$archive.sha256"
            Get-Artifact "$ArtifactUrl.sha256" $sumFile
            $Checksum = ((Get-Content -Raw $sumFile) -split '\s+')[0]
        }
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $zip).Hash.ToLower()
        if ($actual -ne $Checksum.ToLower()) {
            Die "checksum mismatch for $archive`n  expected $($Checksum.ToLower())`n  got      $actual"
        }
        Step 'checksum verified'
    }

    Expand-Archive -LiteralPath $zip -DestinationPath $work -Force
    $exe = Join-Path $work 'q2.exe'
    if (-not (Test-Path -LiteralPath $exe)) { Die 'archive did not contain q2.exe' }

    New-Item -ItemType Directory -Force -Path $Dest | Out-Null
    Copy-Item -LiteralPath $exe -Destination (Join-Path $Dest 'q2.exe') -Force
    Step "installed $(Join-Path $Dest 'q2.exe')"
}
finally {
    Remove-Item -Recurse -Force -LiteralPath $work -ErrorAction SilentlyContinue
}

# PATH hint (don't mutate the user's PATH silently).
$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not $userPath -or ($userPath -split ';' -notcontains $Dest)) {
    Write-Warning "$Dest is not on your PATH. Add it for your user with:"
    Write-Host "  [Environment]::SetEnvironmentVariable('Path', ([Environment]::GetEnvironmentVariable('Path','User') + ';$Dest'), 'User')"
    Write-Host '  (then open a new terminal)'
}

Step ('done: ' + (& (Join-Path $Dest 'q2.exe') --version))
