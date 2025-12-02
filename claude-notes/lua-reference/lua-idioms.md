# Lua Idioms and Patterns for Filter Programming

This document covers common Lua idioms and patterns particularly useful for Pandoc Lua filter development.

## Table Patterns

### Safe Table Access

```lua
-- Problem: accessing nested tables can error if intermediate is nil
-- Bad: t.a.b.c  -- errors if t.a is nil

-- Safe access with and
local value = t and t.a and t.a.b and t.a.b.c

-- Safe access with default
local value = (t.a or {}).b or default
```

### Table as Set

```lua
local set = {}
set["key1"] = true
set["key2"] = true

if set["key1"] then
    -- key exists
end

-- From list to set
local function make_set(list)
    local s = {}
    for _, v in ipairs(list) do
        s[v] = true
    end
    return s
end
```

### Table as Queue/Stack

```lua
-- Stack (LIFO)
local stack = {}
table.insert(stack, value)     -- push
local top = table.remove(stack) -- pop

-- Queue (FIFO)
local queue = {}
table.insert(queue, value)        -- enqueue
local first = table.remove(queue, 1) -- dequeue
```

### Shallow Copy

```lua
local function shallow_copy(t)
    local copy = {}
    for k, v in pairs(t) do
        copy[k] = v
    end
    return copy
end
```

### Deep Copy

```lua
local function deep_copy(t)
    if type(t) ~= "table" then return t end
    local copy = {}
    for k, v in pairs(t) do
        copy[deep_copy(k)] = deep_copy(v)
    end
    return setmetatable(copy, getmetatable(t))
end
```

### Merge Tables

```lua
local function merge(t1, t2)
    local result = {}
    for k, v in pairs(t1) do result[k] = v end
    for k, v in pairs(t2) do result[k] = v end
    return result
end

-- In-place merge (modifies t1)
local function merge_into(t1, t2)
    for k, v in pairs(t2) do t1[k] = v end
    return t1
end
```

### Filter/Map/Reduce

```lua
-- Filter
local function filter(t, pred)
    local result = {}
    for _, v in ipairs(t) do
        if pred(v) then
            table.insert(result, v)
        end
    end
    return result
end

-- Map
local function map(t, fn)
    local result = {}
    for i, v in ipairs(t) do
        result[i] = fn(v)
    end
    return result
end

-- Reduce
local function reduce(t, fn, init)
    local acc = init
    for _, v in ipairs(t) do
        acc = fn(acc, v)
    end
    return acc
end
```

---

## String Patterns

### Common Pattern Idioms

```lua
-- Split string
local function split(s, sep)
    local result = {}
    for match in s:gmatch("([^" .. sep .. "]+)") do
        table.insert(result, match)
    end
    return result
end

-- Trim whitespace
local function trim(s)
    return s:match("^%s*(.-)%s*$")
end

-- Check prefix/suffix
local function starts_with(s, prefix)
    return s:sub(1, #prefix) == prefix
end

local function ends_with(s, suffix)
    return s:sub(-#suffix) == suffix
end

-- Escape pattern magic characters
local function escape_pattern(s)
    return s:gsub("([%%%.%^%$%(%)%[%]%*%+%-%?])", "%%%1")
end
```

### Pattern Replacement with Function

```lua
-- Replace with transformation
local result = s:gsub("(%w+)", function(word)
    return word:upper()
end)

-- Replace with table lookup
local translations = {foo = "bar", baz = "qux"}
local result = s:gsub("%w+", translations)

-- Conditional replacement
local result = s:gsub("(%d+)", function(num)
    local n = tonumber(num)
    if n > 100 then
        return "large"
    else
        return num
    end
end)
```

---

## Function Patterns

### Default Arguments

```lua
function foo(a, b, c)
    a = a or "default"
    b = b or 0
    c = c ~= nil and c or true  -- for boolean defaults
end

-- With options table
function foo(opts)
    opts = opts or {}
    local a = opts.a or "default"
    local b = opts.b or 0
end
```

### Multiple Return Values

```lua
-- Return multiple values
function divide(a, b)
    return a // b, a % b  -- quotient, remainder
end

local q, r = divide(10, 3)

-- Ignore some returns
local q = divide(10, 3)  -- r discarded
local _, r = divide(10, 3)  -- q discarded

-- Collect all returns
local results = {divide(10, 3)}
```

### Variable Arguments

```lua
function vararg_func(first, ...)
    local args = {...}  -- collect into table
    local n = select("#", ...)  -- count including nil

    for i = 1, n do
        local arg = select(i, ...)  -- get ith argument
        print(arg)
    end
end

-- Forward varargs
function wrapper(...)
    return other_func(...)
end
```

### Memoization

```lua
local function memoize(fn)
    local cache = {}
    return function(arg)
        if cache[arg] == nil then
            cache[arg] = fn(arg)
        end
        return cache[arg]
    end
end

local expensive = memoize(function(n)
    -- computation
    return result
end)
```

### Method Chaining

```lua
local Builder = {}
Builder.__index = Builder

function Builder:new()
    return setmetatable({parts = {}}, self)
end

function Builder:add(part)
    table.insert(self.parts, part)
    return self  -- enable chaining
end

function Builder:build()
    return table.concat(self.parts)
end

local result = Builder:new():add("a"):add("b"):add("c"):build()
```

---

## Object-Oriented Patterns

### Simple Class

```lua
local MyClass = {}
MyClass.__index = MyClass

function MyClass.new(x, y)
    local self = setmetatable({}, MyClass)
    self.x = x
    self.y = y
    return self
end

function MyClass:method()
    return self.x + self.y
end

local obj = MyClass.new(1, 2)
print(obj:method())
```

### Inheritance

```lua
local Animal = {}
Animal.__index = Animal

function Animal.new(name)
    return setmetatable({name = name}, Animal)
end

function Animal:speak()
    return "..."
end

local Dog = setmetatable({}, {__index = Animal})
Dog.__index = Dog

function Dog.new(name)
    local self = Animal.new(name)
    return setmetatable(self, Dog)
end

function Dog:speak()
    return "Woof!"
end
```

### Prototype-Based

```lua
local prototype = {
    greet = function(self)
        return "Hello, " .. self.name
    end
}

local function new_person(name)
    return setmetatable({name = name}, {__index = prototype})
end
```

---

## Error Handling Patterns

### Try-Catch Idiom

```lua
local function try(fn)
    local ok, result = pcall(fn)
    if ok then
        return result
    else
        return nil, result  -- result is error message
    end
end

-- Usage
local result, err = try(function()
    return risky_operation()
end)

if err then
    print("Error:", err)
end
```

### Error with Context

```lua
local function with_context(msg)
    return function(err)
        return msg .. ": " .. tostring(err)
    end
end

local ok, result = xpcall(
    function() return risky() end,
    with_context("Failed to process")
)
```

### Assert with Custom Message

```lua
local function check(condition, ...)
    if not condition then
        error(string.format(...), 2)
    end
    return condition
end

check(x > 0, "x must be positive, got %d", x)
```

---

## Iterator Patterns

### Custom Iterator

```lua
-- Stateless iterator
local function squares(max)
    local function iter(max, i)
        i = i + 1
        if i <= max then
            return i, i * i
        end
    end
    return iter, max, 0
end

for i, sq in squares(5) do
    print(i, sq)
end

-- Stateful iterator (closure)
local function range(start, stop, step)
    step = step or 1
    local i = start - step
    return function()
        i = i + step
        if i <= stop then
            return i
        end
    end
end

for i in range(1, 10, 2) do
    print(i)
end
```

### Coroutine-Based Iterator

```lua
local function permutations(t)
    return coroutine.wrap(function()
        if #t == 0 then
            coroutine.yield({})
        else
            for i = 1, #t do
                local rest = {}
                for j = 1, #t do
                    if j ~= i then
                        table.insert(rest, t[j])
                    end
                end
                for perm in permutations(rest) do
                    table.insert(perm, 1, t[i])
                    coroutine.yield(perm)
                end
            end
        end
    end)
end
```

---

## Module Patterns

### Simple Module

```lua
local M = {}

local private_var = "secret"

local function private_fn()
    return private_var
end

function M.public_fn()
    return private_fn()
end

return M
```

### Module with Metatable

```lua
local M = {}

function M.new(value)
    return setmetatable({value = value}, {__index = M})
end

function M:get()
    return self.value
end

return M
```

---

## Common Gotchas

### Table Length with Holes

```lua
-- WRONG: undefined behavior
local t = {1, 2, nil, 4}
print(#t)  -- Could be 2 or 4!

-- Use explicit count
t.n = 4
-- Or table.pack
local t = table.pack(1, 2, nil, 4)
print(t.n)  -- 4
```

### Reference vs Value

```lua
-- Tables are references
local t1 = {1, 2, 3}
local t2 = t1  -- Same table!
t2[1] = 99
print(t1[1])  -- 99

-- Use copy for independence
local t2 = shallow_copy(t1)
```

### String Indexing

```lua
-- Strings are immutable
local s = "hello"
-- s[1] = "H"  -- ERROR!

-- Use sub or gsub
local new_s = "H" .. s:sub(2)
```

### Nil in Tables

```lua
-- Setting to nil removes the key
local t = {a = 1, b = 2}
t.a = nil
-- Now t has only key "b"

-- Use rawset to check existence
rawget(t, "a")  -- nil
```

### Numeric For Variables

```lua
-- Loop variable is local and read-only
for i = 1, 10 do
    i = i + 1  -- Has no effect on loop!
end
```

### Boolean Coercion

```lua
-- Only nil and false are falsy
if 0 then print("0 is true") end  -- prints
if "" then print("empty string is true") end  -- prints

-- Be explicit for these
if x ~= 0 then ... end
if x ~= "" then ... end
```

---

## Performance Tips

1. **Local variables are faster than globals**
   ```lua
   local print = print  -- Cache global in local
   ```

2. **Avoid creating tables in loops**
   ```lua
   -- Bad
   for i = 1, 1000 do
       local t = {i}
   end

   -- Better
   local t = {}
   for i = 1, 1000 do
       t[1] = i
   end
   ```

3. **Use table.concat for string building**
   ```lua
   -- Bad
   local s = ""
   for i = 1, 1000 do
       s = s .. i
   end

   -- Good
   local parts = {}
   for i = 1, 1000 do
       parts[i] = i
   end
   local s = table.concat(parts)
   ```

4. **Pre-size tables when possible**
   ```lua
   -- If you know the size
   local t = {}
   for i = 1, 1000 do t[i] = i end  -- Fine

   -- In C: lua_createtable(L, 1000, 0)
   ```

5. **Avoid table.insert for append when index known**
   ```lua
   -- Slower
   table.insert(t, value)

   -- Faster
   t[#t + 1] = value
   ```
