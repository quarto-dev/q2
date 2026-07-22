-- bd-2j048yfm: topdown walk with truncation — Para returns (p, false),
-- so Strs inside the Para keep their case; the bullet item's Plain is
-- still descended into and uppercased.
function Div(d)
  return d:walk{
    traverse = 'topdown',
    Para = function(p) return p, false end,
    Str = function(s) return pandoc.Str(s.text:upper()) end,
  }
end
