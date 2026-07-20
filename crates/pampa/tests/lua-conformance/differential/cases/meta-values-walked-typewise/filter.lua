-- Pins that element filter functions traverse metadata values: the Str
-- handler uppercases everywhere (including inside meta.title), and the
-- Pandoc handler materializes the transformed title into the block
-- stream before clearing meta.
function Str(el)
  return pandoc.Str(el.text:upper())
end
function Pandoc(doc)
  local blocks = doc.blocks
  blocks:insert(pandoc.Para(pandoc.Str(pandoc.utils.stringify(doc.meta.title))))
  return pandoc.Pandoc(blocks, {})
end
