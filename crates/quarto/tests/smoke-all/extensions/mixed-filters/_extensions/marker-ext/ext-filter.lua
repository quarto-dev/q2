-- Extension filter: replaces EXT-PLACEHOLDER
function Str(elem)
    if elem.text == "EXT-PLACEHOLDER" then
        return pandoc.Str("EXT-FILTER-RAN")
    end
end
