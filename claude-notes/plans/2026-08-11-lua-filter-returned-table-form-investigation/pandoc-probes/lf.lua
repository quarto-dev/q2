return {
  { Str = function(el) if el.text == "MARKER" then return pandoc.Str("PASS-ONE") end end },
  { Str = function(el) if el.text == "PASS-ONE" then return pandoc.Str("LIST-FORM-RAN") end end },
}
