-- Filter-authored highlighting for structured log lines.
--
-- For every fenced code block with class `systemd-log`, assign a
-- capture to each severity tag inside `<  >` brackets and to the
-- timestamp at the start of each line. Shows two features:
--
--   1. Mapping multiple patterns onto capture names in a table.
--   2. Idiomatic `cb.attr.attributes["data-hl-spans"] = …`
--      mutation (no pandoc.Attr rebuild workaround needed).
--
-- This complements 04-filter (a simple literal-word highlighter) by
-- demonstrating a structured multi-capture pattern close to what a
-- real user filter would write.

local severity_captures = {
  ["<emerg>"]   = "severity.emerg",
  ["<alert>"]   = "severity.alert",
  ["<crit>"]    = "severity.crit",
  ["<err>"]     = "severity.err",
  ["<warning>"] = "severity.warning",
  ["<notice>"]  = "severity.notice",
  ["<info>"]    = "severity.info",
  ["<debug>"]   = "severity.debug",
}

local function find_all_literal(haystack, needle)
  -- Yields all 0-based half-open (start, end) ranges of `needle`
  -- within `haystack`, using literal (non-pattern) search.
  local results = {}
  local init = 1
  while true do
    local s, e = haystack:find(needle, init, true)
    if not s then break end
    results[#results + 1] = { s - 1, e }
    init = e + 1
  end
  return results
end

function CodeBlock(cb)
  if cb.attr.classes[1] ~= "systemd-log" then
    return nil
  end

  local spans = {}

  for needle, capture in pairs(severity_captures) do
    for _, range in ipairs(find_all_literal(cb.text, needle)) do
      spans[#spans + 1] = { range[1], range[2], capture }
    end
  end

  -- Timestamp at the start of each line, shape like "Apr 21 12:34:56".
  -- Lua patterns: %a = letter, %d = digit, %s = whitespace.
  local init = 1
  while init <= #cb.text do
    local s, e = cb.text:find("%a%a%a +%d+ %d%d:%d%d:%d%d", init)
    if not s then break end
    spans[#spans + 1] = { s - 1, e, "timestamp" }
    init = e + 1
  end

  table.sort(spans, function(a, b) return a[1] < b[1] end)

  cb.attr.attributes["data-hl-spans"] = pandoc.json.encode(spans)
  return cb
end
