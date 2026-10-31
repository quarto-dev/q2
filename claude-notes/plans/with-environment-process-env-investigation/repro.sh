#!/usr/bin/env bash
# Repro for bd-with-environment-process-env-5folaygy.
# Usage: repro.sh <path-to-q2>
# Builds the project in a temp dir, so no .qmd lands in the claude-notes project.
set -euo pipefail
q2=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
cd "$dir"

cat > _quarto.yml <<'YML'
project:
  type: website
YML

# Case 1: one document's call is visible to another, concurrently rendering one.
cat > setter.lua <<'LUA'
function Pandoc(doc)
  pandoc.system.with_environment({ Q2_LEAK = "from-setter" }, function()
    local t = os.clock()
    while os.clock() - t < 3 do end
  end)
  return doc
end
LUA
cat > reader.lua <<'LUA'
function Pandoc(doc)
  local samples, leak, nopath = 0, 0, 0
  local t = os.clock()
  while os.clock() - t < 3 do
    local env = pandoc.system.environment()
    samples = samples + 1
    if env.Q2_LEAK then leak = leak + 1 end
    if not env.PATH then nopath = nopath + 1 end
  end
  io.stderr:write(("READER samples=%d saw_Q2_LEAK=%d PATH_missing=%d\n"):format(samples, leak, nopath))
  return doc
end
LUA
printf -- '---\ntitle: s\nfilters: [setter.lua]\n---\n\ns\n' > s.qmd
printf -- '---\ntitle: r\nfilters: [reader.lua]\n---\n\nr\n' > r.qmd
echo "== case 1: cross-document leak (expect both counts 0 once fixed)"
"$q2" render 2>&1 | grep READER || echo "no READER line"

# Case 2: the readers disagree inside the callback of a single document.
mkdir single && cd single
cat > probe.lua <<'LUA'
function Pandoc(doc)
  pandoc.system.with_environment({ Q2_PROBE = "set", PATH = os.getenv("PATH") }, function()
    local ok, p = pcall(io.popen, 'printf %s "$Q2_PROBE"')
    local child = "raises"
    if ok then child = p:read("a"); p:close() end
    local _, piped = pcall(pandoc.pipe, "sh", { "-c", 'printf %s "$Q2_PROBE"' }, "")
    io.stderr:write(("PROBE os.getenv=%s environment=%s popen=%s pipe=%s os.getenv(HOME)=%s\n"):format(
      tostring(os.getenv("Q2_PROBE")), tostring(pandoc.system.environment().Q2_PROBE),
      child, tostring(piped), tostring(os.getenv("HOME"))))
  end)
  return doc
end
LUA
printf -- '---\ntitle: p\nfilters: [probe.lua]\n---\n\np\n' > p.qmd
echo "== case 2: readers inside the callback (expect set, popen=raises, HOME nil, once fixed)"
"$q2" render p.qmd 2>&1 | grep PROBE || echo "no PROBE line"

# Case 3: a bad value aborts part-way and leaves the process environment altered.
# Table order is unspecified, so use many good keys around one bad value.
cat > bad.lua <<'LUA'
function Pandoc(doc)
  local t = { Q2_BAD = true }
  for i = 1, 40 do t["Q2_GOOD_" .. i] = "x" end
  pcall(pandoc.system.with_environment, t, function() end)
  local env, leaked = pandoc.system.environment(), 0
  for i = 1, 40 do if env["Q2_GOOD_" .. i] then leaked = leaked + 1 end end
  io.stderr:write(("BAD after-error leaked=%d/40 PATH=%s\n"):format(leaked, tostring(env.PATH ~= nil)))
  return doc
end
LUA
printf -- '---\ntitle: b\nfilters: [bad.lua]\n---\n\nb\n' > b.qmd
echo "== case 3: bad table value (expect leaked=0/40 PATH=true once fixed)"
"$q2" render b.qmd 2>&1 | grep 'BAD after' || echo "no BAD line"
