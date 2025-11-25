# Plan: pico-quarto-render HTML Template Integration

## Overview

Extend pico-quarto-render to produce complete HTML documents using the quarto-doctemplate crate and embedded HTML templates. This involves:

1. Creating an `EmbeddedResolver` that implements `PartialResolver` for templates bundled via `include_dir`
2. Converting `MetaValueWithSourceInfo` to `TemplateValue` for template evaluation
3. Building a complete template context from document metadata and rendered body
4. Producing full HTML documents using the template system

## Components

### 1. EmbeddedResolver Implementation

**Purpose**: Load templates and partials from embedded resources at compile time.

**Location**: New file `crates/pico-quarto-render/src/embedded_resolver.rs`

```rust
use include_dir::{Dir, include_dir};
use quarto_doctemplate::resolver::{PartialResolver, resolve_partial_path};
use std::path::Path;

static HTML_TEMPLATES: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/resources/html-template");

pub struct EmbeddedResolver;

impl PartialResolver for EmbeddedResolver {
    fn get_partial(&self, name: &str, base_path: &Path) -> Option<String> {
        let partial_path = resolve_partial_path(name, base_path);
        // Get the filename portion for embedded lookup
        let filename = partial_path.file_name()?.to_str()?;
        HTML_TEMPLATES
            .get_file(filename)
            .and_then(|f| f.contents_utf8())
            .map(|s| s.to_string())
    }
}

pub fn get_main_template() -> Option<&'static str> {
    HTML_TEMPLATES
        .get_file("template.html")
        .and_then(|f| f.contents_utf8())
}
```

**Note**: The templates are flat (no subdirectories), so we just need the filename.

### 2. Metadata to TemplateValue Conversion

**Purpose**: Convert Pandoc `MetaValueWithSourceInfo` to `TemplateValue` for template evaluation.

**Location**: New file `crates/pico-quarto-render/src/template_context.rs`

**Key challenge**: `MetaInlines` and `MetaBlocks` need to be rendered to HTML strings before being used in templates.

```rust
use quarto_doctemplate::context::{TemplateContext, TemplateValue};
use quarto_markdown_pandoc::pandoc::{MetaValueWithSourceInfo, MetaMapEntry, Pandoc};
use std::collections::HashMap;

/// Convert MetaValueWithSourceInfo to TemplateValue, rendering Inlines/Blocks to HTML
pub fn meta_to_template_value(meta: &MetaValueWithSourceInfo) -> TemplateValue {
    match meta {
        MetaValueWithSourceInfo::MetaString { value, .. } => {
            TemplateValue::String(value.clone())
        }
        MetaValueWithSourceInfo::MetaBool { value, .. } => {
            TemplateValue::Bool(*value)
        }
        MetaValueWithSourceInfo::MetaInlines { content, .. } => {
            // Render inlines to HTML
            let mut buf = Vec::new();
            quarto_markdown_pandoc::writers::html::write_inlines(content, &mut buf).ok();
            TemplateValue::String(String::from_utf8_lossy(&buf).into_owned())
        }
        MetaValueWithSourceInfo::MetaBlocks { content, .. } => {
            // Render blocks to HTML
            let mut buf = Vec::new();
            quarto_markdown_pandoc::writers::html::write_blocks(content, &mut buf).ok();
            TemplateValue::String(String::from_utf8_lossy(&buf).into_owned())
        }
        MetaValueWithSourceInfo::MetaList { items, .. } => {
            TemplateValue::List(items.iter().map(meta_to_template_value).collect())
        }
        MetaValueWithSourceInfo::MetaMap { entries, .. } => {
            let map = entries
                .iter()
                .map(|e| (e.key.clone(), meta_to_template_value(&e.value)))
                .collect();
            TemplateValue::Map(map)
        }
    }
}

/// Build a template context from a Pandoc document
pub fn build_template_context(pandoc: &Pandoc) -> TemplateContext {
    let mut ctx = TemplateContext::new();

    // Add all metadata fields
    if let MetaValueWithSourceInfo::MetaMap { entries, .. } = &pandoc.meta {
        for entry in entries {
            ctx.insert(&entry.key, meta_to_template_value(&entry.value));
        }
    }

    // Render body content
    let mut body_buf = Vec::new();
    quarto_markdown_pandoc::writers::html::write(pandoc, &mut body_buf).ok();
    ctx.insert("body", TemplateValue::String(String::from_utf8_lossy(&body_buf).into_owned()));

    // Add derived values needed by templates
    add_derived_values(&mut ctx, pandoc);

    ctx
}

fn add_derived_values(ctx: &mut TemplateContext, pandoc: &Pandoc) {
    // pagetitle: Plain-text version of title for <title> element
    // (HTML title element shouldn't contain markup)
    if ctx.get("pagetitle").is_none() {
        if let Some(title_meta) = get_meta_entry(&pandoc.meta, "title") {
            let plain_text = meta_to_plain_text(title_meta);
            ctx.insert("pagetitle", TemplateValue::String(plain_text));
        } else {
            ctx.insert("pagetitle", TemplateValue::String(String::new()));
        }
    }

    // author-meta: Plain-text version of authors for <meta> tags
    if ctx.get("author-meta").is_none() {
        if let Some(author_meta) = get_meta_entry(&pandoc.meta, "author") {
            let author_metas = meta_to_plain_text_list(author_meta);
            ctx.insert("author-meta", TemplateValue::List(
                author_metas.into_iter().map(TemplateValue::String).collect()
            ));
        }
    }

    // date-meta: Plain-text/ISO version of date for <meta> tags
    if ctx.get("date-meta").is_none() {
        if let Some(date_meta) = get_meta_entry(&pandoc.meta, "date") {
            let plain_text = meta_to_plain_text(date_meta);
            ctx.insert("date-meta", TemplateValue::String(plain_text));
        }
    }

    // document-css: Default to true for styling
    if ctx.get("document-css").is_none() {
        ctx.insert("document-css", TemplateValue::Bool(true));
    }

    // lang: Default to "en" if not specified
    if ctx.get("lang").is_none() {
        ctx.insert("lang", TemplateValue::String("en".to_string()));
    }

    // abstract-title: Default to "Abstract"
    if ctx.get("abstract-title").is_none() {
        ctx.insert("abstract-title", TemplateValue::String("Abstract".to_string()));
    }
}

/// Convert MetaValueWithSourceInfo to plain text (strips all formatting)
fn meta_to_plain_text(meta: &MetaValueWithSourceInfo) -> String {
    match meta {
        MetaValueWithSourceInfo::MetaString { value, .. } => value.clone(),
        MetaValueWithSourceInfo::MetaInlines { content, .. } => {
            // Use write_inlines_as_text to get plain text
            quarto_markdown_pandoc::writers::html::write_inlines_as_text_to_string(content)
        }
        MetaValueWithSourceInfo::MetaBlocks { content, .. } => {
            // For blocks, extract text from each block
            // This is a simplification - full implementation would walk all inlines
            String::new() // TODO: implement if needed
        }
        _ => String::new(),
    }
}

/// Convert a list MetaValue to a list of plain text strings
fn meta_to_plain_text_list(meta: &MetaValueWithSourceInfo) -> Vec<String> {
    match meta {
        MetaValueWithSourceInfo::MetaList { items, .. } => {
            items.iter().map(meta_to_plain_text).collect()
        }
        _ => vec![meta_to_plain_text(meta)],
    }
}
```

### 3. HTML Writer Modifications

**Purpose**: Expose functions for rendering Inlines and Blocks to HTML strings, and plain text.

**Location**: `crates/quarto-markdown-pandoc/src/writers/html.rs`

The `write_inlines`, `write_blocks`, and `write_inlines_as_text` functions are currently private. We need to make them public for use in metadata conversion.

```rust
// Make these functions public:
pub fn write_inlines<T: std::io::Write>(inlines: &Inlines, buf: &mut T) -> std::io::Result<()>
pub fn write_blocks<T: std::io::Write>(blocks: &[Block], buf: &mut T) -> std::io::Result<()>
pub fn write_inlines_as_text<T: std::io::Write>(inlines: &Inlines, buf: &mut T) -> std::io::Result<()>

// Add convenience wrappers that return String:
pub fn write_inlines_to_string(inlines: &Inlines) -> String {
    let mut buf = Vec::new();
    write_inlines(inlines, &mut buf).ok();
    String::from_utf8_lossy(&buf).into_owned()
}

pub fn write_blocks_to_string(blocks: &[Block]) -> String {
    let mut buf = Vec::new();
    write_blocks(blocks, &mut buf).ok();
    String::from_utf8_lossy(&buf).into_owned()
}

pub fn write_inlines_as_text_to_string(inlines: &Inlines) -> String {
    let mut buf = Vec::new();
    write_inlines_as_text(inlines, &mut buf).ok();
    String::from_utf8_lossy(&buf).into_owned()
}
```

### 4. Main Integration

**Purpose**: Use the template system in pico-quarto-render to produce complete HTML documents.

**Location**: `crates/pico-quarto-render/src/main.rs`

```rust
mod embedded_resolver;
mod template_context;

use embedded_resolver::{EmbeddedResolver, get_main_template};
use quarto_doctemplate::parser::Template;
use template_context::build_template_context;

fn process_qmd_file(...) -> Result<PathBuf> {
    // ... existing parsing code ...

    // Build template context
    let context = build_template_context(&pandoc);

    // Load and compile template with embedded resolver
    let resolver = EmbeddedResolver;
    let template_path = std::path::Path::new("template.html");
    let template_source = get_main_template()
        .ok_or_else(|| anyhow::anyhow!("Main template not found"))?;

    let template = Template::compile_with_resolver(
        template_source,
        template_path,
        &resolver,
        0,  // depth
    ).map_err(|e| anyhow::anyhow!("Template compilation error: {:?}", e))?;

    // Render the template
    let html_output = template.render(&context)
        .map_err(|e| anyhow::anyhow!("Template rendering error: {:?}", e))?;

    // Write to file
    fs::write(&output_path, html_output)?;

    Ok(output_path)
}
```

### 5. Cargo.toml Updates

**Location**: `crates/pico-quarto-render/Cargo.toml`

```toml
[dependencies]
quarto-markdown-pandoc = { workspace = true }
quarto-doctemplate = { workspace = true }
anyhow.workspace = true
clap = { version = "4.0", features = ["derive"] }
walkdir = "2.5"
include_dir = "0.7"
```

## Task Breakdown

### Phase 1: Infrastructure
1. Add `quarto-doctemplate` and `include_dir` dependencies to pico-quarto-render
2. Create `EmbeddedResolver` implementation
3. Make `write_inlines` and `write_blocks` public in html.rs (or add public wrappers)

### Phase 2: Context Building
4. Create `template_context.rs` with `meta_to_template_value`
5. Implement `build_template_context`
6. Add derived value computation (pagetitle, lang, etc.)

### Phase 3: Integration
7. Update `process_qmd_file` to use templates
8. Add error handling for template compilation/rendering

### Phase 4: Testing
9. Create test QMD files with various metadata configurations
10. Verify output matches expected HTML structure
11. Test with missing metadata (graceful degradation)

## Variables Expected by Templates

From analyzing the templates, these variables are used:

**Required for basic rendering**:
- `body` - rendered document content
- `lang` - document language (default: "en")
- `pagetitle` - page title for <title> tag

**Optional metadata**:
- `title`, `subtitle` - document title block
- `author` (list) - author names
- `date` - publication date
- `abstract`, `abstract-title` - abstract block
- `keywords` (list) - for meta tags
- `css` (list) - additional stylesheets
- `header-includes` (list) - extra head content
- `include-before`, `include-after` (list) - body injections
- `toc`, `toc-title`, `table-of-contents` - table of contents
- `dir` - text direction (ltr/rtl)

**Styling variables**:
- `document-css` (bool) - enable default styles
- `mainfont`, `fontsize`, `fontcolor` - font settings
- `backgroundcolor`, `linkcolor` - colors
- `margin-*`, `maxwidth` - layout

**Math**:
- `math`, `mathjax` - math rendering

## Risks and Considerations

1. **HTML escaping**: Template values containing HTML should be rendered as-is (since MetaInlines/MetaBlocks are already HTML). The template system may need to handle this correctly.

2. **Missing variables**: Templates use `$if(var)$` liberally, so missing variables should be handled gracefully (empty/falsy).

3. **Complex metadata**: Author lists with structured fields (name, affiliation, etc.) need to work with applied partials.

4. **Path resolution**: The `resolve_partial_path` function expects paths relative to a base template. With embedded resources, we need to ensure this works correctly.

## Success Criteria

- Running `pico-quarto-render` on a directory of QMD files produces complete HTML documents
- HTML includes proper DOCTYPE, head with meta tags, styled body with title block
- Metadata from YAML front matter appears correctly in the output
- Missing metadata doesn't cause errors (graceful degradation)
- Partials (metadata.html, title-block.html, styles.html, toc.html) are correctly resolved and included
