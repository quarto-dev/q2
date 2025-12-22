# Pre Filters (quarto_pre_filters)

**Source**: `main.lua` lines 261-381

**Purpose**: Process shortcodes, handle hidden content, set up figures/theorems, and prepare document for rendering.

---

## Stages

### 1. flags

**Source**: `normalize/flags.lua` - `compute_flags()`

**Side Effects**: **PURE**
- Scans AST to set boolean flags used to skip unnecessary filter stages
- Sets flags like `has_shortcodes`, `has_tables`, `has_cites`, `has_lightbox`, etc.

**Pandoc API**: None

---

### 2. pre-server-shiny

**Source**: `quarto-pre/shiny.lua` - `server_shiny()`

**Condition**: Only runs if `param("is-shiny-python")` is true

**Side Effects**:
| Type | Details |
|------|---------|
| `S` (Subprocess) | `pandoc.pipe()` to run `python -m shiny get-shiny-deps` |
| `S` (Subprocess) | `pandoc.pipe()` to run `python -m shiny cells-to-app` |
| `FW` (File Write) | Writes `*-cells.tmp.json` file |
| `FW` (File Write) | Creates `app.py` file |
| `FR` (File Read) | Reads subprocess output |

**Pandoc API**: `pandoc.pipe()`, `pandoc.path.*`

**Notes**: This is a specialized stage for Shiny Python documents. Completely blocks WASM.

---

### 3. pre-read-options-again

**Source**: `quarto-pre/options.lua` - `init_options()`

**Side Effects**: **PURE**
- Re-reads options from meta in case user filters modified them

**Pandoc API**: None

---

### 4. pre-bibliography-formats

**Source**: `quarto-pre/bibliography-formats.lua` - `bibliography_formats()`

**Condition**: Only for bibliography output formats

**Side Effects**: **PURE (but uses Pandoc API)**

**Pandoc API**: `pandoc.utils.references(doc)` - extracts references from document

---

### 5. pre-shortcodes-filter

**Source**: `customnodes/shortcodes.lua` - `shortcodes_filter()`

**Condition**: Only if `flags.has_shortcodes` is true

**Side Effects**:
| Type | Details |
|------|---------|
| `ENV` | `os.getenv()` for `{{< env VAR >}}` shortcode |
| `FR` | User shortcode files loaded via `loadfile()` (at init time) |

**Pandoc API**: None during filter execution (shortcode loading happens at init)

**Notes**:
- Built-in shortcodes: `meta`, `var`, `env`, `pagebreak`, `brand`, `contents`
- `env` shortcode reads environment variables
- User shortcodes can do arbitrary things (including file I/O)

---

### 6. pre-contents-shortcode-filter

**Source**: `quarto-pre/contentsshortcode.lua` - `contents_shortcode_filter()`

**Condition**: Only if `flags.has_contents_shortcode` is true

**Side Effects**: **PURE**
- Processes `{{< contents >}}` shortcode for TOC generation

**Pandoc API**: None

---

### 7. strip-notes-from-hidden

**Source**: `quarto-pre/hidden.lua` - `strip_notes_from_hidden()`

**Condition**: Only if `flags.has_notes` is true

**Side Effects**: **PURE**
- Removes footnotes from hidden elements to prevent duplicates

**Pandoc API**: None

---

### 8. pre-combined-hidden

**Source**: Combines `hidden()` and `content_hidden()`

**Condition**: Only if `flags.has_hidden` or `flags.has_conditional_content`

**Side Effects**: **PURE**
- Strips or processes hidden elements based on format and options

**Pandoc API**: None

---

### 9. pre-table-captions

**Source**: `quarto-pre/table-captions.lua` - `table_captions()`

**Condition**: Only if `flags.has_table_captions` is true

**Side Effects**: **PURE**
- Processes table captions from cell attributes

**Pandoc API**: None

---

### 10. pre-code-annotations

**Source**: `quarto-pre/code-annotation.lua` - `code_annotations()`

**Condition**: Only if `flags.has_code_annotations` is true

**Side Effects**: **PURE**
- Processes `<1>`, `<2>` style code annotations

**Pandoc API**: None

---

### 11. pre-code-annotations-meta

**Source**: `quarto-pre/code-annotation.lua` - `code_meta()`

**Side Effects**: **PURE**
- Injects code annotation metadata

**Pandoc API**: None

---

### 12. pre-unroll-cell-outputs

**Source**: `quarto-pre/outputs.lua` - `unroll_cell_outputs()`

**Condition**: Only if `flags.needs_output_unrolling` is true

**Side Effects**: **PURE**
- Unrolls cell output divs when output-divs is false

**Pandoc API**: None

---

### 13. pre-output-location

**Source**: `quarto-pre/output-location.lua` - `output_location()`

**Side Effects**: **PURE**
- Handles output-location attribute for cell outputs

**Pandoc API**: None

---

### 14. pre-scope-resolution

**Source**: `quarto-pre/resolvescopedelements.lua` - `resolve_scoped_elements()`

**Condition**: Only if `flags.has_tables` is true

**Side Effects**: **PURE**
- Resolves scoped elements (like table options)

**Pandoc API**: None

---

### 15. pre-combined-figures-theorems-etc

**Source**: Combines many filters

**Combines**:
- `file_metadata()` - parses file metadata from HTML comments
- `index_book_file_targets()` - indexes cross-reference targets
- `book_numbering()` - chapter/section numbering
- `include_paths()` - processes include paths
- `resource_files()` - records image resources
- `quarto_pre_figures()` - figure processing
- `quarto_pre_theorems()` - theorem processing
- `docx_callout_and_table_fixup()` - DOCX-specific fixes
- `engine_escape()` - escapes engine-specific content
- `line_numbers()` - line number processing
- `bootstrap_panel_input()` - panel input handling
- `bootstrap_panel_layout()` - panel layout
- `bootstrap_panel_sidebar()` - sidebar panels
- `table_respecify_gt_css()` - GT table CSS
- `table_classes()` - table class handling
- `input_traits()` - input trait processing
- `resolve_book_file_targets()` - resolves book targets
- `project_paths()` - project path resolution

**Side Effects**: **PURE**
- All sub-filters are pure AST transformations

**Pandoc API**: Uses `pandoc.path.*` utilities (safe)

---

### 16. pre-quarto-pre-meta-inject

**Source**: `quarto-pre/meta.lua` - `quarto_pre_meta_inject()`

**Side Effects**: **PURE**
- Injects metadata into document

**Pandoc API**: None

---

### 17. pre-write-results

**Source**: `quarto-pre/results.lua` - `write_results()`

**Side Effects**:
| Type | Details |
|------|---------|
| `FW` (File Write) | Writes results JSON to file via `io.open()` |

**Notes**:
- Writes to file specified by `param("results-file")`
- Contains resource files, cross-references, and other computed results

---

## Summary

| Stage | Side Effects | Pandoc API | WASM-Safe |
|-------|--------------|------------|-----------|
| flags | Pure | None | Yes |
| pre-server-shiny | `S`, `FW`, `FR` | `pandoc.pipe` | **No** |
| pre-read-options-again | Pure | None | Yes |
| pre-bibliography-formats | Pure | `pandoc.utils.references` | Partial |
| pre-shortcodes-filter | `ENV`, `FR`* | None | Partial |
| pre-contents-shortcode-filter | Pure | None | Yes |
| strip-notes-from-hidden | Pure | None | Yes |
| pre-combined-hidden | Pure | None | Yes |
| pre-table-captions | Pure | None | Yes |
| pre-code-annotations | Pure | None | Yes |
| pre-code-annotations-meta | Pure | None | Yes |
| pre-unroll-cell-outputs | Pure | None | Yes |
| pre-output-location | Pure | None | Yes |
| pre-scope-resolution | Pure | None | Yes |
| pre-combined-figures-theorems-etc | Pure | `pandoc.path` | Yes |
| pre-quarto-pre-meta-inject | Pure | None | Yes |
| pre-write-results | `FW` | None | **No** |

**Total**: 17 stages, 2 blocking for WASM, 2 with partial issues

*`pre-shortcodes-filter`: User shortcode files loaded at init, `env` shortcode reads env vars

**WASM Notes**:
- `pre-server-shiny` is completely incompatible (subprocess calls)
- `pre-write-results` writes results file - could be handled differently in WASM
- `env` shortcode reads environment variables - could be pre-populated
- `pandoc.utils.references()` may require Pandoc - needs investigation

---

## Data Flow

```
Normalized Quarto AST
       ↓
[flags]
  - Scans document for optimization flags
       ↓
[pre-server-shiny] (Shiny Python only)
  - Extracts code cells
  - Generates app.py
       ↓
[pre-read-options-again]
  - Refresh options after user filters
       ↓
[pre-bibliography-formats] (bibliography output only)
  - Extract references for bibliography
       ↓
[pre-shortcodes-filter]
  - Process {{< shortcode >}} syntax
  - Expand meta, var, env, pagebreak, etc.
       ↓
[pre-contents-shortcode-filter]
  - Process {{< contents >}} for TOC
       ↓
[hidden/content-hidden stages]
  - Remove or process hidden elements
       ↓
[table/code processing stages]
  - Table captions, code annotations
       ↓
[pre-combined-figures-theorems-etc]
  - Main document preparation
  - Figure/theorem/panel processing
  - Resource file tracking
       ↓
[pre-quarto-pre-meta-inject]
  - Final metadata injection
       ↓
[pre-write-results]
  - Write computed results to file
       ↓
Prepared document ready for crossref
```

---

## Key Observations

1. **Most stages are pure**: 14 of 17 stages have no external side effects.

2. **Two blocking stages for WASM**:
   - `pre-server-shiny`: Shiny Python support (can be disabled)
   - `pre-write-results`: Results file writing (could write to virtual FS or skip)

3. **Environment variables**: The `env` shortcode reads `os.getenv()`. In WASM, this could be pre-populated or disabled.

4. **User shortcodes**: Custom shortcode handlers loaded from files can do arbitrary I/O. For WASM, user shortcodes would need to be pre-loaded or disabled.

5. **Pandoc utilities**: `pandoc.utils.references()` and `pandoc.path.*` are used. Path utilities should be safe in WASM; references extraction needs investigation.
