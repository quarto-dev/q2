#!/usr/bin/env bash
#
# Measures the cost of building the test binaries in the workspace.
# Used by bd-xvdop to compare baseline vs consolidated layouts.
#
# Usage:
#   scripts/measure-test-build.sh debug   <label>
#   scripts/measure-test-build.sh release <label>
#
# Writes a structured report block to stdout. Caller is expected to
# `cargo clean` *before* invoking this script — we don't, because we
# never want to silently nuke the target/ tree.

set -euo pipefail

if [ $# -lt 1 ]; then
  echo "usage: $0 <debug|release> [label]" >&2
  exit 2
fi

profile="$1"
label="${2:-unlabeled}"

case "$profile" in
  debug)   build_flags="" ; out_dir="target/debug" ;;
  release) build_flags="--release" ; out_dir="target/release" ;;
  *) echo "unknown profile: $profile (expected debug|release)" >&2; exit 2 ;;
esac

# Ensure cargo bin on PATH for nextest etc., but the measurement uses
# plain cargo build --tests so cargo-nextest is not required.
cargo_bin="$(command -v cargo)"
host_triple="$(rustc -vV | awk '/^host:/ {print $2}')"

echo "=========================================="
echo "measure-test-build  profile=$profile  label=$label"
echo "  date:      $(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo "  host:      $host_triple"
echo "  cargo:     $cargo_bin ($(cargo --version))"
echo "  rustc:     $(rustc --version)"
echo "=========================================="

# Don't auto-clean: caller must do it. But sanity-check that target/<p> is
# either absent or small (<= 100 MB) so we don't conflate runs.
if [ -d "$out_dir" ]; then
  pre_size_bytes="$(du -sk "$out_dir" | awk '{print $1*1024}')"
  if [ "$pre_size_bytes" -gt $((100*1024*1024)) ]; then
    echo "WARNING: $out_dir already contains $(du -sh "$out_dir" | cut -f1)" >&2
    echo "         Measurement will not be a clean-build number." >&2
  fi
fi

# Time the build. We use the shell's builtin `time` via a subshell trick so
# both real wall time and exit status propagate cleanly.
echo
echo "--- build: cargo build --workspace --tests $build_flags ---"
start_ns=$(date +%s)
# shellcheck disable=SC2086
cargo build --workspace --tests $build_flags
end_ns=$(date +%s)
wall_s=$(( end_ns - start_ns ))

echo
echo "--- results ---"
echo "build_wall_seconds: $wall_s"

# Total target/<profile> size.
total_size_h="$(du -sh "$out_dir" | cut -f1)"
total_size_b="$(du -sk "$out_dir" | awk '{print $1*1024}')"
echo "target_${profile}_size_human: $total_size_h"
echo "target_${profile}_size_bytes: $total_size_b"

# Whole-target size for reference (sometimes target/* gathers shared
# build-script outputs that the per-profile du doesn't see).
echo "target_root_size_human: $(du -sh target | cut -f1)"

# Per-binary breakdown. Integration test binaries are emitted under
# target/<profile>/deps/ and unit test binaries also live there. Both
# follow the convention <name>-<hash>(.exe). We list the largest 25.
deps_dir="$out_dir/deps"
if [ -d "$deps_dir" ]; then
  echo
  echo "--- top 25 binaries under $deps_dir ---"
  # On macOS du -k prints sizes in 1K blocks; that's plenty of precision.
  # Filter to executables only (skip .rlib, .rmeta, .d, .o, etc.).
  find "$deps_dir" -maxdepth 1 -type f -perm -u+x \
    ! -name '*.rlib' ! -name '*.rmeta' ! -name '*.d' ! -name '*.dSYM' \
    ! -name '*.o' ! -name '*.so' ! -name '*.dylib' \
    -print0 \
    | xargs -0 du -k \
    | sort -rn \
    | head -25 \
    | awk '{ kb=$1; $1=""; sub(/^ /,""); printf "  %8d KB  %s\n", kb, $0 }'

  # Count distinct test binaries (anything that's executable & doesn't
  # look like a dylib). This is the headline number — single binary per
  # test target vs many.
  test_bin_count=$(find "$deps_dir" -maxdepth 1 -type f -perm -u+x \
    ! -name '*.rlib' ! -name '*.rmeta' ! -name '*.d' ! -name '*.dSYM' \
    ! -name '*.o' ! -name '*.so' ! -name '*.dylib' \
    | wc -l | tr -d ' ')
  echo
  echo "executable_count_in_deps: $test_bin_count"

  # Sum bytes across only those executables (excludes rlibs/dSYMs).
  exe_bytes=$(find "$deps_dir" -maxdepth 1 -type f -perm -u+x \
    ! -name '*.rlib' ! -name '*.rmeta' ! -name '*.d' ! -name '*.dSYM' \
    ! -name '*.o' ! -name '*.so' ! -name '*.dylib' \
    -print0 \
    | xargs -0 du -k 2>/dev/null \
    | awk 'BEGIN{s=0} {s+=$1} END{printf "%d", s*1024}')
  echo "executable_bytes_total: $exe_bytes"
fi

echo "=========================================="
