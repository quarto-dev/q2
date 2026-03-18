-- Lua filter that replaces marker text
function Str(elem)
    if elem.text == "FMT-PLACEHOLDER" then
        return pandoc.Str("FORMAT-FILTER-ACTIVE")
    end
end
