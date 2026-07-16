-- Control for content-insert-inplace: whole-property reassignment,
-- which persists in both Pandoc and q2.
function Div(div)
  local c = div.content
  c:insert(pandoc.Div(pandoc.Plain(pandoc.Str('hello'))))
  div.content = c
  return div
end
