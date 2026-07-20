-- Pins applyFully topdown order: Pandoc -> Meta -> element walk. The
-- Str handler observes flags set by the doc-level handlers; the suffix
-- proves both ran first. (No frontmatter: meta stays {} on both sides.)
traverse = 'topdown'
local pandoc_ran, meta_ran = false, false
function Pandoc(doc)
  pandoc_ran = true
end
function Meta(meta)
  meta_ran = true
end
function Str(el)
  return pandoc.Str(el.text .. (pandoc_ran and '-P' or '') .. (meta_ran and '-M' or ''))
end
