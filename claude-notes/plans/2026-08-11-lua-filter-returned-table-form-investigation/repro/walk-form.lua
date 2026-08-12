-- Proof that the table-driven machinery is fine: a *global* Pandoc
-- handler that walks the document with an inline handler table. This
-- runs, and its Str handler fires — so q2 can consume a filter table
-- perfectly well when one is handed to it.
function Pandoc(doc)
  return doc:walk{
    Str = function(el)
      if el.text == "MARKER" then return pandoc.Str("WALK-TABLE-RAN") end
    end,
  }
end
