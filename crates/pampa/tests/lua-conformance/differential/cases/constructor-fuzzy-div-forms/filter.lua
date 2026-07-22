-- Guards the 2026-04-01 fuzzy-peeker constructor coercions: all five
-- Pandoc-legal spellings of the same Div must produce identical ASTs.
function Div(div)
  local function s() return pandoc.Str('hello') end
  local forms = {
    pandoc.Div({ pandoc.Plain(pandoc.Inlines({ s() })) }),
    pandoc.Div({ pandoc.Plain({ s() }) }),
    pandoc.Div(pandoc.Plain({ s() })),
    pandoc.Div(pandoc.Plain(s())),
    pandoc.Div(s()),
  }
  local c = div.content
  for _, d in ipairs(forms) do
    c:insert(d)
  end
  div.content = c
  return div
end
