-- bd-sgfiiktn S2: ListAttributes userdata + OrderedList aliases.
-- Builds an OrderedList from ListAttributes userdata, mutates the
-- triple in place through the aliased ol.listAttributes read, and
-- writes through the start/delimiter aliases.
function Para(p)
  local la = pandoc.ListAttributes(2, 'LowerAlpha', 'OneParen')
  local ol = pandoc.OrderedList({ { pandoc.Plain(p.content) } }, la)
  ol.listAttributes.style = 'UpperRoman'
  ol.start = 7
  ol.delimiter = 'TwoParens'
  return ol
end
