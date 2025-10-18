# Ariadne Integration Design for quarto-error-reporting

**Date**: 2025-10-13
**Context**: Design for Phase 2 - Adding ariadne rendering with source context to quarto-error-reporting

## Current State

### What We Have
- **Phase 1 Complete**: Core types (DiagnosticMessage, builder API, error codes)
- **Simple text rendering**: Used in validate-yaml with bullets (✖ ℹ ?)
- **Source location data**: ValidationError has file/line/column
- **YAML node tracking**: ValidationError can store YamlWithSourceInfo

### What's Missing
- **Source context**: Can't show actual source code around errors
- **Visual spans**: Can't highlight specific ranges in source
- **Multiple labels**: Can't point to multiple related locations
- **Ariadne rendering**: No integration with ariadne for compiler-quality output

## Learning from quarto-markdown-pandoc

### How Ariadne is Used There (qmd_error_messages.rs)

**Key Pattern**:
```rust
// 1. Build ariadne Report
let report = Report::build(ReportKind::Error, filename, byte_offset)
    .with_message(&entry.error_info.title)
    .with_label(
        Label::new((filename, span))  // span is byte_offset..end_offset
            .with_message(&entry.error_info.message)
            .with_color(Color::Red),
    )
    .finish();

// 2. Render to string with source
let mut output = Vec::new();
report.write((filename, Source::from(&input_str)), &mut output)?;
```

**Important Observations**:
1. **Byte offsets**: Ariadne uses byte offsets (not line/column)
2. **Source required**: Must provide original source text for context
3. **Multiple labels**: Can add multiple labels pointing to different spans
4. **Colors**: Red for errors, Blue for info/notes
5. **Span format**: `start_byte..end_byte` (Range<usize>)

## Design Goals

1. **Maintain flexibility**: DiagnosticMessage should work with or without source
2. **Backwards compatible**: Existing code (validate-yaml) continues to work
3. **Optional ariadne**: Only render with ariadne when source is available
4. **Multiple renderers**: Support both simple text and ariadne output
5. **No tight coupling**: Core types remain independent of ariadne

## Proposed Architecture

### 1. Add Source Span Tracking to DiagnosticMessage

```rust
/// A source span pointing to a location in a file
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSpan {
    /// File path or identifier
    pub file: String,
    /// Byte offset where span starts
    pub start: usize,
    /// Byte offset where span ends (exclusive)
    pub end: usize,
    /// Optional label for this span (shown in ariadne output)
    pub label: Option<String>,
    /// Color for this span (Red, Blue, Yellow, etc.)
    pub color: SpanColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanColor {
    Red,    // Errors, primary problem
    Blue,   // Info, related context
    Yellow, // Warnings
    Green,  // Hints, suggestions
}

/// A source file with its content
#[derive(Debug, Clone)]
pub struct SourceFile {
    /// File path or identifier (must match SourceSpan.file)
    pub path: String,
    /// Full source text
    pub content: String,
}

/// Update DiagnosticMessage to include spans
pub struct DiagnosticMessage {
    pub code: Option<String>,
    pub title: String,
    pub kind: DiagnosticKind,
    pub problem: Option<MessageContent>,
    pub details: Vec<DetailItem>,
    pub hints: Vec<MessageContent>,

    // NEW: Source spans for ariadne rendering
    pub source_spans: Vec<SourceSpan>,  // NEW
}
```

### 2. Create Rendering Module (Phase 2)

**New file**: `quarto-error-reporting/src/render.rs`

```rust
use ariadne::{Color, Label, Report, ReportKind, Source};
use crate::DiagnosticMessage;

/// Rendering options
pub enum RenderFormat {
    /// Simple text with bullets (existing behavior)
    SimpleText,
    /// Ariadne with source context
    Ariadne,
    /// JSON for machine consumption
    Json,
}

/// Render a diagnostic message to a string
pub fn render_diagnostic(
    diagnostic: &DiagnosticMessage,
    sources: &[SourceFile],
    format: RenderFormat,
) -> Result<String, RenderError> {
    match format {
        RenderFormat::SimpleText => render_simple_text(diagnostic),
        RenderFormat::Ariadne => render_ariadne(diagnostic, sources),
        RenderFormat::Json => render_json(diagnostic),
    }
}

fn render_ariadne(
    diagnostic: &DiagnosticMessage,
    sources: &[SourceFile],
) -> Result<String, RenderError> {
    // Find primary span (first one, or first Red one)
    let primary_span = diagnostic.source_spans.iter()
        .find(|s| s.color == SpanColor::Red)
        .or_else(|| diagnostic.source_spans.first())
        .ok_or(RenderError::NoSourceSpans)?;

    // Build ariadne Report
    let kind = match diagnostic.kind {
        DiagnosticKind::Error => ReportKind::Error,
        DiagnosticKind::Warning => ReportKind::Warning,
        DiagnosticKind::Info => ReportKind::Advice,
        DiagnosticKind::Note => ReportKind::Advice,
    };

    let mut report = Report::build(kind, &primary_span.file, primary_span.start);

    // Set title (with error code if available)
    let title = if let Some(code) = &diagnostic.code {
        format!("{} ({})", diagnostic.title, code)
    } else {
        diagnostic.title.clone()
    };
    report = report.with_message(title);

    // Add all source spans as labels
    for span in &diagnostic.source_spans {
        let color = match span.color {
            SpanColor::Red => Color::Red,
            SpanColor::Blue => Color::Blue,
            SpanColor::Yellow => Color::Yellow,
            SpanColor::Green => Color::Green,
        };

        let mut label = Label::new((&span.file, span.start..span.end))
            .with_color(color);

        if let Some(msg) = &span.label {
            label = label.with_message(msg);
        }

        report = report.with_label(label);
    }

    // Add notes from details (if no source spans)
    if diagnostic.source_spans.is_empty() {
        for detail in &diagnostic.details {
            report = report.with_note(detail.content.as_str());
        }
    }

    // Add hints
    for hint in &diagnostic.hints {
        report = report.with_help(hint.as_str());
    }

    // Add docs URL
    if let Some(url) = diagnostic.docs_url() {
        report = report.with_note(format!("See {} for more information", url));
    }

    let report = report.finish();

    // Render to string
    let mut output = Vec::new();

    // Build source cache
    let source_cache: HashMap<String, Source> = sources.iter()
        .map(|sf| (sf.path.clone(), Source::from(&sf.content)))
        .collect();

    // Write with all sources
    for (file, source) in &source_cache {
        report.write((file.as_str(), source), &mut output)?;
    }

    Ok(String::from_utf8(output)?)
}
```

### 3. Update Builder API

```rust
impl DiagnosticMessageBuilder {
    /// Add a source span for ariadne rendering
    pub fn with_span(
        mut self,
        file: impl Into<String>,
        start: usize,
        end: usize,
        label: impl Into<String>,
        color: SpanColor,
    ) -> Self {
        self.source_spans.push(SourceSpan {
            file: file.into(),
            start,
            end,
            label: Some(label.into()),
            color,
        });
        self
    }

    /// Add primary error span (red, first in list)
    pub fn with_primary_span(
        mut self,
        file: impl Into<String>,
        start: usize,
        end: usize,
    ) -> Self {
        self.source_spans.insert(0, SourceSpan {
            file: file.into(),
            start,
            end,
            label: None,
            color: SpanColor::Red,
        });
        self
    }

    /// Add a related span (blue)
    pub fn with_related_span(
        mut self,
        file: impl Into<String>,
        start: usize,
        end: usize,
        label: impl Into<String>,
    ) -> Self {
        self.source_spans.push(SourceSpan {
            file: file.into(),
            start,
            end,
            label: Some(label.into()),
            color: SpanColor::Blue,
        });
        self
    }
}
```

### 4. Helper: Convert Line/Column to Byte Offset

**Problem**: We have line/column but ariadne needs byte offsets

**Solution**: Utility function (can copy from qmd_error_messages.rs:330)

```rust
/// Calculate byte offset from line and column (0-indexed)
pub fn line_col_to_byte_offset(source: &str, line: usize, col: usize) -> usize {
    let mut current_line = 0;
    let mut current_col = 0;

    for (i, ch) in source.char_indices() {
        if current_line == line && current_col == col {
            return i;
        }

        if ch == '\n' {
            current_line += 1;
            current_col = 0;
        } else {
            current_col += 1;
        }
    }

    source.len() // If past end, return end of source
}
```

## Usage in validate-yaml

### Before (Simple Text)
```rust
let diagnostic = validation_error_to_diagnostic(&error);
display_diagnostic(&diagnostic);  // Simple text with bullets
```

### After (with Ariadne)
```rust
let diagnostic = validation_error_to_diagnostic(&error);

// If we have source available, render with ariadne
if let Some(source) = try_load_source(&error.location) {
    let rendered = render_diagnostic(
        &diagnostic,
        &[source],
        RenderFormat::Ariadne,
    )?;
    eprintln!("{}", rendered);
} else {
    // Fall back to simple text
    display_diagnostic(&diagnostic);
}
```

### Enhanced Conversion (with source spans)
```rust
fn validation_error_to_diagnostic(
    error: &ValidationError,
    source_content: Option<&str>,  // NEW: optional source
) -> DiagnosticMessage {
    let mut builder = DiagnosticMessageBuilder::error("YAML Validation Failed")
        .with_code(infer_error_code(error))
        .problem(error.message.clone());

    // If we have source and location, add primary span
    if let (Some(source), Some(loc)) = (source_content, &error.location) {
        let start = line_col_to_byte_offset(source, loc.line - 1, loc.column - 1);
        let end = start + 1; // Or calculate actual token length

        builder = builder.with_primary_span(
            loc.file.clone(),
            start,
            end,
        );
    } else {
        // No source, use detail items (existing behavior)
        builder = builder.add_detail(format!("At document path: `{}`", error.instance_path));
    }

    // ... rest of conversion
}
```

## Example Output Comparison

### Simple Text (current)
```
Error: YAML Validation Failed (Q-1-10)

Problem: Missing required property 'author'

  ✖ At document root
  ℹ Schema constraint: object
  ✖ In file `invalid.yaml` at line 2, column 6

  ? Add the `author` property to your YAML document?

See https://quarto.org/docs/errors/Q-1-10 for more information
```

### Ariadne (with source)
```
Error[Q-1-10]: YAML Validation Failed
   ┌─ invalid.yaml:2:6
   │
 2 │ title: "My Document"
   │ ^^^^^ Missing required property 'author'
   │
   = help: Add the `author` property to your YAML document?
   = note: See https://quarto.org/docs/errors/Q-1-10 for more information
```

## Implementation Steps

### Step 1: Core Types (diagnostic.rs)
- Add `SourceSpan` struct
- Add `SpanColor` enum
- Add `SourceFile` struct
- Add `source_spans: Vec<SourceSpan>` to `DiagnosticMessage`

### Step 2: Builder API Updates (builder.rs)
- Add `source_spans` field to builder
- Add `.with_span()`, `.with_primary_span()`, `.with_related_span()` methods
- Update `.build()` to include source_spans

### Step 3: Rendering Module (render.rs) - NEW FILE
- Add `RenderFormat` enum
- Add `render_diagnostic()` function
- Implement `render_simple_text()` (extract from validate-yaml)
- Implement `render_ariadne()` with full ariadne integration
- Implement `render_json()` for machine consumption
- Add helper: `line_col_to_byte_offset()`

### Step 4: Dependencies (Cargo.toml)
- Add `ariadne = "0.4"` (optional feature?)

### Step 5: Update validate-yaml
- Enhance `validation_error_to_diagnostic()` to accept source
- Add source loading logic
- Use `render_diagnostic()` instead of custom `display_diagnostic()`

### Step 6: Documentation & Tests
- Update README with ariadne examples
- Add render tests
- Add integration tests with real source files

## Questions for Discussion

### 1. Feature Flags?
Should ariadne be an optional feature?
```toml
[features]
ariadne = ["dep:ariadne"]
```

**Pro**: Lighter dependency for users who don't need visual output
**Con**: More complex API, two code paths to maintain

**Recommendation**: Start without feature flag, add later if needed

### 2. Source Storage Strategy?

**Option A: User provides source** (Recommended)
```rust
render_diagnostic(&diagnostic, &[source_file], RenderFormat::Ariadne)
```
- User is responsible for loading/caching source
- More flexible, less memory overhead
- Used by qmd_error_messages.rs

**Option B: Store in DiagnosticMessage**
```rust
pub struct DiagnosticMessage {
    // ...
    pub source_content: Option<HashMap<String, String>>,
}
```
- Simpler API
- Higher memory overhead
- Tighter coupling

**Recommendation**: Option A (user provides)

### 3. Backward Compatibility?

Current validate-yaml uses custom `display_diagnostic()`. Options:

**Option A**: Keep both (Recommended)
- Simple text rendering remains in validate-yaml
- Ariadne rendering is opt-in via render module
- Users can choose based on whether they have source

**Option B**: Replace with render module
- Always use `render_diagnostic()`
- Falls back to simple text when no source
- Cleaner but requires changes to working code

**Recommendation**: Option A initially, migrate to B in future

### 4. Line/Column vs Byte Offset API?

Should builder accept line/column or byte offset?

**Option A: Accept both**
```rust
.with_span_bytes(file, start, end, label, color)
.with_span_lines(file, line, col, length, label, color)  // converts internally
```

**Option B: Only byte offsets** (Recommended)
```rust
.with_span(file, start, end, label, color)
// User calls line_col_to_byte_offset() if needed
```

**Recommendation**: Option B - keep API simple, provide utility

## Open Design Questions

1. **Multi-file errors**: How to handle errors spanning multiple files?
2. **Span calculation**: Who calculates end offset (single char vs token length)?
3. **Cache strategy**: Should render module cache sources between calls?
4. **Color customization**: Allow users to override colors?
5. **Terminal detection**: Auto-detect if output is TTY and choose format?

## Dependencies

### New Dependencies
```toml
[dependencies]
ariadne = "0.4"
```

### Already Have
- serde, serde_json (for JSON rendering)
- thiserror (for RenderError)

## Benefits

1. ✅ **Compiler-quality errors**: Visual spans with source context
2. ✅ **Flexible rendering**: Simple text, ariadne, or JSON
3. ✅ **Backward compatible**: Existing code continues to work
4. ✅ **Reusable**: Other projects can use render module
5. ✅ **Consistent**: Same infrastructure as quarto-markdown-pandoc will use
6. ✅ **Optional source**: Works with or without source text

## Next Steps

Please confirm design decisions:
1. No feature flag for ariadne (add later if needed)?
2. User provides source (not stored in DiagnosticMessage)?
3. Keep simple text rendering in validate-yaml initially?
4. Builder API accepts byte offsets only (with utility for conversion)?

Once confirmed, I can proceed with implementation.
