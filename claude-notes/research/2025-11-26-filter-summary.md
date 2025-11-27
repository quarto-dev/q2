# Pandoc Filter Architecture - Quick Reference

## Key Files Referenced

### JSON Filters
- **Primary Implementation:** `/external-sources/pandoc/src/Text/Pandoc/Filter/JSON.hs`
- **Filter Type Definition:** `/external-sources/pandoc/src/Text/Pandoc/Filter.hs` (lines 40-66)
- **Main Entry Point:** `apply :: MonadIO m => Environment -> [String] -> FilePath -> Pandoc -> m Pandoc`
- **Documentation:** `/external-sources/pandoc/doc/filters.md`

### Lua Filters
- **Filter Loading:** `/external-sources/pandoc/pandoc-lua-engine/src/Text/Pandoc/Lua/Filter.hs`
- **Engine Interface:** `/external-sources/pandoc/pandoc-lua-engine/src/Text/Pandoc/Lua/Engine.hs`
- **Module Setup:** `/external-sources/pandoc/pandoc-lua-engine/src/Text/Pandoc/Lua/Module.hs`
- **AST Marshaling:** `/external-sources/pandoc/pandoc-lua-engine/src/Text/Pandoc/Lua/Orphans.hs`
- **Documentation:** `/external-sources/pandoc/doc/lua-filters.md`

### Filter Composition
- **Filter Application:** `/external-sources/pandoc/src/Text/Pandoc/Filter.hs` (lines 76-101)
- **Filter Types:** `/external-sources/pandoc/src/Text/Pandoc/Filter.hs` (lines 40-66)

## Three Filter Types

### 1. JSON Filters
**What:** External processes that read/write JSON AST

**Protocol:**
```
stdin:  Pandoc AST as JSON
stdout: Modified Pandoc AST as JSON
args:   [target_format, ...]
env:    PANDOC_VERSION, PANDOC_READER_OPTIONS
```

**Language Support:** Any language with JSON library
- Python (most common)
- Haskell, Perl, Ruby, PHP, JavaScript/Node, R

**Performance:** 35-40% overhead vs baseline (JSON marshaling + IPC)

**File:** `src/Text/Pandoc/Filter/JSON.hs`

### 2. Lua Filters
**What:** Embedded Lua 5.4 interpreter with direct AST marshaling

**Structure:**
```lua
return {
  Str = function(elem) ... end,
  Para = function(elem) ... end,
  traverse = 'typewise'  -- or 'topdown'
}
```

**Performance:** Negligible overhead vs baseline (no serialization)

**File:** `pandoc-lua-engine/src/Text/Pandoc/Lua/Filter.hs`

### 3. Citeproc Filter
**What:** Built-in citation processor (no external process needed)

**Functionality:** Resolves bibliography references and formats citations

**Performance:** Built-in (no process overhead)

**File:** `src/Text/Pandoc/Citeproc.hs`

## Filter Execution Model

**Pattern:** Sequential left-to-right application (fold)

```haskell
applyFilters :: ScriptingEngine -> Environment -> [Filter] -> [String] -> Pandoc -> m Pandoc
applyFilters scrngin fenv filters args d = do
  expandedFilters <- mapM expandFilterPath filters
  foldM applyFilter d expandedFilters
```

**Key Properties:**
1. Output of filter N becomes input of filter N+1
2. All filters see the progressively transformed AST
3. Can mix JSON, Lua, and built-in filters freely
4. Applied in command-line order

## JSON Filter Protocol Details

### Invocation
```haskell
-- File: src/Text/Pandoc/Filter/JSON.hs:35-82
apply :: MonadIO m => Environment -> [String] -> FilePath -> Pandoc -> m Pandoc
```

### Process Spawning
1. Check if filter is executable
2. If not executable, guess interpreter from file extension
3. Spawn subprocess with args: `[target_format, ...]`

### Environment Variables
```
PANDOC_VERSION = "2.19.2"
PANDOC_READER_OPTIONS = {
  "abbreviations": [...],
  "columns": 80,
  "default-image-extension": ".png",
  "extensions": 12345,
  "indented-code-classes": [],
  "standalone": false,
  "strip-comments": false,
  "tab-stop": 4,
  "track-changes": "accept-changes"
}
```

### Interpreter Mapping
```
.py  → python
.hs  → runhaskell
.pl  → perl
.rb  → ruby
.php → php
.js  → node
.r   → Rscript
```

### Error Handling
- Filter not found: `PandocFilterError` "Could not find executable"
- Non-zero exit code: `PandocFilterError` "Filter returned error status N"
- JSON parse error: `PandocFilterError` with parsing error message

## Lua Filter Execution Pipeline

### 1. Environment Setup
```haskell
-- File: pandoc-lua-engine/src/Text/Pandoc/Lua/Engine.hs:49-73
applyFilter fenv args fp doc = do
  let globals = 
    [ FORMAT target_format
    , PANDOC_READER_OPTIONS (envReaderOptions fenv)
    , PANDOC_WRITER_OPTIONS (envWriterOptions fenv)
    , PANDOC_SCRIPT_FILE fp
    ]
  runLua >=> forceResult fp $ do
    setGlobals globals
    runFilterFile fp doc
```

### 2. Filter Detection
- If script returns value → use that
- If list returned → apply each in sequence
- If nothing returned → collect global functions (Str, Para, Header, etc.)

### 3. Traversal Modes

#### Typewise (Default)
Order of filter application (skip missing):
1. Inline element filters (Str, Emph, Strong, Link, etc.)
2. Inlines filter (operates on inline lists)
3. Block element filters (Para, Header, CodeBlock, etc.)
4. Blocks filter (operates on block lists)
5. Meta filter
6. Pandoc filter

#### Topdown
- Depth-first from root to leaves
- Single pass through AST
- Can return `false` to skip children

## Return Value Semantics

| Return | Meaning |
|--------|---------|
| `nil` | Element unchanged |
| Same type element | Replaces original |
| List of same type | Splices into parent |
| Empty list `{}` | Deletes element |
| `false` (topdown) | Skip children |

## Filter Discovery

Search order:
1. Direct path (as specified)
2. `$DATADIR/filters/` user data directory
3. `$PATH` (executable-only, JSON filters)

## Pandoc Lua Module

**Available to filters:**
- Element constructors: `Str`, `Para`, `Header`, `Link`, `Image`, etc.
- Functions: `read()`, `pipe()`, `walk_block()`, `walk_inline()`
- Submodules: `pandoc.utils`, `pandoc.text`, `pandoc.mediabag`, `pandoc.layout`
- Libraries: `lpeg`, `re`, standard Lua libs

## Global Variables in Lua Filters

| Variable | Type | Example |
|----------|------|---------|
| `FORMAT` | string | `"html5"`, `"latex"` |
| `PANDOC_VERSION` | table | `{2, 19, 2}` |
| `PANDOC_API_VERSION` | table | `{1, 23, 1}` |
| `PANDOC_SCRIPT_FILE` | string | `/path/to/filter.lua` |
| `PANDOC_READER_OPTIONS` | table | Reader config |
| `PANDOC_WRITER_OPTIONS` | table | Writer config |
| `PANDOC_STATE` | table | CommonState (read-only) |

## AST Structure Example

```json
{
  "pandoc-api-version": [1, 23, 1],
  "meta": {
    "title": { "t": "MetaInlines", "c": [...] }
  },
  "blocks": [
    {
      "t": "Para",
      "c": [
        { "t": "Str", "c": "Hello" },
        { "t": "Space", "c": [] },
        { "t": "Str", "c": "world" }
      ]
    }
  ]
}
```

## Filter Type Definition

```haskell
data Filter = LuaFilter FilePath
            | JSONFilter FilePath
            | CiteprocFilter
            deriving (Show, Generic, Eq)
```

## Performance Comparison

| Implementation | Time | Overhead |
|---|---|---|
| pandoc (baseline) | 1.01s | - |
| Lua filter | 1.03s | +2% |
| Compiled Haskell (JSON) | 1.36s | +35% |
| Python (JSON) | 1.40s | +39% |

Source: `doc/lua-filters.md` (lines 63-76)

## Implementation Notes

1. **Filter composition is functional**: Uses `foldM` to thread AST through filters
2. **Type safety preserved**: Lua marshaling maintains AST types
3. **Lazy evaluation**: Filters only process what's needed
4. **Error handling**: PandocError wraps all failures
5. **Extensibility**: New filter types can be added by extending `Filter` type

## When to Use Each

**JSON Filters:**
- Need to write in specific language not supported by Lua
- Complex I/O or external library requirements
- Legacy code integration

**Lua Filters:**
- Performance-critical applications
- Want no external dependencies
- Need access to Pandoc's built-in utilities
- Prefer simpler, more maintainable code

**Citeproc:**
- Processing bibliography and citations
- CSL-based citation formatting

## Common Mistakes

1. **Not returning modified element in Lua**: Return nil is no-op
2. **Type mismatch**: Returning wrong element type causes error
3. **Assuming serial order**: Typewise mode processes by type, not position
4. **Modifying elements in-place**: Lua is immutable-style; must return new element
5. **Not considering Lua string limitations**: Use `pandoc.text` for Unicode

## References

- **Complete filter documentation**: `/external-sources/pandoc/doc/filters.md`
- **Lua filter guide**: `/external-sources/pandoc/doc/lua-filters.md`
- **Haskell source**: `/external-sources/pandoc/src/Text/Pandoc/Filter.hs`
- **Lua source**: `/external-sources/pandoc/pandoc-lua-engine/src/Text/Pandoc/Lua/`

