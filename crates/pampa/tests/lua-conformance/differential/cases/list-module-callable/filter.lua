-- bd-1fjtodu8: pandoc.List is a callable module (hslua-list parity).
-- Exercises List{...}, :map, :includes, :insert through the real pipeline.
function Para(p)
  local List = pandoc.List
  local words = List{'alpha', 'beta'}
  local strs = words:map(function(w) return pandoc.Str(w) end)
  if words:includes('beta') then
    strs:insert(pandoc.Str('yes'))
  end
  return pandoc.Para(strs)
end
