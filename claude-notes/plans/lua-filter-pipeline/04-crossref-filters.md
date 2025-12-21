# Crossref Filters (quarto_crossref_filters)

**Source**: `main.lua` lines 626-662

**Purpose**: Process cross-references (figures, tables, equations, theorems, sections) and generate reference index.

**Condition**: Only runs if `param("enable-crossref", true)` is true (default enabled)

---

## Stages

### 1. crossref-preprocess-floats

**Source**: `crossref/preprocess.lua` - `crossref_mark_subfloats()`

**Side Effects**: **PURE**
- Marks subfloats with parent references
- Sets `parent_id` on subfloat FloatRefTarget nodes
- Populates `crossref.subfloats` table

**Pandoc API**: None

---

### 2. crossref-preprocessTheorems

**Source**: `crossref/theorems.lua` - `crossref_preprocess_theorems()`

**Condition**: Only if `flags.has_theorem_refs` is true

**Side Effects**: **PURE**
- Preprocesses theorem environments for cross-referencing

**Pandoc API**: None

---

### 3. crossref-combineFilters

**Source**: Combines multiple filters

**Combines**:
- `file_metadata()` - parses file metadata comments
- `qmd()` - QMD-specific processing
- `sections()` - section numbering and indexing
- `crossref_figures()` - figure cross-reference indexing
- `equations()` - equation numbering
- `crossref_theorems()` - theorem cross-reference indexing
- `crossref_callouts()` - callout cross-reference indexing

**Side Effects**: **PURE**
- All operations are in-memory AST transformations
- Populates `crossref.index.entries` table

**Pandoc API**: None

---

### 4. crossref-resolveRefs

**Source**: `crossref/refs.lua` - `resolveRefs()`

**Condition**: Only if `flags.has_cites` is true

**Side Effects**: **PURE**
- Resolves `@fig-xxx`, `@tbl-xxx`, `@eq-xxx` citations to formatted references
- Generates format-specific output (LaTeX `\ref{}`, Typst `#ref()`, HTML links)

**Pandoc API**: None (but generates format-specific raw content)

**Notes**:
- Emits `RawInline` for LaTeX, Typst, AsciiDoc
- Creates `Link` elements for HTML
- Uses `crossref.index.entries` populated by earlier stages

---

### 5. crossref-crossrefMetaInject

**Source**: `crossref/meta.lua` - `crossrefMetaInject()`

**Side Effects**: **PURE (but uses Pandoc API)**

**Pandoc API**: `pandoc.write()` - converts inlines to LaTeX for caption setup

**Notes**:
- Injects LaTeX preamble for figure/table captions
- Uses `metaInjectLatex()` to add includes
- Only affects LaTeX output

---

### 6. crossref-writeIndex

**Source**: `crossref/index.lua` - `writeIndex()`

**Condition**: Only if `param("crossref-index-file")` is set

**Side Effects**:
| Type | Details |
|------|---------|
| `FW` (File Write) | Writes crossref index JSON via `io.open()` |

**Notes**:
- Writes entries with keys, captions, order, parent relationships
- Used for multi-file book projects to share cross-references
- `writeKeysIndex()` for QMD input (minimal), `writeFullIndex()` for other inputs

---

## Summary

| Stage | Side Effects | Pandoc API | WASM-Safe |
|-------|--------------|------------|-----------|
| crossref-preprocess-floats | Pure | None | Yes |
| crossref-preprocessTheorems | Pure | None | Yes |
| crossref-combineFilters | Pure | None | Yes |
| crossref-resolveRefs | Pure | None | Yes |
| crossref-crossrefMetaInject | Pure | `pandoc.write` | Partial |
| crossref-writeIndex | `FW` | None | **No** |

**Total**: 6 stages, 1 with file write, 1 with Pandoc API

**WASM Notes**:
- `crossref-writeIndex` writes index file - could be handled via virtual FS
- `crossrefMetaInject` uses `pandoc.write()` for LaTeX output - may need Pandoc

---

## Data Flow

```
Prepared document (from pre filters)
       ↓
[crossref-preprocess-floats]
  - Mark subfloats with parent IDs
       ↓
[crossref-preprocessTheorems]
  - Preprocess theorem environments
       ↓
[crossref-combineFilters]
  - Index all cross-referenceable elements:
    * Figures with #fig-xxx IDs
    * Tables with #tbl-xxx IDs
    * Equations with #eq-xxx IDs
    * Theorems with #thm-xxx IDs
    * Sections with #sec-xxx IDs
       ↓
[crossref-resolveRefs]
  - Convert @fig-xxx citations to:
    * LaTeX: \ref{fig-xxx}
    * HTML: <a href="#fig-xxx">Figure 1</a>
    * Typst: #ref(<fig-xxx>)
       ↓
[crossref-crossrefMetaInject]
  - Inject LaTeX caption setup
  - Configure float names
       ↓
[crossref-writeIndex]
  - Write crossref index for book projects
       ↓
Document with resolved references
```

---

## Key Observations

1. **Mostly pure**: 5 of 6 stages have no external side effects.

2. **Index file for books**: `crossref-writeIndex` is used for multi-file book projects to share cross-reference information. For single documents, this could be skipped.

3. **Format-specific rendering**: `resolveRefs` generates format-specific raw content. This is an example of where the Lua pipeline mixes AST transformation with rendering.

4. **Pandoc API minimal**: Only `crossrefMetaInject` uses Pandoc API (`pandoc.write`), and only for LaTeX output.

---

## Cross-Reference Types

The crossref system handles these reference types:

| Type | Prefix | Example |
|------|--------|---------|
| Figure | `fig` | `@fig-myplot` |
| Table | `tbl` | `@tbl-data` |
| Equation | `eq` | `@eq-formula` |
| Section | `sec` | `@sec-intro` |
| Listing | `lst` | `@lst-code` |
| Theorem | `thm` | `@thm-main` |
| Lemma | `lem` | `@lem-helper` |
| Corollary | `cor` | `@cor-result` |
| Definition | `def` | `@def-term` |
| Example | `exm` | `@exm-case` |
| Exercise | `exr` | `@exr-problem` |
