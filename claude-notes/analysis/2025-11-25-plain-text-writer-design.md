# Analysis: Plain-Text Writer Design for Template Context Preparation

## Executive Summary

The proposed design is **sound and preferable** to Pandoc's approach. The key insight is that by keeping the typed `MetaValue` structure longer and adding a plain-text writer, we can:

1. Compute derived fields (`pagetitle`, `author-meta`, etc.) directly from `MetaInlines` using plain-text rendering
2. Avoid the inefficiency of rendering to HTML and then "stringifying" it back
3. Maintain a cleaner separation of concerns

This analysis examines the relevant code structures and validates the feasibility of this approach.

---

## 1. Current Architecture

### 1.1 quarto-doctemplate is Intentionally Decoupled

The doctemplate crate's `lib.rs` explicitly states:

> The template engine is **independent of Pandoc AST types**. It defines its own `TemplateValue` and `TemplateContext` types. Conversion from Pandoc's `MetaValue` to `TemplateValue` happens in the writer layer (not in this crate).

This is the correct design. The template engine should not know about Pandoc internals.

### 1.2 TemplateValue is Simple and Literal

```rust
pub enum TemplateValue {
    String(String),    // Rendered as-is, no transformation
    Bool(bool),        // true → "true", false → ""
    List(Vec<TemplateValue>),
    Map(HashMap<String, TemplateValue>),
    Null,              // Renders as empty string
}
```

**Critical observation**: When `TemplateValue::String` is rendered via `to_doc()`, it becomes `Doc::Text(s)` which renders as `s.clone()` — **no HTML escaping, no transformation**. The template engine outputs strings literally.

This is the correct behavior for a document template engine. It assumes that values have been properly prepared for the target format.

### 1.3 MetaValueWithSourceInfo has Typed Structure

```rust
pub enum MetaValueWithSourceInfo {
    MetaString { value: String, source_info: ... },
    MetaBool { value: bool, source_info: ... },
    MetaInlines { content: Inlines, source_info: ... },  // Rich text
    MetaBlocks { content: Blocks, source_info: ... },    // Rich blocks
    MetaList { items: Vec<MetaValueWithSourceInfo>, ... },
    MetaMap { entries: Vec<MetaMapEntry>, ... },
}
```

The key variants are:
- **MetaString**: Already a plain string (e.g., from `!str` tag or numeric values)
- **MetaInlines**: Rich inline content (bold, links, etc.) that needs rendering
- **MetaBlocks**: Rich block content (paragraphs, lists, etc.) that needs rendering

---

## 2. The Two Rendering Contexts

When converting metadata to template values, we need to consider the **output context**:

### 2.1 HTML Content Context

For fields that will be inserted into HTML content (like `<h1>$title$</h1>`), MetaInlines should be rendered to **HTML**:

```yaml
title: "My *Bold* Title"
```

Should become: `"My <em>Bold</em> Title"` for the template's `title` variable.

### 2.2 Plain Text Context

For fields used in HTML attributes or the `<title>` element, MetaInlines should be rendered to **plain text**:

```yaml
title: "My *Bold* Title"
```

Should become: `"My Bold Title"` for the template's `pagetitle` variable.

The `<title>` element cannot contain HTML markup — browsers would show literal `<em>` tags.

---

## 3. Current State: write_inlines_as_text in html.rs

There's already a function that extracts plain text from inlines:

```rust
fn write_inlines_as_text<T: std::io::Write>(inlines: &Inlines, buf: &mut T) -> std::io::Result<()> {
    for inline in inlines {
        match inline {
            Inline::Str(s) => write!(buf, "{}", escape_html(&s.text))?,  // ← Note: escapes HTML
            Inline::Space(_) => write!(buf, " ")?,
            Inline::Emph(e) => write_inlines_as_text(&e.content, buf)?,
            Inline::Strong(s) => write_inlines_as_text(&s.content, buf)?,
            // ... etc
        }
    }
}
```

**Issue**: This function calls `escape_html()` because it's designed for HTML alt-text contexts. A true plain-text writer should NOT escape HTML entities.

---

## 4. Proposed Design

### 4.1 Add a New Plain-Text Writer Module

Create `crates/quarto-markdown-pandoc/src/writers/plaintext.rs`:

```rust
//! Pure plain-text writer for Pandoc AST.
//!
//! This writer produces plain text with no HTML escaping or markup.
//! It's used for generating metadata values that will appear in
//! plain-text contexts (HTML <title>, meta tags, etc.)
//!
//! Design decisions:
//! - RawInline/RawBlock: echo contents if format is "plaintext", otherwise drop with warning
//! - Unsupported nodes emit diagnostic warnings and are dropped
//! - No HTML escaping (unlike write_inlines_as_text in html.rs)

use crate::pandoc::{Block, Inline, Inlines};
use quarto_error_reporting::DiagnosticMessage;

/// Context for plain-text writing, threading diagnostics through.
pub struct PlainTextWriterContext {
    diagnostics: Vec<DiagnosticMessage>,
}

impl PlainTextWriterContext {
    pub fn new() -> Self {
        Self { diagnostics: Vec::new() }
    }

    pub fn into_diagnostics(self) -> Vec<DiagnosticMessage> {
        self.diagnostics
    }

    fn warn_dropped_node(&mut self, description: &str, source_info: &quarto_source_map::SourceInfo) {
        use quarto_error_reporting::DiagnosticMessageBuilder;
        let diag = DiagnosticMessageBuilder::warning(
            format!("Node dropped in plain-text output: {}", description)
        )
            .with_location(source_info.clone())
            .build();
        self.diagnostics.push(diag);
    }
}

impl Default for PlainTextWriterContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Write inlines as pure plain text (no escaping, no markup)
pub fn write_inlines<T: std::io::Write>(
    inlines: &Inlines,
    buf: &mut T,
    ctx: &mut PlainTextWriterContext,
) -> std::io::Result<()> {
    for inline in inlines {
        match inline {
            Inline::Str(s) => write!(buf, "{}", s.text)?,  // No escape_html!
            Inline::Space(_) => write!(buf, " ")?,
            Inline::SoftBreak(_) | Inline::LineBreak(_) => write!(buf, " ")?,
            Inline::Emph(e) => write_inlines(&e.content, buf, ctx)?,
            Inline::Strong(s) => write_inlines(&s.content, buf, ctx)?,
            Inline::Underline(u) => write_inlines(&u.content, buf, ctx)?,
            Inline::Strikeout(s) => write_inlines(&s.content, buf, ctx)?,
            Inline::Superscript(s) => write_inlines(&s.content, buf, ctx)?,
            Inline::Subscript(s) => write_inlines(&s.content, buf, ctx)?,
            Inline::SmallCaps(s) => write_inlines(&s.content, buf, ctx)?,
            Inline::Span(span) => write_inlines(&span.content, buf, ctx)?,
            Inline::Quoted(q) => {
                // Use actual quote characters
                let (open, close) = match q.quote_type {
                    QuoteType::SingleQuote => ("'", "'"),
                    QuoteType::DoubleQuote => (""", """),
                };
                write!(buf, "{}", open)?;
                write_inlines(&q.content, buf, ctx)?;
                write!(buf, "{}", close)?;
            }
            Inline::Code(c) => write!(buf, "{}", c.text)?,
            Inline::Math(m) => write!(buf, "{}", m.text)?,  // Raw TeX
            Inline::Link(link) => write_inlines(&link.content, buf, ctx)?,
            Inline::Image(image) => write_inlines(&image.content, buf, ctx)?,
            Inline::RawInline(raw) => {
                if raw.format == "plaintext" {
                    write!(buf, "{}", raw.text)?;
                } else {
                    ctx.warn_dropped_node(
                        &format!("RawInline with format '{}'", raw.format),
                        &raw.source_info,
                    );
                }
            }
            Inline::Note(_) => {}  // Skip footnotes (no warning - expected behavior)
            Inline::Cite(cite) => write_inlines(&cite.content, buf, ctx)?,
            // Quarto extensions - drop with warning
            Inline::Shortcode(sc) => {
                ctx.warn_dropped_node("Shortcode", &sc.source_info);
            }
            Inline::NoteReference(nr) => {
                ctx.warn_dropped_node("NoteReference", &nr.source_info);
            }
            Inline::Attr(_, source_info) => {
                ctx.warn_dropped_node("Attr", source_info);
            }
            // CriticMarkup extensions
            Inline::Insert(ins) => write_inlines(&ins.content, buf, ctx)?,
            Inline::Delete(del) => write_inlines(&del.content, buf, ctx)?,
            Inline::Highlight(h) => write_inlines(&h.content, buf, ctx)?,
            Inline::EditComment(c) => write_inlines(&c.content, buf, ctx)?,
        }
    }
    Ok(())
}

/// Write blocks as plain text
pub fn write_blocks<T: std::io::Write>(
    blocks: &[Block],
    buf: &mut T,
    ctx: &mut PlainTextWriterContext,
) -> std::io::Result<()> {
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            write!(buf, "\n")?;  // Separate blocks with newline
        }
        write_block(block, buf, ctx)?;
    }
    Ok(())
}

fn write_block<T: std::io::Write>(
    block: &Block,
    buf: &mut T,
    ctx: &mut PlainTextWriterContext,
) -> std::io::Result<()> {
    match block {
        Block::Plain(p) => write_inlines(&p.content, buf, ctx)?,
        Block::Paragraph(p) => write_inlines(&p.content, buf, ctx)?,
        Block::Header(h) => write_inlines(&h.content, buf, ctx)?,
        Block::CodeBlock(c) => write!(buf, "{}", c.text)?,
        Block::BlockQuote(q) => write_blocks(&q.content, buf, ctx)?,
        Block::RawBlock(raw) => {
            if raw.format == "plaintext" {
                write!(buf, "{}", raw.text)?;
            } else {
                ctx.warn_dropped_node(
                    &format!("RawBlock with format '{}'", raw.format),
                    &raw.source_info,
                );
            }
        }
        // Quarto extensions - drop with warning
        Block::BlockMetadata(bm) => {
            ctx.warn_dropped_node("BlockMetadata", &bm.source_info);
        }
        Block::NoteDefinitionPara(nd) => {
            ctx.warn_dropped_node("NoteDefinitionPara", &nd.source_info);
        }
        Block::NoteDefinitionFencedBlock(nd) => {
            ctx.warn_dropped_node("NoteDefinitionFencedBlock", &nd.source_info);
        }
        Block::CaptionBlock(cb) => {
            ctx.warn_dropped_node("CaptionBlock", &cb.source_info);
        }
        // ... handle other block types
    }
    Ok(())
}

// Convenience functions that return (String, Vec<DiagnosticMessage>)
pub fn inlines_to_string(inlines: &Inlines) -> (String, Vec<DiagnosticMessage>) {
    let mut buf = Vec::new();
    let mut ctx = PlainTextWriterContext::new();
    write_inlines(inlines, &mut buf, &mut ctx).ok();
    (String::from_utf8_lossy(&buf).into_owned(), ctx.into_diagnostics())
}

pub fn blocks_to_string(blocks: &[Block]) -> (String, Vec<DiagnosticMessage>) {
    let mut buf = Vec::new();
    let mut ctx = PlainTextWriterContext::new();
    write_blocks(blocks, &mut buf, &mut ctx).ok();
    (String::from_utf8_lossy(&buf).into_owned(), ctx.into_diagnostics())
}
```

### 4.2 Template Context Preparation in pico-quarto-render

The preparation step converts `MetaValueWithSourceInfo` to `TemplateValue`:

```rust
use quarto_markdown_pandoc::writers::{html, plaintext};

/// Convert metadata to template value for HTML content contexts
fn meta_to_html_value(meta: &MetaValueWithSourceInfo) -> TemplateValue {
    match meta {
        MetaValueWithSourceInfo::MetaString { value, .. } => {
            // Plain string - use as-is (will be output literally by template)
            TemplateValue::String(value.clone())
        }
        MetaValueWithSourceInfo::MetaBool { value, .. } => {
            TemplateValue::Bool(*value)
        }
        MetaValueWithSourceInfo::MetaInlines { content, .. } => {
            // Render to HTML for insertion into HTML content
            TemplateValue::String(html::inlines_to_string(content))
        }
        MetaValueWithSourceInfo::MetaBlocks { content, .. } => {
            TemplateValue::String(html::blocks_to_string(content))
        }
        MetaValueWithSourceInfo::MetaList { items, .. } => {
            TemplateValue::List(items.iter().map(meta_to_html_value).collect())
        }
        MetaValueWithSourceInfo::MetaMap { entries, .. } => {
            TemplateValue::Map(
                entries.iter()
                    .map(|e| (e.key.clone(), meta_to_html_value(&e.value)))
                    .collect()
            )
        }
    }
}

/// Convert metadata to plain text (for pagetitle, author-meta, etc.)
fn meta_to_plain_text(meta: &MetaValueWithSourceInfo) -> String {
    match meta {
        MetaValueWithSourceInfo::MetaString { value, .. } => value.clone(),
        MetaValueWithSourceInfo::MetaInlines { content, .. } => {
            plaintext::inlines_to_string(content)
        }
        MetaValueWithSourceInfo::MetaBlocks { content, .. } => {
            plaintext::blocks_to_string(content)
        }
        _ => String::new(),
    }
}

/// Build the template context with derived values
fn build_template_context(pandoc: &Pandoc) -> TemplateContext {
    let mut ctx = TemplateContext::new();

    // Add all metadata fields as HTML-rendered values
    if let MetaValueWithSourceInfo::MetaMap { entries, .. } = &pandoc.meta {
        for entry in entries {
            ctx.insert(&entry.key, meta_to_html_value(&entry.value));
        }
    }

    // Render body content
    ctx.insert("body", TemplateValue::String(html::document_to_string(pandoc)));

    // Derive plain-text values from MetaInlines
    derive_plain_text_fields(&mut ctx, &pandoc.meta);

    ctx
}

fn derive_plain_text_fields(ctx: &mut TemplateContext, meta: &MetaValueWithSourceInfo) {
    // pagetitle: plain-text version of title
    if ctx.get("pagetitle").is_none() {
        if let Some(title) = meta.get("title") {
            ctx.insert("pagetitle", TemplateValue::String(meta_to_plain_text(title)));
        }
    }

    // author-meta: plain-text version of author(s)
    if ctx.get("author-meta").is_none() {
        if let Some(author) = meta.get("author") {
            match author {
                MetaValueWithSourceInfo::MetaList { items, .. } => {
                    let texts: Vec<TemplateValue> = items.iter()
                        .map(|a| TemplateValue::String(meta_to_plain_text(a)))
                        .collect();
                    ctx.insert("author-meta", TemplateValue::List(texts));
                }
                _ => {
                    ctx.insert("author-meta", TemplateValue::List(vec![
                        TemplateValue::String(meta_to_plain_text(author))
                    ]));
                }
            }
        }
    }

    // date-meta: plain-text version of date
    if ctx.get("date-meta").is_none() {
        if let Some(date) = meta.get("date") {
            ctx.insert("date-meta", TemplateValue::String(meta_to_plain_text(date)));
        }
    }
}
```

---

## 5. Why This is Better Than Pandoc's Approach

### 5.1 Pandoc's Approach (Inferred)

Based on the user's analysis of Pandoc's code:

```haskell
defField "pagetitle" (literal . stringifyHTML . docTitle $ meta)
```

This suggests:
1. `docTitle` extracts the title (already rendered to HTML?)
2. `stringifyHTML` converts HTML back to plain text
3. `literal` wraps it for the template

This is inefficient: render to HTML, then parse/strip HTML to get plain text.

### 5.2 Our Approach

1. Keep `MetaInlines` as typed structure
2. For `title`: render to HTML string
3. For `pagetitle`: render to plain text string **directly from MetaInlines**

No intermediate HTML parsing required.

### 5.3 Correctness Advantages

- **MetaString is preserved**: If a user writes `title: !str "My Title"`, it stays as a plain string throughout. No risk of HTML escaping issues.
- **Clear semantics**: Each conversion has a clear purpose:
  - `meta_to_html_value()`: for insertion into HTML content
  - `meta_to_plain_text()`: for plain-text contexts
- **No double-escaping risk**: Since we know when we're producing HTML vs plain text, we can apply escaping correctly.

---

## 6. Implementation Considerations

### 6.1 What About MetaString with HTML Characters?

If a user writes:
```yaml
title: !str "<script>alert('hi')</script>"
```

This becomes `MetaString` which is used as-is. When inserted into HTML via the template:
```html
<h1>$title$</h1>
```

The output would be:
```html
<h1><script>alert('hi')</script></h1>
```

This is **correct behavior** — the user explicitly requested no markdown parsing with `!str`. They're responsible for the content.

If they wanted safe output, they should not use `!str` and let the markdown parser handle it.

### 6.2 Template Literal Output Considerations

The template engine outputs strings literally. This means:
- **HTML templates**: Values should be properly escaped or formatted for HTML
- **LaTeX templates**: Values should be properly escaped for LaTeX

This is the responsibility of the context-building code (in pico-quarto-render or equivalent), not the template engine.

### 6.3 The html::inlines_to_string Question

When rendering MetaInlines to HTML for template variables, should we include block-level HTML tags for block content, or inline-only?

For example, if `abstract` contains multiple paragraphs:
- Should `$abstract$` in a template produce `<p>...</p><p>...</p>`?
- Or just the text without `<p>` tags?

Looking at the templates:
```html
<div class="abstract">
<div class="abstract-title">$abstract-title$</div>
$abstract$
</div>
```

The `$abstract$` appears in a block context, so it should include `<p>` tags. This matches Pandoc's behavior.

---

## 7. Conclusion

The proposed design is sound:

1. **Feasibility**: ✅ The existing crate structure supports this cleanly
2. **Type safety**: ✅ `MetaString` vs `MetaInlines` distinction is preserved
3. **Efficiency**: ✅ No HTML→plaintext round-trip needed
4. **Correctness**: ✅ Clear semantics for each conversion path
5. **Maintainability**: ✅ Separation of concerns (template engine knows nothing about HTML)

### Recommended Implementation Order

1. Add `writers/plaintext.rs` to quarto-markdown-pandoc
2. Expose `write_inlines`, `write_blocks` (and string wrappers) as public API in both `html.rs` and `plaintext.rs`
3. Implement context-building in pico-quarto-render with the two conversion functions
4. Derive `pagetitle`, `author-meta`, `date-meta` from MetaInlines using plaintext writer
5. Test with various metadata configurations

### Files to Modify/Create

| File | Action |
|------|--------|
| `quarto-markdown-pandoc/src/writers/mod.rs` | Add `pub mod plaintext;` |
| `quarto-markdown-pandoc/src/writers/plaintext.rs` | New file: plain-text writer |
| `quarto-markdown-pandoc/src/writers/html.rs` | Make functions public, add string wrappers |
| `pico-quarto-render/src/template_context.rs` | New file: MetaValue→TemplateValue conversion |
| `pico-quarto-render/src/main.rs` | Use template_context for building context |
