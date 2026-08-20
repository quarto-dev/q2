-- `traverse` is per entry. Entry 1 is topdown; entry 2 must not inherit it.
--   entry 1 (topdown):  Para -> "P", then Str -> "1-P"
--   entry 2 (typewise): Str -> "2-1-P", then Para discards it -> "Q"
-- Inheriting topdown in entry 2 would give "2-Q".
return {
  { traverse = 'topdown',
    Para = function(el) return pandoc.Para({pandoc.Str("P")}) end,
    Str = function(el) return pandoc.Str("1-" .. el.text) end },
  { Para = function(el) return pandoc.Para({pandoc.Str("Q")}) end,
    Str = function(el) return pandoc.Str("2-" .. el.text) end },
}
