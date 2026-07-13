-- Class A2: returning a table with non-userdata entries.
-- Pandoc coerces each entry; q2 currently drops the bare strings.
function Str(s)
  if s.text == 'target' then
    return { pandoc.Emph({ pandoc.Str('x') }), pandoc.Str('and'), 'y' }
  end
end
