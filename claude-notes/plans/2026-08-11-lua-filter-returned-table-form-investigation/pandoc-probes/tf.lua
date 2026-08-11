return {
  Str = function(el)
    if el.text == "MARKER" then return pandoc.Str("TABLE-FORM-RAN") end
  end,
}
