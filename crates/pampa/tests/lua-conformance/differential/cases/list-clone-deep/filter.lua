-- bd-1fjtodu8: Inlines:clone is deep — mutating the clone must not
-- affect the original list.
function Para(p)
  local ils = pandoc.Inlines('Hello, World!')
  local cl = ils:clone()
  cl[1].text = 'CHANGED'
  return pandoc.Para(ils)
end
