# Lua 5.4 Reference Manual - Structured Index

This document provides a structured navigation index for the Lua 5.4 manual located at `external-sources/lua-manual/manual.md`. Line numbers are provided for quick navigation.

## Document Structure Overview

| Section | Title | Line Range |
|---------|-------|------------|
| 1 | Introduction | 13-51 |
| 2 | Basic Concepts | 53-712 |
| 3 | The Language | 714-1805 |
| 4 | The Application Program Interface (C API) | 1806-4483 |
| 5 | The Auxiliary Library | 4484-5524 |
| 6 | The Standard Libraries | 5525-7806 |
| 7 | Lua Standalone | 7807-7920 |
| 8 | Incompatibilities with Previous Version | 7921-8025 |
| 9 | The Complete Syntax of Lua | 8026-end |

---

## Section 2: Basic Concepts

### 2.1 Values and Types (Lines 57-162)
Eight basic types:
- **nil** - Single value `nil`, represents absence
- **boolean** - `true` and `false`
- **number** - Two subtypes: integer (64-bit) and float (double)
- **string** - Immutable byte sequences, 8-bit clean
- **function** - Both Lua and C functions
- **userdata** - Full (Lua-managed memory) and light (C pointer)
- **thread** - Coroutine, not OS thread
- **table** - Associative array, the only data structure

**Key points:**
- Tables, functions, threads, userdata are objects (reference types)
- Float keys equal to integers are converted to integers (e.g., `t[2.0]` becomes `t[2]`)

### 2.2 Environments and Global Environment (Lines 163-196)
- Free names translated to `_ENV.var`
- `_ENV` is an external local variable for chunks
- Global environment stored at special registry index
- `_G` initialized with global environment

### 2.3 Error Handling (Lines 197-243)
- `error()` raises errors (never returns)
- `pcall()` and `xpcall()` for protected calls
- Message handler can add stack traceback
- `warn()` for warnings (non-interrupting)

### 2.4 Metatables and Metamethods (Lines 244-419)
**Arithmetic metamethods:** `__add`, `__sub`, `__mul`, `__div`, `__mod`, `__pow`, `__unm`, `__idiv`

**Bitwise metamethods:** `__band`, `__bor`, `__bxor`, `__bnot`, `__shl`, `__shr`

**Comparison metamethods:** `__eq`, `__lt`, `__le`

**Other metamethods:**
- `__concat` - string concatenation (`..`)
- `__len` - length operator (`#`)
- `__index` - table access for missing keys
- `__newindex` - table assignment for missing keys
- `__call` - function call on non-function

**Special metatable fields:** `__gc`, `__close`, `__mode`, `__name`

### 2.5 Garbage Collection (Lines 421-617)
- Incremental mode (2.5.1, line 460): pause, step multiplier, step size
- Generational mode (2.5.2, line 493): minor/major multiplier
- `__gc` metamethod for finalizers (2.5.3, line 518)
- Weak tables via `__mode`: "k", "v", or "kv" (2.5.4, line 572)

### 2.6 Coroutines (Lines 618-713)
- `coroutine.create(f)` - create
- `coroutine.resume(co, ...)` - start/continue
- `coroutine.yield(...)` - suspend
- `coroutine.wrap(f)` - create returning a function

---

## Section 3: The Language

### 3.1 Lexical Conventions (Lines 727-853)
**Reserved keywords (line 740):**
```
and       break     do        else      elseif    end
false     for       function  goto      if        in
local     nil       not       or        repeat    return
then      true      until     while
```

**Operators (line 752):**
```
+  -  *  /  %  ^  #
&  ~  |  <<  >>  //
==  ~=  <=  >=  <  >  =
(  )  {  }  [  ]  ::
;  :  ,  .  ..  ...
```

**String escapes (line 761):** `\a`, `\b`, `\f`, `\n`, `\r`, `\t`, `\v`, `\\`, `\"`, `\'`, `\z`, `\xXX`, `\ddd`, `\u{XXX}`

**Long brackets (line 788):** `[[...]]`, `[=[...]=]`, `[==[...]==]`

### 3.2 Variables (Lines 854-888)
- Global, local, and table fields
- `var.Name` is sugar for `var["Name"]`
- Global `x` is `_ENV.x`

### 3.3 Statements (Lines 889-1206)

**3.3.1 Blocks (line 895):** `do block end`

**3.3.2 Chunks (line 936):** Compilation unit, anonymous function with `_ENV`

**3.3.3 Assignment (line 961):** Multiple assignment, all reads before writes

**3.3.4 Control Structures (line 1006):**
- `while exp do block end`
- `repeat block until exp`
- `if exp then block {elseif exp then block} [else block] end`
- `goto Name`, `::label::`
- `break`, `return [explist] [';']`

**3.3.5 For Statement (line 1058):**
- Numerical: `for Name = exp, exp [, exp] do block end`
- Generic: `for namelist in explist do block end`

**3.3.7 Local Declarations (line 1144):**
- `local name <const>` - constant
- `local name <close>` - to-be-closed

**3.3.8 To-be-closed Variables (line 1172):**
- Requires `__close` metamethod
- Called on scope exit (normal, break, goto, return, error)

### 3.4 Expressions (Lines 1207-1761)

**3.4.1 Arithmetic Operators (line 1238):**
- `+`, `-`, `*`, `/` (float division), `//` (floor division), `%`, `^`, unary `-`

**3.4.2 Bitwise Operators (line 1273):**
- `&`, `|`, `~` (XOR and unary NOT), `>>`, `<<`

**3.4.3 Coercions (line 1293):** String library coerces strings to numbers in arithmetic

**3.4.4 Relational Operators (line 1340):**
- `==`, `~=`, `<`, `>`, `<=`, `>=`
- Different types are never equal (except via metamethods)

**3.4.5 Logical Operators (line 1388):**
- `and`, `or`, `not`
- Short-circuit evaluation
- `and`/`or` return operand values, not just booleans

**3.4.6 Concatenation (line 1412):** `..` operator

**3.4.7 Length Operator (line 1420):**
- `#` returns border for tables (sequence length)
- Table must be a sequence for predictable `#`

**3.4.8 Precedence (line 1461):** From lowest to highest:
```
or
and
<  >  <=  >=  ~=  ==
|
~
&
<<  >>
..            (right associative)
+  -
*  /  //  %
unary: not  #  -  ~
^             (right associative)
```

**3.4.9 Table Constructors (line 1484):**
- `{[exp] = exp, name = exp, exp, ...}`
- Consecutive integers assigned to positional values

**3.4.10 Function Calls (line 1528):**
- `f{table}` is `f({table})`
- `f"string"` is `f("string")`
- Tail calls: `return f(...)` (proper tail recursion)

**3.4.11 Function Definitions (line 1580):**
- `function t.a.b.c:f(params) body end` desugars to `t.a.b.c.f = function(self, params) body end`
- Variadic: `...` (vararg expression)

**3.4.12 Multiple Results (line 1685):**
- Only last expression in list can expand
- Adjustment to length with `nil`

### 3.5 Visibility Rules (Lines 1762-1805)
- Lexical scoping
- Upvalues (external local variables)
- Each `local` execution creates new variables

---

## Section 4: C API

### Core Concepts

**4.1 The Stack (line 1837):**
- Positive indices: absolute (1 = bottom)
- Negative indices: relative (-1 = top)
- `LUA_MINSTACK` = 20 guaranteed slots

**4.1.2 Valid and Acceptable Indices (line 1885):**
- Valid: 1 to top, plus pseudo-indices
- Acceptable: valid, or any positive index within allocated space

**4.2 C Closures (line 1946):**
- `lua_pushcclosure()` with upvalues
- `lua_upvalueindex(n)` for pseudo-index

**4.3 Registry (line 1964):**
- `LUA_REGISTRYINDEX` pseudo-index
- `LUA_RIDX_MAINTHREAD`, `LUA_RIDX_GLOBALS`

**4.4 Error Handling (line 1992):**
- Uses `longjmp` (or C++ exceptions)
- Status codes: `LUA_OK`, `LUA_ERRRUN`, `LUA_ERRMEM`, `LUA_ERRERR`, `LUA_ERRSYNTAX`, `LUA_YIELD`, `LUA_ERRFILE`

### Key C API Functions (Section 4.6, starts line 2150)

**State management:**
- `lua_newstate(alloc, ud)` - create state
- `lua_close(L)` - destroy state
- `lua_newthread(L)` - create coroutine

**Stack operations:**
- `lua_gettop(L)` - get stack size
- `lua_settop(L, n)` - set stack size
- `lua_pop(L, n)` - pop n elements
- `lua_pushvalue(L, idx)` - duplicate value
- `lua_insert(L, idx)` - move top to idx
- `lua_copy(L, from, to)` - copy value
- `lua_checkstack(L, n)` - ensure n slots

**Push functions:**
- `lua_pushnil(L)`
- `lua_pushboolean(L, b)`
- `lua_pushinteger(L, n)`
- `lua_pushnumber(L, n)`
- `lua_pushstring(L, s)` / `lua_pushlstring(L, s, len)`
- `lua_pushfstring(L, fmt, ...)` - formatted string
- `lua_pushcfunction(L, f)` / `lua_pushcclosure(L, f, n)`
- `lua_newtable(L)` / `lua_createtable(L, narr, nrec)`
- `lua_newuserdatauv(L, size, nuvalue)`

**Get functions (query values):**
- `lua_toboolean(L, idx)`
- `lua_tointeger(L, idx)` / `lua_tointegerx(L, idx, isnum)`
- `lua_tonumber(L, idx)` / `lua_tonumberx(L, idx, isnum)`
- `lua_tostring(L, idx)` / `lua_tolstring(L, idx, len)`
- `lua_topointer(L, idx)`
- `lua_touserdata(L, idx)`
- `lua_tothread(L, idx)`

**Type checking:**
- `lua_type(L, idx)` - returns type constant
- `lua_typename(L, tp)` - type name string
- `lua_isnil(L, idx)`, `lua_isboolean(L, idx)`, `lua_isnumber(L, idx)`, `lua_isstring(L, idx)`, `lua_istable(L, idx)`, `lua_isfunction(L, idx)`, `lua_isuserdata(L, idx)`, `lua_isthread(L, idx)`, `lua_isnone(L, idx)`, `lua_isnoneornil(L, idx)`

**Type constants:** `LUA_TNIL`, `LUA_TBOOLEAN`, `LUA_TNUMBER`, `LUA_TSTRING`, `LUA_TTABLE`, `LUA_TFUNCTION`, `LUA_TUSERDATA`, `LUA_TTHREAD`, `LUA_TNONE`

**Table operations:**
- `lua_gettable(L, idx)` - `t[key]` (key on stack)
- `lua_settable(L, idx)` - `t[key] = value` (key, value on stack)
- `lua_getfield(L, idx, k)` - `t[k]` (string key)
- `lua_setfield(L, idx, k)` - `t[k] = value`
- `lua_geti(L, idx, n)` - `t[n]` (integer key)
- `lua_seti(L, idx, n)` - `t[n] = value`
- `lua_rawget(L, idx)`, `lua_rawset(L, idx)` - no metamethods
- `lua_rawgeti(L, idx, n)`, `lua_rawseti(L, idx, n)`
- `lua_rawlen(L, idx)` - raw length
- `lua_next(L, idx)` - table iteration

**Global operations:**
- `lua_getglobal(L, name)` - push global
- `lua_setglobal(L, name)` - pop and set global

**Metatable operations:**
- `lua_getmetatable(L, idx)` - push metatable
- `lua_setmetatable(L, idx)` - pop and set metatable

**Function calls:**
- `lua_call(L, nargs, nresults)` - unprotected call
- `lua_pcall(L, nargs, nresults, msgh)` - protected call
- `lua_callk(L, ...)` / `lua_pcallk(L, ...)` - yieldable

**Loading:**
- `lua_load(L, reader, data, chunkname, mode)`

**Error handling:**
- `lua_error(L)` - raise error (never returns)
- `lua_atpanic(L, panicf)` - set panic handler

**Comparison:**
- `lua_compare(L, idx1, idx2, op)` - with `LUA_OPEQ`, `LUA_OPLT`, `LUA_OPLE`
- `lua_rawequal(L, idx1, idx2)` - no metamethods

---

## Section 5: Auxiliary Library (luaL_*)

All functions prefixed with `luaL_`, declared in `lauxlib.h`.

**Common patterns:**

**Argument checking:**
- `luaL_checkinteger(L, arg)` - get integer or error
- `luaL_checknumber(L, arg)` - get number or error
- `luaL_checkstring(L, arg)` - get string or error
- `luaL_checklstring(L, arg, len)` - get string with length
- `luaL_checktype(L, arg, t)` - check type
- `luaL_checkany(L, arg)` - check for any value
- `luaL_argcheck(L, cond, arg, msg)` - conditional check

**Optional arguments:**
- `luaL_optinteger(L, arg, default)`
- `luaL_optnumber(L, arg, default)`
- `luaL_optstring(L, arg, default)`
- `luaL_optlstring(L, arg, default, len)`

**String buffer:**
- `luaL_Buffer` type
- `luaL_buffinit(L, B)` - initialize
- `luaL_addchar(B, c)`, `luaL_addstring(B, s)`, `luaL_addlstring(B, s, len)`
- `luaL_pushresult(B)` - finalize and push

**Loading and running:**
- `luaL_loadfile(L, filename)` - load file
- `luaL_loadstring(L, s)` - load string
- `luaL_dofile(L, filename)` - load and run file
- `luaL_dostring(L, s)` - load and run string

**Error reporting:**
- `luaL_error(L, fmt, ...)` - raise formatted error
- `luaL_argerror(L, arg, msg)` - argument error

**References:**
- `luaL_ref(L, LUA_REGISTRYINDEX)` - create reference
- `luaL_unref(L, LUA_REGISTRYINDEX, ref)` - release reference

**Metatables:**
- `luaL_newmetatable(L, tname)` - create named metatable in registry
- `luaL_getmetatable(L, tname)` - get from registry
- `luaL_setmetatable(L, tname)` - set metatable by name

---

## Section 6: Standard Libraries

### 6.1 Basic Functions (Lines 5579-5925)

| Function | Description |
|----------|-------------|
| `assert(v, msg)` | Error if v is false |
| `collectgarbage(opt, arg)` | GC control |
| `dofile(filename)` | Execute file |
| `error(msg, level)` | Raise error |
| `_G` | Global environment table |
| `getmetatable(obj)` | Get metatable |
| `ipairs(t)` | Integer iterator |
| `load(chunk, name, mode, env)` | Load chunk |
| `loadfile(filename, mode, env)` | Load file |
| `next(t, idx)` | Table iteration |
| `pairs(t)` | Generic iterator |
| `pcall(f, ...)` | Protected call |
| `print(...)` | Print values |
| `rawequal(v1, v2)` | Equality without metamethods |
| `rawget(t, idx)` | Get without metamethods |
| `rawlen(v)` | Length without metamethods |
| `rawset(t, idx, v)` | Set without metamethods |
| `select(idx, ...)` | Return selected args |
| `setmetatable(t, mt)` | Set metatable |
| `tonumber(e, base)` | Convert to number |
| `tostring(v)` | Convert to string |
| `type(v)` | Get type string |
| `_VERSION` | "Lua 5.4" |
| `warn(msg, ...)` | Emit warning |
| `xpcall(f, msgh, ...)` | Protected call with handler |

### 6.2 Coroutine Library (Lines 5926-6010)

| Function | Description |
|----------|-------------|
| `coroutine.create(f)` | Create coroutine |
| `coroutine.resume(co, ...)` | Start/continue |
| `coroutine.yield(...)` | Suspend |
| `coroutine.status(co)` | "running", "suspended", "normal", "dead" |
| `coroutine.running()` | Current coroutine |
| `coroutine.isyieldable(co)` | Can yield? |
| `coroutine.wrap(f)` | Create as function |
| `coroutine.close(co)` | Close coroutine |

### 6.3 Package Library (Lines 6011-6249)

| Element | Description |
|---------|-------------|
| `require(modname)` | Load/return module |
| `package.loaded` | Loaded modules table |
| `package.path` | Lua module search path |
| `package.cpath` | C module search path |
| `package.preload` | Preload table |
| `package.searchers` | Search function list |
| `package.searchpath(name, path)` | Find module file |

### 6.4 String Library (Lines 6250-6765)

| Function | Description |
|----------|-------------|
| `string.byte(s, i, j)` | Get character codes |
| `string.char(...)` | Create string from codes |
| `string.dump(f, strip)` | Dump function to binary |
| `string.find(s, pattern, init, plain)` | Find pattern |
| `string.format(fmt, ...)` | Formatted string |
| `string.gmatch(s, pattern, init)` | Pattern iterator |
| `string.gsub(s, pattern, repl, n)` | Global substitute |
| `string.len(s)` | String length |
| `string.lower(s)` | Lowercase |
| `string.upper(s)` | Uppercase |
| `string.match(s, pattern, init)` | Match pattern |
| `string.rep(s, n, sep)` | Repeat string |
| `string.reverse(s)` | Reverse string |
| `string.sub(s, i, j)` | Substring |
| `string.pack(fmt, ...)` | Pack binary |
| `string.unpack(fmt, s, pos)` | Unpack binary |
| `string.packsize(fmt)` | Packed size |

**Pattern Syntax (6.4.1, line 6547):**
- `.` - any character
- `%a` - letter, `%A` - non-letter
- `%c` - control, `%C` - non-control
- `%d` - digit, `%D` - non-digit
- `%l` - lowercase, `%L` - non-lowercase
- `%p` - punctuation, `%P` - non-punctuation
- `%s` - space, `%S` - non-space
- `%u` - uppercase, `%U` - non-uppercase
- `%w` - alphanumeric, `%W` - non-alphanumeric
- `%x` - hex digit, `%X` - non-hex
- `%z` - zero character
- `[set]` - character class, `[^set]` - complement
- `*` - 0 or more (greedy), `+` - 1 or more (greedy)
- `-` - 0 or more (lazy), `?` - 0 or 1
- `^` - anchor at start, `$` - anchor at end
- `(...)` - capture, `%n` - backreference
- `%bxy` - balanced delimiter
- `%f[set]` - frontier pattern

### 6.5 UTF-8 Library (Lines 6766-6853)

| Function | Description |
|----------|-------------|
| `utf8.char(...)` | Create UTF-8 string from codepoints |
| `utf8.codes(s)` | Iterator over codepoints |
| `utf8.codepoint(s, i, j)` | Get codepoints |
| `utf8.len(s, i, j)` | String length in codepoints |
| `utf8.offset(s, n, i)` | Byte position of nth codepoint |
| `utf8.charpattern` | Pattern matching one UTF-8 character |

### 6.6 Table Library (Lines 6854-6942)

| Function | Description |
|----------|-------------|
| `table.concat(t, sep, i, j)` | Join elements |
| `table.insert(t, pos, v)` | Insert element |
| `table.remove(t, pos)` | Remove element |
| `table.move(a1, f, e, t, a2)` | Move elements |
| `table.pack(...)` | Pack args into table with `.n` |
| `table.unpack(t, i, j)` | Return elements |
| `table.sort(t, comp)` | Sort in-place |

### 6.7 Math Library (Lines 6943-7158)

| Element | Description |
|---------|-------------|
| `math.abs(x)` | Absolute value |
| `math.ceil(x)` | Round up |
| `math.floor(x)` | Round down |
| `math.fmod(x, y)` | Remainder |
| `math.modf(x)` | Integer and fractional parts |
| `math.max(...)` | Maximum |
| `math.min(...)` | Minimum |
| `math.sqrt(x)` | Square root |
| `math.exp(x)` | e^x |
| `math.log(x, base)` | Logarithm |
| `math.sin(x)`, `cos`, `tan` | Trig |
| `math.asin(x)`, `acos`, `atan` | Inverse trig |
| `math.deg(x)` | Radians to degrees |
| `math.rad(x)` | Degrees to radians |
| `math.random(m, n)` | Random number |
| `math.randomseed(x, y)` | Seed RNG |
| `math.tointeger(x)` | Convert to integer |
| `math.type(x)` | "integer", "float", or nil |
| `math.ult(m, n)` | Unsigned comparison |
| `math.huge` | Infinity |
| `math.maxinteger` | Max integer |
| `math.mininteger` | Min integer |
| `math.pi` | Pi |

### 6.8 I/O Library (Lines 7159-7409)

| Function | Description |
|----------|-------------|
| `io.open(filename, mode)` | Open file |
| `io.close(file)` | Close file |
| `io.input(file)` | Set/get default input |
| `io.output(file)` | Set/get default output |
| `io.read(...)` | Read from default input |
| `io.write(...)` | Write to default output |
| `io.lines(filename, ...)` | Line iterator |
| `io.flush()` | Flush default output |
| `io.type(obj)` | "file", "closed file", or nil |
| `io.popen(prog, mode)` | Open process |
| `io.tmpfile()` | Temp file |
| `io.stdin`, `io.stdout`, `io.stderr` | Standard streams |

**File methods:** `file:read(...)`, `file:write(...)`, `file:lines(...)`, `file:flush()`, `file:seek(whence, offset)`, `file:setvbuf(mode, size)`, `file:close()`

**Read modes:** `"n"` (number), `"a"` (all), `"l"` (line, no newline), `"L"` (line with newline), number (bytes)

### 6.9 OS Library (Lines 7410-7577)

| Function | Description |
|----------|-------------|
| `os.clock()` | CPU time |
| `os.date(fmt, t)` | Format date |
| `os.difftime(t2, t1)` | Time difference |
| `os.execute(cmd)` | Run shell command |
| `os.exit(code, close)` | Exit program |
| `os.getenv(var)` | Get environment variable |
| `os.remove(filename)` | Delete file |
| `os.rename(old, new)` | Rename file |
| `os.setlocale(locale, cat)` | Set locale |
| `os.time(table)` | Get/convert time |
| `os.tmpname()` | Temp filename |

### 6.10 Debug Library (Lines 7578-7806)

| Function | Description |
|----------|-------------|
| `debug.debug()` | Enter interactive debugger |
| `debug.getinfo(f, what)` | Function info |
| `debug.getlocal(f, idx)` | Local variable |
| `debug.setlocal(f, idx, v)` | Set local |
| `debug.getupvalue(f, idx)` | Upvalue |
| `debug.setupvalue(f, idx, v)` | Set upvalue |
| `debug.getmetatable(v)` | Get metatable (any type) |
| `debug.setmetatable(v, mt)` | Set metatable (any type) |
| `debug.getuservalue(u, n)` | Get userdata value |
| `debug.setuservalue(u, v, n)` | Set userdata value |
| `debug.getregistry()` | Get registry |
| `debug.traceback(msg, level)` | Stack trace |
| `debug.sethook(f, mask, count)` | Set debug hook |
| `debug.gethook()` | Get debug hook |

---

## Quick Reference: Common Tasks

### Table Operations

```lua
-- Create table
local t = {}
local t = {1, 2, 3}
local t = {key = "value", [1] = "first"}

-- Access
t.key or t["key"]
t[1]

-- Length (sequences only)
#t

-- Iteration
for i, v in ipairs(t) do ... end  -- sequence
for k, v in pairs(t) do ... end   -- all keys

-- Manipulation
table.insert(t, value)            -- append
table.insert(t, pos, value)       -- insert at pos
table.remove(t)                   -- remove last
table.remove(t, pos)              -- remove at pos
table.sort(t)                     -- sort in place
table.concat(t, sep)              -- join to string
```

### String Operations

```lua
-- Length
#s or string.len(s)

-- Substring
string.sub(s, start, stop)
s:sub(start, stop)

-- Find/match
string.find(s, pattern)           -- returns start, end
string.match(s, pattern)          -- returns captures
string.gmatch(s, pattern)         -- iterator

-- Replace
string.gsub(s, pattern, repl)     -- global replace
string.gsub(s, pattern, repl, n)  -- n replacements

-- Case
string.lower(s), string.upper(s)

-- Format
string.format("%s %d %.2f", str, int, float)
```

### Error Handling

```lua
-- Protected call
local ok, result = pcall(func, arg1, arg2)
if not ok then
    print("Error:", result)
end

-- With message handler
local ok, result = xpcall(func, debug.traceback, arg1)

-- Raise error
error("message")
error("message", 2)  -- level 2 = caller

-- Assert
assert(condition, "message")
```

### Metatables

```lua
-- Set metatable
setmetatable(t, {
    __index = function(t, k) return default end,
    __newindex = function(t, k, v) rawset(t, k, v) end,
    __call = function(t, ...) return result end,
    __tostring = function(t) return "string" end,
    __len = function(t) return count end,
    __add = function(a, b) return a + b end,
})

-- Get metatable
local mt = getmetatable(t)
```

### Coroutines

```lua
-- Create and use
local co = coroutine.create(function(arg)
    local result = coroutine.yield(value)
    return final
end)

local ok, value = coroutine.resume(co, arg)  -- start
local ok, value = coroutine.resume(co, send) -- continue

-- Status
coroutine.status(co)  -- "suspended", "running", "dead"

-- Wrap (simpler API)
local f = coroutine.wrap(function() ... end)
local value = f(arg)  -- errors propagate
```

---

## Filter Provenance Tracking Pattern

For tracking which Lua filter created/modified an AST node. The provenance is converted
to `SourceInfo` when marshaling back to Rust, integrating with the existing source tracking system.

### Design: Hidden `__provenance` Field

Store `{source, line}` in a hidden field on Lua element tables:
- `source`: The file path from `debug.getinfo().source` (e.g., `@filters/helper.lua`)
- `line`: Line number from `debug.getinfo().currentline`

Rust parses the source string and lazily registers files in `SourceContext` to get `FileId`.

**Why use source string instead of a pre-set FileId?**

A global `__FILTER_FILE_ID` fails with `require()` and dynamic loading:
```lua
local helper = require("helper")  -- different file!
function Str(elem)
    return helper.transform(elem)  -- should point to helper.lua, not main filter
end
```

The `debug.getinfo().source` field contains the actual file path where code executes.

### Source Format (Manual lines 4157-4162)

The `source` field follows this convention:
- `@path/to/file.lua` - File-based code (strip `@` to get path)
- `=description` - Custom source name (no file context available)
- literal string - Code loaded from a string (no file context)

### Capturing Caller Location

```lua
local function get_caller_provenance()
    local info = debug.getinfo(3, "Sl")  -- 3 = caller of constructor
    if info and info.currentline > 0 then
        return {
            source = info.source,  -- "@path/to/file.lua"
            line = info.currentline,
        }
    end
    return nil
end
```

### Stack Levels for debug.getinfo

When capturing provenance in a constructor like `pandoc.Str()`:

```
Level 0: debug.getinfo itself
Level 1: get_caller_provenance
Level 2: pandoc.Str (the constructor)
Level 3: The filter code that called pandoc.Str  <-- target
```

### The "Sl" Parameter

Each character selects fields to populate (from manual line 4261-4273):

- `S`: `source`, `short_src`, `linedefined`, `lastlinedefined`, `what`
- `l`: `currentline`

### Constructor with Provenance

```lua
function pandoc.Str(text)
    local elem = {t = "Str", c = text}
    elem.__provenance = get_caller_provenance()
    return elem
end
```

### Rust Conversion to SourceInfo

```rust
fn provenance_to_source_info(
    table: &LuaTable,
    source_context: &mut SourceContext,
    file_cache: &mut HashMap<String, FileId>,
) -> LuaResult<SourceInfo> {
    let prov: Option<LuaTable> = table.get("__provenance").ok();
    let Some(prov) = prov else {
        return Ok(SourceInfo::default());
    };

    let source: String = prov.get("source")?;
    let line: usize = prov.get("line")?;

    // Parse "@path/to/file.lua" -> "path/to/file.lua"
    let path = source.strip_prefix('@')?;

    // Lazy registration with caching
    let file_id = file_cache.entry(path.to_string())
        .or_insert_with(|| source_context.add_file(path.to_string(), None));

    let file = source_context.get_file(*file_id)?;
    let file_info = file.file_info.as_ref()?;

    let start_offset = file_info.line_to_offset(line - 1);
    let end_offset = file_info.line_to_offset(line);

    Ok(SourceInfo::original(*file_id, start_offset, end_offset))
}
```

---

## File Locations for C Source Reference

The Lua source repository is at `external-sources/lua/` (branch v5.4). Key files:

- `lua.h` - Main Lua C API
- `lauxlib.h` - Auxiliary library
- `lualib.h` - Standard library openers
- `luaconf.h` - Configuration
- `lapi.c` - C API implementation
- `lbaselib.c` - Basic library
- `lstrlib.c` - String library
- `ltablib.c` - Table library
- `lmathlib.c` - Math library
- `liolib.c` - I/O library
- `loslib.c` - OS library
- `lcorolib.c` - Coroutine library
- `lutf8lib.c` - UTF-8 library
- `ldblib.c` - Debug library
- `ldebug.c` / `ldebug.h` - Debug interface implementation
- `lobject.h` - Core object definitions (including `Proto` for function debug info)
