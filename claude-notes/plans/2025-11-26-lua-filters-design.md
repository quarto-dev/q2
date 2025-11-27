# Lua Filter Support Design Plan

**Issue:** k-409
**Epic:** k-407 (Extensible filters for quarto-markdown-pandoc)
**Created:** 2025-11-26

## Overview

Implement embedded Lua filter support using the mlua crate. Lua filters are significantly faster than JSON filters (2% vs 35-40% overhead) because they avoid process spawning and JSON serialization. This design aims for compatibility with Pandoc's Lua filter API while being pragmatic about implementation scope.

## Background Research

### Pandoc Lua Filter Architecture

From exploration of `external-sources/pandoc/pandoc-lua-engine/`:

1. **Filter structure**: Lua table with element type names as keys
2. **Traversal modes**:
   - `typewise` (default): Process by element type in fixed order
   - `topdown`: Depth-first traversal from root
3. **Return semantics**:
   - `nil`: Element unchanged
   - Same type: Replaces element
   - List: Splice into parent
   - Empty list `{}`: Delete element
   - `false` (topdown): Skip children

### mlua Key APIs

From exploration of `external-sources/mlua/`:

```rust
// Create Lua state
let lua = Lua::new();

// Execute code
lua.load(code).exec()?;
let result: T = lua.load(code).eval()?;

// Create functions
let func = lua.create_function(|lua, args| Ok(result))?;
globals.set("func_name", func)?;

// UserData for custom types
impl UserData for MyType {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("name", |lua, this, args| Ok(result));
    }
}
```

### Existing Internal Filter System

Our `src/filters.rs` already has:
- `Filter` struct with per-element-type callbacks
- `topdown_traverse_*` functions for AST traversal
- `FilterReturn` enum for unchanged/modified/recurse semantics

We can reuse traversal patterns, but Lua filters need a different representation.

## Design

### 1. Module Structure

```
src/
├── external_filters/
│   ├── mod.rs              # ExternalFilter enum, apply_filters
│   ├── json_filter.rs      # JSON filter implementation (k-408)
│   └── filter_path.rs      # Path resolution utilities
└── lua/
    ├── mod.rs              # Public API
    ├── state.rs            # Lua state management
    ├── marshal/
    │   ├── mod.rs          # Marshal trait definitions
    │   ├── inline.rs       # Inline type marshaling
    │   ├── block.rs        # Block type marshaling
    │   ├── meta.rs         # MetaValue marshaling
    │   └── attr.rs         # Attr tuple marshaling
    ├── constructors.rs     # pandoc.Str(), pandoc.Para(), etc.
    ├── filter.rs           # Filter loading and execution
    └── globals.rs          # FORMAT, PANDOC_VERSION, etc.
```

### 2. Cargo.toml Changes

```toml
[dependencies]
mlua = { version = "0.11", features = ["lua54", "vendored", "send"] }
```

Features explained:
- `lua54`: Use Lua 5.4 (matches Pandoc)
- `vendored`: Build Lua from source (no system dependency)
- `send`: Make Lua state Send + Sync (for potential future async)

### 3. Lua State Management

```rust
// src/lua/state.rs

use mlua::prelude::*;

pub struct LuaFilterEngine {
    lua: Lua,
}

impl LuaFilterEngine {
    pub fn new() -> LuaResult<Self> {
        let lua = Lua::new();

        // Register pandoc module
        let pandoc = lua.create_table()?;
        register_constructors(&lua, &pandoc)?;
        lua.globals().set("pandoc", pandoc)?;

        Ok(Self { lua })
    }

    pub fn load_filter(&self, path: &Path) -> LuaResult<LuaFilter> {
        let source = std::fs::read_to_string(path)?;
        self.lua.load(&source)
            .set_name(path.display().to_string())
            .exec()?;

        LuaFilter::from_lua_state(&self.lua)
    }

    pub fn set_globals(&self, context: &FilterContext) -> LuaResult<()> {
        let globals = self.lua.globals();
        globals.set("FORMAT", context.target_format.clone())?;
        globals.set("PANDOC_VERSION", create_version_table(&self.lua)?)?;
        globals.set("PANDOC_API_VERSION", create_api_version_table(&self.lua)?)?;
        globals.set("PANDOC_SCRIPT_FILE", context.script_path.display().to_string())?;
        // PANDOC_READER_OPTIONS, PANDOC_WRITER_OPTIONS later
        Ok(())
    }
}
```

### 4. AST Marshaling Design

Key decision: **Use Lua tables, not UserData, for AST elements.**

Rationale:
- Pandoc uses tables: `{t = "Str", c = "text"}`
- Tables are easier to construct/modify in Lua
- UserData requires more boilerplate
- Simpler mental model for filter authors

#### Marshal Trait

```rust
// src/lua/marshal/mod.rs

pub trait ToLua {
    fn to_lua(&self, lua: &Lua) -> LuaResult<LuaValue>;
}

pub trait FromLua: Sized {
    fn from_lua(value: LuaValue, lua: &Lua) -> LuaResult<Self>;
}

// Convenience for tables
fn create_element(lua: &Lua, tag: &str, content: LuaValue) -> LuaResult<LuaTable> {
    let table = lua.create_table()?;
    table.set("t", tag)?;
    table.set("c", content)?;
    Ok(table)
}
```

#### Inline Marshaling

```rust
// src/lua/marshal/inline.rs

impl ToLua for Inline {
    fn to_lua(&self, lua: &Lua) -> LuaResult<LuaValue> {
        match self {
            Inline::Str(s) => {
                create_element(lua, "Str", s.value.clone().into_lua(lua)?)
            }
            Inline::Emph(e) => {
                let content = e.content.iter()
                    .map(|i| i.to_lua(lua))
                    .collect::<LuaResult<Vec<_>>>()?;
                create_element(lua, "Emph", lua.create_sequence_from(content)?.into())
            }
            Inline::Strong(s) => {
                let content = s.content.iter()
                    .map(|i| i.to_lua(lua))
                    .collect::<LuaResult<Vec<_>>>()?;
                create_element(lua, "Strong", lua.create_sequence_from(content)?.into())
            }
            Inline::Link(l) => {
                // [attr, content, target]
                let attr = attr_to_lua(&l.attr, lua)?;
                let content = inlines_to_lua(&l.content, lua)?;
                let target = lua.create_sequence_from(vec![
                    l.target.0.clone(),
                    l.target.1.clone(),
                ])?;
                let c = lua.create_sequence_from(vec![attr, content.into(), target.into()])?;
                create_element(lua, "Link", c.into())
            }
            // ... other variants
            Inline::Space(_) => create_element(lua, "Space", LuaValue::Nil),
            Inline::SoftBreak(_) => create_element(lua, "SoftBreak", LuaValue::Nil),
            Inline::LineBreak(_) => create_element(lua, "LineBreak", LuaValue::Nil),
            // etc.
        }
    }
}

impl FromLua for Inline {
    fn from_lua(value: LuaValue, lua: &Lua) -> LuaResult<Self> {
        let table = value.as_table()
            .ok_or_else(|| LuaError::FromLuaConversionError {
                from: value.type_name(),
                to: "Inline",
                message: Some("expected table".into()),
            })?;

        let tag: String = table.get("t")?;
        let content: LuaValue = table.get("c")?;

        match tag.as_str() {
            "Str" => Ok(Inline::Str(pandoc::Str {
                value: String::from_lua(content, lua)?,
                source_info: SourceInfo::none(),  // Filters lose source info
            })),
            "Emph" => {
                let inlines = Vec::<Inline>::from_lua(content, lua)?;
                Ok(Inline::Emph(pandoc::Emph {
                    content: inlines,
                    source_info: SourceInfo::none(),
                }))
            }
            // ... etc
            _ => Err(LuaError::FromLuaConversionError {
                from: "table",
                to: "Inline",
                message: Some(format!("unknown inline type: {}", tag)),
            }),
        }
    }
}
```

#### Block Marshaling

Similar pattern for Block types.

### 5. Element Constructors

Expose `pandoc.*` constructor functions:

```rust
// src/lua/constructors.rs

pub fn register_constructors(lua: &Lua, pandoc: &LuaTable) -> LuaResult<()> {
    // Inline constructors
    pandoc.set("Str", lua.create_function(constructor_str)?)?;
    pandoc.set("Emph", lua.create_function(constructor_emph)?)?;
    pandoc.set("Strong", lua.create_function(constructor_strong)?)?;
    pandoc.set("Link", lua.create_function(constructor_link)?)?;
    pandoc.set("Image", lua.create_function(constructor_image)?)?;
    pandoc.set("Code", lua.create_function(constructor_code)?)?;
    pandoc.set("Math", lua.create_function(constructor_math)?)?;
    pandoc.set("Span", lua.create_function(constructor_span)?)?;
    pandoc.set("Space", lua.create_function(constructor_space)?)?;
    pandoc.set("SoftBreak", lua.create_function(constructor_softbreak)?)?;
    pandoc.set("LineBreak", lua.create_function(constructor_linebreak)?)?;
    // ... more inlines

    // Block constructors
    pandoc.set("Para", lua.create_function(constructor_para)?)?;
    pandoc.set("Plain", lua.create_function(constructor_plain)?)?;
    pandoc.set("Header", lua.create_function(constructor_header)?)?;
    pandoc.set("CodeBlock", lua.create_function(constructor_codeblock)?)?;
    pandoc.set("BlockQuote", lua.create_function(constructor_blockquote)?)?;
    pandoc.set("BulletList", lua.create_function(constructor_bulletlist)?)?;
    pandoc.set("OrderedList", lua.create_function(constructor_orderedlist)?)?;
    pandoc.set("Div", lua.create_function(constructor_div)?)?;
    // ... more blocks

    // Meta constructors
    pandoc.set("MetaString", lua.create_function(constructor_metastring)?)?;
    pandoc.set("MetaInlines", lua.create_function(constructor_metainlines)?)?;
    pandoc.set("MetaBlocks", lua.create_function(constructor_metablocks)?)?;
    pandoc.set("MetaList", lua.create_function(constructor_metalist)?)?;
    pandoc.set("MetaMap", lua.create_function(constructor_metamap)?)?;

    Ok(())
}

fn constructor_str(_lua: &Lua, text: String) -> LuaResult<LuaTable> {
    // Returns {t = "Str", c = text}
    create_element(_lua, "Str", text.into_lua(_lua)?)
}

fn constructor_emph(lua: &Lua, content: LuaValue) -> LuaResult<LuaTable> {
    // Accept single inline or list
    let content = normalize_to_list(lua, content)?;
    create_element(lua, "Emph", content)
}

fn constructor_link(lua: &Lua, args: LuaMultiValue) -> LuaResult<LuaTable> {
    // pandoc.Link(content, target, [title], [attr])
    // or pandoc.Link(attr, content, target)
    // Pandoc accepts multiple calling conventions
    let (attr, content, target) = parse_link_args(lua, args)?;
    let c = lua.create_sequence_from(vec![attr, content, target])?;
    create_element(lua, "Link", c.into())
}
```

### 6. Filter Structure Detection

```rust
// src/lua/filter.rs

pub struct LuaFilter {
    /// Callbacks for each element type
    callbacks: HashMap<String, LuaFunction>,
    /// Traversal mode
    traverse: TraverseMode,
}

pub enum TraverseMode {
    Typewise,  // Default: process by type in fixed order
    Topdown,   // Depth-first from root
}

impl LuaFilter {
    pub fn from_lua_state(lua: &Lua) -> LuaResult<Self> {
        let globals = lua.globals();

        // Check for explicit return value
        // (filter script might return a table)

        // Otherwise, collect global functions that match element names
        let mut callbacks = HashMap::new();

        let element_names = [
            // Inlines
            "Str", "Emph", "Strong", "Underline", "Strikeout",
            "Superscript", "Subscript", "SmallCaps", "Quoted",
            "Cite", "Code", "Space", "SoftBreak", "LineBreak",
            "Math", "RawInline", "Link", "Image", "Note", "Span",
            // Blocks
            "Plain", "Para", "LineBlock", "CodeBlock", "RawBlock",
            "BlockQuote", "OrderedList", "BulletList", "DefinitionList",
            "Header", "HorizontalRule", "Table", "Figure", "Div",
            // Collections
            "Inlines", "Blocks",
            // Meta
            "Meta", "Pandoc",
        ];

        for name in element_names {
            if let Ok(func) = globals.get::<LuaFunction>(name) {
                callbacks.insert(name.to_string(), func);
            }
        }

        // Check for traverse field
        let traverse = if let Ok(mode) = globals.get::<String>("traverse") {
            match mode.as_str() {
                "topdown" => TraverseMode::Topdown,
                _ => TraverseMode::Typewise,
            }
        } else {
            TraverseMode::Typewise
        };

        Ok(Self { callbacks, traverse })
    }
}
```

### 7. Filter Execution

#### Typewise Traversal

Process elements in Pandoc's canonical order:

```rust
impl LuaFilter {
    pub fn apply_typewise(&self, lua: &Lua, doc: Pandoc) -> LuaResult<Pandoc> {
        let mut doc = doc;

        // Order from Pandoc: Inline filters first, then Block filters, then Meta, then Pandoc

        // 1. Individual inline element filters
        for name in &["Str", "Emph", "Strong", "Underline", /* ... */] {
            if let Some(callback) = self.callbacks.get(*name) {
                doc = self.apply_to_all_inlines(lua, doc, name, callback)?;
            }
        }

        // 2. Inlines filter (operates on inline lists)
        if let Some(callback) = self.callbacks.get("Inlines") {
            doc = self.apply_to_inline_lists(lua, doc, callback)?;
        }

        // 3. Individual block element filters
        for name in &["Para", "Plain", "Header", "CodeBlock", /* ... */] {
            if let Some(callback) = self.callbacks.get(*name) {
                doc = self.apply_to_all_blocks(lua, doc, name, callback)?;
            }
        }

        // 4. Blocks filter
        if let Some(callback) = self.callbacks.get("Blocks") {
            doc = self.apply_to_block_lists(lua, doc, callback)?;
        }

        // 5. Meta filter
        if let Some(callback) = self.callbacks.get("Meta") {
            doc.meta = self.apply_to_meta(lua, doc.meta, callback)?;
        }

        // 6. Pandoc filter
        if let Some(callback) = self.callbacks.get("Pandoc") {
            doc = self.apply_to_document(lua, doc, callback)?;
        }

        Ok(doc)
    }

    fn apply_callback<T: ToLua + FromLua>(
        &self,
        lua: &Lua,
        element: T,
        callback: &LuaFunction,
    ) -> LuaResult<FilterResult<T>> {
        let lua_value = element.to_lua(lua)?;
        let result: LuaValue = callback.call(lua_value)?;

        match result {
            LuaValue::Nil => Ok(FilterResult::Unchanged),
            LuaValue::Table(t) => {
                // Could be single element or list
                if is_element_table(&t)? {
                    Ok(FilterResult::Replaced(T::from_lua(LuaValue::Table(t), lua)?))
                } else {
                    // It's a list - splice
                    let items = table_to_vec::<T>(&t, lua)?;
                    Ok(FilterResult::Spliced(items))
                }
            }
            _ => Err(LuaError::RuntimeError(
                "filter must return nil, element, or list of elements".into()
            )),
        }
    }
}

enum FilterResult<T> {
    Unchanged,
    Replaced(T),
    Spliced(Vec<T>),
    // For topdown: Skip(T) - skip children
}
```

#### Topdown Traversal

```rust
impl LuaFilter {
    pub fn apply_topdown(&self, lua: &Lua, doc: Pandoc) -> LuaResult<Pandoc> {
        // Walk document depth-first
        // At each node, check for matching callback
        // If callback returns false, skip children

        let blocks = self.traverse_blocks_topdown(lua, doc.blocks)?;
        let meta = self.traverse_meta_topdown(lua, doc.meta)?;

        // Apply Pandoc filter if present
        let doc = Pandoc { meta, blocks };
        if let Some(callback) = self.callbacks.get("Pandoc") {
            self.apply_to_document(lua, doc, callback)
        } else {
            Ok(doc)
        }
    }

    fn traverse_blocks_topdown(&self, lua: &Lua, blocks: Blocks) -> LuaResult<Blocks> {
        let mut result = vec![];

        for block in blocks {
            let block_type = block.type_name();

            // Check for specific callback
            if let Some(callback) = self.callbacks.get(block_type) {
                match self.apply_block_callback(lua, block, callback)? {
                    TopdownResult::Unchanged(b) => {
                        result.push(self.traverse_block_children(lua, b)?);
                    }
                    TopdownResult::Replaced(b) => {
                        result.push(self.traverse_block_children(lua, b)?);
                    }
                    TopdownResult::Spliced(bs) => {
                        for b in bs {
                            result.push(self.traverse_block_children(lua, b)?);
                        }
                    }
                    TopdownResult::SkipChildren(b) => {
                        result.push(b);  // Don't recurse
                    }
                }
            } else if let Some(callback) = self.callbacks.get("Block") {
                // Generic Block callback
                // ... similar handling
            } else {
                result.push(self.traverse_block_children(lua, block)?);
            }
        }

        Ok(result)
    }
}
```

### 8. Global Variables

```rust
// src/lua/globals.rs

pub fn set_filter_globals(lua: &Lua, context: &FilterContext) -> LuaResult<()> {
    let globals = lua.globals();

    // FORMAT - target format string
    globals.set("FORMAT", context.target_format.clone())?;

    // PANDOC_VERSION - version as table {major, minor, patch}
    let version = lua.create_table()?;
    version.set(1, env!("CARGO_PKG_VERSION_MAJOR").parse::<i32>().unwrap_or(0))?;
    version.set(2, env!("CARGO_PKG_VERSION_MINOR").parse::<i32>().unwrap_or(0))?;
    version.set(3, env!("CARGO_PKG_VERSION_PATCH").parse::<i32>().unwrap_or(0))?;
    globals.set("PANDOC_VERSION", version)?;

    // PANDOC_API_VERSION - AST API version (use Pandoc's for compatibility)
    let api_version = lua.create_table()?;
    api_version.set(1, 1)?;
    api_version.set(2, 23)?;  // Match recent Pandoc
    api_version.set(3, 1)?;
    globals.set("PANDOC_API_VERSION", api_version)?;

    // PANDOC_SCRIPT_FILE - path to filter script
    globals.set("PANDOC_SCRIPT_FILE", context.script_path.display().to_string())?;

    // PANDOC_READER_OPTIONS - minimal for now
    let reader_opts = lua.create_table()?;
    globals.set("PANDOC_READER_OPTIONS", reader_opts)?;

    // PANDOC_WRITER_OPTIONS - minimal for now
    let writer_opts = lua.create_table()?;
    globals.set("PANDOC_WRITER_OPTIONS", writer_opts)?;

    Ok(())
}
```

### 9. CLI Integration

```rust
// In external_filters/mod.rs

impl ExternalFilter {
    pub fn apply(&self, doc: Pandoc, context: &FilterContext) -> Result<Pandoc, FilterError> {
        match self {
            ExternalFilter::Json(path) => {
                json_filter::apply_json_filter(doc, path, &context.target_format, context)
            }
            ExternalFilter::Lua(path) => {
                lua_filter::apply_lua_filter(doc, path, context)
            }
        }
    }
}

// src/lua/mod.rs
pub fn apply_lua_filter(
    doc: Pandoc,
    path: &Path,
    context: &FilterContext,
) -> Result<Pandoc, FilterError> {
    let engine = LuaFilterEngine::new()
        .map_err(|e| FilterError::LuaInit(e.to_string()))?;

    let mut lua_context = context.clone();
    lua_context.script_path = path.to_owned();
    engine.set_globals(&lua_context)
        .map_err(|e| FilterError::LuaGlobals(e.to_string()))?;

    let filter = engine.load_filter(path)
        .map_err(|e| FilterError::LuaLoad(path.to_owned(), e.to_string()))?;

    filter.apply(&engine.lua, doc)
        .map_err(|e| FilterError::LuaExec(path.to_owned(), e.to_string()))
}
```

## Implementation Phases

### Phase 1: Minimal Viable Lua Filters
- [ ] Add mlua dependency
- [ ] Implement basic marshaling for common types (Str, Para, Header, Link, Emph, Strong)
- [ ] Implement `pandoc.Str()`, `pandoc.Para()`, etc. constructors
- [ ] Implement typewise traversal (simplified)
- [ ] Add `--lua-filter` CLI argument
- [ ] Test with simple filter: uppercase all Str elements

### Phase 2: Complete Element Support
- [ ] Implement marshaling for all inline types
- [ ] Implement marshaling for all block types
- [ ] Implement Meta marshaling
- [ ] Add remaining constructors
- [ ] Implement list splicing semantics

### Phase 3: Full Traversal Support
- [ ] Complete typewise traversal (all element callbacks)
- [ ] Implement topdown traversal
- [ ] Add `traverse` field support
- [ ] Implement `Inlines` and `Blocks` callbacks

### Phase 4: Global Variables and Utilities
- [ ] Complete global variable support
- [ ] Add `pandoc.read()` (parse text)
- [ ] Add `pandoc.write()` (serialize to format)
- [ ] Add `pandoc.utils.stringify()` (common utility)

### Phase 5: Polish and Compatibility
- [ ] Test with real-world Pandoc filters
- [ ] Document compatibility differences
- [ ] Performance benchmarking
- [ ] Error message improvements

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_inline_roundtrip() {
    let lua = Lua::new();
    let inline = Inline::Str(pandoc::Str { value: "hello".into(), ... });
    let lua_value = inline.to_lua(&lua).unwrap();
    let back = Inline::from_lua(lua_value, &lua).unwrap();
    assert_eq!(inline.value(), back.value());
}

#[test]
fn test_constructor_str() {
    let lua = Lua::new();
    register_constructors(&lua, &lua.globals()).unwrap();
    let result: LuaTable = lua.load("return pandoc.Str('hello')").eval().unwrap();
    assert_eq!(result.get::<String>("t").unwrap(), "Str");
    assert_eq!(result.get::<String>("c").unwrap(), "hello");
}
```

### Integration Tests

```lua
-- tests/lua_filters/uppercase.lua
function Str(elem)
    return pandoc.Str(elem.c:upper())
end
```

```lua
-- tests/lua_filters/delete_headers.lua
function Header(elem)
    return {}  -- Delete all headers
end
```

```lua
-- tests/lua_filters/wrap_code.lua
function CodeBlock(elem)
    return pandoc.Div({elem}, {class = "code-wrapper"})
end
```

```rust
#[test]
fn test_lua_filter_uppercase() {
    let input = "Hello world";
    let doc = parse_qmd(input);
    let filtered = apply_lua_filter(&doc, "tests/lua_filters/uppercase.lua", &default_context())?;
    // Verify Str elements are uppercase
}
```

## Design Decisions

### 1. Tables vs UserData for AST Elements

**Decision:** Use tables

**Rationale:**
- Pandoc compatibility (filters expect tables)
- Easier modification in Lua
- Simpler implementation
- UserData would require more boilerplate

**Tradeoff:** Less type safety, but matches Pandoc behavior

### 2. Source Info Handling

**Decision:** Filters receive/produce elements without source info

**Rationale:**
- Pandoc filters don't have source info
- Filtering transforms content; source positions become invalid
- Simpler marshaling

**Tradeoff:** Round-trip through Lua loses source locations

### 3. Error Handling

**Decision:** Lua errors become FilterError variants

**Rationale:**
- Integrate with existing error reporting
- Don't crash on filter bugs
- Provide filter path in error messages

### 4. pandoc.utils Scope

**Decision:** Defer most pandoc.utils to later phases

**Rationale:**
- Core filter functionality first
- Many utils require additional infrastructure
- Can add incrementally based on user needs

Initial utils to include:
- `pandoc.utils.stringify()` - Very commonly used

## Compatibility Notes

### Differences from Pandoc Lua Filters

1. **No `pandoc.mediabag`** - We don't have media bag concept yet
2. **No `pandoc.system`** - System operations deferred
3. **Limited `pandoc.utils`** - Only essential functions initially
4. **No template operations** - `pandoc.template` deferred
5. **Source info not available** - Unlike our JSON, Lua sees no positions

### Compatible Features

1. Element construction: `pandoc.Str()`, `pandoc.Para()`, etc.
2. Return semantics: nil, element, list, empty list
3. Traversal modes: typewise, topdown
4. Global variables: FORMAT, PANDOC_VERSION, PANDOC_API_VERSION

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| mlua API complexity | Dev time | Start minimal, expand incrementally |
| Pandoc filter incompatibility | User frustration | Document differences clearly |
| Memory overhead | Large docs | Benchmark; consider streaming |
| Lua security | Arbitrary code | Document that filters are trusted |
| Error message quality | Debug difficulty | Include Lua stack traces |

## Dependencies

- `mlua = "0.11"` with features `["lua54", "vendored", "send"]`
- No other new dependencies

## References

- mlua documentation: `external-sources/mlua/`
- Pandoc Lua filter docs: `external-sources/pandoc/doc/lua-filters.md`
- hslua implementation: `external-sources/hslua/`
- Explorer notes: `FILTER_SUMMARY.md`, mlua exploration results
