---@meta

-- Block element types for Pandoc Lua filters
-- These types are returned by filter functions and created by pandoc.* constructors

---@class pandoc.Block
---@field t string The element type tag (e.g., "Para", "Header")
---@field tag string Alias for t
local Block = {}

---@class pandoc.Para : pandoc.Block
---@field t "Para"
---@field tag "Para"
---@field content pandoc.Inlines The paragraph content
pandoc.Para = {}

--[[
Creates a paragraph element.

Example:
```lua
local para = pandoc.Para({pandoc.Str("Hello"), pandoc.Space(), pandoc.Str("world")})
```
]]
---@param content pandoc.Inlines|pandoc.Inline[] The paragraph content
---@return pandoc.Para
function pandoc.Para(content) end

---@class pandoc.Plain : pandoc.Block
---@field t "Plain"
---@field tag "Plain"
---@field content pandoc.Inlines The plain content
pandoc.Plain = {}

--[[
Creates a plain (non-paragraph) block of inline content.

Plain blocks are used in contexts where paragraph spacing is not desired,
such as inside list items or table cells.

Example:
```lua
local plain = pandoc.Plain({pandoc.Str("List item text")})
```
]]
---@param content pandoc.Inlines|pandoc.Inline[] The content
---@return pandoc.Plain
function pandoc.Plain(content) end

---@class pandoc.Header : pandoc.Block
---@field t "Header"
---@field tag "Header"
---@field level integer The heading level (1-6)
---@field content pandoc.Inlines The heading text
---@field attr pandoc.Attr Attributes (identifier, classes, key-value pairs)
pandoc.Header = {}

--[[
Creates a header element.

Example:
```lua
local h1 = pandoc.Header(1, {pandoc.Str("Introduction")})
local h2_with_id = pandoc.Header(2, {pandoc.Str("Methods")}, pandoc.Attr("methods"))
```
]]
---@param level integer The heading level (1-6)
---@param content pandoc.Inlines|pandoc.Inline[] The heading text
---@param attr? pandoc.Attr Optional attributes
---@return pandoc.Header
function pandoc.Header(level, content, attr) end

---@class pandoc.CodeBlock : pandoc.Block
---@field t "CodeBlock"
---@field tag "CodeBlock"
---@field text string The code content
---@field attr pandoc.Attr Attributes (identifier, classes for language, key-value pairs)
pandoc.CodeBlock = {}

--[[
Creates a code block element.

The first class is typically used as the language for syntax highlighting.

Example:
```lua
local code = pandoc.CodeBlock("print('hello')", pandoc.Attr("", {"python"}))
```
]]
---@param text string The code content
---@param attr? pandoc.Attr Optional attributes (first class = language)
---@return pandoc.CodeBlock
function pandoc.CodeBlock(text, attr) end

---@class pandoc.RawBlock : pandoc.Block
---@field t "RawBlock"
---@field tag "RawBlock"
---@field format string The format (e.g., "html", "latex")
---@field text string The raw content
pandoc.RawBlock = {}

--[[
Creates a raw block element for format-specific content.

Example:
```lua
local html = pandoc.RawBlock("html", "<div class='custom'>content</div>")
local latex = pandoc.RawBlock("latex", "\\begin{center}content\\end{center}")
```
]]
---@param format string The format (e.g., "html", "latex", "tex")
---@param text string The raw content
---@return pandoc.RawBlock
function pandoc.RawBlock(format, text) end

---@class pandoc.BlockQuote : pandoc.Block
---@field t "BlockQuote"
---@field tag "BlockQuote"
---@field content pandoc.Blocks The quoted content
pandoc.BlockQuote = {}

--[[
Creates a block quote element.

Example:
```lua
local quote = pandoc.BlockQuote({
    pandoc.Para({pandoc.Str("To be or not to be...")})
})
```
]]
---@param content pandoc.Blocks|pandoc.Block[] The quoted content
---@return pandoc.BlockQuote
function pandoc.BlockQuote(content) end

---@class pandoc.BulletList : pandoc.Block
---@field t "BulletList"
---@field tag "BulletList"
---@field content pandoc.Block[][] List of items (each item is a list of blocks)
pandoc.BulletList = {}

--[[
Creates a bullet (unordered) list element.

Each item in the list is itself a list of blocks.

Example:
```lua
local list = pandoc.BulletList({
    {pandoc.Plain({pandoc.Str("First item")})},
    {pandoc.Plain({pandoc.Str("Second item")})},
})
```
]]
---@param items pandoc.Block[][] List of items (each item is a list of blocks)
---@return pandoc.BulletList
function pandoc.BulletList(items) end

---@class pandoc.OrderedList : pandoc.Block
---@field t "OrderedList"
---@field tag "OrderedList"
---@field content pandoc.Block[][] List of items (each item is a list of blocks)
---@field listAttributes table List attributes (start number, style, delimiter)
pandoc.OrderedList = {}

--[[
Creates an ordered (numbered) list element.

Each item in the list is itself a list of blocks.

Example:
```lua
local list = pandoc.OrderedList({
    {pandoc.Plain({pandoc.Str("First item")})},
    {pandoc.Plain({pandoc.Str("Second item")})},
})
```
]]
---@param items pandoc.Block[][] List of items (each item is a list of blocks)
---@param listAttributes? table Optional list attributes
---@return pandoc.OrderedList
function pandoc.OrderedList(items, listAttributes) end

---@class pandoc.Div : pandoc.Block
---@field t "Div"
---@field tag "Div"
---@field content pandoc.Blocks The div content
---@field attr pandoc.Attr Attributes (identifier, classes, key-value pairs)
pandoc.Div = {}

--[[
Creates a div element (generic block container with attributes).

Example:
```lua
local div = pandoc.Div(
    {pandoc.Para({pandoc.Str("Content inside div")})},
    pandoc.Attr("myid", {"warning", "note"})
)
```
]]
---@param content pandoc.Blocks|pandoc.Block[] The div content
---@param attr? pandoc.Attr Optional attributes
---@return pandoc.Div
function pandoc.Div(content, attr) end

---@class pandoc.HorizontalRule : pandoc.Block
---@field t "HorizontalRule"
---@field tag "HorizontalRule"
pandoc.HorizontalRule = {}

--[[
Creates a horizontal rule (thematic break) element.

Example:
```lua
local hr = pandoc.HorizontalRule()
```
]]
---@return pandoc.HorizontalRule
function pandoc.HorizontalRule() end
