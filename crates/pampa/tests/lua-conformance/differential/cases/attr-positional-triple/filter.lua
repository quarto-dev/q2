-- Class B1: Pandoc positional attr triple {id, classes, kv-pairs}.
-- q2's parse_attr currently ignores it (silent empty attr).
function Para(p)
  return pandoc.Div({ pandoc.Plain(p.content) },
                    { 'the-id', { 'c1', 'c2' }, { { 'k', 'v' } } })
end
