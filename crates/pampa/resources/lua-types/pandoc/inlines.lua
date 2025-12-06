---@meta

-- Inline element types for Pandoc Lua filters
-- These types are returned by filter functions and created by pandoc.* constructors

---@class pandoc.Inline
---@field t string The element type tag (e.g., "Str", "Emph")
---@field tag string Alias for t
local Inline = {}

---@class pandoc.Str : pandoc.Inline
---@field t "Str"
---@field tag "Str"
---@field text string The text content
pandoc.Str = {}

--[[
Creates a text element.

Example:
```lua
local str = pandoc.Str("Hello")
print(str.text)  -- "Hello"
```
]]
---@param text string The text content
---@return pandoc.Str
function pandoc.Str(text) end

---@class pandoc.Space : pandoc.Inline
---@field t "Space"
---@field tag "Space"
pandoc.Space = {}

--[[
Creates a space element (inter-word space).

Example:
```lua
local content = {pandoc.Str("Hello"), pandoc.Space(), pandoc.Str("world")}
```
]]
---@return pandoc.Space
function pandoc.Space() end

---@class pandoc.SoftBreak : pandoc.Inline
---@field t "SoftBreak"
---@field tag "SoftBreak"
pandoc.SoftBreak = {}

--[[
Creates a soft line break element.

A soft break may be rendered as a space or newline depending on output format.
]]
---@return pandoc.SoftBreak
function pandoc.SoftBreak() end

---@class pandoc.LineBreak : pandoc.Inline
---@field t "LineBreak"
---@field tag "LineBreak"
pandoc.LineBreak = {}

--[[
Creates a hard line break element.

A hard break is always rendered as a line break in output.
]]
---@return pandoc.LineBreak
function pandoc.LineBreak() end

---@class pandoc.Emph : pandoc.Inline
---@field t "Emph"
---@field tag "Emph"
---@field content pandoc.Inlines The emphasized content
pandoc.Emph = {}

--[[
Creates an emphasis (italic) element.

Example:
```lua
local em = pandoc.Emph({pandoc.Str("emphasized")})
-- or
local em = pandoc.Emph(pandoc.Inlines{"emphasized"})
```
]]
---@param content pandoc.Inlines|pandoc.Inline[] Content to emphasize
---@return pandoc.Emph
function pandoc.Emph(content) end

---@class pandoc.Strong : pandoc.Inline
---@field t "Strong"
---@field tag "Strong"
---@field content pandoc.Inlines The strong content
pandoc.Strong = {}

--[[
Creates a strong (bold) element.

Example:
```lua
local strong = pandoc.Strong({pandoc.Str("bold text")})
```
]]
---@param content pandoc.Inlines|pandoc.Inline[] Content to make bold
---@return pandoc.Strong
function pandoc.Strong(content) end

---@class pandoc.Underline : pandoc.Inline
---@field t "Underline"
---@field tag "Underline"
---@field content pandoc.Inlines The underlined content
pandoc.Underline = {}

--[[
Creates an underlined element.
]]
---@param content pandoc.Inlines|pandoc.Inline[] Content to underline
---@return pandoc.Underline
function pandoc.Underline(content) end

---@class pandoc.Strikeout : pandoc.Inline
---@field t "Strikeout"
---@field tag "Strikeout"
---@field content pandoc.Inlines The struck-out content
pandoc.Strikeout = {}

--[[
Creates a strikeout (strikethrough) element.
]]
---@param content pandoc.Inlines|pandoc.Inline[] Content to strike out
---@return pandoc.Strikeout
function pandoc.Strikeout(content) end

---@class pandoc.Superscript : pandoc.Inline
---@field t "Superscript"
---@field tag "Superscript"
---@field content pandoc.Inlines The superscript content
pandoc.Superscript = {}

--[[
Creates a superscript element.
]]
---@param content pandoc.Inlines|pandoc.Inline[] Content to superscript
---@return pandoc.Superscript
function pandoc.Superscript(content) end

---@class pandoc.Subscript : pandoc.Inline
---@field t "Subscript"
---@field tag "Subscript"
---@field content pandoc.Inlines The subscript content
pandoc.Subscript = {}

--[[
Creates a subscript element.
]]
---@param content pandoc.Inlines|pandoc.Inline[] Content to subscript
---@return pandoc.Subscript
function pandoc.Subscript(content) end

---@class pandoc.SmallCaps : pandoc.Inline
---@field t "SmallCaps"
---@field tag "SmallCaps"
---@field content pandoc.Inlines The small caps content
pandoc.SmallCaps = {}

--[[
Creates a small caps element.
]]
---@param content pandoc.Inlines|pandoc.Inline[] Content to render in small caps
---@return pandoc.SmallCaps
function pandoc.SmallCaps(content) end

---@alias pandoc.QuoteType "SingleQuote"|"DoubleQuote"

---@class pandoc.Quoted : pandoc.Inline
---@field t "Quoted"
---@field tag "Quoted"
---@field quotetype pandoc.QuoteType The quote style
---@field content pandoc.Inlines The quoted content
pandoc.Quoted = {}

--[[
Creates a quoted element.

Example:
```lua
local quoted = pandoc.Quoted("DoubleQuote", {pandoc.Str("quoted text")})
```
]]
---@param quotetype pandoc.QuoteType "SingleQuote" or "DoubleQuote"
---@param content pandoc.Inlines|pandoc.Inline[] Content to quote
---@return pandoc.Quoted
function pandoc.Quoted(quotetype, content) end

---@class pandoc.Code : pandoc.Inline
---@field t "Code"
---@field tag "Code"
---@field text string The code text
---@field attr pandoc.Attr Attributes (identifier, classes, key-value pairs)
pandoc.Code = {}

--[[
Creates an inline code element.

Example:
```lua
local code = pandoc.Code("x = 1")
local code_with_attr = pandoc.Code("print()", pandoc.Attr("", {"lua"}))
```
]]
---@param text string The code text
---@param attr? pandoc.Attr Optional attributes
---@return pandoc.Code
function pandoc.Code(text, attr) end

---@alias pandoc.MathType "InlineMath"|"DisplayMath"

---@class pandoc.Math : pandoc.Inline
---@field t "Math"
---@field tag "Math"
---@field mathtype pandoc.MathType The math display type
---@field text string The LaTeX math content
pandoc.Math = {}

--[[
Creates a math element.

Example:
```lua
local inline_math = pandoc.Math("InlineMath", "x^2 + y^2 = z^2")
local display_math = pandoc.Math("DisplayMath", "\\int_0^\\infty e^{-x} dx")
```
]]
---@param mathtype pandoc.MathType "InlineMath" or "DisplayMath"
---@param text string The LaTeX math content
---@return pandoc.Math
function pandoc.Math(mathtype, text) end

---@class pandoc.RawInline : pandoc.Inline
---@field t "RawInline"
---@field tag "RawInline"
---@field format string The format (e.g., "html", "latex")
---@field text string The raw content
pandoc.RawInline = {}

--[[
Creates a raw inline element for format-specific content.

Example:
```lua
local html = pandoc.RawInline("html", "<span class='custom'>text</span>")
local latex = pandoc.RawInline("latex", "\\textcolor{red}{text}")
```
]]
---@param format string The format (e.g., "html", "latex", "tex")
---@param text string The raw content
---@return pandoc.RawInline
function pandoc.RawInline(format, text) end

---@class pandoc.Link : pandoc.Inline
---@field t "Link"
---@field tag "Link"
---@field content pandoc.Inlines The link text
---@field target string The URL
---@field title string The title attribute
---@field attr pandoc.Attr Attributes
pandoc.Link = {}

--[[
Creates a hyperlink element.

Example:
```lua
local link = pandoc.Link({pandoc.Str("Click here")}, "https://example.com")
local link_with_title = pandoc.Link(
    {pandoc.Str("Example")},
    "https://example.com",
    "Example Website"
)
```
]]
---@param content pandoc.Inlines|pandoc.Inline[] The link text
---@param target string The URL
---@param title? string Optional title attribute
---@param attr? pandoc.Attr Optional attributes
---@return pandoc.Link
function pandoc.Link(content, target, title, attr) end

---@class pandoc.Image : pandoc.Inline
---@field t "Image"
---@field tag "Image"
---@field content pandoc.Inlines The alt text
---@field src string The image source URL
---@field title string The title attribute
---@field attr pandoc.Attr Attributes
pandoc.Image = {}

--[[
Creates an image element.

Example:
```lua
local img = pandoc.Image({pandoc.Str("Alt text")}, "image.png")
local img_with_title = pandoc.Image(
    {pandoc.Str("A photo")},
    "photo.jpg",
    "Photo title"
)
```
]]
---@param content pandoc.Inlines|pandoc.Inline[] The alt text
---@param src string The image source URL
---@param title? string Optional title attribute
---@param attr? pandoc.Attr Optional attributes
---@return pandoc.Image
function pandoc.Image(content, src, title, attr) end

---@class pandoc.Span : pandoc.Inline
---@field t "Span"
---@field tag "Span"
---@field content pandoc.Inlines The span content
---@field attr pandoc.Attr Attributes (identifier, classes, key-value pairs)
pandoc.Span = {}

--[[
Creates a span element (generic inline container with attributes).

Example:
```lua
local span = pandoc.Span({pandoc.Str("text")}, pandoc.Attr("id", {"class1", "class2"}))
```
]]
---@param content pandoc.Inlines|pandoc.Inline[] The span content
---@param attr? pandoc.Attr Optional attributes
---@return pandoc.Span
function pandoc.Span(content, attr) end

---@class pandoc.Note : pandoc.Inline
---@field t "Note"
---@field tag "Note"
---@field content pandoc.Blocks The note content (blocks)
pandoc.Note = {}

--[[
Creates a footnote element.

Example:
```lua
local note = pandoc.Note({pandoc.Para({pandoc.Str("Footnote text")})})
```
]]
---@param content pandoc.Blocks|pandoc.Block[] The note content
---@return pandoc.Note
function pandoc.Note(content) end
