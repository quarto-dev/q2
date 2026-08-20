-- Pandoc's standard idiom: the script returns its handler table.
return {
  Str = function(el)
    if el.text == "MARKER" then return pandoc.Str("TABLE-RAN") end
  end,
}
