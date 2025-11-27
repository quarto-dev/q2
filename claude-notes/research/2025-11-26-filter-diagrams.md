# Pandoc Filter Architecture - Visual Diagrams

## 1. Overall Processing Pipeline

```
┌─────────────────────────────────────────────────────────────────┐
│                    Pandoc Document Processing                   │
└─────────────────────────────────────────────────────────────────┘

Input File (markdown, docx, etc.)
        │
        ▼
    [READER]
        │
        ▼
   AST (Pandoc)
        │
        ├──────────────────────────────────────────────┐
        │                                              │
        ▼                                              │
   [FILTER 1]                                          │
        │                                              │
        ▼                                              │
   AST (modified)                                      │
        │                                              │
        ▼                                              │
   [FILTER 2]                 Sequential application   │
        │                      (foldM pattern)         │
        ▼                                              │
   AST (modified)                                      │
        │                                              │
        ▼                                              │
   [FILTER N]                                          │
        │                                              │
        └──────────────────────────────────────────────┘
        │
        ▼
   AST (final)
        │
        ▼
    [WRITER]
        │
        ▼
Output File (html, latex, docx, etc.)
```

## 2. JSON Filter Communication

```
┌──────────────────────────────────────────────────────────────────┐
│                      JSON Filter Process                         │
└──────────────────────────────────────────────────────────────────┘

    PANDOC (main)                    FILTER (subprocess)
         │                                  │
         │  spawn process                   │
         ├─────────────────────────────────>│
         │                                  │
         │  [1] write JSON AST to stdin     │
         │                                  │
         ├─────stdin───────────────────────>│
         │                                  │
         │                    [2] parse JSON AST
         │                              │
         │                    [3] transform AST
         │                              │
         │                    [4] serialize to JSON
         │                              │
         │  [5] read JSON from stdout      │
         │<────stdout─────────────────────┤
         │                                  │
         │  [6] parse output JSON          │
         │                                  │
         ├─ wait for process ──────────────>│
         │                                  │
         │<──── exit code ────────────────┤
         │                                  │

Environment Variables Passed:
  • PANDOC_VERSION=2.x.x
  • PANDOC_READER_OPTIONS={JSON object with reader config}

Argument:
  • args[0] = target_format (e.g., "html5", "latex")
```

## 3. JSON Filter Data Format

```
INPUT/OUTPUT JSON Format

{
  "pandoc-api-version": [1, 23, 1],
  "meta": {
    "title": { "t": "MetaInlines", "c": [...] },
    "author": { "t": "MetaInlines", "c": [...] }
  },
  "blocks": [
    {
      "t": "Header",
      "c": [
        1,                           # level
        ["id", ["class1"], []],      # attributes
        [                            # content (Inline list)
          { "t": "Str", "c": "Hello" },
          { "t": "Space", "c": [] },
          { "t": "Str", "c": "World" }
        ]
      ]
    },
    {
      "t": "Para",
      "c": [
        { "t": "Str", "c": "Paragraph text" }
      ]
    }
  ]
}
```

## 4. Lua Filter Execution Flow

```
┌──────────────────────────────────────────────────────────────────┐
│                  Lua Filter Execution (Embedded)                 │
└──────────────────────────────────────────────────────────────────┘

applyFilter (Environment, Args, FilePath, Pandoc)
    │
    ├─ Set global variables:
    │  ├─ FORMAT = "html5" (or target format)
    │  ├─ PANDOC_READER_OPTIONS = {reader config}
    │  ├─ PANDOC_WRITER_OPTIONS = {writer config}
    │  ├─ PANDOC_SCRIPT_FILE = "/path/to/filter.lua"
    │  ├─ PANDOC_VERSION = {2, 19, 2}
    │  └─ pandoc = {module with constructors}
    │
    ▼
runLua (Lua interpreter initialization)
    │
    ├─ Initialize Lua 5.4 VM
    ├─ Load standard libraries
    ├─ Load pandoc module
    ├─ Load lpeg and re modules
    └─ Load any init.lua customizations
    │
    ▼
loadFile filter.lua
    │
    ├─ Parse Lua script
    ├─ Execute in Lua VM
    └─ Capture return value (if any)
    │
    ▼
peekFilter (extract filter from Lua stack)
    │
    ├─ Check if return value exists
    ├─ If list of filters: extract each
    ├─ If single filter: wrap in list
    └─ If nothing returned: extract global functions
    │         (Str, Para, Header, etc.)
    │
    ▼
runAll filters doc
    │
    ├─ Apply filter 1 to doc
    ├─ Apply filter 2 to result
    └─ ... continue through all filters
    │
    ▼
applyFully filter doc
    │
    ├─ Walk AST according to filter's traverse mode:
    │
    │  TYPEWISE (default):
    │  ├─ Apply Inline-element filters (Str, Emph, etc.)
    │  ├─ Apply Inlines filter to inline lists
    │  ├─ Apply Block-element filters (Para, Header, etc.)
    │  ├─ Apply Blocks filter to block lists
    │  ├─ Apply Meta filter
    │  └─ Apply Pandoc filter
    │
    │  TOPDOWN:
    │  └─ Depth-first traversal from root:
    │     Blocks → Plain → Inlines → Str → ... → Para → Inlines → Str
    │
    ▼
Modified Pandoc document
```

## 5. Lua Filter Structure Options

```
┌──────────────────────────────────────────────────────────────────┐
│           Lua Filter Definition Patterns                         │
└──────────────────────────────────────────────────────────────────┘

OPTION 1: Return filter table with explicit functions
┌─────────────────────────────────────────────────────┐
│ return {                                            │
│   Strong = function(elem) ... end,                 │
│   Emph = function(elem) ... end,                   │
│   traverse = 'typewise'  -- optional               │
│ }                                                   │
└─────────────────────────────────────────────────────┘
                         │
                         ▼
     Functions extracted and applied in typewise order


OPTION 2: Implicit global functions (legacy)
┌─────────────────────────────────────────────────────┐
│ function Strong(elem)                              │
│   return pandoc.SmallCaps(elem.content)            │
│ end                                                 │
│                                                     │
│ function Emph(elem)                                │
│   return elem  -- unchanged                        │
│ end                                                 │
└─────────────────────────────────────────────────────┘
                         │
                         ▼
     If filter returns nothing, search for global functions


OPTION 3: Multiple filters (deprecated)
┌─────────────────────────────────────────────────────┐
│ return {                                            │
│   filter1,                                          │
│   filter2,                                          │
│   filter3                                           │
│ }                                                   │
└─────────────────────────────────────────────────────┘
                         │
                         ▼
     Apply filters sequentially (discouraged in favor of walk method)


OPTION 4: Mixed with walk method
┌─────────────────────────────────────────────────────┐
│ function Pandoc(doc)                               │
│   doc = doc:walk { Meta = Meta }         -- (1)   │
│   doc = doc:walk { Str = Str }           -- (2)   │
│   return doc                                        │
│ end                                                 │
└─────────────────────────────────────────────────────┘
                         │
                         ▼
     Custom execution order control via explicit walk calls
```

## 6. Typewise vs Topdown Traversal

```
Example AST: [Plain [Str "a"], Para [Str "b"]]

TYPEWISE (default):
┌──────────────────────────────────────┐
│ Block List [Plain [...], Para [...]] │
└──────────────────────────────────────┘
           │
           ▼ (no inline filters yet)
    
    Apply Inlines filter: No
           │
           ▼
    
┌──────────────────────────────────────┐
│ Block-level filters:                 │
│  1. Plain function(Plain elem)       │
│  2. Para function(Para elem)         │
└──────────────────────────────────────┘
           │
           ▼
    
    Apply Blocks filter: on modified list
    Apply Meta filter: on metadata
    Apply Pandoc filter: on whole document

Order: Blocks → Plain → Para → Inlines → Str → Inlines → Str
       (but applied in typewise order)


TOPDOWN (depth-first):
┌──────────────────────────────────────┐
│ Blocks filter ([Plain, Para])        │ ◄── Start
└──────────────────────────────────────┘
           │
           ▼
┌──────────────────────────────────────┐
│ Plain (first element)                │
└──────────────────────────────────────┘
           │
           ▼
┌──────────────────────────────────────┐
│ Inlines ([Str "a"])                  │
└──────────────────────────────────────┘
           │
           ▼
┌──────────────────────────────────────┐
│ Str "a"                              │
└──────────────────────────────────────┘
           │
           ▼ (if function returns false, stop here)
┌──────────────────────────────────────┐
│ Para (second element)                │
└──────────────────────────────────────┘
           │
           ▼
┌──────────────────────────────────────┐
│ Inlines ([Str "b"])                  │
└──────────────────────────────────────┘
           │
           ▼
┌──────────────────────────────────────┐
│ Str "b"                              │
└──────────────────────────────────────┘

Order: Blocks → Plain → Inlines → Str → Para → Inlines → Str
```

## 7. Filter Return Value Semantics

```
┌──────────────────────────────────────────────────────────────────┐
│     Lua Filter Function Return Values                            │
└──────────────────────────────────────────────────────────────────┘

function myFilter(elem)
    return ??? 
end

┌─────────────────────────────┬──────────────────────────────────┐
│ Return Value                │ Effect                           │
├─────────────────────────────┼──────────────────────────────────┤
│ nil (nothing)               │ Element unchanged                │
│                             │ elem = elem                      │
├─────────────────────────────┼──────────────────────────────────┤
│ Same element modified       │ Element replaced                 │
│ return elem                 │ Must be same type as input       │
├─────────────────────────────┼──────────────────────────────────┤
│ Different element (same typ)│ Element replaced                 │
│ return pandoc.Para({...})   │ Must be same type as input       │
├─────────────────────────────┼──────────────────────────────────┤
│ List of elements            │ Spliced into parent list         │
│ return {elem1, elem2}       │ Types must match container       │
├─────────────────────────────┼──────────────────────────────────┤
│ Empty list                  │ Element deleted                  │
│ return {}                   │ Removed from parent list         │
├─────────────────────────────┼──────────────────────────────────┤
│ false (topdown only)        │ Skip children                    │
│ return elem, false          │ Prevents descending into element │
└─────────────────────────────┴──────────────────────────────────┘

Example transformations:

Str "hello" → nil
  Result: unchanged

Str "hello" → Str "HELLO"
  Result: "HELLO"

Str "hello" → {Str "HEL", Str "LO"}
  Result: spliced into parent as two separate Str elements

Strong [Str "bold"] → Emph [Str "italic"]
  ERROR: Type mismatch (Strong != Emph)
  
Para [...] → {}
  Result: deleted from document
```

## 8. Filter Discovery and Resolution

```
┌──────────────────────────────────────────────────────────────────┐
│         Filter Path Resolution                                   │
└──────────────────────────────────────────────────────────────────┘

$ pandoc --filter ./myfilter.py input.md

Filter specification: "./myfilter.py"
                   │
                   ▼
            Search in order:
                   │
        ┌──────────┴──────────┐
        │                     │
        ▼                     ▼
  [1] Direct path        [2] Data directory
   (as specified)         $DATADIR/filters/
        │                     │
        ├─ ./myfilter.py      ├─ ~/.config/pandoc/filters/myfilter.py
        ├─ ~/myfilter.py      └─ /usr/share/pandoc/filters/myfilter.py
        └─ /abs/path/...      
                   │                     │
                   ├─────────┬───────────┘
                   │         │
            First found?     │
                   │         │
        ┌──────────┘         │
        │                    ▼
        ▼              [3] System PATH
    Use path          (JSON filters only)
                              │
                    ┌─────────┴────────┐
                    │                  │
            /usr/bin/myfilter.py   ~/.local/bin/myfilter.py
                    │
                    ▼
            Executable check

For non-executable files, guess interpreter:
  myfilter.py  → python myfilter.py
  myfilter.hs  → runhaskell myfilter.hs
  myfilter.pl  → perl myfilter.pl
  myfilter.rb  → ruby myfilter.rb
  myfilter.php → php myfilter.php
  myfilter.js  → node myfilter.js
  myfilter.r   → Rscript myfilter.r
```

## 9. Filter Composition Example

```
┌──────────────────────────────────────────────────────────────────┐
│      Multiple Filter Execution (left-to-right)                   │
└──────────────────────────────────────────────────────────────────┘

Command:
  pandoc input.md \
    --filter ./filter1.lua \
    --filter ./filter2.py \
    --citeproc \
    --lua-filter ./filter3.lua \
    -o output.html

Execution timeline:

Input AST
    │
    ├─> [Filter 1: filter1.lua]
    │   Lua interpreter
    │   Traverse: typewise
    │   Output: modified AST
    │
    ├─> [Filter 2: filter2.py]
    │   External process
    │   Input: JSON on stdin
    │   Output: JSON on stdout
    │   Output: modified AST
    │
    ├─> [Filter 3: citeproc (built-in)]
    │   Citation processing
    │   Output: modified AST
    │
    └─> [Filter 4: filter3.lua]
        Lua interpreter
        Traverse: typewise
        Output: final AST

Writer
    │
    └─> HTML output

Key points:
• Each filter runs in sequence
• Output of filter N = input to filter N+1
• Can mix JSON, Lua, and built-in filters
• All filters see the transformed AST state
```

## 10. AST Marshaling: Haskell ↔ Lua

```
┌──────────────────────────────────────────────────────────────────┐
│     AST Type Conversion Between Haskell and Lua                  │
└──────────────────────────────────────────────────────────────────┘

HASKELL (Text.Pandoc.Definition)
┌──────────────────────────────┐
│ data Pandoc = Pandoc Meta    │
│                   [Block]    │
│                              │
│ data Block = Para [Inline]   │
│           | Header Int Attr  │
│                  [Inline]    │
│           | CodeBlock ...    │
│                              │
│ data Inline = Str Text       │
│            | Strong [Inline] │
│            | Link Attr ...   │
└──────────────────────────────┘
        │
        │ (marshaling via HsLua)
        │
        ▼
LUA (Pandoc userdata tables)
┌──────────────────────────────┐
│ Pandoc:                      │
│  {t="Pandoc",                │
│   meta={...},                │
│   blocks={...}}              │
│                              │
│ Para:                        │
│  {t="Para",                  │
│   content={...}}             │
│                              │
│ Str:                         │
│  {t="Str",                   │
│   text="hello"}              │
└──────────────────────────────┘

Conversion functions:
Haskell → Lua:  pushPandoc, pushBlock, pushInline, etc.
Lua → Haskell:  peekPandoc, peekBlock, peekInline, etc.

Error handling:
• Type mismatch during peek: LuaError
• Propagated as PandocError
• Caught by Pandoc and reported to user
```

