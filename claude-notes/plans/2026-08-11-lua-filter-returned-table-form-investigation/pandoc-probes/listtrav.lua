return {
  { traverse = 'topdown',
    Para = function(el) return pandoc.Para({pandoc.Str("P1")}) end },
  { Str = function(el) return pandoc.Str("S-"..el.text) end },
}
