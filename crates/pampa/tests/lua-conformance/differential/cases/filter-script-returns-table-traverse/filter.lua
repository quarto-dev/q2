-- `traverse` is read off the returned table. Topdown replaces the Para
-- first, so Str then visits the NEW content: "S-P". Typewise would give "P".
return {
  traverse = 'topdown',
  Para = function(el) return pandoc.Para({pandoc.Str("P")}) end,
  Str = function(el) return pandoc.Str("S-" .. el.text) end,
}
