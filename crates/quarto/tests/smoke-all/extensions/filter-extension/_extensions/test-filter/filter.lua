-- Lua filter that adds a marker prefix to text
function Str(elem)
    if elem.text == "MARKER-PLACEHOLDER" then
        return pandoc.Str("EXTENSION-FILTER-ACTIVE")
    end
end
