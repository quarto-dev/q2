#!/bin/bash
# Pandoc capture shim: records argv, QUARTO_* env, stdin, and copies of
# file-valued args (--defaults, --metadata-file, --template, ...) before
# exec'ing the real pandoc. Point QUARTO_PANDOC at this script.
set -u
REAL_PANDOC="${REAL_PANDOC:?set REAL_PANDOC to the real pandoc binary}"
CAPTURE_DIR="${CAPTURE_DIR:?set CAPTURE_DIR}"

mkdir -p "$CAPTURE_DIR"
i=1
while [ -d "$CAPTURE_DIR/call-$i" ]; do i=$((i+1)); done
d="$CAPTURE_DIR/call-$i"
mkdir -p "$d/files"

printf '%s\n' "$@" > "$d/argv.txt"
env | grep '^QUARTO_' | sort > "$d/env.txt" || true

# decode filter params for readability
if [ -n "${QUARTO_FILTER_PARAMS:-}" ]; then
  printf '%s' "$QUARTO_FILTER_PARAMS" | base64 -d > "$d/filter-params.json" 2>/dev/null || true
fi

# copy file-valued options (handles "--opt value" form, which Quarto uses)
prev=""
n=0
for a in "$@"; do
  case "$prev" in
    --defaults|-d|--metadata-file|--template|--reference-doc|--from|-f|--include-in-header|--include-before-body|--include-after-body|--css|--epub-cover-image|--lua-filter|--filter)
      if [ -f "$a" ]; then
        n=$((n+1))
        opt="${prev#--}"; opt="${opt#-}"
        cp "$a" "$d/files/$n-$opt-$(basename "$a")"
      fi
      ;;
  esac
  prev="$a"
done

# copy positional input files (*.md)
for a in "$@"; do
  case "$a" in
    *.md) [ -f "$a" ] && cp "$a" "$d/files/input-$(basename "$a")";;
  esac
done

# copy file paths referenced inside any --defaults yml (template:, theme, input)
for a in "$@"; do
  if [ -f "$a" ]; then
    case "$a" in
      *defaults*.yml|*.yaml)
        grep -oE '(/[^" ]+)' "$a" | while read -r p; do
          [ -f "$p" ] && cp "$p" "$d/files/defaultsref-$(basename "$p")" 2>/dev/null
        done
        ;;
    esac
  fi
done

# capture stdin while passing it through
stdin_file="$d/stdin.txt"
cat > "$stdin_file"

exec "$REAL_PANDOC" "$@" < "$stdin_file"
