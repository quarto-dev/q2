# Plan: Add Optional Source Location to DiagnosticMessage

## Context

During the ErrorCollector → DiagnosticCollector migration, we had to remove useful source location information from warnings. Specifically in `postprocess.rs`:

**Before (with old ErrorCollector):**
```rust
error_collector_ref.borrow_mut().warn(
    "Caption found without a preceding table".to_string(),
    Some(&crate::utils::error_collector::SourceInfo::new(
        caption_block.source_info.range.start.row + 1,
        caption_block.source_info.range.start.column + 1,
    )),
);
```

**After (current):**
```rust
error_collector_ref.borrow_mut().warn(
    "Caption found without a preceding table".to_string(),
);
```

We have `caption_block.source_info` available (type: `crate::pandoc::location::SourceInfo`) but nowhere to put it.

## Current State

### DiagnosticMessage Structure (quarto-error-reporting)

Already has commented placeholders for source spans:
```rust
pub struct DiagnosticMessage {
    pub code: Option<String>,
    pub title: String,
    pub kind: DiagnosticKind,
    pub problem: Option<MessageContent>,
    pub details: Vec<DetailItem>,
    pub hints: Vec<MessageContent>,
    // Future: Source spans for pointing to specific code locations
    // pub source_spans: Vec<SourceSpan>,
}
```

### Location Types in pandoc::location

```rust
pub struct Location {
    pub offset: usize,    // Byte offset in source
    pub row: usize,       // 0-based row
    pub column: usize,    // 0-based column
}

pub struct Range {
    pub start: Location,
    pub end: Location,
}

pub struct SourceInfo {
    pub filename_index: Option<usize>,  // Index into ASTContext.filenames
    pub range: Range,
}
```

### Old ErrorCollector SourceInfo (deleted)

```rust
pub struct SourceInfo {
    pub row: usize,
    pub column: usize,
}
```

## Design Questions

### Q1: Should quarto-error-reporting depend on quarto-markdown-pandoc types?

**Options:**

**A. Define own location types in quarto-error-reporting**
- ✅ Keep quarto-error-reporting independent
- ✅ Can be used by other crates without pandoc dependency
- ❌ Duplication of location types
- ❌ Need conversion between types

**B. Re-export or use pandoc::location types directly**
- ✅ No duplication
- ❌ Creates circular dependency (error-reporting is currently used BY pandoc)
- ❌ Ties error infrastructure to markdown parsing

**C. Extract location types to separate shared crate (quarto-source-location?)**
- ✅ Clean separation of concerns
- ✅ Both crates can depend on it
- ❌ More complex project structure
- ❌ Overkill for current needs

**Recommendation: Option A** - Keep quarto-error-reporting independent with its own location types.

### Q2: How detailed should location information be?

**Options:**

**A. Minimal (row + column only)**
```rust
pub struct SourceLocation {
    pub row: usize,
    pub column: usize,
}
```
- ✅ Simple
- ✅ Sufficient for most error messages
- ❌ No filename
- ❌ No ranges for multi-token errors
- ❌ No offset for efficient indexing

**B. Single position with filename**
```rust
pub struct SourceLocation {
    pub filename: Option<String>,
    pub row: usize,
    pub column: usize,
    pub offset: Option<usize>,
}
```
- ✅ Good for point errors
- ✅ Can show filename in messages
- ⚠️ No ranges for spans

**C. Full range with filename**
```rust
pub struct SourceLocation {
    pub filename: Option<String>,
    pub start_row: usize,
    pub start_column: usize,
    pub end_row: usize,
    pub end_column: usize,
    pub start_offset: Option<usize>,
    pub end_offset: Option<usize>,
}
```
- ✅ Complete information
- ✅ Can highlight spans
- ⚠️ More complex

**Recommendation: Option C** - Future-proof with full range support, even if we only use start position initially.

### Q3: Where should location information attach?

**Options:**

**A. Top-level on DiagnosticMessage**
```rust
pub struct DiagnosticMessage {
    // ... existing fields ...
    pub location: Option<SourceLocation>,
}
```
- ✅ Simple
- ✅ Covers "main" location
- ❌ Only one location per message
- ❌ Can't point to multiple problem sites

**B. Multiple spans at top level**
```rust
pub struct DiagnosticMessage {
    // ... existing fields ...
    pub source_spans: Vec<SourceSpan>,
}
```
- ✅ Supports multiple locations
- ✅ Can annotate each span with label
- ⚠️ More complex

**C. On individual detail items**
```rust
pub struct DetailItem {
    pub kind: DetailKind,
    pub content: MessageContent,
    pub location: Option<SourceLocation>,  // <-- Add this
}
```
- ✅ Each detail can have its own location
- ✅ Natural for "this field has X, that field has Y" messages
- ⚠️ More granular

**Recommendation: Hybrid - Option A + Option C** - Add both top-level location and per-detail locations. Start with top-level, add per-detail later.

### Q4: How should this integrate with builder API?

```rust
DiagnosticMessageBuilder::error("Caption found without table")
    .with_location(source_info)  // <-- Add this
    .build()
```

Or for generic macros:
```rust
generic_error!(message, location)  // <-- Add optional location
```

## Proposed Design

### Phase 1: Add Basic Location Support

#### 1.1 Add location types to quarto-error-reporting

Create `src/location.rs`:
```rust
use serde::{Deserialize, Serialize};

/// A position in a source file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub row: usize,
    pub column: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
}

/// A range in a source file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    pub start: Position,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<Position>,
}
```

#### 1.2 Update DiagnosticMessage

```rust
pub struct DiagnosticMessage {
    pub code: Option<String>,
    pub title: String,
    pub kind: DiagnosticKind,
    pub problem: Option<MessageContent>,
    pub details: Vec<DetailItem>,
    pub hints: Vec<MessageContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<SourceSpan>,  // <-- Add this
}
```

#### 1.3 Update DiagnosticMessageBuilder

Add method:
```rust
impl DiagnosticMessageBuilder {
    pub fn with_location(mut self, location: SourceSpan) -> Self {
        self.location = Some(location);
        self
    }
}
```

#### 1.4 Add conversion from pandoc::location types

In quarto-markdown-pandoc, add helper:
```rust
impl From<&crate::pandoc::location::SourceInfo> for quarto_error_reporting::SourceSpan {
    fn from(si: &crate::pandoc::location::SourceInfo) -> Self {
        quarto_error_reporting::SourceSpan {
            filename: None,  // Filename requires ASTContext
            start: quarto_error_reporting::Position {
                row: si.range.start.row,
                column: si.range.start.column,
                offset: Some(si.range.start.offset),
            },
            end: Some(quarto_error_reporting::Position {
                row: si.range.end.row,
                column: si.range.end.column,
                offset: Some(si.range.end.offset),
            }),
        }
    }
}
```

Or better, add a method:
```rust
impl crate::pandoc::location::SourceInfo {
    pub fn to_source_span(&self, context: Option<&ASTContext>) -> quarto_error_reporting::SourceSpan {
        quarto_error_reporting::SourceSpan {
            filename: self.filename_index
                .and_then(|idx| context.and_then(|c| c.filenames.get(idx)))
                .map(|s| s.clone()),
            start: quarto_error_reporting::Position {
                row: self.range.start.row,
                column: self.range.start.column,
                offset: Some(self.range.start.offset),
            },
            end: Some(quarto_error_reporting::Position {
                row: self.range.end.row,
                column: self.range.end.column,
                offset: Some(self.range.end.offset),
            }),
        }
    }
}
```

#### 1.5 Update DiagnosticCollector helpers

```rust
impl DiagnosticCollector {
    pub fn error(&mut self, message: impl Into<String>) {
        self.add(quarto_error_reporting::generic_error!(message.into()));
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.add(quarto_error_reporting::generic_warning!(message.into()));
    }

    // Add new methods with location support
    pub fn error_at(&mut self, message: impl Into<String>, location: quarto_error_reporting::SourceSpan) {
        self.add(
            quarto_error_reporting::generic_error!(message.into())
                .with_location(location)
        );
    }

    pub fn warn_at(&mut self, message: impl Into<String>, location: quarto_error_reporting::SourceSpan) {
        self.add(
            quarto_error_reporting::generic_warning!(message.into())
                .with_location(location)
        );
    }
}
```

#### 1.6 Update rendering

**to_text():**
```rust
pub fn to_text(&self) -> String {
    // ...
    if let Some(code) = &self.code {
        write!(result, "{} [{}]: {}", kind_str, code, self.title).unwrap();
    } else {
        write!(result, "{}: {}", kind_str, self.title).unwrap();
    }

    // Add location info if present
    if let Some(loc) = &self.location {
        if let Some(filename) = &loc.filename {
            write!(result, " at {}:{}:{}", filename, loc.start.row + 1, loc.start.column + 1).unwrap();
        } else {
            write!(result, " at {}:{}", loc.start.row + 1, loc.start.column + 1).unwrap();
        }
    }
    // ...
}
```

**to_json():**
```rust
pub fn to_json(&self) -> serde_json::Value {
    // ...
    if let Some(location) = &self.location {
        obj["location"] = json!(location);
    }
    // ...
}
```

#### 1.7 Use in postprocess.rs

```rust
error_collector_ref.borrow_mut().warn_at(
    "Caption found without a preceding table".to_string(),
    caption_block.source_info.to_source_span(None),
);
```

Or if we want the filename from context (passed to postprocess):
```rust
error_collector_ref.borrow_mut().warn_at(
    "Caption found without a preceding table".to_string(),
    caption_block.source_info.to_source_span(Some(context)),
);
```

## Implementation Steps

1. **Create location types** in quarto-error-reporting (src/location.rs)
2. **Update DiagnosticMessage** struct to include optional location
3. **Update builder API** with `.with_location()`
4. **Update rendering** (to_text and to_json) to include location
5. **Add conversion helper** in pandoc::location (to_source_span)
6. **Add convenience methods** to DiagnosticCollector (error_at, warn_at)
7. **Update postprocess.rs** to use new warn_at method
8. **Update tests** in diagnostic_collector.rs to test location rendering
9. **Run full test suite**

## Future Extensions (Phase 2+)

- Add location to DetailItem for per-detail locations
- Support multiple source spans at top level
- Integration with ariadne for fancy terminal rendering with source snippets
- Add label field to SourceSpan for annotating different spans ("expected this", "found that")

## Open Questions

1. Should locations use 0-based or 1-based indexing?
   - **Internal representation**: 0-based (matches tree-sitter, Rust conventions)
   - **Display**: 1-based (matches user expectations, editor line numbers)

2. Should we pass ASTContext to DiagnosticCollector to resolve filenames?
   - **Defer**: For now, use `to_source_span(None)` which omits filename
   - **Later**: When we need filenames, pass context or store filename_index and resolve at render time

3. Should generic_error!/generic_warning! macros support location?
   - **No**: Keep macros simple (they're for migration)
   - **Yes**: Use builder API directly for location-aware messages
