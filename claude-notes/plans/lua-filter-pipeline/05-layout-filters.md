# Layout Filters (quarto_layout_filters)

**Source**: `main.lua` lines 586-624

**Purpose**: Handle document layout including manuscripts, lightbox galleries, multi-column layouts, citation positioning, and panel layouts.

---

## Stages Overview

| Stage | Purpose | Side Effects | Pandoc API |
|-------|---------|--------------|------------|
| manuscript filtering (x2) | Manuscript document processing | `FR` (notebooks JSON) | None |
| layout-lightbox | Lightbox image galleries | None | `pandoc.read`, `pandoc.write` |
| layout-columns-preprocess | Multi-column preprocessing | None | None |
| layout-columns | Multi-column layouts | None | None |
| layout-cites-preprocess | Citation layout prep | None | None |
| layout-cites | Citation positioning (margin/footer) | None | None |
| layout-panels | Panel layout rendering | None | None |
| post-fold-code-and-lift-codeblocks | Code folding and lifting | None | None |

---

## Key Side Effects

### manuscript filtering

**Source**: `layout/manuscript.lua`

```lua
local notebooks = quarto.json.decode(io.open(notebooks_filename, "r"):read("*a"))
```

**File Read**: Reads notebooks JSON file for manuscript processing.

### layout-lightbox

**Source**: `layout/lightbox.lua`

```lua
local doc = pandoc.read(el.attr.attributes[attrName])
return pandoc.write(pandoc.Pandoc(attrInlines), "html")
```

**Pandoc API**: Uses `pandoc.read()` and `pandoc.write()` for attribute parsing.

---

## Summary

| Metric | Value |
|--------|-------|
| Total Stages | 9 |
| Pure | 6 |
| File Read | 1 (manuscript notebooks) |
| File Write | 0 |
| Pandoc API | 1 (lightbox) |
| WASM Blocked | 0 |

**WASM Notes**:
- `manuscript` filter reads notebooks JSON - could be pre-loaded in VFS
- `lightbox` uses `pandoc.read/write` for HTML generation - needs investigation
