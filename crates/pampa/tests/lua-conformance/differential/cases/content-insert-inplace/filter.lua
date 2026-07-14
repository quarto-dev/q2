-- Class D0 (bd-grkrb9nj): idiomatic in-place mutation of div.content.
-- Pandoc persists the insert; q2 currently discards it silently.
function Div(div)
  div.content:insert(pandoc.Div(pandoc.Plain(pandoc.Str('hello'))))
  return div
end
