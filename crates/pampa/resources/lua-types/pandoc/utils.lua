---@meta

-- pandoc.utils module
-- Utility functions for working with Pandoc elements

---@class pandoc.utils
pandoc.utils = {}

--[[
Converts an element or list of elements to plain text.

Recursively extracts text content from inline and block elements,
stripping all formatting. Useful for generating plain text versions
of formatted content.

Example:
```lua
function Para(elem)
    local text = pandoc.utils.stringify(elem.content)
    print("Paragraph text: " .. text)
    return elem
end

-- With nested formatting
local inlines = pandoc.Inlines{
    pandoc.Strong{pandoc.Str("Hello")},
    pandoc.Space(),
    pandoc.Emph{pandoc.Str("world")}
}
print(pandoc.utils.stringify(inlines))  -- "Hello world"
```
]]
---@param element pandoc.Inline|pandoc.Block|pandoc.Inlines|pandoc.Blocks|string Element(s) to stringify
---@return string text Plain text representation
function pandoc.utils.stringify(element) end
