---@meta

-- Global variables available in Pandoc Lua filters
-- These are set by quarto-markdown-pandoc before filter execution

--[[
The target output format (e.g., "html", "latex", "json", "native").

This corresponds to the `-t`/`--to` argument passed to quarto-markdown-pandoc.
Use this to conditionally apply transformations based on output format.

Example:
```lua
function Image(elem)
    if FORMAT == "html" then
        -- Add lazy loading for HTML output
        elem.attributes["loading"] = "lazy"
    end
    return elem
end
```
]]
---@type string
FORMAT = ""

--[[
The Pandoc version as a table with numeric indices.

quarto-markdown-pandoc emulates Pandoc 3.x behavior.

Example:
```lua
if PANDOC_VERSION[1] >= 3 then
    -- Use Pandoc 3.x features
end
```
]]
---@type table<integer, integer>
PANDOC_VERSION = {}

--[[
The pandoc-types API version as a table with numeric indices.

Example:
```lua
print(PANDOC_API_VERSION[1], PANDOC_API_VERSION[2], PANDOC_API_VERSION[3])
-- Output: 1  23  1
```
]]
---@type table<integer, integer>
PANDOC_API_VERSION = {}

--[[
The absolute path to the current Lua filter script file.

Useful for loading resources relative to the filter location.

Example:
```lua
local filter_dir = PANDOC_SCRIPT_FILE:match("(.*/)")
local config = dofile(filter_dir .. "config.lua")
```
]]
---@type string
PANDOC_SCRIPT_FILE = ""
