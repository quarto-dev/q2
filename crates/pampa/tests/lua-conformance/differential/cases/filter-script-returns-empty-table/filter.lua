-- An EMPTY returned table still wins, so the global Str does not run and
-- the document comes back untouched.
function Str(el)
  if el.text == "MARKER" then return pandoc.Str("GLOBAL-RAN") end
end
return {}
