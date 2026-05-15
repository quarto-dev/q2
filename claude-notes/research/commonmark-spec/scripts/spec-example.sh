#!/usr/bin/env bash
# spec-example.sh — print one numbered example from the CommonMark spec.
#
# Usage:
#   spec-example.sh <number>
#
# Examples are sequentially numbered (1..655 in v0.31.2). Each lives in
# a block delimited by lines of >= 32 backticks. The block contains:
#   - the markdown input
#   - a `.` line (separator)
#   - the expected HTML output
#
# This script prints the full block including the fence lines so the
# structure is obvious. Trim the first and last line if you want just
# the body.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SPEC="${SPEC_PATH:-$SCRIPT_DIR/../../../../external-sources/commonmark/spec.txt}"

if [[ ! -f "$SPEC" ]]; then
  echo "error: spec file not found at $SPEC" >&2
  echo "       set SPEC_PATH to override" >&2
  exit 2
fi

if [[ $# -ne 1 ]] || ! [[ "$1" =~ ^[0-9]+$ ]]; then
  cat >&2 <<EOF
Usage: $(basename "$0") <example-number>

Example:
  $(basename "$0") 95
EOF
  exit 1
fi

TARGET="$1"

awk -v target="$TARGET" '
  BEGIN { in_fence = 0; count = 0; printing = 0 }
  /^`{32,}/ {
    if (!in_fence) {
      in_fence = 1
      count++
      if (count == target) { printing = 1; print; next }
    } else {
      in_fence = 0
      if (printing) { print; exit }
      next
    }
    next
  }
  printing { print }
  END {
    if (!printing) {
      print "error: example "target" not found (max in spec is "count")" > "/dev/stderr"
      exit 3
    }
  }
' "$SPEC"
