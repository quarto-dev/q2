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
#   -Nightly             install the latest nightly build instead: the
#                        rolling prerelease built from main whenever it
#                        changes (version like 0.33.0-nightly.20260919,
#                        replaced daily). Same archive contract; resolved
#                        by tag since releases/latest skips prereleases.
#                        Pass it through the one-liner as
#                          & ([scriptblock]::Create((irm <url>))) -Nightly
#   -Dest <dir>          install directory (default: %USERPROFILE%\.local\bin)
#   -ArtifactUrl <u>     override the download (a URL or a local path — the
#                        local path form is what the CI smoke test uses)
#   -Checksum <hex>      expected sha256; skips fetching the .sha256 file
#   -NoVerify            skip checksum verification (discouraged)
#
# Environment:
#   GH_TOKEN / GITHUB_TOKEN  optional GitHub token for the release lookup
#                        (api.github.com; anonymous reads are limited to
#                        60/hour per IP address, a token raises that)
[CmdletBinding()]
param(
    [string]$Version,
    [string]$Dest,
    [string]$ArtifactUrl,
    [string]$Checksum,
    [switch]$NoVerify,
    [switch]$Nightly
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

# GitHub API reads (bd-n9yh30c8). Anonymous calls share a budget of 60
# per hour per IP address, which shared CI runners and NATed networks
# exhaust; so a token from GH_TOKEN or GITHUB_TOKEN is sent when set. A
# stale token gets a 401 where an anonymous call would work, so a 401
# drops the token and retries. Other failures retry with a doubling
# backoff from 2s (2+4+8+16 = 30s in all): transient API errors, and the
# seconds after the Nightly workflow replaces the release, when the tag
# can briefly 404 or list no assets. A rate limit (403/429) is not
# retried: it lasts up to an hour.
# A hashtable, not a plain variable, so Invoke-GitHubApi can drop the
# token: under the README one-liner (& ([scriptblock]::Create(...))) this
# is not a script scope, so `$script:` would not reach it.
$Api = @{ Token = $(if ($env:GH_TOKEN) { $env:GH_TOKEN } else { $env:GITHUB_TOKEN }) }
$ApiAttempts = 5

# HTTP status of a failed web request (0: no HTTP response). Works for
# both Windows PowerShell 5.1 (WebException) and PowerShell 7
# (HttpResponseException).
function Get-HttpStatus($err) {
    $resp = $err.Exception.PSObject.Properties['Response']
    if ($resp -and $resp.Value) { return [int]$resp.Value.StatusCode }
    return 0
}

function Get-ApiFailureReason([int]$status) {
    switch ($status) {
        0 { return 'no response from the GitHub API' }
        { $_ -in 403, 429 } { return "HTTP ${status}: GitHub API rate limit, most likely; set GH_TOKEN to a GitHub token to raise it" }
        404 { return 'HTTP 404: no such release' }
        default { return "HTTP $status from the GitHub API" }
    }
}

# GET a GitHub API URL, retrying as above. $Ready says whether a
# successful response is usable yet (a nightly must list this
# platform's asset); one that never becomes ready is returned as-is for
# the caller to diagnose. $What names the lookup in the final error.
function Invoke-GitHubApi([string]$Uri, [string]$What, [scriptblock]$Ready = { $true }) {
    $delay = 2
    for ($attempt = 1; ; $attempt++) {
        $headers = @{ 'User-Agent' = 'q2-install'; 'Accept' = 'application/vnd.github+json' }
        if ($Api.Token) { $headers['Authorization'] = "Bearer $($Api.Token)" }
        try {
            $rel = Invoke-RestMethod -Uri $Uri -Headers $headers
            if ((& $Ready $rel) -or $attempt -ge $ApiAttempts) { return $rel }
            $why = 'the release does not list the expected asset yet'
        }
        catch {
            $status = Get-HttpStatus $_
            if ($status -eq 401 -and $Api.Token) {
                Write-Warning 'q2 install: GitHub rejected the token in GH_TOKEN/GITHUB_TOKEN (HTTP 401); retrying without it'
                $Api.Token = $null
                $attempt--
                continue
            }
            $why = Get-ApiFailureReason $status
            if ($status -in 403, 429 -or $attempt -ge $ApiAttempts) { Die "could not resolve $What ($why)" }
        }
        Step "$why; retrying in ${delay}s ($($attempt + 1)/$ApiAttempts)..."
        Start-Sleep -Seconds $delay
        $delay *= 2
    }
}

# Channel selection is one or the other (mirrors install.sh): nightlies
# are replaced daily, so a nightly version can never be pinned by tag.
if ($Nightly -and $Version) { Die '-Nightly and -Version are mutually exclusive: -Nightly for the latest nightly, -Version vX.Y.Z for a release' }
if ($Version -match '-nightly\.') { Die "nightly builds cannot be pinned by version (they are replaced daily); use -Nightly instead of -Version $Version" }

if ($Nightly -and -not $ArtifactUrl) {
    # The rolling `nightly` prerelease: invisible to releases/latest, and
    # its tag carries no version — the version is in the asset name
    # (q2-<version>-windows_amd64.zip), so pick the asset by name.
    Step 'resolving nightly release...'
    $isAsset = { param($a) $a.name -match "^q2-.+-$Platform\.zip$" }
    $rel = Invoke-GitHubApi "https://api.github.com/repos/$Owner/$Repo/releases/tags/nightly" `
        "the nightly release (is one published at https://github.com/$Owner/$Repo/releases/tag/nightly?)" `
        { param($r) @($r.assets | Where-Object { & $isAsset $_ }).Count -gt 0 }
    $asset = @($rel.assets | Where-Object { & $isAsset $_ })
    if ($asset.Count -eq 0) { Die "the nightly release has no $Platform archive (expected q2-<version>-$Platform.zip)" }
    $ArtifactUrl = $asset[0].browser_download_url
    $Version = $asset[0].name -replace '^q2-', '' -replace "-$Platform\.zip$", ''
    Step "nightly release: $Version"
}
elseif (-not $Version -and -not $ArtifactUrl) {
    Step 'resolving latest release...'
    $rel = Invoke-GitHubApi "https://api.github.com/repos/$Owner/$Repo/releases/latest" 'the latest release'
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
