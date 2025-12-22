# Finalize Filters (quarto_finalize_filters)

**Source**: `main.lua` lines 538-584

**Purpose**: Final document cleanup, dependency injection, mediabag handling, and output preparation.

---

## Stages Overview

| Stage | Purpose | Side Effects | Pandoc API |
|-------|---------|--------------|------------|
| finalize-combined | File metadata, mediabag, LaTeX vault | `FW` | None |
| finalize-bookCleanup | Book project cleanup | None | None |
| finalize-cites | Write citation data | `FW` | None |
| finalize-metaCleanup | Clean internal metadata | None | None |
| finalize-dependencies | Process dependencies | Varies* | None |
| finalize-combined-1 | Combined finalizations | None | None |
| finalize-wrapped-writer | Wrapped writer setup | None | None |

*Dependencies processing may involve file writes depending on the format.

---

## Key Side Effects Details

### finalize-combined (mediabag_filter)

**Source**: `quarto-finalize/mediabag.lua`

```lua
local mediaFile = _quarto.modules.mediabag.write_mediabag_entry(el.src)
```

The `write_mediabag_entry` function:
```lua
local file = _quarto.file.write(mediaFile, contents)
```

Writes mediabag entries (images, etc.) to the filesystem for non-Office formats.

### finalize-cites (writeCites)

**Source**: `quarto-post/cites.lua` (referenced from finalize)

Writes citation index data to JSON file.

### finalize-dependencies

**Source**: `quarto-finalize/dependencies.lua`

```lua
_quarto.processDependencies(meta)
```

Processes accumulated dependencies (CSS, JS, etc.) and writes them to appropriate locations. The actual I/O depends on the format and whether dependencies need to be externalized.

---

## Summary

| Metric | Value |
|--------|-------|
| Total Stages | 7 |
| Pure | 4 |
| File Write | 3 (mediabag, cites, dependencies) |
| File Read | 0 |
| Subprocess | 0 |
| Pandoc API | 0 |
| WASM Blocked | 0* |

*File writes could be redirected to virtual filesystem in WASM.

**WASM Notes**:
- `mediabag_filter`: Writes image files - could use VFS or data URIs
- `finalize-cites`: Writes cites JSON - could accumulate in memory
- `finalize-dependencies`: Depends on how dependencies are handled - could be deferred

---

## Data Flow

```
Post-processed document
       ↓
[finalize-combined]
  - Write mediabag entries to filesystem
  - Handle LaTeX vault content
       ↓
[finalize-bookCleanup]
  - Clean up book-specific elements
       ↓
[finalize-cites]
  - Write citation index
       ↓
[finalize-metaCleanup]
  - Remove internal metadata keys
       ↓
[finalize-dependencies]
  - Process and inject dependencies
       ↓
[finalize-combined-1]
  - Additional finalizations
       ↓
[finalize-wrapped-writer]
  - Set up writer customizations
       ↓
Final document ready for output
```

---

## Critical Observations

1. **File writes are output-oriented**: The file writes in finalize stages are for producing output artifacts (images, cites, dependencies), not for intermediate processing. In WASM, these could:
   - Accumulate in memory
   - Use data URIs for inline embedding
   - Write to virtual filesystem

2. **No Pandoc API**: Unlike post filters, finalize stages don't use `pandoc.read/write`. They operate on the already-rendered AST.

3. **Dependencies are format-specific**: The dependency processing varies by output format. HTML needs CSS/JS injection, LaTeX needs package includes, etc.
