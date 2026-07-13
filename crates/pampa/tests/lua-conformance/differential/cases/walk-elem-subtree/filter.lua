-- bd-2j048yfm: elem:walk applies the filter to the element's subtree.
function Para(p)
  return p:walk{ Str = function(s) return pandoc.Str(s.text:upper()) end }
end
