---@meta

-- Diagnostic functions for Quarto Lua filters
-- These functions allow filter authors to emit warnings and errors

--[[
Emit a warning message during filter execution.

The warning will be collected and reported after the filter completes.
Warnings do not stop filter execution.

Source location is automatically captured from the Lua call stack,
pointing to where `quarto.warn()` was called in the filter.

Example:
```lua
function Link(elem)
    if not elem.target:match("^https?://") then
        quarto.warn("Link target is not an HTTP URL: " .. elem.target)
    end
    return elem
end
```

Note: The optional `element` parameter for attaching source location
from an AST element is not yet fully functional for elements from
the original document. See issue k-481.
]]
---@param message string Warning message to emit
---@param element? pandoc.Inline|pandoc.Block Optional element (reserved for future use)
function quarto.warn(message, element) end

--[[
Emit an error message during filter execution.

The error will be collected and reported after the filter completes.
Unlike Lua's built-in `error()`, this does NOT stop filter execution -
it only records the error for later reporting.

Use this for serious issues that should be flagged but don't prevent
the filter from completing its work.

Source location is automatically captured from the Lua call stack,
pointing to where `quarto.error()` was called in the filter.

Example:
```lua
function CodeBlock(elem)
    if elem.attr.classes:includes("python") then
        local ok, err = pcall(validate_python_syntax, elem.text)
        if not ok then
            quarto.error("Invalid Python syntax: " .. err)
        end
    end
    return elem
end
```

Note: The optional `element` parameter for attaching source location
from an AST element is not yet fully functional for elements from
the original document. See issue k-481.
]]
---@param message string Error message to emit
---@param element? pandoc.Inline|pandoc.Block Optional element (reserved for future use)
function quarto.error(message, element) end
