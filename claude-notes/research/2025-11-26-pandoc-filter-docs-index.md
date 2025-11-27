# Pandoc Filter Architecture Documentation Index

This directory contains comprehensive documentation of the Pandoc filter system, extracted from the external-sources/pandoc repository.

## Documents

### 1. FILTER_SUMMARY.md
**Quick reference guide** - Start here for a high-level overview

- 285 lines, quick lookup format
- Best for: Quick answers, decision making
- Contents:
  - Key file locations
  - Three filter types at a glance
  - JSON filter protocol specifics
  - Lua filter execution pipeline
  - Return value semantics
  - Performance comparison
  - Common mistakes

**When to use:** You need to understand how a specific aspect works quickly

---

### 2. FILTER_ARCHITECTURE_FINDINGS.md
**Comprehensive deep-dive** - Read this for understanding the complete system

- 505 lines, detailed analysis
- Best for: Understanding the architecture
- Contents:
  - Part 1: JSON Filter Protocol (communication, data format, error handling)
  - Part 2: Lua Filter Architecture (structure, execution pipeline, marshaling)
  - Part 3: Filter Composition and Execution Order (overall pipeline, sequential application)
  - Part 4: Performance Analysis (JSON overhead, Lua advantages)
  - Part 5: Citeproc Integration
  - Part 6: Module and API Structure

**When to use:** You want to understand how everything fits together

---

### 3. FILTER_DIAGRAMS.md
**Visual explanations** - Diagrams and ASCII art for visual learners

- 523 lines, visual format
- Best for: Understanding flow and relationships
- Contents:
  - 1. Overall Processing Pipeline
  - 2. JSON Filter Communication (process diagram)
  - 3. JSON Filter Data Format (structure example)
  - 4. Lua Filter Execution Flow (step-by-step)
  - 5. Lua Filter Structure Options (multiple patterns)
  - 6. Typewise vs Topdown Traversal (comparison)
  - 7. Filter Return Value Semantics (table and examples)
  - 8. Filter Discovery and Resolution (search path)
  - 9. Filter Composition Example (multiple filters)
  - 10. AST Marshaling: Haskell ↔ Lua

**When to use:** You prefer visual explanations or need to understand a complex flow

---

## Quick Navigation

### I want to...

**Understand the basics**
→ Read FILTER_SUMMARY.md

**Implement a JSON filter**
→ FILTER_SUMMARY.md (JSON Protocol section) + FILTER_DIAGRAMS.md (section 2-3)

**Implement a Lua filter**
→ FILTER_SUMMARY.md (Lua section) + FILTER_DIAGRAMS.md (sections 4-7)

**Understand filter composition**
→ FILTER_DIAGRAMS.md (section 1, 9) + FILTER_ARCHITECTURE_FINDINGS.md (Part 3)

**Optimize filter performance**
→ FILTER_SUMMARY.md (Performance Comparison) + FILTER_ARCHITECTURE_FINDINGS.md (Part 4)

**Understand traversal modes**
→ FILTER_DIAGRAMS.md (section 6) + FILTER_ARCHITECTURE_FINDINGS.md (Filter Execution Order)

**Debug a filter issue**
→ FILTER_SUMMARY.md (Common Mistakes) + FILTER_ARCHITECTURE_FINDINGS.md (Part 2)

**Understand return values**
→ FILTER_DIAGRAMS.md (section 7) + FILTER_SUMMARY.md (Return Value Semantics)

---

## Key Concepts at a Glance

### Three Filter Types

| Type | Execution | Communication | Performance | File |
|------|-----------|---|---|---|
| **JSON** | External process | stdin/stdout JSON | -35% overhead | src/Text/Pandoc/Filter/JSON.hs |
| **Lua** | Embedded VM | Direct marshaling | ~2% overhead | pandoc-lua-engine/src/Text/Pandoc/Lua/Filter.hs |
| **Citeproc** | Built-in | Internal | Negligible | src/Text/Pandoc/Citeproc.hs |

### Filter Execution Model

All filters are applied sequentially using `foldM`:
```
Input AST → [Filter1] → [Filter2] → [Filter3] → Output AST
```

Output of filter N becomes input to filter N+1

### Lua Filter Structure

```lua
return {
  -- Element-specific filters
  Str = function(elem) ... end,
  Para = function(elem) ... end,
  
  -- Optional: traversal mode
  traverse = 'typewise',  -- or 'topdown'
}
```

### Return Value Semantics

| Return | Effect |
|--------|--------|
| `nil` | Element unchanged |
| Same element | Replaces original |
| List of elements | Splices into parent |
| Empty list | Deletes element |

### Filter Discovery

Search in this order:
1. Direct path (as specified)
2. `$DATADIR/filters/`
3. `$PATH` (executables only)

---

## Source Code References

### JSON Filters
- **Main implementation:** `src/Text/Pandoc/Filter/JSON.hs` (lines 35-82)
- **Filter type:** `src/Text/Pandoc/Filter.hs` (lines 40-66)
- **Application:** `src/Text/Pandoc/Filter.hs` (lines 76-101)

### Lua Filters
- **Filter loading:** `pandoc-lua-engine/src/Text/Pandoc/Lua/Filter.hs` (lines 26-71)
- **Engine interface:** `pandoc-lua-engine/src/Text/Pandoc/Lua/Engine.hs` (lines 49-73)
- **AST marshaling:** `pandoc-lua-engine/src/Text/Pandoc/Lua/Orphans.hs`

### Documentation
- **Filter guide:** `doc/filters.md`
- **Lua guide:** `doc/lua-filters.md`

### Pandoc Modules
- All in `pandoc-lua-engine/src/Text/Pandoc/Lua/Module/`
- Main module: `Module/Pandoc.hs`

---

## Filter Execution Pipeline

```
1. Reader → AST

2. Filter sequence (foldM):
   - Expand filter paths
   - For each filter in order:
     - JSON: spawn subprocess, pipe AST as JSON
     - Lua: load script, marshal AST to Lua, apply functions
     - Citeproc: process citations
   - Thread result to next filter

3. Final AST → Writer → Output
```

---

## Important Implementation Details

### JSON Filter Protocol
- **Process argument:** Target format (e.g., "html5", "latex")
- **Environment:** PANDOC_VERSION, PANDOC_READER_OPTIONS
- **Input format:** JSON-encoded Pandoc document on stdin
- **Output format:** JSON-encoded Pandoc document on stdout
- **Error handling:** Non-zero exit code raises PandocFilterError

### Lua Filter Execution
- **Interpreter:** Lua 5.4 (embedded in Pandoc)
- **Globals set:** FORMAT, PANDOC_READER_OPTIONS, PANDOC_WRITER_OPTIONS, etc.
- **Filter detection:** Return value, list of returns, or implicit globals
- **Traversal modes:** Typewise (default) or topdown (added 2.17)
- **Return type checking:** Must match input element type

### Filter Composition
- **Pattern:** Monadic fold (foldM)
- **Order:** Left-to-right as specified on command line
- **Mixing:** Can freely mix JSON, Lua, and built-in filters
- **Visibility:** Each filter sees progressively modified AST

---

## Terminology

- **AST** - Abstract Syntax Tree (Pandoc's intermediate representation)
- **Pandoc** - The document type (Meta + [Block])
- **Block** - Top-level document elements (Para, Header, CodeBlock, etc.)
- **Inline** - Inline elements (Str, Emph, Strong, Link, etc.)
- **Meta** - Document metadata (title, author, etc.)
- **Marshaling** - Converting between Haskell and Lua type systems
- **Typewise** - Process all Inlines, then Blocks, then Meta, then Pandoc
- **Topdown** - Depth-first traversal from document root

---

## Performance Notes

Baseline: 1.01s to convert MANUAL.txt to HTML

| Implementation | Time | Overhead |
|---|---|---|
| Pandoc (baseline) | 1.01s | - |
| Lua filter | 1.03s | +2% |
| Compiled Haskell JSON | 1.36s | +35% |
| Python JSON | 1.40s | +39% |

**Key insight:** Lua filters have negligible overhead, JSON filters have significant overhead due to serialization and subprocess communication.

---

## File Organization

```
/Users/cscheid/repos/github/cscheid/kyoto/
├── PANDOC_FILTER_DOCS_INDEX.md      (this file - navigation)
├── FILTER_SUMMARY.md                (quick reference)
├── FILTER_ARCHITECTURE_FINDINGS.md  (detailed analysis)
├── FILTER_DIAGRAMS.md               (visual explanations)
└── external-sources/pandoc/
    ├── src/Text/Pandoc/
    │   ├── Filter.hs
    │   ├── Filter/
    │   │   ├── JSON.hs
    │   │   └── Environment.hs
    │   ├── Citeproc.hs
    │   └── Scripting.hs
    ├── pandoc-lua-engine/src/Text/Pandoc/Lua/
    │   ├── Filter.hs
    │   ├── Engine.hs
    │   ├── Module.hs
    │   ├── Module/
    │   │   ├── Pandoc.hs
    │   │   └── ... (other modules)
    │   └── Marshal/
    │       └── ... (marshaling functions)
    └── doc/
        ├── filters.md
        └── lua-filters.md
```

---

## Related Documentation

- **Pandoc manual:** external-sources/pandoc/MANUAL.txt
- **Pandoc types API:** https://hackage.haskell.org/package/pandoc-types
- **HsLua documentation:** https://github.com/hslua/hslua
- **Lua 5.4 manual:** https://www.lua.org/manual/5.4/
- **Pandoc filters wiki:** https://github.com/jgm/pandoc/wiki/Pandoc-Filters

---

## Document History

- **Created:** 2025-11-26
- **Source:** external-sources/pandoc commit b9c2b2e (main branch)
- **Analyzed files:**
  - src/Text/Pandoc/Filter.hs
  - src/Text/Pandoc/Filter/JSON.hs
  - src/Text/Pandoc/Filter/Environment.hs
  - pandoc-lua-engine/src/Text/Pandoc/Lua/Filter.hs
  - pandoc-lua-engine/src/Text/Pandoc/Lua/Engine.hs
  - pandoc-lua-engine/src/Text/Pandoc/Lua/Module.hs
  - pandoc-lua-engine/src/Text/Pandoc/Lua/Orphans.hs
  - doc/filters.md
  - doc/lua-filters.md
