-- User filter: replaces USER-PLACEHOLDER
function Str(elem)
    if elem.text == "USER-PLACEHOLDER" then
        return pandoc.Str("USER-FILTER-RAN")
    end
end
