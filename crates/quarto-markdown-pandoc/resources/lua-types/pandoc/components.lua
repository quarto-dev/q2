---@meta

-- Component types for Pandoc Lua filters
-- These are supporting types used by inline and block elements

---@alias pandoc.Attr { [1]: string, [2]: string[], [3]: table<string, string> }

--[[
Attributes tuple: (identifier, classes, key-value pairs)

The Attr type is a tuple (Lua table with numeric indices):
- [1]: identifier (string, may be empty "")
- [2]: classes (array of strings)
- [3]: attributes (table mapping strings to strings)

Example:
```lua
-- Attr with id "myid", classes "warning" and "note", and custom attribute
local attr = pandoc.Attr("myid", {"warning", "note"}, {["data-custom"] = "value"})

-- Access components
print(attr[1])        -- "myid" (identifier)
print(attr[2][1])     -- "warning" (first class)
print(attr[3]["data-custom"])  -- "value"

-- Using the identifier, classes, attributes fields (aliases)
print(attr.identifier)  -- "myid"
print(attr.classes[1])  -- "warning"
```
]]
---@class pandoc.AttrObj
---@field [1] string The identifier (id attribute)
---@field [2] string[] The classes
---@field [3] table<string, string> Key-value attributes
---@field identifier string Alias for [1]
---@field classes string[] Alias for [2]
---@field attributes table<string, string> Alias for [3]
pandoc.AttrObj = {}

--[[
Creates an Attr (attributes) object.

All parameters are optional and default to empty values.

Example:
```lua
-- Empty attr
local attr1 = pandoc.Attr()

-- Just identifier
local attr2 = pandoc.Attr("section-1")

-- Identifier and classes
local attr3 = pandoc.Attr("", {"highlight", "python"})

-- Full attr
local attr4 = pandoc.Attr("myid", {"class1", "class2"}, {key = "value"})
```
]]
---@param identifier? string The identifier (id attribute), defaults to ""
---@param classes? string[] The classes, defaults to {}
---@param attributes? table<string, string> Key-value attributes, defaults to {}
---@return pandoc.Attr
function pandoc.Attr(identifier, classes, attributes) end

---@class pandoc.Inlines : pandoc.List
---@field [integer] pandoc.Inline

--[[
Creates an Inlines list (a list of inline elements).

Inlines is a specialized List type for inline elements with all
the standard List methods (clone, filter, map, etc.) plus walk().

Example:
```lua
-- From array of inlines
local inlines = pandoc.Inlines({pandoc.Str("Hello"), pandoc.Space(), pandoc.Str("world")})

-- From a single string (converts to Str)
local inlines2 = pandoc.Inlines("Hello")

-- From a single inline
local inlines3 = pandoc.Inlines(pandoc.Str("Hello"))

-- Empty
local inlines4 = pandoc.Inlines()
```
]]
---@param content? pandoc.Inline[]|string|pandoc.Inline Content to wrap
---@return pandoc.Inlines
function pandoc.Inlines(content) end

---@class pandoc.Blocks : pandoc.List
---@field [integer] pandoc.Block

--[[
Creates a Blocks list (a list of block elements).

Blocks is a specialized List type for block elements with all
the standard List methods (clone, filter, map, etc.) plus walk().

Example:
```lua
local blocks = pandoc.Blocks({
    pandoc.Para({pandoc.Str("First paragraph")}),
    pandoc.Para({pandoc.Str("Second paragraph")})
})
```
]]
---@param content? pandoc.Block[] Content to wrap
---@return pandoc.Blocks
function pandoc.Blocks(content) end
