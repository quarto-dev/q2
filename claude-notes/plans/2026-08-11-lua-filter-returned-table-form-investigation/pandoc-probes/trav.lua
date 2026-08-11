return {
  traverse = 'topdown',
  Para = function(el) return pandoc.Para({pandoc.Str("TOPDOWN-PARA")}) end,
  Str = function(el) return pandoc.Str("STR-"..el.text) end,
}
