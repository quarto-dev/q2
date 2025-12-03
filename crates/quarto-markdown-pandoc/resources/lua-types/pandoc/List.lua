---@meta

-- pandoc.List metatable methods
-- List is a Lua table with additional methods provided by the metatable

---@class pandoc.List
---@field [integer] any Array elements
local List = {}

--[[
Creates a new List.

List is a Lua table with a metatable that provides convenience methods
for working with arrays.

Example:
```lua
local list = pandoc.List({1, 2, 3})
local empty = pandoc.List()
```
]]
---@param items? any[] Optional initial items
---@return pandoc.List
function pandoc.List(items) end

--[[
Returns the element at the given position.

Supports negative indices: -1 is the last element, -2 is second to last, etc.

Example:
```lua
local list = pandoc.List({10, 20, 30})
print(list:at(1))      -- 10 (first element)
print(list:at(-1))     -- 30 (last element)
print(list:at(100, 0)) -- 0 (default value when out of bounds)
```
]]
---@param index integer Position (1-based, negative counts from end)
---@param default? any Value to return if index is out of bounds
---@return any
function List:at(index, default) end

--[[
Returns a shallow copy of the list.

Example:
```lua
local original = pandoc.List({1, 2, 3})
local copy = original:clone()
copy[1] = 100
print(original[1])  -- 1 (unchanged)
```
]]
---@return pandoc.List
function List:clone() end

--[[
Appends all elements from another list to this list (mutates self).

Example:
```lua
local list = pandoc.List({1, 2})
list:extend({3, 4})
-- list is now {1, 2, 3, 4}
```
]]
---@param other any[] List of elements to append
function List:extend(other) end

--[[
Returns a new list containing only elements that satisfy the predicate.

The predicate function receives (element, index) and should return true/false.

Example:
```lua
local list = pandoc.List({1, 2, 3, 4, 5})
local evens = list:filter(function(x) return x % 2 == 0 end)
-- evens is {2, 4}
```
]]
---@param predicate fun(element: any, index: integer): boolean Filter function
---@return pandoc.List
function List:filter(predicate) end

--[[
Finds the first occurrence of an element in the list.

Returns the element and its index, or nil if not found.

Example:
```lua
local list = pandoc.List({"a", "b", "c"})
local elem, idx = list:find("b")
print(idx)  -- 2
```
]]
---@param needle any Element to search for
---@return any? element The found element (or nil)
---@return integer? index The index (or nil if not found)
function List:find(needle) end

--[[
Finds the first element satisfying a predicate.

The predicate function receives (element, index) and should return true/false.

Example:
```lua
local list = pandoc.List({1, 2, 3, 4})
local elem, idx = list:find_if(function(x) return x > 2 end)
print(elem, idx)  -- 3, 3
```
]]
---@param predicate fun(element: any, index: integer): boolean Test function
---@return any? element The found element (or nil)
---@return integer? index The index (or nil if not found)
function List:find_if(predicate) end

--[[
Checks if the list contains a specific element.

Example:
```lua
local list = pandoc.List({"a", "b", "c"})
print(list:includes("b"))  -- true
print(list:includes("x"))  -- false
```
]]
---@param needle any Element to search for
---@return boolean
function List:includes(needle) end

--[[
Returns an iterator for use in for loops.

Example:
```lua
local list = pandoc.List({10, 20, 30})
for item in list:iter() do
    print(item)
end
```
]]
---@return fun(): any
function List:iter() end

--[[
Returns a new list with a function applied to each element.

The transform function receives (element, index) and returns the new value.

Example:
```lua
local list = pandoc.List({1, 2, 3})
local doubled = list:map(function(x) return x * 2 end)
-- doubled is {2, 4, 6}
```
]]
---@param transform fun(element: any, index: integer): any Transform function
---@return pandoc.List
function List:map(transform) end

--[[
Inserts an element at the specified position.

This is an alias for Lua's table.insert.

Example:
```lua
local list = pandoc.List({1, 3})
list:insert(2, 2)  -- Insert 2 at position 2
-- list is now {1, 2, 3}

list:insert(4)  -- Append 4 at end
-- list is now {1, 2, 3, 4}
```
]]
---@param pos integer Position to insert at
---@param value any Value to insert
---@overload fun(self: pandoc.List, value: any) Append at end
function List:insert(pos, value) end

--[[
Removes and returns the element at the specified position.

This is an alias for Lua's table.remove.

Example:
```lua
local list = pandoc.List({1, 2, 3})
local removed = list:remove(2)
print(removed)  -- 2
-- list is now {1, 3}
```
]]
---@param pos? integer Position to remove (default: last element)
---@return any removed The removed element
function List:remove(pos) end

--[[
Sorts the list in place.

This is an alias for Lua's table.sort.

Example:
```lua
local list = pandoc.List({3, 1, 2})
list:sort()
-- list is now {1, 2, 3}

list:sort(function(a, b) return a > b end)
-- list is now {3, 2, 1}
```
]]
---@param comp? fun(a: any, b: any): boolean Comparison function
function List:sort(comp) end

-- Inlines-specific method

--[[
Applies filter functions to inline elements (Inlines only).

Returns a new Inlines list with the filter applied. Uses two-pass traversal:
1. First pass: all individual inline element filters
2. Second pass: the Inlines list filter

Example:
```lua
function Para(elem)
    local walked = elem.content:walk{
        Str = function(s)
            return pandoc.Str(string.upper(s.text))
        end
    }
    return pandoc.Para(walked)
end
```
]]
---@param filter table<string, function> Table of element type -> filter function
---@return pandoc.Inlines
function List:walk(filter) end
