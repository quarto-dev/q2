-- Class A2 (oracle probe P12): a table returned from a Block filter is
-- coerced element-wise; a bare string entry becomes Plain(word-split).
function Para(p)
  return { 'two words', pandoc.HorizontalRule() }
end
