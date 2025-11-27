# Pandoc Filter Architecture Analysis

## Executive Summary

Pandoc implements a sophisticated two-tier filter system:
1. **JSON Filters** - External processes communicating via JSON-formatted AST over stdin/stdout
2. **Lua Filters** - Embedded Lua interpreter with direct AST marshaling
3. **Citeproc Filter** - Built-in citation processor

All filters are applied sequentially in the order specified on the command line, with each filter's output becoming the input to the next.

---

## Part 1: JSON Filter Protocol

### Overview
JSON filters are external processes that read a JSON representation of the Pandoc AST from stdin and write a modified JSON AST to stdout.

### Data Format

**Input/Output Format:**
- JSON encoding of the `Pandoc` document structure
- The AST is serialized using Haskell's `Data.Aeson` library
- Binary representation is efficient UTF-8 encoded JSON

**Pandoc AST Structure (simplified):**
```
Pandoc
├── Meta (metadata)
└── [Block] (list of block elements)
    ├── Header
    ├── Para
    ├── CodeBlock
    ├── BlockQuote
    └── ... (other block types)

Block elements contain [Inline] elements:
├── Str
├── Emph
├── Strong
├── Link
├── Image
└── ... (other inline types)
```

### Communication Protocol

**Process Invocation:**
```haskell
-- File: src/Text/Pandoc/Filter/JSON.hs (lines 35-82)
apply :: MonadIO m
      => Environment
      -> [String]
      -> FilePath
      -> Pandoc
      -> m Pandoc
apply ropts args f = liftIO . externalFilter ropts f args
```

**Execution Steps:**

1. **Filter Discovery** (File extension mapping - lines 50-61)
   ```
   If file is not executable, pandoc guesses interpreter from extension:
   
   .py  → python <filter.py>
   .hs  → runhaskell <filter.hs>
   .pl  → perl <filter.pl>
   .rb  → ruby <filter.rb>
   .php → php <filter.php>
   .js  → node <filter.js>
   .r   → Rscript <filter.r>
   
   Otherwise: executes directly with ./ prefix
   ```

2. **Environment Variables** (Lines 67-71)
   ```
   PANDOC_VERSION: version string (e.g., "2.11.1")
   PANDOC_READER_OPTIONS: JSON object containing reader configuration
   ```

3. **AST Serialization** (Line 73)
   - Document encoded to JSON using `Data.Aeson.encode`
   - JSON written to filter's stdin as binary ByteString

4. **Filter Execution** (Line 72-73)
   ```haskell
   (exitcode, outbs) <- pipeProcess env' f' args'' $ encode d
   ```
   - Filter runs with args: `[target_format, ...additional_args]`
   - Input: JSON AST on stdin
   - Output: JSON AST on stdout
   - Stderr: Filter error messages

5. **Result Processing** (Lines 74-78)
   ```haskell
   case exitcode of
     ExitSuccess    → Deserialize JSON output using eitherDecode'
     ExitFailure ec → Throw PandocFilterError with exit code
   ```

### Environment Variables Passed to JSON Filters

**PANDOC_READER_OPTIONS** JSON structure:
```json
{
  "abbreviations": ["string array"],
  "columns": 80,
  "default-image-extension": ".png",
  "extensions": 12345,  // bitfield of extensions
  "indented-code-classes": [],
  "standalone": false,
  "strip-comments": false,
  "tab-stop": 4,
  "track-changes": "accept-changes"|"reject-changes"|"all-changes"
}
```

### Error Handling
- If filter is not found: `PandocFilterError` with "Could not find executable"
- If filter returns non-zero exit code: `PandocFilterError` with "Filter returned error status N"
- If JSON parsing fails: `PandocFilterError` with parsing error message

---

## Part 2: Lua Filter Architecture

### Overview
Lua filters use an embedded Lua 5.4 interpreter built directly into Pandoc. They avoid JSON serialization overhead by marshaling AST elements directly into Lua tables.

### Filter Structure

**File: pandoc-lua-engine/src/Text/Pandoc/Lua/Filter.hs (lines 24-71)**

**Filter Initialization:**
```lua
-- A Lua filter is a table with element names as keys:
return {
  -- Option 1: Named filter functions
  Strong = function (elem)
    return pandoc.SmallCaps(elem.content)
  end,
  
  -- Option 2: Traverse order (added pandoc 2.17)
  traverse = 'typewise',  -- or 'topdown'
  
  -- Option 3: List filters (deprecated but still supported)
  -- return { filter1, filter2, filter3 }
}

-- Option 4: Implicit global filter (if nothing returned)
function Strong(elem)
  return pandoc.SmallCaps(elem.content)
end
```

**Return Value Semantics:**
- `nil` → Element unchanged
- Pandoc object (same type) → Replaces original
- List of objects (same type) → Splices into parent list
- Empty list → Deletes element
- Type mismatch → Error

### Lua Execution Pipeline

**File: pandoc-lua-engine/src/Text/Pandoc/Lua/Engine.hs (lines 49-73)**

**Step 1: Environment Setup**
```haskell
applyFilter :: (PandocMonad m, MonadIO m)
            => Environment
            -> [String]
            -> FilePath
            -> Pandoc
            -> m Pandoc
applyFilter fenv args fp doc = do
  let globals = 
    [ FORMAT $ T.pack target_format
    , PANDOC_READER_OPTIONS (envReaderOptions fenv)
    , PANDOC_WRITER_OPTIONS (envWriterOptions fenv)
    , PANDOC_SCRIPT_FILE fp
    ]
  runLua >=> forceResult fp $ do
    setGlobals globals
    runFilterFile fp doc
```

**Global Variables Set (lines 56-62):**
- `FORMAT`: Target format (e.g., "html5", "latex")
- `PANDOC_READER_OPTIONS`: ReaderOptions table
- `PANDOC_WRITER_OPTIONS`: WriterOptions table
- `PANDOC_SCRIPT_FILE`: Path to filter file
- `PANDOC_VERSION`: Version object
- `PANDOC_API_VERSION`: Pandoc types version
- `PANDOC_STATE`: Read-only CommonState

**Step 2: Filter Loading and Execution**

**File: pandoc-lua-engine/src/Text/Pandoc/Lua/Filter.hs (lines 26-52)**

```haskell
runFilterFile :: FilePath -> Pandoc -> LuaE PandocError Pandoc
runFilterFile filterPath doc = do
  Lua.pushglobaltable
  runFilterFile' Lua.top filterPath doc <* Lua.pop 1

runFilterFile' :: StackIndex -> FilePath -> Pandoc
               -> LuaE PandocError Pandoc
runFilterFile' envIdx filterPath doc = do
  oldtop <- gettop
  stat <- dofileTrace' envIdx (Just filterPath)  -- Load and execute file
  
  if stat /= OK
    then throwErrorAsException
    else do
      newtop <- gettop
      -- Determine filter(s) to apply
      luaFilters ← forcePeek $
        if newtop - oldtop >= 1
        then liftLua (rawlen top) >>= \case
          0 → (:[]) <$!> peekFilter top        -- Single filter returned
          _ → peekList peekFilter top          -- List of filters returned
        else (:[]) <$!> peekFilter envIdx      -- Implicit global filter
      
      settop oldtop
      runAll luaFilters doc

runAll :: [Filter] -> Pandoc -> LuaE PandocError Pandoc
runAll = foldr ((>=>) . applyFully) return
```

**Process:**
1. Push global table onto Lua stack
2. Load and execute filter script in that environment
3. Check if script returned a value:
   - No return → Look for globally-defined filter functions
   - Return table → Check if it's a filter or list of filters
   - Single filter → Wrap in list
4. Apply all filters sequentially using `applyFully`

### Filter Execution Order

**File: doc/lua-filters.md (lines 182-247)**

**Two Traversal Modes:**

#### Typewise Traversal (Default, pandoc ≥2.17)
```
Order of execution (skip missing functions):
1. Inline element filters (Str, Emph, Strong, Link, Image, etc.)
2. Inlines filter (operates on inline element lists)
3. Block element filters (Para, Header, CodeBlock, etc.)
4. Blocks filter (operates on block element lists)
5. Meta filter
6. Pandoc filter
```

**Example:**
```lua
-- This processes elements in fixed order:
return {
  Str = function(e) ... end,      -- #1
  Strong = function(e) ... end,   -- #1
  Inlines = function(list) ... end, -- #2
  Para = function(e) ... end,     -- #3
  Header = function(e) ... end,   -- #3
  Blocks = function(list) ... end, -- #4
  Meta = function(m) ... end,     -- #5
  Pandoc = function(d) ... end    -- #6
}
```

#### Topdown Traversal (pandoc ≥2.17)
```lua
traverse = 'topdown'
```
- Depth-first traversal from root to leaves
- Single pass through tree
- Functions called in visit order:
  - Parent before children
  - Can return `false` as second value to skip children

**Example:**
```
For: [Plain [Str "a"], Para [Str "b"]]
Order:
  Blocks (list)       → blocks filter
    Plain (elem)      → Plain filter
      Inlines (list)  → Inlines filter
        Str (elem)    → Str filter
    Para (elem)       → Para filter
      Inlines (list)  → Inlines filter
        Str (elem)    → Str filter
```

### AST Marshaling

**File: pandoc-lua-engine/src/Text/Pandoc/Lua/Orphans.hs**

Lua objects are created as Haskell userdata with metatables:
```haskell
-- Pushable instances convert Haskell values to Lua:
instance Pushable Pandoc where push = pushPandoc
instance Pushable Meta where push = pushMeta
instance Pushable Block where push = pushBlock
instance Pushable Inline where push = pushInline
-- ... etc for all AST types
```

**How Elements Appear in Lua:**
```lua
-- Str element in Lua:
{
  t = "Str",           -- Element type
  text = "hello"       -- Content
}

-- Para element in Lua:
{
  t = "Para",
  content = {          -- List of Inline elements
    { t = "Str", text = "Hello " },
    { t = "Str", text = "world" }
  }
}

-- Can also construct using pandoc module:
pandoc.Str("hello")      -- returns Str element
pandoc.Para({...})       -- returns Para element
```

---

## Part 3: Filter Composition and Execution Order

### Overall Pipeline

**File: src/Text/Pandoc/Filter.hs (lines 76-101)**

```haskell
applyFilters :: (PandocMonad m, MonadIO m)
             => ScriptingEngine
             -> Environment
             -> [Filter]        -- List of filters to apply
             -> [String]        -- Args (e.g., [format])
             -> Pandoc
             -> m Pandoc
applyFilters scrngin fenv filters args d = do
  expandedFilters ← mapM expandFilterPath filters
  foldM applyFilter d expandedFilters
  
 where
  applyFilter doc (JSONFilter f) =
    withMessages f $ JSONFilter.apply fenv args f doc
  
  applyFilter doc (LuaFilter f) =
    withMessages f $ engineApplyFilter scrngin fenv args f doc
  
  applyFilter doc CiteprocFilter =
    withMessages "citeproc" $ processCitations doc
```

**Key Characteristics:**
1. **Sequential Application**: `foldM applyFilter` applies filters left-to-right
2. **Filter Chaining**: Output of filter N becomes input of filter N+1
3. **Three Filter Types**:
   - JSON filters: External process via pipes
   - Lua filters: Embedded interpreter
   - Citeproc: Built-in citation processor
4. **Mixed Types**: Can mix JSON and Lua filters in same command

### Complete Processing Pipeline

```
Source Document
      ↓
[Reader] → AST
      ↓
[Filter 1 (could be JSON or Lua or citeproc)]
      ↓
[Modified AST]
      ↓
[Filter 2]
      ↓
[Modified AST]
      ↓
... (repeat for each filter in order)
      ↓
[Final AST]
      ↓
[Writer] → Output Document
```

### Filter Discovery

**File: src/Text/Pandoc/Filter.hs (lines 109-110)**

```haskell
filterPath :: PandocMonad m => FilePath -> m FilePath
filterPath fp = fromMaybe fp <$> findFileWithDataFallback "filters" fp
```

**Search Order:**
1. Absolute path or relative path as specified
2. `$DATADIR/filters/` (user data directory)
3. `$PATH` (for JSON filters only; must be executable)

---

## Part 4: Performance and Advantages

### JSON Filter Overhead

From doc/lua-filters.md (lines 63-76):
```
Time to convert MANUAL.txt to HTML:
  pandoc                              1.01s
  pandoc --filter ./smallcaps          1.36s  (compiled Haskell)
  pandoc --filter ./smallcaps.py       1.40s  (interpreted Python)
  pandoc --lua-filter ./smallcaps.lua  1.03s  (embedded Lua)
```

**Overhead Sources:**
- JSON serialization/deserialization on every filter
- Process creation and IPC overhead
- No direct AST access

### Lua Filter Advantages

1. **No External Dependencies**: Lua 5.4 built into Pandoc
2. **Direct AST Marshaling**: No JSON serialization
3. **Better Performance**: ~3-35% faster than JSON filters
4. **Richer Environment**: Access to pandoc module and utilities
5. **Single Process**: No fork/pipe overhead

---

## Part 5: Citeproc Integration

**File: src/Text/Pandoc/Citeproc.hs**

Citeproc is a built-in filter that processes citations:
- Parses citation references in text
- Looks up bibliography entries
- Formats citations according to CSL style
- No JSON serialization (built-in)

Can be specified as:
- Command line: `--citeproc` or explicitly in filter list
- Within filter sequence: `pandoc input.md --filter citeproc output.html`

---

## Part 6: Module and API Structure

### Pandoc Lua Module (`pandoc`)

**Available Functions:**
- Element constructors: `Str`, `Para`, `Header`, `Link`, `Image`, etc.
- Utility functions:
  - `walk_block(elem, filter)` - Apply filter to block
  - `walk_inline(elem, filter)` - Apply filter to inline
  - `read(text, format)` - Parse text into Pandoc
  - `pipe(command, args, input)` - Run external command
- Submodules:
  - `pandoc.mediabag` - Access embedded media
  - `pandoc.utils` - Utility functions
  - `pandoc.text` - Unicode string functions
  - `pandoc.layout` - Document layout
  - ... and more

### Available Lua Libraries

- `lpeg` - Parsing Expression Grammars
- `re` - Regex engine (built on lpeg)
- Standard Lua libraries: `string`, `table`, `math`, etc.

---

## Summary Table

| Aspect | JSON Filters | Lua Filters | Citeproc |
|--------|---|---|---|
| **Language** | Any | Lua 5.4 | Built-in |
| **Execution** | External process | Embedded VM | Built-in |
| **Communication** | stdin/stdout JSON | Direct marshal | Direct marshal |
| **Performance** | Slower (3-35% overhead) | Faster | Fast (built-in) |
| **Dependencies** | Language-specific | None | None |
| **Flexibility** | Very high | High | Fixed (citations) |
| **Invocation** | `--filter` | `--lua-filter` | `--citeproc` |
| **Application Order** | Sequential, left-to-right | Sequential, left-to-right | Sequential |

---

## Key Insights for Implementation

1. **AST-Centric Design**: All filters operate on the Pandoc AST, not raw text
2. **Filter Composition**: Multiple filters compose cleanly via sequential piping
3. **Type Safety**: Lua filters maintain type correctness through marshaling
4. **Mixed Types**: Can freely mix JSON and Lua filters in single pipeline
5. **Execution Semantics**: Filters are applicative functors (foldM pattern)
6. **Traversal Control**: Lua filters can choose depth-first or breadth-first traversal
7. **Extensibility**: New filter types can be added by implementing the Filter type and applyFilter handler

