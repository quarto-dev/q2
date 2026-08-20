-- rawlen > 0 makes this a LIST; the named `Str` key is discarded.
return {
  Str = function(el)
    if el.text == "MARKER" then return pandoc.Str("NAMED-RAN") end
  end,
  { Str = function(el)
      if el.text == "MARKER" then return pandoc.Str("ARRAY-RAN") end
    end },
}
