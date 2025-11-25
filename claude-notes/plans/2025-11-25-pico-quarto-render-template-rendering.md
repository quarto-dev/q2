# Plan: Template Rendering for pico-quarto-render

## Overview

This plan extends pico-quarto-render to produce complete HTML documents using quarto-doctemplate and embedded templates. The architecture separates concerns into:

1. **Metadata preparation** - Deriving values like `pagetitle` from document metadata
2. **Metadata conversion** - Converting `MetaValueWithSourceInfo` to `TemplateValue` with format-specific writers
3. **Template rendering** - Evaluating the template with the context

## Architecture

### Data Flow

```
Pandoc Document
    │
    ▼
prepare_template_metadata()     ← Mutates metadata (adds pagetitle, etc.)
    │
    ▼
render_with_template()          ← Converts to TemplateContext, renders body, evaluates template
    │
    ├── meta_to_template_value()    ← Converts MetaValueWithSourceInfo using format-specific writers
    │       ├── MetaString → TemplateValue::String (literal)
    │       ├── MetaBool → TemplateValue::Bool
    │       ├── MetaInlines → TemplateValue::String (write_inlines → String)
    │       ├── MetaBlocks → TemplateValue::String (write_blocks → String)
    │       ├── MetaList → TemplateValue::List (recursive)
    │       └── MetaMap → TemplateValue::Map (recursive)
    │
    ├── write_blocks(&pandoc.blocks) → "body" key
    │
    └── template.render(&context) → final HTML string
    │
    ▼
HTML Output
```

## Components

### 1. prepare_template_metadata

**Purpose**: Mutate document metadata to add derived values needed by templates.

**Signature**:
```rust
/// Prepare document metadata for template rendering.
///
/// This mutates the document to add derived metadata fields:
/// - `pagetitle`: Plain-text version of `title` (for HTML <title> element)
/// - More fields can be added in the future (author-meta, date-meta, etc.)
pub fn prepare_template_metadata(pandoc: &mut Pandoc)
```

**Implementation**:
```rust
use quarto_markdown_pandoc::writers::plaintext;

pub fn prepare_template_metadata(pandoc: &mut Pandoc) {
    // Only mutate if meta is a MetaMap
    let MetaValueWithSourceInfo::MetaMap { entries, source_info } = &mut pandoc.meta else {
        return;
    };

    // Check if pagetitle already exists
    let has_pagetitle = entries.iter().any(|e| e.key == "pagetitle");
    if has_pagetitle {
        return;
    }

    // Look for title field
    let title_entry = entries.iter().find(|e| e.key == "title");
    if let Some(entry) = title_entry {
        let plain_text = match &entry.value {
            MetaValueWithSourceInfo::MetaString { value, .. } => value.clone(),
            MetaValueWithSourceInfo::MetaInlines { content, .. } => {
                let (text, _diagnostics) = plaintext::inlines_to_string(content);
                text
            }
            MetaValueWithSourceInfo::MetaBlocks { content, .. } => {
                let (text, _diagnostics) = plaintext::blocks_to_string(content);
                text
            }
            _ => return, // Other types: skip
        };

        // Add pagetitle entry
        entries.push(MetaMapEntry {
            key: "pagetitle".to_string(),
            key_source: source_info.clone(), // Use the map's source_info
            value: MetaValueWithSourceInfo::MetaString {
                value: plain_text,
                source_info: source_info.clone(),
            },
        });
    }
}
```

**Note**: This function mutates the document in place. The derived `pagetitle` is a `MetaString`, not `MetaInlines`. This is important because:
- It's plain text (no formatting)
- The template engine will output it literally
- The HTML writer will not need to render it (it's already a string)

### 2. FormatWriters trait/struct

**Purpose**: Abstract over format-specific block/inline writers.

**Signature**:
```rust
/// Format-specific writers for converting Pandoc AST to strings.
pub trait FormatWriters {
    /// Write blocks to a string.
    fn write_blocks(&self, blocks: &[Block]) -> Result<String, Error>;

    /// Write inlines to a string.
    fn write_inlines(&self, inlines: &Inlines) -> Result<String, Error>;
}

/// HTML format writers.
pub struct HtmlWriters;

impl FormatWriters for HtmlWriters {
    fn write_blocks(&self, blocks: &[Block]) -> Result<String, Error> {
        let mut buf = Vec::new();
        quarto_markdown_pandoc::writers::html::write_blocks(blocks, &mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn write_inlines(&self, inlines: &Inlines) -> Result<String, Error> {
        let mut buf = Vec::new();
        quarto_markdown_pandoc::writers::html::write_inlines(inlines, &mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}
```

### 3. meta_to_template_value

**Purpose**: Convert `MetaValueWithSourceInfo` to `TemplateValue` using format-specific writers.

**Signature**:
```rust
/// Convert document metadata to template values.
///
/// This recursively converts the metadata structure:
/// - MetaString → TemplateValue::String (literal, no rendering)
/// - MetaBool → TemplateValue::Bool
/// - MetaInlines → TemplateValue::String (rendered via format writers)
/// - MetaBlocks → TemplateValue::String (rendered via format writers)
/// - MetaList → TemplateValue::List (recursive)
/// - MetaMap → TemplateValue::Map (recursive)
fn meta_to_template_value<W: FormatWriters>(
    meta: &MetaValueWithSourceInfo,
    writers: &W,
) -> Result<TemplateValue, Error>
```

**Implementation**:
```rust
fn meta_to_template_value<W: FormatWriters>(
    meta: &MetaValueWithSourceInfo,
    writers: &W,
) -> Result<TemplateValue, Error> {
    Ok(match meta {
        MetaValueWithSourceInfo::MetaString { value, .. } => {
            // MetaString is already a plain string - use as literal
            TemplateValue::String(value.clone())
        }
        MetaValueWithSourceInfo::MetaBool { value, .. } => {
            TemplateValue::Bool(*value)
        }
        MetaValueWithSourceInfo::MetaInlines { content, .. } => {
            // Render inlines using format-specific writer
            TemplateValue::String(writers.write_inlines(content)?)
        }
        MetaValueWithSourceInfo::MetaBlocks { content, .. } => {
            // Render blocks using format-specific writer
            TemplateValue::String(writers.write_blocks(content)?)
        }
        MetaValueWithSourceInfo::MetaList { items, .. } => {
            let values: Result<Vec<_>, _> = items
                .iter()
                .map(|item| meta_to_template_value(item, writers))
                .collect();
            TemplateValue::List(values?)
        }
        MetaValueWithSourceInfo::MetaMap { entries, .. } => {
            let mut map = std::collections::HashMap::new();
            for entry in entries {
                map.insert(
                    entry.key.clone(),
                    meta_to_template_value(&entry.value, writers)?,
                );
            }
            TemplateValue::Map(map)
        }
    })
}
```

### 4. render_with_template

**Purpose**: Main entry point for template-based rendering.

**Signature**:
```rust
/// Render a document using a template.
///
/// # Arguments
/// - `pandoc` - The document (should have been through prepare_template_metadata)
/// - `template` - A compiled template with partials resolved
/// - `writers` - Format-specific writers for metadata conversion
///
/// # Returns
/// The rendered document as a string, or an error.
pub fn render_with_template<W: FormatWriters>(
    pandoc: &Pandoc,
    template: &Template,
    writers: &W,
) -> Result<String, Error>
```

**Implementation**:
```rust
pub fn render_with_template<W: FormatWriters>(
    pandoc: &Pandoc,
    template: &Template,
    writers: &W,
) -> Result<String, Error> {
    // 1. Convert metadata to TemplateValue::Map
    let meta_value = meta_to_template_value(&pandoc.meta, writers)?;

    // 2. Build TemplateContext from metadata
    let mut context = TemplateContext::new();
    if let TemplateValue::Map(map) = meta_value {
        for (key, value) in map {
            context.insert(key, value);
        }
    }

    // 3. Render document body and add to context
    let body = writers.write_blocks(&pandoc.blocks)?;
    context.insert("body", TemplateValue::String(body));

    // 4. Evaluate template
    let output = template.render(&context)
        .map_err(|e| anyhow::anyhow!("Template error: {:?}", e))?;

    Ok(output)
}
```

### 5. EmbeddedResolver

**Purpose**: Load templates from compiled-in resources via `include_dir`.

**Location**: `crates/pico-quarto-render/src/embedded_resolver.rs`

**Implementation**:
```rust
use include_dir::{Dir, include_dir};
use quarto_doctemplate::resolver::{PartialResolver, resolve_partial_path};
use std::path::Path;

static HTML_TEMPLATES: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/resources/html-template");

/// Resolver that loads templates from embedded resources.
pub struct EmbeddedResolver;

impl PartialResolver for EmbeddedResolver {
    fn get_partial(&self, name: &str, base_path: &Path) -> Option<String> {
        // Resolve the partial path following Pandoc rules
        let partial_path = resolve_partial_path(name, base_path);

        // Get the filename portion for embedded lookup
        // (templates are flat, so we just need the filename)
        let filename = partial_path.file_name()?.to_str()?;

        HTML_TEMPLATES
            .get_file(filename)
            .and_then(|f| f.contents_utf8())
            .map(|s| s.to_string())
    }
}

/// Get the main template source.
pub fn get_main_template() -> Option<&'static str> {
    HTML_TEMPLATES
        .get_file("template.html")
        .and_then(|f| f.contents_utf8())
}
```

### 6. Updated process_qmd_file

**Location**: `crates/pico-quarto-render/src/main.rs`

```rust
fn process_qmd_file(
    qmd_path: &Path,
    input_dir: &Path,
    output_dir: &Path,
    verbose: u8,
) -> Result<PathBuf> {
    // Read and parse QMD file (existing code)...
    let (mut pandoc, _context, warnings) = /* ... */;

    // NEW: Prepare template metadata
    prepare_template_metadata(&mut pandoc);

    // NEW: Load and compile template
    let template_source = embedded_resolver::get_main_template()
        .ok_or_else(|| anyhow::anyhow!("Main template not found"))?;
    let resolver = embedded_resolver::EmbeddedResolver;
    let template = Template::compile_with_resolver(
        template_source,
        Path::new("template.html"),
        &resolver,
        0,
    ).map_err(|e| anyhow::anyhow!("Template compilation error: {:?}", e))?;

    // NEW: Render with template
    let writers = HtmlWriters;
    let html_output = render_with_template(&pandoc, &template, &writers)?;

    // Write output (existing code)...
    fs::write(&output_path, html_output)?;

    Ok(output_path)
}
```

## Dependencies to Add

`crates/pico-quarto-render/Cargo.toml`:
```toml
[dependencies]
quarto-markdown-pandoc = { workspace = true }
quarto-doctemplate = { workspace = true }
anyhow.workspace = true
clap = { version = "4.0", features = ["derive"] }
walkdir = "2.5"
include_dir = "0.7"
```

## File Structure

```
crates/pico-quarto-render/
├── Cargo.toml                           # Updated with new dependencies
├── src/
│   ├── main.rs                          # Updated with template rendering
│   ├── embedded_resolver.rs             # NEW: EmbeddedResolver
│   ├── template_context.rs              # NEW: meta_to_template_value, prepare_template_metadata
│   └── format_writers.rs                # NEW: FormatWriters trait and HtmlWriters
└── src/resources/
    └── html-template/
        ├── template.html
        ├── title-block.html
        ├── metadata.html
        ├── styles.html
        ├── toc.html
        └── html.styles
```

## Task Breakdown

### Phase 1: Expose HTML writers (prerequisite)
1. Make `write_inlines` and `write_blocks` public in `quarto-markdown-pandoc/src/writers/html.rs`

### Phase 2: Core infrastructure
2. Add dependencies to pico-quarto-render (quarto-doctemplate, include_dir)
3. Create `embedded_resolver.rs` with `EmbeddedResolver` and `get_main_template()`
4. Create `format_writers.rs` with `FormatWriters` trait and `HtmlWriters`

### Phase 3: Metadata and template rendering
5. Create `template_context.rs` with:
   - `prepare_template_metadata()` - adds `pagetitle` from `title`
   - `meta_to_template_value()` - converts metadata using format writers

6. Create `render_with_template()` function that:
   - Converts metadata to TemplateContext
   - Renders body with format writer
   - Evaluates template

### Phase 4: Integration
7. Update `process_qmd_file()` in main.rs to use the new template rendering
8. Test with sample QMD files

## Open Questions

1. **Error handling**: Should template errors be fatal or warnings?
   - Proposed: Fatal for now (template errors indicate bugs)

2. **Caching**: Should compiled templates be cached across files?
   - Proposed: Not initially (simple is better)

3. **Diagnostics**: Should we use `render_with_diagnostics` and surface warnings?
   - Proposed: Yes, surface warnings about undefined variables

4. **Default values**: Should we add more derived values (lang, abstract-title)?
   - Proposed: Add minimally needed defaults in `prepare_template_metadata`

## Future Extensions

- **author-meta, date-meta**: Plain-text versions for `<meta>` tags
- **table-of-contents**: Generated TOC HTML
- **quarto-version**: Version string for templates
- **Multiple templates**: Support for different output formats
- **Template overrides**: Allow user-provided templates
