---@meta

--[[
The quarto module provides Quarto-specific functions for Lua filters.

This module is automatically available as the global `quarto` table
when a Lua filter is executed by quarto-markdown-pandoc.

Currently supported functions:
- `quarto.warn(message, element?)` - Emit a warning diagnostic
- `quarto.error(message, element?)` - Emit an error diagnostic

See quarto/diagnostics.lua for detailed documentation.
]]
---@class quarto
quarto = {}

-- Re-export documentation from diagnostics.lua
