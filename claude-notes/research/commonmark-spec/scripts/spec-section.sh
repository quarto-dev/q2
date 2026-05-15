#!/usr/bin/env bash
# spec-section.sh — print one section of the CommonMark spec.
#
# Usage:
#   spec-section.sh "<title fragment>"   # case-insensitive substring match
#   spec-section.sh <line-number>        # section containing that line
#
# A section runs from a `^# ` / `^## ` / `^### ` heading up to (but not
# including) the next heading of equal or higher level. Headings that
# appear inside 32-backtick "example" fences are ignored.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SPEC="${SPEC_PATH:-$SCRIPT_DIR/../../../../external-sources/commonmark/spec.txt}"

if [[ ! -f "$SPEC" ]]; then
  echo "error: spec file not found at $SPEC" >&2
  echo "       set SPEC_PATH to override" >&2
  exit 2
fi

if [[ $# -lt 1 ]]; then
  cat >&2 <<EOF
Usage:
  $(basename "$0") "<title fragment>"   # case-insensitive substring
  $(basename "$0") <line-number>        # section containing that line

Examples:
  $(basename "$0") "fenced code blocks"
  $(basename "$0") 1934
EOF
  exit 1
fi

QUERY="$*"

# Build list of (heading_line, level, title) excluding lines inside
# 32-backtick example fences. Output: "line|level|title".
HEADINGS=$(awk '
  BEGIN { in_fence = 0 }
  /^`{32,}/ { in_fence = 1 - in_fence; next }
  !in_fence && /^#{1,3} / {
    match($0, /^#+/)
    print NR"|"RLENGTH"|"substr($0, RLENGTH + 2)
  }
' "$SPEC")

TOTAL_LINES=$(wc -l < "$SPEC" | tr -d ' ')

# Resolve QUERY to a heading line number.
TARGET_LINE=""
if [[ "$QUERY" =~ ^[0-9]+$ ]]; then
  # Numeric: find the latest heading <= QUERY.
  TARGET_LINE=$(awk -F'|' -v q="$QUERY" '
    $1 <= q { last = $1 }
    END { print last }
  ' <<< "$HEADINGS")
  if [[ -z "$TARGET_LINE" ]]; then
    echo "error: no heading found before line $QUERY" >&2
    exit 3
  fi
else
  # Substring (case-insensitive).
  MATCHES=$(awk -F'|' -v q="$QUERY" '
    BEGIN { qq = tolower(q) }
    index(tolower($3), qq) > 0 { print }
  ' <<< "$HEADINGS")
  COUNT=$(printf '%s\n' "$MATCHES" | grep -c . || true)
  if [[ "$COUNT" -eq 0 ]]; then
    echo "error: no heading matches '$QUERY'" >&2
    exit 3
  fi
  if [[ "$COUNT" -gt 1 ]]; then
    echo "warning: multiple headings match '$QUERY', using first:" >&2
    printf '%s\n' "$MATCHES" | awk -F'|' '{ print "  line "$1" (level "$2"): "$3 }' >&2
  fi
  TARGET_LINE=$(printf '%s\n' "$MATCHES" | head -n 1 | cut -d'|' -f1)
fi

# Find the end line: next heading with level <= target's level, minus 1.
TARGET_LEVEL=$(awk -F'|' -v t="$TARGET_LINE" '$1 == t { print $2 }' <<< "$HEADINGS")
END_LINE=$(awk -F'|' -v t="$TARGET_LINE" -v lvl="$TARGET_LEVEL" -v total="$TOTAL_LINES" '
  BEGIN { result = total }
  $1 > t && $2 <= lvl { result = $1 - 1; exit }
  END { print result }
' <<< "$HEADINGS")

awk -v s="$TARGET_LINE" -v e="$END_LINE" 'NR >= s && NR <= e { print }' "$SPEC"
