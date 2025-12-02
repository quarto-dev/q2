# mlua Analysis for Lua Filter Implementation

**Date:** 2025-12-02
**Updated:** 2025-12-02 (revised based on review feedback)
**Related Issue:** k-409
**Related Documents:**
- [Lua Filters Design](./2025-11-26-lua-filters-design.md)
- [Filter Diagnostics Analysis](./2025-12-02-filter-diagnostics-analysis.md)
- [FilterContext Refactoring](./2025-12-02-filter-context-refactoring.md)

## Executive Summary

This document analyzes the mlua crate to understand how to implement Pandoc-compatible Lua filter elements. The analysis concludes that mlua's UserData system fully supports our requirements, with moderate implementation complexity.

**Key finding:** Pandoc uses **userdata** (not plain tables) for AST elements. We verified this experimentally:

```lua
function Str(elem)
    print(type(elem))  -- prints "userdata"
end
```

Modern Pandoc elements:
- Are userdata with metatables
- Use named fields (`text`, `content`, `target`) not `c`
- Have methods (`clone`, `walk`)
- Return `nil` for the old `c` field

---

## mlua Capabilities

### 1. UserData Trait

The core abstraction for exposing Rust types to Lua:

```rust
impl UserData for MyType {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        // Static fields (shared by all instances)
        fields.add_field("constant", 42);

        // Instance field getters/setters
        fields.add_field_method_get("x", |_, this| Ok(this.x));
        fields.add_field_method_set("x", |_, this, val| { this.x = val; Ok(()) });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // Instance methods
        methods.add_method("get", |_, this, ()| Ok(this.value));
        methods.add_method_mut("set", |_, this, val| { this.value = val; Ok(()) });

        // Metamethods
        methods.add_meta_method(MetaMethod::Index, |_, this, key: String| { ... });
        methods.add_meta_method(MetaMethod::Pairs, |lua, this, ()| { ... });
    }
}
```

### 2. Available MetaMethods

| MetaMethod | Lua | Purpose |
|------------|-----|---------|
| `Index` | `__index` | Field/method access (`elem.text`) |
| `NewIndex` | `__newindex` | Field assignment (`elem.text = "x"`) |
| `Pairs` | `__pairs` | Iteration (`for k,v in pairs(elem)`) |
| `ToString` | `__tostring` | String conversion |
| `Type` | `__name`/`__type` | Type name for `tostring()`/`typeof()` |
| `Call` | `__call` | Call as function |
| `Eq`, `Lt`, `Le` | Comparison operators |

### 3. IntoLua / FromLua Traits

For automatic value conversion:

```rust
impl IntoLua for MyType {
    fn into_lua(self, lua: &Lua) -> Result<Value> { ... }
}

impl FromLua for MyType {
    fn from_lua(value: Value, lua: &Lua) -> Result<Self> { ... }
}
```

### 4. Creating UserData

```rust
// From a type implementing UserData trait
let ud = lua.create_userdata(MyType { ... })?;

// From any type with registered methods
lua.register_userdata_type::<String>(|reg| {
    reg.add_method("len", |_, this, ()| Ok(this.len()));
})?;
let ud = lua.create_any_userdata("hello".to_string())?;
```

### 5. Accessing UserData from Rust

```rust
// Type checking
ud.is::<MyType>()
ud.type_id()

// Borrowing
let borrowed: UserDataRef<MyType> = ud.borrow()?;
let borrowed_mut: UserDataRefMut<MyType> = ud.borrow_mut()?;
```

---

## Pandoc Element Behavior (Verified Experimentally)

We tested actual Pandoc Lua filter behavior to understand the interface:

### Element Structure

```lua
function Str(elem)
    print("type(elem) = " .. type(elem))        -- "userdata"
    print("elem.t = " .. tostring(elem.t))      -- "Str"
    print("elem.tag = " .. tostring(elem.tag))  -- "Str"
    print("elem.text = " .. tostring(elem.text)) -- "hello"
    print("elem.c = " .. tostring(elem.c))      -- "nil" (old format, not used)

    for k, v in pairs(elem) do
        print("  " .. k .. " = " .. type(v))
    end
    -- Output:
    --   tag = string
    --   text = string
    --   clone = function
    --   walk = function
end
```

### Link Element

```lua
function Link(elem)
    print("elem.target = " .. elem.target)      -- URL string
    print("elem.title = " .. elem.title)        -- title string
    print("type(elem.content) = " .. type(elem.content))  -- "table"
    print("type(elem.attr) = " .. type(elem.attr))        -- "userdata"
end
```

### Constructor Return Types

```lua
local str = pandoc.Str("test")
print(type(str))  -- "userdata"

local para = pandoc.Para({pandoc.Str("hello")})
print(type(para))  -- "userdata"
```

### Plain Table Returns

**Important:** Pandoc does NOT accept plain tables as filter return values:

```lua
function Str(elem)
    -- This does NOT work - element is silently dropped
    return {t = "Str", text = "REPLACED"}
end
```

Filters must return userdata from constructors:

```lua
function Str(elem)
    -- This works
    return pandoc.Str("REPLACED")
end
```

---

## Proposed Implementation Strategy

### Recommended: Wrapper Types with Dynamic Field Access

Create two main wrapper types that use metamethods for Pandoc-style access:

```rust
pub struct LuaInline(pub pandoc::Inline);
pub struct LuaBlock(pub pandoc::Block);

impl UserData for LuaInline {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        // Static fields accessible on all inlines
        fields.add_field_method_get("t", |_, this| Ok(this.tag_name()));
        fields.add_field_method_get("tag", |_, this| Ok(this.tag_name()));
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // Dynamic field access via __index
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: String| {
            this.get_field(lua, &key)
        });

        // Dynamic field assignment via __newindex
        methods.add_meta_method_mut(MetaMethod::NewIndex,
            |lua, this, (key, val): (String, Value)| {
                this.set_field(&key, val, lua)
            });

        // Methods
        methods.add_method("clone", |lua, this, ()| {
            lua.create_userdata(LuaInline(this.0.clone()))
        });

        // Iteration
        methods.add_meta_method(MetaMethod::Pairs, |lua, this, ()| {
            this.create_pairs_iterator(lua)
        });
    }
}

impl LuaInline {
    fn tag_name(&self) -> &'static str {
        match &self.0 {
            Inline::Str(_) => "Str",
            Inline::Emph(_) => "Emph",
            Inline::Strong(_) => "Strong",
            // ... etc
        }
    }

    fn get_field(&self, lua: &Lua, key: &str) -> Result<Value> {
        match (&self.0, key) {
            (Inline::Str(s), "text") => s.text.clone().into_lua(lua),
            (Inline::Link(l), "target") => l.target.0.clone().into_lua(lua),
            (Inline::Link(l), "title") => l.target.1.clone().into_lua(lua),
            (Inline::Link(l), "content") => {
                // Convert Vec<Inline> to Lua table of LuaInline userdata
                let table = lua.create_table()?;
                for (i, inline) in l.content.iter().enumerate() {
                    table.set(i + 1, lua.create_userdata(LuaInline(inline.clone()))?)?;
                }
                Ok(Value::Table(table))
            },
            _ => Ok(Value::Nil),
        }
    }

    fn set_field(&mut self, key: &str, val: Value, lua: &Lua) -> Result<()> {
        match (&mut self.0, key) {
            (Inline::Str(s), "text") => {
                s.text = String::from_lua(val, lua)?;
                Ok(())
            },
            // ... etc
            _ => Err(Error::runtime(format!("cannot set field '{}'", key)))
        }
    }
}
```

### Alternative Approaches Considered

#### Option A: Separate UserData per Element Type

```rust
struct LuaStr { text: String, source_info: SourceInfo }
struct LuaEmph { content: Vec<LuaInline>, source_info: SourceInfo }
// ... one type per element
```

**Pros:** Type-safe, better IDE support
**Cons:** Many types to implement, verbose

#### Option B: Generic Element with Enum

```rust
enum ElementKind { Str { text: String }, Emph { content: Vec<...> }, ... }
struct LuaElement { tag: &'static str, kind: ElementKind }
```

**Pros:** Single type
**Cons:** Still need match arms, less direct

**Recommendation:** Use wrapper approach (LuaInline/LuaBlock) as it:
- Wraps existing AST types directly
- Minimizes new types (just 2-3 wrappers)
- Uses dynamic dispatch via metamethods (matches Pandoc's behavior)

---

## Implementation Complexity Assessment

| Feature | Difficulty | Notes |
|---------|------------|-------|
| Basic field access (`t`, `tag`) | **Easy** | Static field methods |
| Named field access (`text`, `target`) | **Medium** | Dynamic dispatch in `__index` |
| Field mutation | **Medium** | `__newindex` metamethod |
| `clone()` method | **Easy** | Simple method |
| `walk()` method | **Hard** | Requires AST traversal callback |
| `pairs()` iteration | **Medium** | Iterator function creation |
| Constructors (`pandoc.Str()`) | **Easy** | Global function registration |
| List handling (`content`) | **Medium** | Tables of UserData |
| Filter return handling | **Medium** | Detect nil/element/list |
| Attr special access patterns | **Medium** | Userdata with identifier/classes/attributes |
| Filter provenance tracking | **Medium** | Capture source location for diagnostics |

---

## Handling Filter Return Values

Pandoc accepts several return types from filters:

1. **`nil`** → Element unchanged
2. **Single element (userdata)** → Replace element
3. **Empty table `{}`** → Delete element
4. **Table of elements** → Splice into parent
5. **`false`** (topdown only) → Skip children

```rust
enum FilterResult<T> {
    Unchanged,
    Replace(T),
    Delete,
    Splice(Vec<T>),
    SkipChildren(T),  // topdown only
}

fn handle_filter_return<T>(value: Value, lua: &Lua) -> Result<FilterResult<T>>
where
    T: FromLua,
{
    match value {
        Value::Nil => Ok(FilterResult::Unchanged),
        Value::Boolean(false) => Ok(FilterResult::SkipChildren(...)),
        Value::UserData(ud) => {
            let elem = T::from_lua(Value::UserData(ud), lua)?;
            Ok(FilterResult::Replace(elem))
        }
        Value::Table(t) if t.raw_len() == 0 => Ok(FilterResult::Delete),
        Value::Table(t) => {
            let elems: Vec<T> = t.sequence_values()
                .map(|v| T::from_lua(v?, lua))
                .collect::<Result<_>>()?;
            Ok(FilterResult::Splice(elems))
        }
        _ => Err(Error::runtime("invalid filter return type"))
    }
}
```

---

## Key Design Decisions

### 1. Feature Flag for Lua Filter Support

The entire Lua filter implementation will be behind a cargo feature flag:

```toml
[features]
default = []
lua-filter = ["mlua"]

[dependencies]
mlua = { version = "0.10", features = ["lua54", "vendored"], optional = true }
```

**Rationale:** This allows disabling Lua support in constrained environments (e.g., WASM targets) where mlua may not be available or desirable. This mirrors the existing `json-filter` feature pattern.

In code:
```rust
#[cfg(feature = "lua-filter")]
mod lua;

#[cfg(feature = "lua-filter")]
#[arg(long = "lua-filter", action = clap::ArgAction::Append)]
lua_filters: Vec<std::path::PathBuf>,
```

### 2. No `send` Feature Initially

**Decision:** Do not use mlua's `send` feature for now.

**Rationale:**
- `send` makes `Lua: Send + Sync`, required for async operations
- We don't expect async filters for a long time
- `send` adds synchronization overhead and requires all UserData to implement `Send`
- Our AST wrapper types will naturally be `Send`-compatible (plain data, no `Rc`, `RefCell`, etc.)
- We can add `send` later without breaking changes, as long as our types remain `Send`-compatible

**Future-proofing:** Design all UserData types to be `Send`-compatible from the start, so adding `send` later is a non-breaking change.

### 3. Lists as Tables, Elements as UserData

- `elem.content` returns a Lua **table** containing **userdata** elements
- Matches Pandoc's verified behavior
- Tables are native Lua sequences, elements are userdata

### 4. Interior Mutability

- Filters can mutate elements: `elem.text = "new"`
- mlua handles borrowing automatically via `borrow_mut`
- Use `add_meta_method_mut` for `__newindex`

### 5. Source Info Tracking

- Store `SourceInfo` in wrapper types
- For filter-created elements, capture provenance via constructor
- See [Filter Diagnostics Analysis](./2025-12-02-filter-diagnostics-analysis.md) for details

### 6. Type Names

- `elem.t` and `elem.tag` both return the element type name (string)
- `type(elem)` returns `"userdata"` (matches Pandoc)
- Use `MetaMethod::Type` to set the `__name` field for `tostring()`

---

## Cargo.toml Configuration

```toml
[features]
default = []
lua-filter = ["mlua"]

[dependencies]
mlua = { version = "0.10", features = ["lua54", "vendored"], optional = true }
```

Features used:
- `lua54`: Use Lua 5.4 (matches Pandoc)
- `vendored`: Build Lua from source (no system dependency)

Features **not** used initially:
- `send`: Not needed until we have async filter requirements

---

## Implementation Phases

All phases are **required** for the feature to be considered complete.

### Phase 1: Core Types (Foundation)

- [ ] Add mlua optional dependency to Cargo.toml with `lua-filter` feature
- [ ] Create `src/lua/mod.rs` module structure (behind `#[cfg(feature = "lua-filter")]`)
- [ ] Implement `LuaInline` wrapper with basic fields
- [ ] Implement `LuaBlock` wrapper with basic fields
- [ ] Support common types: Str, Para, Header, Link, Emph, Strong
- [ ] Implement constructors: `pandoc.Str()`, `pandoc.Para()`, etc.
- [ ] Basic typewise traversal
- [ ] Add `--lua-filter` CLI argument (behind feature flag)
- [ ] Test with simple filter (uppercase Str elements)

### Phase 2: Complete Element Coverage

- [ ] All inline element fields
- [ ] All block element fields
- [ ] Meta value handling
- [ ] **Attr (attributes) with special access patterns** (userdata, not simplified tables)
  - `attr.identifier` - string
  - `attr.classes` - list of strings
  - `attr.attributes` - key-value pairs
  - Constructor: `pandoc.Attr(id, classes, attrs)`
- [ ] List splicing semantics

### Phase 3: Methods & Traversal

- [ ] `clone()` method on elements
- [ ] **`walk()` method** (required - too important for practical filters)
  - Must support AST traversal with Lua callback functions
  - Used extensively in real-world filters
- [ ] `pairs()` iteration
- [ ] Topdown traversal mode
- [ ] Global variables (FORMAT, PANDOC_VERSION, etc.)

### Phase 4: Diagnostics & Utilities

- [ ] **Filter provenance tracking** (required for diagnostic messages)
  - Capture source file and line from Lua `debug.getinfo()`
  - Store in wrapper types as `SourceInfo::FilterProvenance`
  - Report in error messages when filter-created elements cause issues
- [ ] `pandoc.utils.stringify()`
- [ ] Additional `pandoc.utils.*` functions as needed
- [ ] Performance optimization

---

## Attr Special Access Patterns

Pandoc's Attr type has a specific structure that filters depend on:

```lua
-- Attr is userdata with special access
local attr = elem.attr
print(attr.identifier)   -- string (the #id)
print(attr.classes)      -- table of strings (the .class values)
print(attr.attributes)   -- table of key-value pairs

-- Can also be accessed positionally (for backward compatibility)
local id, classes, attrs = attr[1], attr[2], attr[3]

-- Constructor
local new_attr = pandoc.Attr("myid", {"class1", "class2"}, {key = "value"})
```

Implementation:

```rust
pub struct LuaAttr(pub pandoc::Attr);

impl UserData for LuaAttr {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("identifier", |_, this| Ok(this.0.identifier.clone()));
        fields.add_field_method_set("identifier", |_, this, val| { this.0.identifier = val; Ok(()) });

        fields.add_field_method_get("classes", |lua, this| {
            // Return as Lua table
            let table = lua.create_table()?;
            for (i, class) in this.0.classes.iter().enumerate() {
                table.set(i + 1, class.clone())?;
            }
            Ok(table)
        });
        // ... etc
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // Positional access via __index
        methods.add_meta_method(MetaMethod::Index, |lua, this, key: Value| {
            match key {
                Value::Integer(1) => this.0.identifier.clone().into_lua(lua),
                Value::Integer(2) => /* classes as table */,
                Value::Integer(3) => /* attributes as table */,
                Value::String(s) => /* named field access */,
                _ => Ok(Value::Nil),
            }
        });
    }
}
```

---

## References

- mlua source: `external-sources/mlua/`
- mlua documentation: `external-sources/mlua/README.md`
- mlua examples: `external-sources/mlua/examples/`
- mlua tests: `external-sources/mlua/tests/userdata.rs`
- Pandoc Lua filter docs: `external-sources/pandoc/doc/lua-filters.md`
