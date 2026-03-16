-- Lua filter that converts all Str elements to uppercase
function Str(elem)
    return pandoc.Str(elem.text:upper())
end
