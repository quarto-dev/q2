function Str(el) if el.text == "MARKER" then return pandoc.Str("GLOBAL-RAN") end end
return { Str = function(el) if el.text == "MARKER" then return pandoc.Str("TABLE-RAN") end end }
