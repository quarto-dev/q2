# Unified Filter CLI with Citeproc Support

**Beads Issue:** k-5ywq

## Summary

Replace the separate `--json-filter` and `--lua-filter` CLI options in `quarto-markdown-pandoc` with a single `-F/--filter` option that:

1. Preserves filter ordering across all filter types
2. Supports three filter types: Lua, JSON (external executable), and citeproc (built-in)
3. Detects filter type automatically based on the argument

## Motivation

Currently, `--json-filter` and `--lua-filter` are separate options that get collected into separate `Vec<PathBuf>` arrays. JSON filters always run before Lua filters, making it impossible to interleave them (e.g., `--lua-filter a.lua --json-filter b.py --lua-filter c.lua`).

This matters for citeproc integration because users may want to run filters before or after citation processing.

## Design

### Filter Type Detection

```
--filter "citeproc"     → Built-in citeproc filter
--filter "foo.lua"      → Lua filter (ends with .lua)
--filter "bar.py"       → JSON filter (executable, everything else)
--filter "./path/to/x"  → JSON filter (explicit path)
```

### CLI Changes

**Before:**
```rust
#[arg(long = "json-filter", action = clap::ArgAction::Append)]
json_filters: Vec<std::path::PathBuf>,

#[arg(short = 'L', long = "lua-filter", action = clap::ArgAction::Append)]
lua_filters: Vec<std::path::PathBuf>,
```

**After:**
```rust
#[arg(short = 'F', long = "filter", action = clap::ArgAction::Append)]
filters: Vec<String>,
```

The `-L` short option for Lua filters will be removed. Users should use `-F` for all filter types.

### Filter Specification Type

```rust
// src/unified_filter.rs

pub enum FilterSpec {
    Citeproc,
    Lua(PathBuf),
    Json(PathBuf),
}

impl FilterSpec {
    pub fn parse(s: &str) -> Self {
        if s == "citeproc" {
            FilterSpec::Citeproc
        } else if s.ends_with(".lua") {
            FilterSpec::Lua(PathBuf::from(s))
        } else {
            FilterSpec::Json(PathBuf::from(s))
        }
    }
}
```

### Unified Error Type

```rust
pub enum FilterError {
    JsonFilter(JsonFilterError),
    LuaFilter(LuaFilterError),
    CiteprocFilter(CiteprocFilterError),
}
```

### Citeproc Filter Implementation

The citeproc filter will:

1. Read configuration from document metadata
2. Load CSL style and bibliography references
3. Walk the AST to find `Cite` inlines
4. Process citations using `quarto_citeproc::Processor`
5. Replace `Cite` elements with rendered inline content
6. Optionally append a bibliography `Div` at the document end

#### Metadata Keys

Following Pandoc conventions for compatibility:

| Key | Type | Description |
|-----|------|-------------|
| `csl` | string | Path to CSL style file (default: chicago-author-date) |
| `bibliography` | string or list | Path(s) to bibliography file(s) in CSL-JSON format |
| `lang` | string | Document language for locale selection (default: en-US) |
| `link-citations` | boolean | Wrap citations in hyperlinks to bibliography (default: false) |
| `link-bibliography` | boolean | Add URLs/DOIs as links in bibliography (default: true) |
| `nocite` | string or list | Reference IDs to include in bibliography without citing |
| `suppress-bibliography` | boolean | Don't output bibliography section (default: false) |

Example YAML front matter:
```yaml
---
csl: ieee.csl
bibliography: references.json
lang: en-US
link-citations: true
---
```

## Implementation Phases

### Phase 1: Define Types and CLI Structure

1. Create `src/unified_filter.rs` with `FilterSpec` enum
2. Update `Args` struct in `main.rs`:
   - Add `filters: Vec<String>` with `-F/--filter`
   - Remove `json_filters` and `lua_filters`
3. Parse filter strings into `FilterSpec` values

### Phase 2: Normalize Return Types

Currently:
- JSON filter returns: `(Pandoc, ASTContext)`
- Lua filter returns: `(Pandoc, ASTContext, Vec<DiagnosticMessage>)`

Update JSON filter to also return diagnostics for consistency:
```rust
pub fn apply_json_filter(...) -> Result<(Pandoc, ASTContext, Vec<DiagnosticMessage>), JsonFilterError>
```

### Phase 3: Unified Filter Application

Create unified application function:

```rust
pub fn apply_filter(
    pandoc: Pandoc,
    context: ASTContext,
    filter: &FilterSpec,
    target_format: &str,
) -> Result<(Pandoc, ASTContext, Vec<DiagnosticMessage>), FilterError>
```

Update `main.rs` to use single loop:

```rust
let mut all_diagnostics = Vec::new();
for filter_str in &args.filters {
    let filter = FilterSpec::parse(filter_str);
    let (new_pandoc, new_context, diagnostics) =
        apply_filter(pandoc, context, &filter, &args.to)?;
    pandoc = new_pandoc;
    context = new_context;
    all_diagnostics.extend(diagnostics);
}
```

### Phase 4: Citeproc Integration

1. Add dependency to `Cargo.toml`:
   ```toml
   quarto-citeproc = { path = "../quarto-citeproc" }
   ```

2. Create `src/citeproc_filter.rs`:
   - `extract_citeproc_config(meta) -> CiteprocConfig` - read metadata
   - `load_bibliography(path) -> Vec<Reference>` - parse CSL-JSON
   - `load_csl_style(path) -> Style` - load and parse CSL
   - `apply_citeproc_filter(pandoc, context, target_format) -> Result<...>`

3. The filter walks the AST using the existing filter infrastructure in `filters.rs`

4. For each `Cite` inline:
   - Build `Citation` from the cite's items
   - Call `processor.process_citation_to_output()`
   - Convert output to `Vec<Inline>`
   - Replace the `Cite` with rendered inlines

5. After processing all citations:
   - Call `processor.generate_bibliography_to_outputs()`
   - Convert to blocks and append to document (unless suppressed)

### Phase 5: Testing

1. **Unit tests** for `FilterSpec::parse()`
2. **Integration tests** for filter ordering:
   - Verify `-F a.lua -F b.py -F c.lua` applies in order
   - Verify citeproc can be interleaved with other filters
3. **Citeproc tests**:
   - Basic citation rendering
   - Bibliography generation
   - Various CSL styles
   - Metadata configuration options
4. **Roundtrip tests** if applicable

## Files to Modify

| File | Changes |
|------|---------|
| `Cargo.toml` | Add `quarto-citeproc` dependency |
| `src/main.rs` | Update CLI args, unified filter loop |
| `src/unified_filter.rs` | New: `FilterSpec`, unified apply function |
| `src/citeproc_filter.rs` | New: citeproc filter implementation |
| `src/json_filter.rs` | Update return type to include diagnostics |
| `src/lib.rs` or `src/mod.rs` | Export new modules |

## Open Questions (Resolved)

1. ~~PathBuf vs String for JSON filters~~ → Use `PathBuf` for explicit paths
2. ~~Citeproc configuration discovery~~ → Use document metadata
3. ~~Feature flag for citeproc~~ → Not needed; locales are embedded (1.5MB, 60 files via rust_embed)
4. ~~Short option for --filter~~ → Use `-F` (matches Pandoc)

## Future Considerations

- Default CSL style bundling (currently requires user to provide path)
- BibTeX/BibLaTeX bibliography format support
- Citation link customization
- Multiple bibliography sections
- Nocite patterns (e.g., `@*` for all references)
