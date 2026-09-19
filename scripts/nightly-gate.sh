#!/usr/bin/env bash
#
# nightly-gate.sh — should the Nightly workflow build this checkout?
#
# "Unreleased changes" (bd-p4ljdp2e, plan Decision 4) means HEAD is
# neither the newest release tag nor the current `nightly` tag:
#
#   HEAD == nearest `v*` tag     → main is fully released; nothing to build
#   HEAD == `nightly` tag        → last night already built this tree
#   otherwise                    → build
#
# Exact-commit comparisons only; no path filters (a docs-only change
# still yields a valid nightly). NIGHTLY_FORCE=1 builds regardless.
#
# The nightly version is the NEXT minor of the workspace version with a
# dated prerelease — `0.32.0` on main → `0.33.0-nightly.20260919` — so it
# sorts above the release it contains (a `0.32.0-nightly.*` would sort
# below `0.32.0` and fail `quarto-required: ">=0.32.0"`). NIGHTLY_DATE
# (YYYYMMDD) pins the date; the default is today, UTC.
#
# Outputs, GitHub-Actions style (`key=value` lines to $GITHUB_OUTPUT, or
# stdout when unset):
#
#   sha               full HEAD sha (build this, not the branch name)
#   prev_release_tag  nearest v* tag reachable from HEAD ("" if none)
#   build             true | false
#   reason            released | nightly-current | unreleased | forced | no-tags
#   version           the nightly version (only when build=true)
#
# Tested by crates/quarto/tests/integration/nightly_gate.rs.

set -euo pipefail

out="${GITHUB_OUTPUT:-/dev/stdout}"
emit() { printf '%s=%s\n' "$1" "$2" >> "$out"; }
note() { printf 'nightly-gate: %s\n' "$*" >&2; }

git rev-parse --is-inside-work-tree >/dev/null 2>&1 \
    || { note "not inside a git repository"; exit 1; }
[ -f Cargo.toml ] || { note "no Cargo.toml here; run from the repository root"; exit 1; }

sha=$(git rev-parse HEAD)

# Nearest release tag reachable from HEAD. `--match 'v*'` keeps
# `nightly` and any other marker tags out of the answer.
prev_release_tag=$(git describe --tags --abbrev=0 --match 'v*' HEAD 2>/dev/null || true)
release_sha=""
if [ -n "$prev_release_tag" ]; then
    release_sha=$(git rev-parse "${prev_release_tag}^{commit}")
fi

# The rolling nightly tag, if one exists (annotated or lightweight).
nightly_sha=$(git rev-parse --verify -q 'refs/tags/nightly^{commit}' || true)

# Same grep as release.yml's preflight, so the two agree on which line
# is "the" version.
cargo_version=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
case "$cargo_version" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) note "could not parse workspace version from Cargo.toml: '$cargo_version'"; exit 1 ;;
esac
IFS=. read -r major minor _patch <<< "$cargo_version"
date="${NIGHTLY_DATE:-$(date -u +%Y%m%d)}"
version="${major}.$((minor + 1)).0-nightly.${date}"

if [ "${NIGHTLY_FORCE:-0}" = "1" ]; then
    build=true;  reason=forced
elif [ -z "$prev_release_tag" ] && [ -z "$nightly_sha" ]; then
    build=true;  reason=no-tags
elif [ "$sha" = "$release_sha" ]; then
    build=false; reason=released
elif [ "$sha" = "$nightly_sha" ]; then
    build=false; reason=nightly-current
else
    build=true;  reason=unreleased
fi

emit sha "$sha"
emit prev_release_tag "$prev_release_tag"
emit build "$build"
emit reason "$reason"
if [ "$build" = true ]; then
    emit version "$version"
    note "build $version from ${sha:0:12} ($reason; last release: ${prev_release_tag:-none})"
else
    note "skip: ${sha:0:12} is ${reason} (last release: ${prev_release_tag:-none})"
fi
