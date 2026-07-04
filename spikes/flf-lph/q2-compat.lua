-- q2-compat.lua
-- Adapter filter: lower q2 (pampa) AST conventions to the shapes Quarto 1's
-- filter chain expects. Runs inside pandoc, BEFORE main.lua, via the
-- defaults-file filter list.

-- pampa parses `$$...$$ {#eq-label}` into
--   Span(("eq-label", ["quarto-math-with-attribute"]), [Math])
-- Q1's crossref/equations.lua expects the raw form the qmd-reader produces:
--   Math, Space, Str "{#eq-label}"
-- so re-expand the span into that token stream.
function Span(el)
  if el.classes:includes("quarto-math-with-attribute") then
    local attr = "{#" .. el.identifier
    for k, v in pairs(el.attributes) do
      attr = attr .. " " .. k .. "=\"" .. v .. "\""
    end
    attr = attr .. "}"
    local result = pandoc.Inlines{}
    result:extend(el.content)
    result:insert(pandoc.Space())
    result:insert(pandoc.Str(attr))
    return result
  end
end
