# Pandoc Lua API Port Plan

**Date**: 2025-12-02
**Related Issue**: k-409 (Lua subsystem work)
**Epic Issue**: k-473

## Executive Summary

This document outlines a comprehensive plan for porting the Pandoc Lua API to our Rust-based quarto-markdown-pandoc crate using mlua. The goal is **full Pandoc API compatibility** since Quarto implicitly promises the complete Pandoc filter API to users.

---

## Current Implementation Status

### Already Implemented

**Core Filter Infrastructure** (`filter.rs`):
- Filter execution engine with typewise traversal (default)
- Top-down traversal mode via `traverse = "topdown"`
- Filter return semantics: `nil`=unchanged, element=replace, list=splice, `{}`=delete
- `Blocks` and `Inlines` list-level filters
- Multiple filter application in sequence

**Element Userdata** (`types.rs`):
- `LuaInline` - wrapper for all inline elements
- `LuaBlock` - wrapper for all block elements
- `LuaAttr` - wrapper for attributes
- All support: field access, `tag`/`t`, `clone()`, `walk()`, `pairs()`

**Element Constructors** (`constructors.rs`):
- Most inline constructors (missing: `Cite`)
- Most block constructors (missing: `Table`, `DefinitionList`, `LineBlock`, `Figure`)

**Utility Functions** (`utils.rs`):
- `pandoc.utils.stringify()`

**Global Variables**:
- `FORMAT`, `PANDOC_VERSION`, `PANDOC_API_VERSION`, `PANDOC_SCRIPT_FILE`

---

## Deep Dive: List/Blocks/Inlines Architecture

Based on analysis of `pandoc-lua-marshal` and `hslua-list`:

### How Pandoc Implements Lists

Pandoc's List types are **regular Lua tables with metatables**, not userdata:

```
┌─────────────────────────────────────────────────────────────┐
│                    "List" metatable                          │
│  (base type from hslua-list/cbits/listmod.c)                │
├─────────────────────────────────────────────────────────────┤
│ Methods (C implementation):                                  │
│   __concat, __eq, __tostring                                │
│   at, clone, extend, filter, find, find_if                  │
│   includes, iter, map                                        │
│ Inherited from table module:                                 │
│   insert, remove, sort                                       │
└─────────────────────────────────────────────────────────────┘
                           │
                           │ extends
                           ▼
┌─────────────────────────────────────────────────────────────┐
│                  "Inlines" metatable                         │
│  (from pandoc-lua-marshal/Marshal/Inline.hs)                │
├─────────────────────────────────────────────────────────────┤
│ Additional methods:                                          │
│   walk(filter)  - apply filter to contained elements        │
│   clone()       - deep copy returning Inlines               │
│   __tostring    - native Haskell representation             │
│   __tojson      - JSON representation                       │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│                   "Blocks" metatable                         │
│  (from pandoc-lua-marshal/Marshal/Block.hs)                 │
├─────────────────────────────────────────────────────────────┤
│ Same structure as Inlines                                    │
└─────────────────────────────────────────────────────────────┘
```

### Key Implementation Details

1. **`pushInlines`/`pushBlocks`** creates a regular Lua table via `pushList`, then attaches the specialized metatable

2. **Metatable inheritance**: `newListMetatable "Inlines"` creates a metatable that inherits from "List" base

3. **Lazy evaluation**: HsLua supports lazy Haskell lists via a `__lazylist` uservalue that stores unevaluated tail

4. **Type coercion**: `peekInlinesFuzzy` accepts:
   - A table of Inline userdata
   - A string (converted to `Str` elements via `B.text`)
   - A single Inline userdata (wrapped in a list)

### Rust Implementation Strategy

**Option A: Pure metatable approach** (recommended for compatibility)
```rust
fn push_inlines(lua: &Lua, inlines: Vec<Inline>) -> Result<Value> {
    let table = lua.create_sequence_from(
        inlines.into_iter().map(|i| LuaInline(i))
    )?;

    // Get or create "Inlines" metatable
    let mt = get_or_create_inlines_metatable(lua)?;
    table.set_metatable(Some(mt));

    Ok(Value::Table(table))
}
```

**Option B: Hybrid approach**
- Use tables for content fields (like Pandoc)
- Implement List methods in Rust via metatable functions
- Avoid userdata for lists to maintain Pandoc semantics

### List Methods to Implement

| Method | Signature | Notes |
|--------|-----------|-------|
| `at(i, default?)` | `(int, any?) -> any` | Get with default |
| `clone()` | `() -> List` | Shallow copy |
| `extend(list)` | `(List) -> self` | Append in-place |
| `filter(fn)` | `((item, i) -> bool) -> List` | New filtered list |
| `find(needle, start?)` | `(any, int?) -> any, int` | Returns item and index |
| `find_if(fn, start?)` | `((item, i) -> bool, int?) -> any, int` | Find by predicate |
| `includes(value)` | `(any) -> bool` | Membership test |
| `insert(pos?, value)` | `(int?, any)` | From table module |
| `iter(step?, start?)` | `(int?, int?) -> iterator` | Value iterator |
| `map(fn)` | `((item, i) -> any) -> List` | Transform elements |
| `remove(pos)` | `(int) -> any` | From table module |
| `sort(fn?)` | `((a, b) -> bool?)` | From table module |
| `__concat` | `(List, List) -> List` | Concatenation |
| `__eq` | `(List, List) -> bool` | Deep equality |
| `__tostring` | `() -> string` | String representation |

---

## Runtime Abstraction Layer Design

### Problem Statement

The Lua subsystem needs to support multiple runtime environments:
- **Native**: Full file system and network access
- **WASM/Emscripten**: Emscripten-provided file primitives, `fetch()` for network
- **Sandboxed**: Restricted access for untrusted filters

### Proposed Architecture: Dependency Injection

```rust
/// Trait defining low-level runtime operations
pub trait LuaRuntime: Send + Sync {
    // File system operations
    fn read_file(&self, path: &Path) -> Result<Vec<u8>>;
    fn write_file(&self, path: &Path, contents: &[u8]) -> Result<()>;
    fn file_exists(&self, path: &Path) -> bool;
    fn list_directory(&self, path: &Path) -> Result<Vec<PathBuf>>;
    fn create_directory(&self, path: &Path) -> Result<()>;
    fn remove_file(&self, path: &Path) -> Result<()>;
    fn remove_directory(&self, path: &Path, recursive: bool) -> Result<()>;

    // Network operations
    fn fetch_url(&self, url: &str) -> Result<(Vec<u8>, String)>; // (content, mime)

    // Environment
    fn get_env(&self, name: &str) -> Option<String>;
    fn get_cwd(&self) -> Result<PathBuf>;
    fn set_cwd(&self, path: &Path) -> Result<()>;
    fn get_os(&self) -> &'static str;
    fn get_arch(&self) -> &'static str;

    // Temporary directories
    fn create_temp_directory(&self, template: &str) -> Result<PathBuf>;
}

/// Native implementation using std::fs
pub struct NativeRuntime {
    security_policy: SecurityPolicy,
}

/// WASM implementation using Emscripten/JS interop
#[cfg(target_arch = "wasm32")]
pub struct WasmRuntime {
    // JS function references for fetch(), file operations
}

/// Sandboxed implementation for untrusted filters
pub struct SandboxedRuntime {
    allowed_paths: Vec<PathBuf>,
    allow_network: bool,
    base_runtime: Box<dyn LuaRuntime>,
}
```

### Integration with FilterContext

```rust
pub struct FilterContext {
    pub format: String,
    pub input_file: Option<PathBuf>,
    pub resource_path: Vec<PathBuf>,
    pub mediabag: MediaBag,
    pub runtime: Arc<dyn LuaRuntime>,  // NEW: runtime abstraction
}
```

### Module Implementation Pattern

```rust
// In pandoc.system module
pub fn register_system_module(
    lua: &Lua,
    runtime: Arc<dyn LuaRuntime>
) -> Result<()> {
    let system = lua.create_table()?;

    let rt = runtime.clone();
    system.set("get_working_directory", lua.create_function(move |_, ()| {
        rt.get_cwd()
            .map(|p| p.to_string_lossy().to_string())
            .map_err(|e| mlua::Error::runtime(e.to_string()))
    })?)?;

    // ... other functions

    Ok(())
}
```

---

## Citeproc Integration

### Current State

`quarto-citeproc` provides:
```rust
pub struct Processor { ... }

impl Processor {
    pub fn new(style: Style) -> Self;
    pub fn add_reference(&mut self, reference: Reference);
    pub fn process_citation(&self, citation: &Citation) -> Result<Vec<Inline>>;
    pub fn generate_bibliography(&self) -> Result<Vec<Block>>;
}
```

### Pandoc's `pandoc.utils.citeproc` API

```lua
-- Process citations in a document
local doc = pandoc.utils.citeproc(doc)

-- Returns document with:
-- - Cite elements replaced with formatted citations
-- - Bibliography added to the end
```

### Integration Design

```rust
// In pandoc.utils module
fn citeproc(lua: &Lua, doc: LuaPandoc) -> Result<LuaPandoc> {
    // 1. Extract references from doc.meta.references
    // 2. Extract CSL style from doc.meta.csl or use default
    // 3. Create Processor
    // 4. Walk document, collecting citations
    // 5. Process all citations
    // 6. Replace Cite elements with formatted inlines
    // 7. Add bibliography to doc.blocks
    // 8. Return modified document
}
```

### Open Questions for Citeproc

1. How to handle missing references gracefully?
2. Should we cache the Processor across filter invocations?
3. How to integrate with Quarto's bibliography handling?

---

## Reader/Writer Functions

### API Surface

```lua
-- Parse a string into a Pandoc document
local doc = pandoc.read(content, "markdown")
local doc = pandoc.read(content, "html", {standalone = true})

-- Write a Pandoc document to a string
local html = pandoc.write(doc, "html")
local md = pandoc.write(doc, "markdown", {columns = 80})
```

### Implementation Challenges

1. **Format support**: We currently only have QMD reader. Need to:
   - Implement HTML reader (or use external library)
   - Implement other readers as needed
   - Leverage existing writers in quarto-markdown-pandoc

2. **Options handling**: Reader/writer options need to be marshalled

3. **Circular dependency**: `pandoc.read` needs to invoke our own parser

### Proposed Approach

```rust
// Initial implementation: limited format support
fn pandoc_read(lua: &Lua, (content, format, opts): (String, String, Option<Table>))
    -> Result<LuaPandoc>
{
    match format.as_str() {
        "markdown" | "qmd" => {
            // Use our parser
            let pandoc = parse_qmd(&content)?;
            Ok(LuaPandoc(pandoc))
        }
        "json" => {
            // Parse Pandoc JSON
            let pandoc: Pandoc = serde_json::from_str(&content)?;
            Ok(LuaPandoc(pandoc))
        }
        _ => Err(mlua::Error::runtime(
            format!("Unsupported input format: {}", format)
        ))
    }
}

fn pandoc_write(lua: &Lua, (doc, format, opts): (LuaPandoc, String, Option<Table>))
    -> Result<String>
{
    match format.as_str() {
        "json" => Ok(serde_json::to_string(&doc.0)?),
        "native" => Ok(format!("{:#?}", doc.0)),
        "html" | "html5" => write_html(&doc.0, opts),
        // ... other formats
        _ => Err(mlua::Error::runtime(
            format!("Unsupported output format: {}", format)
        ))
    }
}
```

---

## Updated Implementation Phases

### Phase 1: List Infrastructure & Missing Constructors

**Goal**: Complete the element type system

1. **List metatable system**
   - Implement base "List" metatable with all methods
   - Implement "Inlines" metatable extending List
   - Implement "Blocks" metatable extending List
   - Update `content` field getters to return tables with metatables

2. **Missing constructors**
   - `Cite(content, citations)`
   - `Table(caption, colspecs, head, bodies, foot, attr)`
   - `TableHead`, `TableBody`, `TableFoot`, `Row`, `Cell`
   - `DefinitionList(items)`
   - `LineBlock(lines)`
   - `Figure(content, caption, attr)`
   - `Caption(short, long)`
   - `ListAttributes(start, style, delim)`
   - `Citation(id, mode, prefix?, suffix?, note_num?, hash?)`

### Phase 2: Core Utility Modules

**Goal**: Essential utilities for most filters

1. **pandoc.utils expansion**
   - `blocks_to_inlines(blocks, sep?)`
   - `equals(elem1, elem2)`
   - `type(value)`
   - `sha1(string)`
   - `normalize_date(string)`

2. **pandoc.text**
   - `lower(str)`, `upper(str)` - Unicode-aware
   - `len(str)`, `sub(str, i, j?)` - Unicode operations

3. **pandoc.json**
   - `decode(str)`, `encode(value)`

### Phase 3: Runtime Abstraction & System Modules

**Goal**: Cross-platform runtime support

1. **LuaRuntime trait** - Define abstraction layer
2. **NativeRuntime** - std::fs implementation
3. **pandoc.path** - Path manipulation
4. **pandoc.system** - System operations via runtime

### Phase 4: MediaBag & Network

**Goal**: Media and network handling

1. **pandoc.mediabag** - All functions
2. **WasmRuntime** (conditional) - Emscripten/fetch support

### Phase 5: Citeproc Integration

**Goal**: Citation processing

1. **pandoc.utils.citeproc** - Full document processing
2. **pandoc.utils.references** - Extract references from doc

### Phase 6: Reader/Writer & Advanced

**Goal**: Full API parity

1. **pandoc.read** - Format parsing
2. **pandoc.write** - Format rendering
3. **pandoc.template** - Template processing
4. **pandoc.layout** - Custom writer support
5. **pandoc.zip** - Archive handling

---

## Testing Strategy

### Unit Tests
- Each List method
- Each constructor with various argument combinations
- Each utility function

### Compatibility Tests
Create test suite comparing our output with Pandoc's:
```lua
-- test-compatibility.lua
local result = pandoc.utils.stringify(pandoc.Emph{pandoc.Str "hello"})
assert(result == "hello", "stringify mismatch")
```

### Integration Tests
- Run Quarto's built-in filters
- Run community filters from awesome-quarto

---

## Dependencies

| Crate | Purpose |
|-------|---------|
| `unicode-segmentation` | Unicode text operations |
| `serde_json` | JSON handling |
| `sha1` | SHA1 hashing |
| `encoding_rs` | Text encoding conversion |
| `zip` | Archive handling |
| `reqwest` (optional) | HTTP fetching for native |

---

## Decisions Needed

1. **List implementation**: Pure metatable vs hybrid? (Recommend: pure metatable for compatibility)

2. **Runtime abstraction priority**: Start with native-only or design abstraction first? (Recommend: design trait first, implement native, defer WASM)

3. **Citeproc scope**: Standalone citeproc design session needed?

4. **Reader/Writer formats**: Which formats beyond QMD/JSON are essential?

---

## Next Steps

1. Create sub-issues under k-473 for each phase
2. Begin Phase 1: List infrastructure
3. Schedule design session for runtime abstraction
4. Schedule design session for citeproc integration
