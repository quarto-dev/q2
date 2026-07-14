-- Class A1: returning a bare string from an Inline filter.
-- Pandoc coerces it; q2 currently ignores it silently.
function Str(s)
  if s.text == 'target' then
    return 'replaced'
  end
end
