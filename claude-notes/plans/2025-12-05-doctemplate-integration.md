# Doctemplate Integration Plan

**Beads Issue**: k-y2f3
**Date**: 2025-12-05
**Status**: Draft - pending user review

## Overview

Add document template support to `quarto-markdown-pandoc` with:
- Bundle-based template resolution (WASM-compatible)
- Filesystem-based template resolution (feature-gated)
- Conversion from Pandoc metadata to template context
- CLI and library API integration

## Design Decisions (from user feedback)

1. **MetaInlines/MetaBlocks rendering**: Use target format (HTML writer for HTML templates, etc.)
2. **Bundle format**: Flat map structure `{"main": "...", "partials": {"header": "...", ...}}`
3. **Body writer**: Caller specifies writer format separately; responsible for coherence with template
4. **API scope**: Both modes supported; filesystem access behind cargo feature flag (default: enabled, disabled for WASM)

## Architecture

### New Module Structure

```
quarto-markdown-pandoc/src/
├── template/
│   ├── mod.rs           # Re-exports, feature gates
│   ├── bundle.rs        # Bundle format parsing/creation
│   ├── context.rs       # MetaValue → TemplateValue conversion
│   ├── render.rs        # Template rendering orchestration
│   └── resolver.rs      # BundleResolver wrapper (optional)
```

### Cargo Features

```toml
[features]
default = ["terminal-support", "json-filter", "lua-filter", "template-fs"]
template-fs = []  # Enable FileSystemResolver for templates
```

### Bundle Format (JSON)

```json
{
  "version": "1.0.0",
  "main": "<!DOCTYPE html>\n<html>$body$</html>",
  "partials": {
    "header": "<header>$title$</header>",
    "footer": "<footer>$date$</footer>"
  }
}
```

Version semantics:
- **Missing `version`**: Best-effort parsing, no schema guarantees
- **`version: "1.0.0"`**: Conforms to documented quarto-doctemplate 1.0.0 schema
- Uses semver strings for future compatibility

### Core Types

```rust
/// A template bundle containing main template and partials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateBundle {
    pub main: String,
    #[serde(default)]
    pub partials: HashMap<String, String>,
}

/// Configuration for template rendering
pub struct TemplateRenderConfig {
    /// The compiled template (with resolved partials)
    pub template: Template,
    /// Function to render body content
    pub body_writer: BodyWriter,
    /// Function to render metadata values (MetaInlines, MetaBlocks)
    pub meta_writer: MetaWriter,
}

/// Strategy for rendering the document body
pub enum BodyWriter {
    Html,
    Plaintext,
    Custom(Box<dyn Fn(&Pandoc, &ASTContext) -> Result<String, ...>>),
}

/// Strategy for rendering metadata inlines/blocks
pub enum MetaWriter {
    Html,
    Plaintext,
    Custom(Box<dyn Fn(&Inlines) -> String>, Box<dyn Fn(&Blocks) -> String>),
}
```

## Implementation Phases

### Phase 1: Core Infrastructure

**Goal**: Establish the foundation without any external API changes

1. **Add `quarto-doctemplate` dependency** to `quarto-markdown-pandoc/Cargo.toml`

2. **Create `src/template/bundle.rs`**
   - `TemplateBundle` struct with serde support
   - `TemplateBundle::from_json(json: &str) -> Result<Self, ...>`
   - `TemplateBundle::to_resolver(&self) -> MemoryResolver`

3. **Create `src/template/context.rs`**
   - `meta_to_template_value(meta: &MetaValueWithSourceInfo, meta_writer: &MetaWriter) -> TemplateValue`
   - Handle all variants: MetaString, MetaBool, MetaList, MetaMap, MetaInlines, MetaBlocks
   - `pandoc_to_context(pandoc: &Pandoc, ctx: &ASTContext, body: String, meta_writer: &MetaWriter) -> TemplateContext`
   - Set `body`, `title`, `author`, `date`, and all metadata fields

4. **Create `src/template/render.rs`**
   - `render_with_template(pandoc: &Pandoc, ctx: &ASTContext, config: &TemplateRenderConfig) -> Result<String, ...>`
   - Orchestrate: convert metadata → render body → build context → evaluate template

5. **Create `src/template/mod.rs`**
   - Re-export public types
   - Feature gate `FileSystemResolver` re-export

### Phase 2: Writer Integration

**Goal**: Create a new template-based output path

1. **Create body rendering helpers**
   - `render_body_html(pandoc: &Pandoc, ctx: &ASTContext) -> String`
   - `render_body_plaintext(pandoc: &Pandoc, ctx: &ASTContext) -> String`
   - These wrap existing writers

2. **Create metadata rendering helpers**
   - `render_inlines_html(inlines: &Inlines) -> String`
   - `render_blocks_html(blocks: &Blocks) -> String`
   - Similar for plaintext

3. **Add to `src/writers/mod.rs`**
   - `pub mod template;`

### Phase 3: Library API

**Goal**: Expose clean public API for both modes

1. **Bundle-based API** (always available)
   ```rust
   pub fn render_with_bundle(
       pandoc: &Pandoc,
       context: &ASTContext,
       bundle: &TemplateBundle,
       body_format: OutputFormat,
   ) -> Result<String, Vec<DiagnosticMessage>>
   ```

2. **Filesystem-based API** (feature-gated)
   ```rust
   #[cfg(feature = "template-fs")]
   pub fn render_with_template_file(
       pandoc: &Pandoc,
       context: &ASTContext,
       template_path: &Path,
       body_format: OutputFormat,
   ) -> Result<String, Vec<DiagnosticMessage>>
   ```

3. **Generic API** (for advanced use)
   ```rust
   pub fn render_with_resolver<R: PartialResolver>(
       pandoc: &Pandoc,
       context: &ASTContext,
       template_source: &str,
       resolver: &R,
       body_format: OutputFormat,
   ) -> Result<String, Vec<DiagnosticMessage>>
   ```

### Phase 4: CLI Integration

**Goal**: Add template support to the command line

1. **Add CLI options**
   ```
   --template <PATH>     Use a template file (requires template-fs feature)
   --template-bundle <PATH>  Use a template bundle JSON file
   --body-format <FORMAT>    Format for body content (default: matches output)
   ```

2. **Add built-in templates**
   - Embed default templates (e.g., `html5`, `plain`) in the binary
   - `--template html5` uses built-in template by name

3. **Add export subcommand**
   ```
   quarto-markdown-pandoc export-template <NAME>
   ```
   - Outputs the built-in template as a JSON bundle to stdout
   - Users can customize and use with `--template-bundle`

4. **Update `src/main.rs`**
   - Parse new options
   - Route to template rendering when template is specified

### Phase 5: WASM Entry Points

**Goal**: Enable template rendering in WASM without filesystem

1. **Add to `src/wasm_entry_points/mod.rs`**
   ```rust
   pub fn render_with_template(
       input: &[u8],           // QMD source
       bundle_json: &str,      // Template bundle JSON
       body_format: &str,      // "html", "plain", etc.
   ) -> Result<String, Vec<String>>
   ```

2. **Update `wasm-qmd-parser/src/lib.rs`**
   - Add `#[wasm_bindgen]` function that calls the new entry point

### Phase 6: Testing

1. **Unit tests** for each module
   - `bundle.rs`: JSON parsing, resolver creation
   - `context.rs`: MetaValue conversion, context building
   - `render.rs`: Full rendering pipeline

2. **Integration tests**
   - End-to-end template rendering
   - Bundle vs filesystem mode
   - Various metadata types

3. **WASM tests** (if applicable)
   - Test bundle rendering in wasm-pack test

## Key Considerations

### Error Handling

- Template parsing errors should include source locations
- Undefined variable warnings (not errors by default, matching Pandoc)
- Clear error messages for malformed bundles

### Compatibility

- Follow Pandoc's template semantics where possible
- Document any intentional deviations

### Performance

- Templates are compiled once, can be reused
- Consider caching compiled templates in long-running scenarios

## Dependencies

Current `quarto-markdown-pandoc` dependencies to verify compatibility:
- Uses `quarto-pandoc-types` for AST
- Uses `quarto-source-map` for source tracking

New dependency:
- `quarto-doctemplate` (workspace crate, already exists)

## Resolved Questions

1. **Bundle metadata**: Yes - include `version` field (semver string, e.g., "1.0.0"). Missing version = best-effort parsing.

2. **Template inheritance**: No inheritance at runtime. Templates are self-contained. Built-in templates can be exported via CLI subcommand for customization.

3. **Magic template variables**: Not supported. Only `$body$` is implicitly provided. All other variables must come from document metadata. This maximizes composability and WASM compatibility. See `docs/template-variables.md` for full rationale and Pandoc migration guide.

## Files to Create/Modify

### New Files
- `src/template/mod.rs`
- `src/template/bundle.rs`
- `src/template/context.rs`
- `src/template/render.rs`
- `docs/template-variables.md` (created)

### Modified Files
- `Cargo.toml` (add dependency, features)
- `src/lib.rs` (add `pub mod template`)
- `src/main.rs` (CLI options)
- `src/wasm_entry_points/mod.rs` (new entry point)
- `../wasm-qmd-parser/src/lib.rs` (WASM bindings)

## Next Steps

1. Review this plan and provide feedback
2. Create subtasks in beads for each phase
3. Begin implementation with Phase 1
