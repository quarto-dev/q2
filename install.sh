#!/usr/bin/env bash
#
# q2 installer — downloads a release binary, verifies its checksum
# and Ed25519 signature, and installs it atomically.
#
# One-liner:
#   curl -fsSL https://raw.githubusercontent.com/quarto-dev/q2/main/install.sh | bash
#
# Pass options after `bash -s --`:
#   curl -fsSL .../install.sh | bash -s -- --dest ~/bin
#
# Ported from cscheid/braid's installer (bd-c6l13j79; design notes in
# claude-notes/plans/2026-06-12-q2-github-releases-bundled-mcp.md):
#   - Non-interactive by construction: the script never reads stdin, so
#     it is safe under `curl | bash` and in CI, and needs none of the
#     piped-stdin re-exec machinery larger installers carry.
#   - Checksum verification is mandatory. Installing an unverified
#     binary requires the explicit --insecure-skip-checksum flag.
#   - Signature verification is mandatory too: a checksum proves
#     integrity, not authenticity — whoever can replace the artifact on
#     the release can replace its .sha256 alongside. Each archive is
#     signed with minisign (Ed25519); the public key is pinned in this
#     script, which ships from the main branch — a trust path an
#     attacker with mere release-asset access cannot touch. minisign is
#     packaged everywhere (brew/apt/dnf/apk/...), so absence is a
#     refusal with install guidance, not a silent downgrade;
#     --insecure-skip-signature is the explicit escape hatch.
#   - Tested by crates/quarto/tests/integration/bootstrap_sh.rs,
#     offline, through --artifact-url file:// + --checksum (and, for
#     --nightly, a file:// releases API via Q2_RELEASES_API_BASE).
#   - --nightly installs the rolling `nightly` prerelease that the
#     Nightly workflow publishes from main (bd-p4ljdp2e). Same archive
#     contract, same signing key; only the lookup differs (the version
#     is in the asset name, not the tag).
#
# Note `q2 mcp` additionally needs Node.js 24+ at runtime (the MCP
# server is embedded in the binary but runs on your node). Everything
# else in q2 works without node.

set -euo pipefail
umask 022

# ============================================================================
# Configuration
# ============================================================================
OWNER="${Q2_REPO_OWNER:-quarto-dev}"
REPO="${Q2_REPO_NAME:-q2}"
BINARY_NAME="q2"

# The q2 release signing key (minisign/Ed25519, key ID 91F595A50BD20376).
# Generated 2026-06-12; signs releases from v0.1.0 on. Also published in
# the README and in release notes.
MINISIGN_PUBKEY="RWR2A9ILpZX1kVF3Q6uk5TRus8FDM25H2F+KKKHEuqlxv+JJSLyPalvN"
# Test hook: lets the test suite simulate a machine without minisign.
# No weaker than PATH, which an attacker in this position also controls.
MINISIGN_BIN="${Q2_MINISIGN:-minisign}"
# Test hook: where the GitHub releases API is read from, so the offline
# test suite can serve `releases/tags/nightly` from a file:// directory.
# Used in exactly one place (resolve_nightly); the stable path through
# releases/latest never consults it. Redirecting it lets an attacker
# choose the archive and its .sha256 sidecar — not the signature, which
# must still verify against the key pinned above with a trusted comment
# equal to the filename. That check is the trust boundary; this variable
# exposes strictly less than the documented --artifact-url flag.
RELEASES_API_BASE="${Q2_RELEASES_API_BASE:-https://api.github.com/repos/${OWNER}/${REPO}}"

VERSION=""
NIGHTLY=0
DEST=""
ARTIFACT_URL=""
CHECKSUM=""
INSECURE_SKIP_CHECKSUM=0
INSECURE_SKIP_SIGNATURE=0
FROM_SOURCE=0
UNINSTALL=0
PRINT_PLATFORM=0
QUIET=0

MAX_RETRIES=3
DOWNLOAD_TIMEOUT=300

# ============================================================================
# Output: progress to stderr (stdout stays clean for scripting); colors
# only when stderr is a terminal. --quiet silences progress, never
# warnings or errors.
# ============================================================================
if [ -t 2 ]; then
    RED=$'\033[0;31m' GREEN=$'\033[0;32m' YELLOW=$'\033[1;33m' BLUE=$'\033[0;34m' NC=$'\033[0m'
else
    RED="" GREEN="" YELLOW="" BLUE="" NC=""
fi

log_step()    { [ "$QUIET" -eq 1 ] || printf '%s\n' "${BLUE}→${NC} $*" >&2; }
log_success() { [ "$QUIET" -eq 1 ] || printf '%s\n' "${GREEN}✓${NC} $*" >&2; }
log_warn()    { printf '%s\n' "${YELLOW}q2 installer:${NC} $*" >&2; }
log_error()   { printf '%s\n' "${RED}q2 installer:${NC} $*" >&2; }
die()         { log_error "$@"; exit 1; }

# A full copy-pasteable re-run line. Refusals that say "re-run with
# <flag>" must show where the flag goes: under curl|bash that is after
# `bash -s --`, which nothing else on screen makes guessable.
rerun_line() {
    printf 'curl -fsSL https://raw.githubusercontent.com/%s/%s/main/install.sh | bash -s -- %s' \
        "$OWNER" "$REPO" "$*"
}

usage() {
    cat <<EOF
q2 installer — install the q2 binary from GitHub releases

Usage:
  curl -fsSL https://raw.githubusercontent.com/${OWNER}/${REPO}/main/install.sh | bash
  curl -fsSL .../install.sh | bash -s -- [OPTIONS]

Options:
  --version vX.Y.Z          Install a specific version (default: latest release)
  --nightly                 Install the latest nightly build instead: the
                            rolling prerelease built from main whenever it
                            changes (version like 0.33.0-nightly.20260919,
                            replaced daily; same signing key)
  --dest DIR                Install directory (default: ~/.local/bin)
  --artifact-url URL        Install from a specific artifact URL (file:// works)
  --checksum SHA256         Expected SHA-256 of the artifact
  --insecure-skip-checksum  Allow installation when no checksum is available
  --minisign-pubkey KEY     minisign public key to verify signatures against
                            (default: the q2 release key pinned in this script)
  --insecure-skip-signature Skip Ed25519 signature verification
  --from-source             Build with cargo from a fresh clone instead
  --uninstall               Remove the installed binary
  --print-platform          Print the detected platform string and exit
  --quiet                   Suppress progress output (warnings/errors still print)
  --help                    Show this help

Environment:
  Q2_INSTALL_DIR            Override the default install directory
                            (the --dest flag wins over it)
  GH_TOKEN / GITHUB_TOKEN   A GitHub token for the --nightly lookup, which
                            reads the GitHub API (anonymous reads are
                            limited to 60/hour per IP address; a token
                            raises that). Optional; never sent anywhere
                            but api.github.com

Supported platforms: linux_amd64, linux_arm64, darwin_amd64,
darwin_arm64. Windows users: use install.ps1 instead. On anything
else, re-run with --from-source (requires Rust; node+npm too if you
want \`q2 mcp\`).
EOF
}

# ============================================================================
# Argument parsing. Unknown flags are errors: a typo silently ignored is
# how a --checksun install ends up unverified.
# ============================================================================
need_value() { [ "$2" -ge 2 ] || die "$1 needs a value (see --help)"; }

while [ $# -gt 0 ]; do
    case "$1" in
        --version)   need_value "$1" $#; VERSION="$2"; shift 2 ;;
        --version=*) VERSION="${1#*=}"; shift ;;
        --nightly)   NIGHTLY=1; shift ;;
        --dest)      need_value "$1" $#; DEST="$2"; shift 2 ;;
        --dest=*)    DEST="${1#*=}"; shift ;;
        --artifact-url)   need_value "$1" $#; ARTIFACT_URL="$2"; shift 2 ;;
        --artifact-url=*) ARTIFACT_URL="${1#*=}"; shift ;;
        --checksum)   need_value "$1" $#; CHECKSUM="$2"; shift 2 ;;
        --checksum=*) CHECKSUM="${1#*=}"; shift ;;
        --insecure-skip-checksum) INSECURE_SKIP_CHECKSUM=1; shift ;;
        --minisign-pubkey)   need_value "$1" $#; MINISIGN_PUBKEY="$2"; shift 2 ;;
        --minisign-pubkey=*) MINISIGN_PUBKEY="${1#*=}"; shift ;;
        --insecure-skip-signature) INSECURE_SKIP_SIGNATURE=1; shift ;;
        --from-source)    FROM_SOURCE=1; shift ;;
        --uninstall)      UNINSTALL=1; shift ;;
        --print-platform) PRINT_PLATFORM=1; shift ;;
        --quiet|-q)       QUIET=1; shift ;;
        -h|--help)        usage; exit 0 ;;
        *) die "unknown option: $1 (see --help)" ;;
    esac
done

# Channel selection is one or the other. Nightlies are replaced daily,
# so there is no tag to pin a nightly version to: `--version 0.33.0-
# nightly.20260919` can never resolve, and the right spelling is --nightly.
if [ "$NIGHTLY" -eq 1 ] && [ -n "$VERSION" ]; then
    die "--nightly and --version are mutually exclusive: pass --nightly for the latest nightly, or --version vX.Y.Z for a release"
fi
case "$VERSION" in
    *-nightly.*) die "nightly builds cannot be pinned by version (they are replaced daily); use --nightly instead of --version $VERSION" ;;
esac
if [ "$NIGHTLY" -eq 1 ] && [ "$FROM_SOURCE" -eq 1 ]; then
    die "--nightly and --from-source are mutually exclusive (--from-source already builds the current main)"
fi

# Dest precedence: --dest flag > Q2_INSTALL_DIR > ~/.local/bin.
if [ -z "$DEST" ]; then
    if [ -n "${Q2_INSTALL_DIR:-}" ]; then
        DEST="$Q2_INSTALL_DIR"
    elif [ -n "${HOME:-}" ]; then
        DEST="$HOME/.local/bin"
    else
        die "HOME is not set; pass --dest DIR"
    fi
fi

# ============================================================================
# Platform detection: os × arch is the whole story.
# ============================================================================
detect_platform() {
    local os arch
    case "$(uname -s)" in
        Linux*)  os="linux" ;;
        Darwin*) os="darwin" ;;
        *) die "unsupported OS: $(uname -s) — re-run with --from-source (requires Rust)" ;;
    esac
    case "$(uname -m)" in
        x86_64|amd64)  arch="amd64" ;;
        aarch64|arm64) arch="arm64" ;;
        *) die "unsupported architecture: $(uname -m) — re-run with --from-source (requires Rust)" ;;
    esac
    printf '%s_%s\n' "$os" "$arch"
}

# ============================================================================
# Version resolution: GitHub API first, releases/latest redirect as the
# fallback. Failure is an error (no implicit source build: surprising a
# user with a multi-minute compile is worse than asking them to re-run
# with --from-source).
# ============================================================================
resolve_version() {
    [ -n "$VERSION" ] && return 0

    log_step "resolving latest release..."
    local tag=""
    tag=$(curl -fsSL --connect-timeout 10 --max-time 30 \
        -H "Accept: application/vnd.github+json" \
        "https://api.github.com/repos/${OWNER}/${REPO}/releases/latest" 2>/dev/null \
        | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1) || true

    if [ -z "$tag" ]; then
        tag=$(curl -fsSL -o /dev/null -w '%{url_effective}' \
            "https://github.com/${OWNER}/${REPO}/releases/latest" 2>/dev/null \
            | sed 's|.*/tag/||') || true
    fi

    case "$tag" in
        v[0-9]*) VERSION="$tag"; log_step "latest release: $VERSION" ;;
        *) die "could not determine the latest release; pass --version vX.Y.Z or --from-source" ;;
    esac
}

# ============================================================================
# Nightly resolution (bd-p4ljdp2e). The rolling `nightly` prerelease is
# invisible to releases/latest (GitHub excludes prereleases), and its
# tag carries no version — the version is in the asset name
# (q2-<version>-<platform>.tar.gz). So: read the release by tag, pick
# this platform's archive by name. The .sha256/.minisig sidecars are
# then fetched from the same URL stem, exactly as for a release.
# ============================================================================
NIGHTLY_ARTIFACT_URL=""

# GitHub API reads (bd-n9yh30c8). Anonymous calls share a budget of 60
# per hour per IP address, which shared CI runners (GitHub's macOS
# fleet especially) and NATed networks exhaust; so a token from
# GH_TOKEN or GITHUB_TOKEN is sent when set — only to api.github.com,
# never to a Q2_RELEASES_API_BASE override. A stale token gets a 401
# where an anonymous call would work, so a 401 drops the token and
# retries once. The token goes to curl on stdin (-K -), not argv, so
# it never shows in `ps`.
API_TOKEN="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
API_BODY=""
API_STATUS=""   # HTTP status of the last api_get; 000 = no HTTP response

# api_get URL: sets API_BODY and API_STATUS; succeeds on a 2xx (or a
# readable file:// URL, which has no status).
api_get() {
    local url="$1" config="" out
    case "$url" in
        https://api.github.com/*)
            [ -n "$API_TOKEN" ] && config="header = \"Authorization: Bearer ${API_TOKEN}\"" ;;
    esac
    API_BODY="" API_STATUS=000
    out=$(printf '%s\n' "$config" | curl -sSL -K - --connect-timeout 10 --max-time 30 \
        -H "Accept: application/vnd.github+json" \
        -w '\n%{http_code}' "$url" 2>/dev/null) || return 1
    API_STATUS="${out##*$'\n'}"
    API_BODY="${out%$'\n'*}"
    if [ "$API_STATUS" = 401 ] && [ -n "$config" ]; then
        log_warn "GitHub rejected the token in GH_TOKEN/GITHUB_TOKEN (HTTP 401); retrying without it"
        API_TOKEN=""
        api_get "$url"
        return
    fi
    case "$API_STATUS" in 2[0-9][0-9]|000) return 0 ;; *) return 1 ;; esac
}

# Why the last api_get failed, in words.
api_failure_reason() {
    case "$API_STATUS" in
        000) echo "no response from the GitHub API" ;;
        403|429) echo "HTTP $API_STATUS: GitHub API rate limit, most likely; set GH_TOKEN to a GitHub token to raise it" ;;
        404) echo "HTTP 404: no release is tagged nightly" ;;
        *) echo "HTTP $API_STATUS from the GitHub API" ;;
    esac
}

# Lookup attempts, with a doubling backoff from 2s (2+4+8+16 = 30s in
# all). The retries cover transient API errors and the seconds after
# the Nightly workflow replaces the release, when the tag can briefly
# 404 or list no assets. A rate limit is not retried: it lasts up to
# an hour.
NIGHTLY_LOOKUP_ATTEMPTS=5

resolve_nightly() {
    local platform="$1" url="" name why attempt=1 delay=2
    log_step "resolving nightly release..."
    while :; do
        if api_get "${RELEASES_API_BASE}/releases/tags/nightly"; then
            # One field per line (GitHub pretty-prints, but do not rely
            # on it), then the archive for this platform — the .tar.gz
            # itself, not its .sha256/.minisig sidecars, which the
            # trailing anchor excludes.
            url=$(printf '%s\n' "$API_BODY" | tr ',' '\n' \
                | sed -n 's/.*"browser_download_url": *"\([^"]*\/'"${BINARY_NAME}"'-[^"/]*-'"${platform}"'\.tar\.gz\)".*/\1/p' \
                | head -n 1)
            [ -n "$url" ] && break
            why="the nightly release lists no ${platform} archive"
        else
            why=$(api_failure_reason)
            case "$API_STATUS" in 403|429) attempt=$NIGHTLY_LOOKUP_ATTEMPTS ;; esac
        fi
        [ "$attempt" -ge "$NIGHTLY_LOOKUP_ATTEMPTS" ] && break
        attempt=$((attempt + 1))
        log_step "${why}; retrying in ${delay}s ($attempt/$NIGHTLY_LOOKUP_ATTEMPTS)..."
        sleep "$delay"
        delay=$((delay * 2))
    done

    if [ -z "$url" ]; then
        case "$why" in
            "the nightly release lists no"*)
                die "the nightly release has no ${platform} archive (expected ${BINARY_NAME}-<version>-${platform}.tar.gz); re-run without --nightly for a release" ;;
            *)
                die "could not resolve the nightly release (${why}; is one published at https://github.com/${OWNER}/${REPO}/releases/tag/nightly?); for a release, drop --nightly" ;;
        esac
    fi

    name="$(basename "$url")"
    VERSION="${name#"${BINARY_NAME}-"}"
    VERSION="${VERSION%"-${platform}.tar.gz"}"
    NIGHTLY_ARTIFACT_URL="$url"
    log_step "nightly release: $VERSION"
}

# ============================================================================
# Download: curl only (preinstalled on macOS, universal on Linux dev
# boxes; a wget fallback would double every download path). Partial
# downloads land in a .part file and are moved into place only on
# success. curl natively honors HTTPS_PROXY et al.
# ============================================================================
download_file() {
    local url="$1" dest="$2" attempt=1
    command -v curl >/dev/null 2>&1 || die "curl is required (https://curl.se)"

    while :; do
        if curl -fL --silent --show-error --retry 2 \
            --connect-timeout 30 --max-time "$DOWNLOAD_TIMEOUT" \
            -o "${dest}.part" "$url" 2>/dev/null; then
            mv -f "${dest}.part" "$dest"
            return 0
        fi
        rm -f "${dest}.part"
        [ "$attempt" -ge "$MAX_RETRIES" ] && return 1
        attempt=$((attempt + 1))
        log_step "download failed; retrying ($attempt/$MAX_RETRIES)..."
        sleep 2
    done
}

# ============================================================================
# Checksum verification: fail closed. A missing checksum aborts the
# install unless --insecure-skip-checksum says otherwise, explicitly.
# ============================================================================
sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        return 1
    fi
}

verify_checksum() {
    local file="$1" expected="$2" name="$3"

    if [ -z "$expected" ]; then
        if [ "$INSECURE_SKIP_CHECKSUM" -eq 1 ]; then
            log_warn "no checksum available for $name; installing UNVERIFIED (--insecure-skip-checksum)"
            return 0
        fi
        die "no checksum available for $name; refusing to install an unverified binary.
  Pass --checksum SHA256, provide ${name}.sha256 next to the artifact,
  or (not recommended) skip verification — flags go after \`bash -s --\`:
    $(rerun_line --insecure-skip-checksum)"
    fi

    case "$expected" in
        *[!0-9a-fA-F]*) die "invalid SHA-256 checksum: $expected" ;;
    esac
    [ "${#expected}" -eq 64 ] || die "invalid SHA-256 checksum (need 64 hex digits): $expected"

    local actual
    if ! actual=$(sha256_of "$file"); then
        if [ "$INSECURE_SKIP_CHECKSUM" -eq 1 ]; then
            log_warn "no SHA-256 tool found; installing UNVERIFIED (--insecure-skip-checksum)"
            return 0
        fi
        die "no SHA-256 tool found (need sha256sum or shasum)"
    fi

    if [ "$expected" != "$actual" ]; then
        die "checksum mismatch for $name:
  expected: $expected
  got:      $actual"
    fi
    log_success "checksum verified"
}

# ============================================================================
# Signature verification: Ed25519 via minisign, against the public key
# pinned at the top of this script. Mandatory, like checksums: skipping
# requires the explicit --insecure-skip-signature flag. The trusted
# comment must equal the archive filename (Zig's discipline), so a
# validly signed *different* artifact — say, an older release — cannot
# be replayed under this name.
# ============================================================================
verify_signature() {
    local file="$1" sig="$2" name="$3"

    if [ "$INSECURE_SKIP_SIGNATURE" -eq 1 ]; then
        log_warn "signature NOT checked; authenticity UNVERIFIED (--insecure-skip-signature)"
        return 0
    fi

    command -v "$MINISIGN_BIN" >/dev/null 2>&1 || die "minisign is required to verify the release signature. Install it first:
    brew install minisign         (macOS)
    sudo apt install minisign     (Debian/Ubuntu)
    sudo dnf install minisign     (Fedora)
    apk add minisign              (Alpine)
  then re-run this script — or (not recommended) skip signature
  verification; flags go after \`bash -s --\`:
    $(rerun_line --insecure-skip-signature)"

    [ -f "$sig" ] || die "no signature (.minisig) available for $name; refusing to install.
  Provide ${name}.minisig next to the artifact, or (not recommended)
  skip signature verification — flags go after \`bash -s --\`:
    $(rerun_line --insecure-skip-signature)"

    local verify_out
    if ! verify_out=$("$MINISIGN_BIN" -Vm "$file" -x "$sig" -P "$MINISIGN_PUBKEY" 2>&1); then
        die "signature verification FAILED for $name:
$verify_out
  The artifact was not signed by the q2 release key. Do not install it."
    fi

    local comment
    comment=$(printf '%s\n' "$verify_out" | sed -n 's/^Trusted comment: //p')
    if [ "$comment" != "$name" ]; then
        die "trusted comment mismatch for $name:
  expected: $name
  got:      $comment
  The signature is valid but for a different artifact (possibly an older
  release replayed under this name). Do not install it."
    fi
    log_success "signature verified (trusted comment: $comment)"
}

# ============================================================================
# Atomic install: write next to the destination, then rename. A crash
# mid-install can never leave a truncated binary on PATH.
# ============================================================================
install_binary() {
    local src="$1"
    mkdir -p "$DEST"
    local tmp_dest="$DEST/$BINARY_NAME.tmp.$$"
    if ! install -m 0755 "$src" "$tmp_dest"; then
        rm -f "$tmp_dest"
        die "failed to write to $DEST (permissions?)"
    fi
    mv -f "$tmp_dest" "$DEST/$BINARY_NAME"
}

# ============================================================================
# Release install: download, verify, extract, install.
# ============================================================================
install_from_artifact() {
    local platform="$1" url archive_name

    if [ -n "$ARTIFACT_URL" ]; then
        url="$ARTIFACT_URL"
        archive_name="$(basename "$ARTIFACT_URL")"
    elif [ "$NIGHTLY" -eq 1 ]; then
        resolve_nightly "$platform"
        url="$NIGHTLY_ARTIFACT_URL"
        archive_name="$(basename "$url")"
    else
        resolve_version
        local tag="v${VERSION#v}" ver="${VERSION#v}"
        archive_name="${BINARY_NAME}-${ver}-${platform}.tar.gz"
        url="https://github.com/${OWNER}/${REPO}/releases/download/${tag}/${archive_name}"
    fi

    log_step "downloading $archive_name..."
    download_file "$url" "$TMP/$archive_name" || die "download failed: $url"

    local expected=""
    if [ -n "$CHECKSUM" ]; then
        expected="${CHECKSUM%% *}"
    elif download_file "${url}.sha256" "$TMP/expected.sha256"; then
        expected="$(awk '{print $1; exit}' "$TMP/expected.sha256")"
    fi
    verify_checksum "$TMP/$archive_name" "$expected" "$archive_name"

    local sig="$TMP/$archive_name.minisig"
    if [ "$INSECURE_SKIP_SIGNATURE" -eq 0 ]; then
        download_file "${url}.minisig" "$sig" || true # absence handled below
    fi
    verify_signature "$TMP/$archive_name" "$sig" "$archive_name"

    log_step "extracting..."
    mkdir -p "$TMP/extract"
    tar -xzf "$TMP/$archive_name" -C "$TMP/extract" \
        || die "could not extract $archive_name"

    [ -f "$TMP/extract/$BINARY_NAME" ] \
        || die "archive does not contain a '$BINARY_NAME' binary"

    install_binary "$TMP/extract/$BINARY_NAME"
    log_success "installed $DEST/$BINARY_NAME"
}

# ============================================================================
# Source build: explicit opt-in only. Requires an existing Rust
# toolchain; this script does not install one behind your back. The
# embedded hub MCP bundle additionally needs node+npm — built when
# available, loudly skipped when not (`q2 mcp` is then non-functional,
# everything else works).
# ============================================================================
build_from_source() {
    command -v git >/dev/null 2>&1 || die "git is required for --from-source"
    command -v cargo >/dev/null 2>&1 \
        || die "cargo is required for --from-source; install Rust via https://rustup.rs and re-run"

    local clone_args=(--quiet --depth 1)
    [ -n "$VERSION" ] && clone_args+=(--branch "v${VERSION#v}")

    log_step "cloning ${OWNER}/${REPO}..."
    git clone "${clone_args[@]}" "https://github.com/${OWNER}/${REPO}.git" "$TMP/src" \
        || die "clone failed"

    if command -v npm >/dev/null 2>&1; then
        log_step "building the hub MCP bundle (npm)..."
        (cd "$TMP/src" && npm install --no-audit --no-fund --silent \
            && npm run bundle -w ts-packages/quarto-hub-mcp --silent) \
            || die "MCP bundle build failed"
    else
        log_warn "npm not found: building without the hub MCP bundle (\`q2 mcp\` will be non-functional; everything else works)"
    fi

    log_step "building with cargo (this may take several minutes)..."
    (cd "$TMP/src" && CARGO_TARGET_DIR="$TMP/target" cargo build --release --quiet -p quarto) \
        || die "build failed"

    [ -f "$TMP/target/release/$BINARY_NAME" ] || die "binary not found after build"
    install_binary "$TMP/target/release/$BINARY_NAME"
    log_success "installed $DEST/$BINARY_NAME (source build)"
}

# ============================================================================
# PATH advice. Printed, never applied: this script does not edit shell
# rc files.
# ============================================================================
warn_path() {
    case ":$PATH:" in
        *:"$DEST":*) ;;
        *) log_warn "$DEST is not on your PATH; add it with:
  export PATH=\"$DEST:\$PATH\"" ;;
    esac
}

do_uninstall() {
    if [ -f "$DEST/$BINARY_NAME" ]; then
        rm -f "$DEST/$BINARY_NAME"
        log_success "removed $DEST/$BINARY_NAME"
    else
        log_warn "nothing to remove at $DEST/$BINARY_NAME"
    fi
}

# ============================================================================
# Main
# ============================================================================
TMP=""
cleanup() { [ -n "$TMP" ] && rm -rf "$TMP"; return 0; }
trap cleanup EXIT

main() {
    if [ "$PRINT_PLATFORM" -eq 1 ]; then
        detect_platform
        exit 0
    fi
    if [ "$UNINSTALL" -eq 1 ]; then
        do_uninstall
        exit 0
    fi

    TMP=$(mktemp -d)

    if [ "$FROM_SOURCE" -eq 1 ]; then
        log_step "install directory: $DEST"
        build_from_source
    else
        local platform
        platform=$(detect_platform)
        log_step "platform: $platform"
        log_step "install directory: $DEST"
        install_from_artifact "$platform"
    fi

    warn_path

    local installed_version
    installed_version=$("$DEST/$BINARY_NAME" --version 2>/dev/null || echo "unknown")
    log_success "done: $installed_version"
}

# The braces make bash parse this whole block before executing it, so a
# `curl | bash` download truncated mid-script can never run half of main.
{ main "$@"; }
