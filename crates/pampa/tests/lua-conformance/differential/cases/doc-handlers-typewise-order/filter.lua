-- Pins applyFully typewise order: element walk (meta values first, then
-- blocks) -> Meta -> Pandoc, with the Pandoc handler observing the Meta
-- handler's result. Meta is cleared at the end so the (deliberately
-- different) meta wire encodings never reach the comparison.
local events = {}
function Str(el)
  events[#events + 1] = 'Str:' .. el.text
end
function Meta(meta)
  events[#events + 1] = 'Meta:' .. pandoc.utils.stringify(meta.title)
  meta.marker = 'set-by-meta'
  return meta
end
function Pandoc(doc)
  events[#events + 1] = 'Pandoc:' .. tostring(doc.meta.marker)
  local blocks = doc.blocks
  blocks:insert(pandoc.Para(pandoc.Str(table.concat(events, '|'))))
  return pandoc.Pandoc(blocks, {})
end
