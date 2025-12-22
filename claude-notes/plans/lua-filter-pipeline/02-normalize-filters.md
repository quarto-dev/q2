# Normalize Filters (quarto_normalize_filters)

**Source**: `main.lua` lines 235-259

**Purpose**: Transform raw Pandoc AST into the "Quarto AST" with custom nodes, normalized metadata, and parsed special content.

---

## Stages

### 1. normalize-draft

**Source**: `normalize/draft.lua` - `normalize_draft()`

**Filter structure**:
```lua
Meta = function(meta)
  -- reads draft-mode from meta
  -- checks if current file is in drafts list
end,
Pandoc = function(doc)
  -- for HTML output, may clear doc.blocks if draft mode is "gone"
  -- adds meta tag via quarto.doc.includeText()
end
```

**Side Effects**: **PURE**
- Only reads from document meta and params
- Modifies AST in memory
- `quarto.doc.includeText()` adds to in-memory includes list

**Pandoc API**: None

---

### 2. normalize

**Source**: `normalize/normalize.lua` - `normalize_filter()`

**Condition**: Only runs if `quarto_global_state.active_filters.normalization` is true

**Filter structure**:
```lua
Meta = function(meta)
  -- processAuthorMeta: normalizes author/affiliation
  -- processCitationMeta: normalizes citations
  -- processLicenseMeta: normalizes license
  -- shortcode_ast.parse: parses shortcodes in meta
  return normalized
end
```

**Side Effects**: **PURE**
- All operations are in-memory metadata transformations
- Uses modules/authors, modules/license, modules/astshortcode

**Pandoc API**: None

---

### 3. normalize-capture-reader-state

**Source**: `normalize/capturereaderstate.lua` - `normalize_capture_reader_state()`

**Filter structure**:
```lua
Meta = function(meta)
  quarto_global_state.reader_options = readqmd.meta_to_options(meta.quarto_pandoc_reader_opts)
  meta.quarto_pandoc_reader_opts = nil
  return meta
end
```

**Side Effects**: **PURE**
- Extracts reader options from meta and stores in global state

**Pandoc API**: None

---

### 4. astpipeline-process-tables

**Source**: `normalize/astpipeline.lua` - `astpipeline_process_tables()`

**Filter structure**:
```lua
Div = function(div)
  -- handles html-table-processing and html-pre-tag-processing attributes
end,
RawBlock = function(el)
  -- parses raw HTML tables into Pandoc Table nodes
end,
Blocks = function(blocks)  -- HTML output only
  -- merges adjacent raw HTML table blocks
end
```

**Side Effects**:
| Type | Details |
|------|---------|
| `PA` (Pandoc API) | `pandoc.read()` to parse HTML tables |
| `PA` (Pandoc API) | `pandoc.system.with_temporary_directory()` for juice processing |
| `FR` (File Read) | Reads from temp file after juice processing |
| `FW` (File Write) | Writes HTML to temp file for juice processing |
| `S` (Subprocess) | Runs `quarto run juice.ts` via `io.popen()` |

**Critical Path**: The `juice()` function (for Typst output) is heavily side-effectful:
```lua
function juice(htmltext)
  return pandoc.system.with_temporary_directory('juice', function(tmpdir)
    local juice_in = pandoc.path.join({tmpdir, 'juice-in.html'})
    local jin = assert(io.open(juice_in, 'w'))
    jin:write(htmltext)
    jin:flush()
    local quarto_path = quarto.config.cli_path()
    local jout, jerr = io.popen(quarto_path .. ' run ' ..
        pandoc.path.join({os.getenv('QUARTO_SHARE_PATH'), 'scripts', 'juice.ts'}) .. ' ' ..
        juice_in, 'r')
    -- ...
  end)
end
```

**Notes**:
- The `juice()` function only runs for Typst output
- For non-Typst output, this stage primarily uses `pandoc.read()` for HTML table parsing
- Environment variable access: `QUARTO_SHARE_PATH`
- **WASM consideration**: juice has an NPM implementation; WASM could call back into JS instead of subprocess

---

### 5. normalize-combined-1

**Source**: `normalize/astpipeline.lua` - part of `quarto_ast_pipeline()`

**Combines**:
- `extract_latex_quartomarkdown_commands()` - LaTeX output only
- `forward_cell_subcaps()` - pure AST
- `parse_extended_nodes()` - pure AST (uses registered handlers)
- `code_filename()` - pure AST
- `normalize_fixup_data_uri_image_extension()` - pure AST
- Str check for malformed fenced divs - pure

**Side Effects**:
| Type | Details |
|------|---------|
| `PA` (Pandoc API) | `string_to_quarto_ast_blocks()` uses `pandoc.read()` internally |

**Notes**:
- `extract_latex_quartomarkdown_commands()` uses `string_to_quarto_ast_blocks()` which calls `pandoc.read()` via `readqmd.readqmd()`
- Most sub-filters are pure AST transformations
- `parse_extended_nodes()` invokes registered handlers to convert Div/Span to custom nodes

---

### 6. normalize-combine-2

**Source**: `normalize/astpipeline.lua` - part of `quarto_ast_pipeline()`

**Combines**:
- `parse_md_in_html_rawblocks()` - parses embedded markdown/pandoc formats
- `parse_floatreftargets()` - pure AST
- `parse_blockreftargets()` - pure AST

**Side Effects**:
| Type | Details |
|------|---------|
| `PA` (Pandoc API) | `pandoc.read()` for parsing embedded formats |

**Notes**:
- `parse_md_in_html_rawblocks()` handles:
  - `RawBlock` with format `pandoc-reader-*`: calls `pandoc.read()`
  - `RawBlock` with format `pandoc-native`: calls `pandoc.read()`
  - `RawBlock` with format `pandoc-json`: calls `pandoc.read()`
  - `Div/Span` with `qmd` or `qmd-base64` attributes: calls `string_to_quarto_ast_blocks()` → `pandoc.read()`

---

## Summary

| Stage | Side Effects | Pandoc API | WASM-Safe |
|-------|--------------|------------|-----------|
| normalize-draft | Pure | None | Yes |
| normalize | Pure | None | Yes |
| normalize-capture-reader-state | Pure | None | Yes |
| astpipeline-process-tables | `FR`, `FW`, `S` | `pandoc.read`, `pandoc.system` | **No** (subprocess) |
| normalize-combined-1 | None* | `pandoc.read`* | Partial* |
| normalize-combine-2 | None | `pandoc.read` | Partial |

**Total**: 6 stages, 1 with heavy side effects, 2-3 with Pandoc API calls

*`normalize-combined-1`: The `extract_latex_quartomarkdown_commands()` only runs for LaTeX output.

**WASM Notes**:
- `astpipeline-process-tables` is **blocking for WASM** when output is Typst (subprocess call to juice.ts)
- For non-Typst output, the main issue is `pandoc.read()` for parsing HTML tables
- `pandoc.read()` calls in normalize-combined-1/2 could potentially be replaced with Rust-native parsing

---

## Data Flow

```
Initialized document (from init filters)
       ↓
[normalize-draft]
  - Checks draft status
  - May clear document for draft-mode=gone
       ↓
[normalize]
  - Normalizes author/affiliation metadata
  - Normalizes citation metadata
  - Normalizes license metadata
  - Parses shortcodes in metadata
       ↓
[normalize-capture-reader-state]
  - Extracts reader options for later use
       ↓
[astpipeline-process-tables]
  - Parses HTML tables → Pandoc Table nodes
  - For Typst: runs juice.ts for CSS inlining
       ↓
[normalize-combined-1]
  - Extracts LaTeX QuartoMarkdown commands
  - Forwards cell subcaps
  - Parses custom nodes (Callout, FloatRefTarget, etc.)
  - Handles code filename attribute
       ↓
[normalize-combine-2]
  - Parses embedded markdown in raw blocks
  - Parses float ref targets
  - Parses block ref targets
       ↓
Quarto AST with custom nodes
```

---

## Key Observations

1. **Pandoc API dependency**: Multiple stages use `pandoc.read()` for parsing embedded content. In the Rust port, this could be replaced with native parsing (pampa).

2. **Typst-specific subprocess**: The `juice.ts` call for Typst output is a blocking I/O operation that cannot run in WASM. Consider:
   - Pre-processing step before WASM
   - Alternative CSS inlining implementation in Rust/WASM

3. **Custom node parsing**: `parse_extended_nodes()` is the key stage that converts Div/Span elements to custom nodes. This is the entry point for the custom node system.
