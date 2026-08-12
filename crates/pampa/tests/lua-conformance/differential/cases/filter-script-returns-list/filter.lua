-- A returned SEQUENCE is a list of filters applied as successive passes.
-- Pass two matches only pass one's output, so this pins the ordering.
return {
  { Str = function(el)
      if el.text == "MARKER" then return pandoc.Str("PASS-ONE") end
    end },
  { Str = function(el)
      if el.text == "PASS-ONE" then return pandoc.Str("PASS-TWO") end
    end },
}
