-- Oracle probe P9: an Inline userdata returned from a Block filter is
-- wrapped in Plain (peekBlocksFuzzy inlines-coercion arm).
function Para(p)
  return pandoc.Str('solo')
end
