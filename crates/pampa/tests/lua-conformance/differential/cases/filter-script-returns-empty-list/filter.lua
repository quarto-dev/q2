-- Same, for a one-element list holding an empty filter.
function Str(el)
  if el.text == "MARKER" then return pandoc.Str("GLOBAL-RAN") end
end
return { {} }
