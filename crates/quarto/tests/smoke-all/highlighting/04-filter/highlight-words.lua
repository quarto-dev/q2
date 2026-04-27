-- Filter that adds `data-hl-spans` to code blocks whose first class is `log`.
-- Highlights the literal words ERROR and WARN as `hl-error` / `hl-warning`
-- spans in the rendered HTML, without needing a tree-sitter grammar.
--
-- This demonstrates Quarto's filter-authored highlighting path: any user
-- filter that writes the right JSON shape into `data-hl-spans` gets
-- rendered identically to the built-in tree-sitter stage. See
-- `claude-notes/plans/2026-04-19-syntax-highlighting-design.md` decision 1.
--
-- Encoding: JSON array of `[start_byte, end_byte, capture_name]` triples.
-- Byte offsets are 0-based half-open (`text[start..end]` in Rust /
-- tree-sitter semantics). Capture names flow through to the HTML writer
-- unchanged, with dots becoming hyphens (`string.escape` →
-- `hl-string-escape`).

function CodeBlock(cb)
  if cb.attr.classes[1] ~= "log" then
    return nil
  end

  local spans = {}
  -- Each entry: { needle, capture-name-without-hl-prefix }
  local patterns = {
    { "ERROR", "error" },
    { "WARN", "warning" },
  }

  for _, row in ipairs(patterns) do
    local needle, capture = row[1], row[2]
    local init = 1
    while true do
      -- `find(..., init, true)` does a literal (non-pattern) search
      -- and returns 1-based inclusive [s, e] offsets. Convert to our
      -- 0-based half-open encoding by subtracting 1 from s and leaving
      -- e as-is (since e is 1-based inclusive == 0-based exclusive).
      local s, e = cb.text:find(needle, init, true)
      if not s then break end
      table.insert(spans, { s - 1, e, capture })
      init = e + 1
    end
  end

  -- Sort by start-offset — the HTML writer walks spans in depth-order,
  -- and a stable input order keeps the output deterministic.
  table.sort(spans, function(a, b) return a[1] < b[1] end)

  -- Idiomatic Pandoc-Lua attribute mutation: this persists on the
  -- block because `cb.attr.attributes` is a live proxy into the
  -- block's Attr (see bd-195t).
  cb.attr.attributes["data-hl-spans"] = pandoc.json.encode(spans)
  return cb
end
