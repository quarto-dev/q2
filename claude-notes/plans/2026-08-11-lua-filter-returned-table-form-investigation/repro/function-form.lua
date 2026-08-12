-- The form q2's own docs show: top-level functions.
function Str(el)
  if el.text == "MARKER" then return pandoc.Str("FUNCTION-FORM-RAN") end
end
