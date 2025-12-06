---@meta

--[[
The pandoc module provides element constructors and utility functions
for Pandoc Lua filters.

This module is automatically available as the global `pandoc` table
when a Lua filter is executed.

Note: This documentation covers the API implemented by quarto-markdown-pandoc,
which is a subset of the full Pandoc Lua API.
]]
---@class pandoc
pandoc = {}

-- Re-export documentation from other files
-- The actual type definitions are in:
-- - pandoc/inlines.lua (Str, Emph, Strong, Link, etc.)
-- - pandoc/blocks.lua (Para, Header, Div, CodeBlock, etc.)
-- - pandoc/components.lua (Attr, Inlines, Blocks)
-- - pandoc/List.lua (List methods)
-- - pandoc/utils.lua (pandoc.utils.stringify)
-- - pandoc/global.lua (FORMAT, PANDOC_VERSION, etc.)
