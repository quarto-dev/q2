#!/bin/sh
# Repro for issue #1205: io.popen in a Lua shortcode hangs a parallel website render.
# Usage: repro.sh <outdir> [pages] [calls-per-page] [popen|pipe]
# Generates a website project in <outdir>; then run `q2 render` inside it.
set -eu
OUT=${1:?usage: repro.sh <outdir> [pages] [calls] [popen|pipe]}
PAGES=${2:-60}
CALLS=${3:-10}
MODE=${4:-popen}
mkdir -p "$OUT/_extensions/pop"
cd "$OUT"
printf 'project:\n  type: website\n' > _quarto.yml
printf 'title: pop\nversion: 0.0.1\ncontributes:\n  shortcodes:\n    - pop.lua\n' > _extensions/pop/_extension.yml
if [ "$MODE" = pipe ]; then
cat > _extensions/pop/pop.lua <<'LUA'
return {
  ["pop"] = function(args)
    return pandoc.Str(pandoc.pipe("sh", {"-c", "sleep 0.01; echo hi"}, ""))
  end
}
LUA
else
cat > _extensions/pop/pop.lua <<'LUA'
return {
  ["pop"] = function(args)
    local h = io.popen("sh -c 'sleep 0.01; echo hi' 2>&1")
    local out = h:read("*a")
    h:close()
    return pandoc.Str(out)
  end
}
LUA
fi
printf -- '---\ntitle: Home\n---\n\nHome.\n' > index.qmd
i=1
while [ "$i" -le "$PAGES" ]; do
  { printf -- '---\ntitle: p%s\n---\n' "$i"; j=1; while [ "$j" -le "$CALLS" ]; do printf '\nx {{< pop >}}\n'; j=$((j+1)); done; } > "p$i.qmd"
  i=$((i+1))
done
