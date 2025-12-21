# Init Filters (quarto_init_filters)

**Source**: `main.lua` lines 207-226

**Purpose**: Initialize document state and read metadata-dependent configuration.

---

## Stages

### 1. init-quarto-meta-init

**Source**: `quarto-init/metainit.lua` - `quarto_meta_init()`

**Filter structure**:
```lua
Meta = function(meta)
  configure_filters()           -- reads param("active-filters"), pure
  read_includes(meta)           -- FILE READ: include files
  init_crossref_options(meta)   -- reads crossref options from meta, pure
  initialize_custom_crossref_categories(meta)  -- reads meta, pure
  return meta
end
```

**Side Effects**:
| Type | Details |
|------|---------|
| `FR` (File Read) | `read_includes()` opens and reads include files specified in meta |

**Files accessed**:
- Files specified in `include-in-header`, `include-before-body`, `include-after-body` metadata

**Pandoc API**: None (beyond AST node construction)

**Notes**:
- `read_includes()` uses `io.open()` to read include file contents
- These are project resources that could be pre-loaded in WASM VFS
- The include file paths come from document metadata

---

### 2. init-quarto-custom-meta-init

**Source**: Inline in `main.lua` (lines 209-213)

**Filter structure**:
```lua
Meta = function(meta)
  content_hidden_meta(meta)  -- saves copy of meta for content-hidden processing
end
```

**Side Effects**: **PURE**
- Only saves a copy of meta to global state `_content_hidden_meta`

**Pandoc API**: None

---

### 3. init-metadata-resource-refs

**Source**: Combines `file_metadata()` and `resourceRefs()`

**Files**:
- `common/filemetadata.lua`
- `quarto-init/resourcerefs.lua`

**Filter structure**:
```lua
-- file_metadata()
RawInline = parseFileMetadata,  -- parses quarto-file-metadata comments
RawBlock = parseFileMetadata

-- resourceRefs()
Image = function(el)
  -- transforms el.src using resourceRef()
end,
RawInline = handle_raw_element_resource_ref,  -- path transforms in raw HTML
RawBlock = handle_raw_element_resource_ref
```

**Side Effects**: **PURE**
- `file_metadata()` parses base64-encoded metadata from HTML comments in AST
- `resourceRefs()` transforms image/resource paths in AST
- No file I/O - all data comes from AST

**Pandoc API**: None

**Notes**:
- `parseFileMetadata` uses `base64_decode` and `quarto.json.decode`
- These are in-memory operations on data already in the document

---

### 4. init-knitr-syntax-fixup

**Source**: `quarto-init/knitr-fixup.lua` - `knitr_fixup()`

**Condition**: Only runs when `param("execution-engine") == "knitr"`

**Filter structure**:
```lua
Div = function(e)
  if e.classes:includes("knitsql-table") then
    return pandoc.Div(e.content, { class = "cell-output-display" })
  end
  return e
end
```

**Side Effects**: **PURE**
- Simple AST transformation: reclassifies knitsql-table divs

**Pandoc API**: None

---

## Summary

| Stage | Side Effects | Pandoc API | WASM-Safe |
|-------|--------------|------------|-----------|
| init-quarto-meta-init | `FR` | None | VFS needed |
| init-quarto-custom-meta-init | Pure | None | Yes |
| init-metadata-resource-refs | Pure | None | Yes |
| init-knitr-syntax-fixup | Pure | None | Yes |

**Total**: 4 stages, 1 with file reads, 3 pure

**WASM Notes**:
- `init-quarto-meta-init` reads include files - these are project resources that should be in VFS
- All other stages are pure AST transformations

---

## Data Flow

```
Document with Meta
       ↓
[init-quarto-meta-init]
  - Reads active-filters param
  - Reads include files → adds to meta
  - Initializes crossref options
       ↓
[init-quarto-custom-meta-init]
  - Saves meta copy for content-hidden
       ↓
[init-metadata-resource-refs]
  - Parses file metadata from HTML comments
  - Transforms resource paths
       ↓
[init-knitr-syntax-fixup] (conditional)
  - Fixes knitr SQL table divs
       ↓
Initialized document
```
