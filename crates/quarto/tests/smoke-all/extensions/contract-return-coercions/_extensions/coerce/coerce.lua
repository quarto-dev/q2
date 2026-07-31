return {
  rstr = function() return "RET-STRING" end,
  rinline = function() return pandoc.Strong({pandoc.Str("RET-INLINE")}) end,
  rinlines = function() return pandoc.Inlines({pandoc.Str("RET-"), pandoc.Str("INLINES")}) end,
  rblock = function() return pandoc.Para({pandoc.Str("RET-BLOCK")}) end,
  rblocks = function() return pandoc.Blocks({
    pandoc.Para({pandoc.Str("RET-BLOCKS-1")}),
    pandoc.Para({pandoc.Str("RET-BLOCKS-2")})
  }) end,
  rarray = function() return {pandoc.Str("RET-"), pandoc.Str("ARRAY")} end
}
