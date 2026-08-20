return {
  Str = function(el) if el.text == "MARKER" then return pandoc.Str("NAMED-RAN") end end,
  { Str = function(el) if el.text == "MARKER" then return pandoc.Str("ARRAY-RAN") end end },
}
