-- Class B2: Pandoc HTML-like attr map (id key, space-split class key,
-- other keys become attributes). q2 currently ignores all of it.
function Para(p)
  return pandoc.Div({ pandoc.Plain(p.content) },
                    { id = 'sid', class = 'c1 c2', foo = 'bar' })
end
