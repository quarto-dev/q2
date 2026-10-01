#!/bin/sh
# QUARTO_PANDOC wrapper: records the exact inputs of a real native pandoc run,
# then execs the real pandoc. Unix-only (POSIX sh); see
# claude-notes/plans/2026-10-01-pandoc-request-R0-foundations.md.
#
#   PANDOC_CAPTURE_DIR   required: raw capture output directory
#   PANDOC_CAPTURE_REAL  real pandoc (default: `pandoc` on PATH)
#
# Raw layout (turned into a committed recording by `pandoc-recording rewrite`):
#   argv.nul      NUL-separated argv (argv[0] recorded as `pandoc`)
#   env.txt       NAME=VALUE lines for the variables pandoc is run with
#   temp/         copy of the pipeline temp root, taken BEFORE pandoc runs
#                 (input JSON, share tree, params, deps file, ...)
#   files/N/      copy of each existing path an argument names outside the
#                 temp root (doc dir, reference-doc, template, ...); N = arg index
#   files.txt     "N<TAB>original path" for each files/N
#   paths.txt     temp_root=, out_path=, out_dir= (the originals)
#   reference/    the native `-o` output (comparison reference)
#   stderr.txt, stdout.txt, status.txt
set -eu

real=${PANDOC_CAPTURE_REAL:-pandoc}

# The `--version` probe (resolve_and_gate_pandoc) is not a run.
if [ "$#" -eq 1 ] && [ "$1" = "--version" ]; then
  exec "$real" --version
fi

: "${PANDOC_CAPTURE_DIR:?PANDOC_CAPTURE_DIR must be set}"
n=1
while [ -e "$PANDOC_CAPTURE_DIR/run-$n" ]; do n=$((n + 1)); done
cap="$PANDOC_CAPTURE_DIR/run-$n"
mkdir -p "$cap/files" "$cap/reference"

{ printf '%s\0' pandoc; for a in "$@"; do printf '%s\0' "$a"; done; } > "$cap/argv.nul"

env | grep -E '^(QUARTO_[A-Z_]*|SOURCE_DATE_EPOCH|TZ|LANG|LC_[A-Z]*)=' | sort > "$cap/env.txt" || true

share=${QUARTO_SHARE_PATH:-}
temp_root=
if [ -n "$share" ]; then
  temp_root=$(dirname "$share")
  mkdir -p "$cap/temp"
  cp -R "$temp_root/." "$cap/temp/"
fi

# `-o <path>` names the output; everything else that exists on disk outside the
# temp root is an input to copy. `--opt=value` is split at the first `=`.
out_path=
prev=
i=0
: > "$cap/files.txt"
for a in "$@"; do
  i=$((i + 1))
  cand=$a
  case $a in --*=*) cand=${a#*=} ;; esac
  if [ "$prev" = "-o" ] || [ "$prev" = "--output" ]; then
    out_path=$a
  elif [ -n "$cand" ] && [ -e "$cand" ]; then
    case $cand in
      "$temp_root"|"$temp_root"/*) ;;   # already in temp/
      *)
        mkdir -p "$cap/files/$i"
        if [ -d "$cand" ]; then cp -R "$cand/." "$cap/files/$i/"; else cp "$cand" "$cap/files/$i/"; fi
        printf '%s\t%s\n' "$i" "$cand" >> "$cap/files.txt"
        ;;
    esac
  fi
  prev=$a
done

{
  printf 'temp_root=%s\n' "$temp_root"
  printf 'out_path=%s\n' "$out_path"
  printf 'out_dir=%s\n' "$( [ -n "$out_path" ] && dirname "$out_path" || true )"
} > "$cap/paths.txt"

set +e
"$real" "$@" > "$cap/stdout.txt" 2> "$cap/stderr.txt"
status=$?
set -e
echo "$status" > "$cap/status.txt"

if [ -n "$out_path" ] && [ -e "$out_path" ]; then
  cp -R "$out_path" "$cap/reference/"
fi

# Re-emit pandoc's streams so the caller (q2) sees what it would have seen.
cat "$cap/stdout.txt"
cat "$cap/stderr.txt" >&2
exit "$status"
