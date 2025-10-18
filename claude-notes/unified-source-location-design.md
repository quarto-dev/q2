# Unified Source Location Design

## Executive Summary

We need to add location information to `quarto-error-reporting` in a way that:
1. Works with the existing `quarto-source-map` infrastructure
2. Supports the future migration of `quarto-yaml` to use `quarto-source-map`
3. Enables migration of `pandoc::location` to `quarto-source-map`
4. Provides a smooth path for incremental adoption

**Recommendation**: Make `quarto-error-reporting` depend on `quarto-source-map` and use its types directly.

## Current State Analysis

### quarto-source-map (Public-Ready, Sophisticated)

**Location**: `crates/quarto-source-map/`

**Purpose**: Unified source location tracking with transformation support

**Core Types**:
```rust
pub struct FileId(pub usize);

pub struct Location {
    pub offset: usize,    // Byte offset
    pub row: usize,       // 0-indexed
    pub column: usize,    // 0-indexed (characters)
}

pub struct Range {
    pub start: Location,
    pub end: Location,
}

pub struct SourceInfo {
    pub range: Range,
    pub mapping: SourceMapping,  // Tracks transformations!
}

pub enum SourceMapping {
    Original { file_id: FileId },
    Substring { parent: Box<SourceInfo>, offset: usize },
    Concat { pieces: Vec<SourcePiece> },
    Transformed { parent: Box<SourceInfo>, mapping: Vec<RangeMapping> },
}
```

**Key Features**:
- Tracks transformation chains (extraction, concatenation, normalization)
- Can map any position back to original source via `map_offset()`
- Uses `SourceContext` to manage files
- All types are `Serialize + Deserialize`
- 0-indexed (internal representation)

**Example**:
```rust
let mut ctx = SourceContext::new();
let file_id = ctx.add_file("main.qmd", Some("# Hello\nWorld"));

let info = SourceInfo::original(file_id, range);
let mapped = info.map_offset(offset, &ctx); // Maps back to original
```

### quarto-yaml (Public-Ready, Transitional)

**Location**: `crates/quarto-yaml/`

**Current SourceInfo** (temporary):
```rust
pub struct SourceInfo {
    pub file: Option<String>,  // Filename
    pub offset: usize,         // 0-indexed
    pub line: usize,           // 1-indexed (!!)
    pub col: usize,            // 1-indexed (!!)
    pub len: usize,
}
```

**Important Comment**:
```rust
/// ## Note on Future Integration
///
/// This is a simplified version for initial implementation. Eventually this
/// will be replaced by the unified SourceInfo type from the main project that
/// supports transformations and non-contiguous mappings.
```

**Status**: Intended to migrate to `quarto-source-map::SourceInfo`

### pandoc::location (Markdown-Pandoc Specific)

**Location**: `crates/quarto-markdown-pandoc/src/pandoc/location.rs`

**Types**:
```rust
pub struct Location {
    pub offset: usize,
    pub row: usize,       // 0-indexed
    pub column: usize,    // 0-indexed
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

**Differences from quarto-source-map**:
- Uses `Option<usize>` for filename instead of `FileId`
- No transformation tracking
- Tied to `ASTContext` (not `SourceContext`)
- Very similar Location/Range structure

### quarto-error-reporting (Public-Ready, No Location Yet)

**Location**: `crates/quarto-error-reporting/`

**Current State**: No location information
- Has commented placeholders for source spans
- Independent of other crates (good for reusability)

## Design Options

### Option A: Define Independent Location Types in quarto-error-reporting

**Approach**: Create minimal location types specific to error reporting

```rust
// In quarto-error-reporting
pub struct Position {
    pub row: usize,
    pub column: usize,
    pub offset: Option<usize>,
}

pub struct SourceSpan {
    pub filename: Option<String>,
    pub start: Position,
    pub end: Option<Position>,
}
```

**Pros**:
- ✅ Keep quarto-error-reporting independent
- ✅ Simple for basic use cases

**Cons**:
- ❌ Duplication of location concepts
- ❌ No transformation tracking
- ❌ Need conversion from all other location types
- ❌ Can't leverage quarto-source-map's mapping capabilities
- ❌ Future yaml integration requires new conversion

**Verdict**: ❌ Not recommended - creates fragmentation

### Option B: Re-use quarto-source-map Types Directly

**Approach**: Make quarto-error-reporting depend on quarto-source-map

```rust
// In quarto-error-reporting/Cargo.toml
[dependencies]
quarto-source-map = { path = "../quarto-source-map" }

// In quarto-error-reporting/src/diagnostic.rs
pub struct DiagnosticMessage {
    // ... existing fields ...
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<quarto_source_map::SourceInfo>,
}
```

**Pros**:
- ✅ Single source of truth for locations
- ✅ Automatic transformation tracking
- ✅ Can map errors back to original sources
- ✅ Works seamlessly when quarto-yaml migrates
- ✅ Enables future pandoc::location migration
- ✅ No conversion needed for quarto-yaml objects

**Cons**:
- ⚠️ Adds dependency (but quarto-source-map is already public-ready)
- ⚠️ Slightly more complex than minimal option

**Verdict**: ✅ **Recommended** - Best long-term solution

### Option C: Abstract via Trait

**Approach**: Define a trait that different location types implement

```rust
// In quarto-error-reporting
pub trait SourceLocation {
    fn to_display_position(&self) -> (Option<String>, usize, usize);
}

// Each crate implements for its own types
```

**Pros**:
- ✅ Flexible
- ✅ No direct dependencies

**Cons**:
- ❌ Complex
- ❌ Loses type information in serialization
- ❌ Can't leverage transformation mapping
- ❌ Trait object overhead

**Verdict**: ❌ Over-engineered for current needs

## Recommended Design: Option B with Extensions

### Phase 1: Add quarto-source-map to quarto-error-reporting

#### 1.1 Update Dependencies

```toml
# quarto-error-reporting/Cargo.toml
[dependencies]
quarto-source-map = { path = "../quarto-source-map" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

#### 1.2 Add Location to DiagnosticMessage

```rust
// quarto-error-reporting/src/diagnostic.rs
pub struct DiagnosticMessage {
    pub code: Option<String>,
    pub title: String,
    pub kind: DiagnosticKind,
    pub problem: Option<MessageContent>,
    pub details: Vec<DetailItem>,
    pub hints: Vec<MessageContent>,

    /// Source location for this diagnostic
    ///
    /// When present, this identifies where in the source code the issue occurred.
    /// The location may track transformation history, allowing the error to be
    /// mapped back through multiple processing steps to the original source file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<quarto_source_map::SourceInfo>,
}
```

#### 1.3 Update Builder API

```rust
impl DiagnosticMessageBuilder {
    /// Attach a source location to this diagnostic
    pub fn with_location(mut self, location: quarto_source_map::SourceInfo) -> Self {
        self.location = Some(location);
        self
    }
}
```

#### 1.4 Update Rendering

**Text Format** (with SourceContext for filename resolution):

```rust
impl DiagnosticMessage {
    /// Render diagnostic as text, optionally resolving locations
    pub fn to_text(&self, ctx: Option<&quarto_source_map::SourceContext>) -> String {
        use std::fmt::Write;
        let mut result = String::new();

        // Title with kind
        let kind_str = match self.kind { /* ... */ };
        if let Some(code) = &self.code {
            write!(result, "{} [{}]: {}", kind_str, code, self.title).unwrap();
        } else {
            write!(result, "{}: {}", kind_str, self.title).unwrap();
        }

        // Add location if present
        if let Some(loc) = &self.location {
            if let Some(ctx) = ctx {
                // Try to map to original source
                if let Some(mapped) = loc.map_offset(loc.range.start.offset, ctx) {
                    if let Some(file) = ctx.get_file(mapped.file_id) {
                        write!(
                            result,
                            " at {}:{}:{}",
                            file.path,
                            mapped.location.row + 1,  // Display as 1-based
                            mapped.location.column + 1
                        ).unwrap();
                    }
                }
            } else {
                // No context, show immediate location
                write!(
                    result,
                    " at {}:{}",
                    loc.range.start.row + 1,
                    loc.range.start.column + 1
                ).unwrap();
            }
        }

        // ... rest of rendering (problem, details, hints) ...

        result
    }
}
```

**JSON Format**:

```rust
pub fn to_json(&self) -> serde_json::Value {
    use serde_json::json;

    let mut obj = json!({
        "kind": kind_str,
        "title": self.title,
    });

    // ... other fields ...

    if let Some(location) = &self.location {
        obj["location"] = json!(location);  // quarto-source-map::SourceInfo is Serialize
    }

    obj
}
```

#### 1.5 Helper in DiagnosticCollector

```rust
// quarto-markdown-pandoc/src/utils/diagnostic_collector.rs
impl DiagnosticCollector {
    /// Add an error with location
    pub fn error_at(
        &mut self,
        message: impl Into<String>,
        location: quarto_source_map::SourceInfo,
    ) {
        self.add(
            quarto_error_reporting::generic_error!(message.into())
                .with_location(location)
        );
    }

    /// Add a warning with location
    pub fn warn_at(
        &mut self,
        message: impl Into<String>,
        location: quarto_source_map::SourceInfo,
    ) {
        self.add(
            quarto_error_reporting::generic_warning!(message.into())
                .with_location(location)
        );
    }
}
```

### Phase 2: Add Conversion Helpers (Temporary Bridge)

For the transition period while `pandoc::location` still exists:

```rust
// quarto-markdown-pandoc/src/pandoc/location.rs

impl SourceInfo {
    /// Convert to quarto-source-map format (temporary bridge)
    ///
    /// This creates an Original mapping without filename resolution.
    /// For proper filename support, use to_source_map_info_with_context.
    pub fn to_source_map_info(&self) -> quarto_source_map::SourceInfo {
        use quarto_source_map::{Location, Range, SourceInfo, FileId};

        // Create a dummy FileId (0) - caller should provide context for real resolution
        quarto_source_map::SourceInfo::original(
            FileId(0),
            Range {
                start: Location {
                    offset: self.range.start.offset,
                    row: self.range.start.row,
                    column: self.range.start.column,
                },
                end: Location {
                    offset: self.range.end.offset,
                    row: self.range.end.row,
                    column: self.range.end.column,
                },
            },
        )
    }

    /// Convert to quarto-source-map format with proper FileId resolution
    pub fn to_source_map_info_with_mapping(
        &self,
        file_id: quarto_source_map::FileId,
    ) -> quarto_source_map::SourceInfo {
        use quarto_source_map::{Location, Range, SourceInfo};

        quarto_source_map::SourceInfo::original(
            file_id,
            Range {
                start: Location {
                    offset: self.range.start.offset,
                    row: self.range.start.row,
                    column: self.range.start.column,
                },
                end: Location {
                    offset: self.range.end.offset,
                    row: self.range.end.row,
                    column: self.range.end.column,
                },
            },
        )
    }
}
```

### Phase 3: Use in postprocess.rs (Immediate Value)

```rust
// In postprocess, we have caption_block.source_info (pandoc::location::SourceInfo)

error_collector_ref.borrow_mut().warn_at(
    "Caption found without a preceding table".to_string(),
    caption_block.source_info.to_source_map_info(),
);
```

### Phase 4: Future Migration Path

#### 4.1 Migrate pandoc::location to use quarto-source-map

Eventually, replace `pandoc::location::SourceInfo` entirely:

```rust
// Instead of pandoc::location::SourceInfo
pub use quarto_source_map::SourceInfo;

// ASTContext becomes a wrapper around SourceContext
pub struct ASTContext {
    source_context: quarto_source_map::SourceContext,
    // ... other AST-specific data
}
```

#### 4.2 Migrate quarto-yaml to use quarto-source-map

When ready, replace `quarto-yaml::SourceInfo` with `quarto-source-map::SourceInfo`:

```rust
// quarto-yaml/src/lib.rs
pub use quarto_source_map::SourceInfo;

// Then errors from YAML parsing automatically have transformation tracking
```

#### 4.3 Cross-System Error Reporting

Once both systems use `quarto-source-map`, errors can track through transformations:

```
Original YAML in document.qmd:35:10
  ↓ [extracted to metadata]
Processed in temporary YAML buffer:5:10
  ↓ [merged with _quarto.yml]
Final config value at line:12:15

Error: Invalid configuration value
  at document.qmd:35:10  # Maps back to original!
```

## Migration Steps (Ordered)

1. **Add quarto-source-map dependency** to quarto-error-reporting
2. **Add location field** to DiagnosticMessage
3. **Update builder API** with .with_location()
4. **Update rendering** (to_text with optional context, to_json)
5. **Add conversion helpers** in pandoc::location
6. **Add convenience methods** to DiagnosticCollector (error_at, warn_at)
7. **Update postprocess.rs** to use warn_at with location
8. **Add tests** for location rendering
9. **Run full test suite**

**Future**:
- Migrate pandoc::location to quarto-source-map
- Migrate quarto-yaml to quarto-source-map
- Remove conversion helpers once migration complete

## Key Design Decisions

### 1. Dependency Direction

```
quarto-error-reporting
  ↓ depends on
quarto-source-map (no dependencies)
```

Both are public-ready crates, so this is acceptable.

### 2. Context Handling

**Question**: Should DiagnosticCollector store a SourceContext?

**Answer**: No, pass it at render time:
```rust
diagnostic_collector.to_text(Some(&source_context))
```

**Rationale**:
- Keeps diagnostic storage separate from file content
- Allows rendering in different contexts
- More flexible for serialization

### 3. Display Conventions

**Internal**: 0-indexed (matches all source-map types)
**Display**: 1-indexed (matches user expectations, editor line numbers)

Convert at display time:
```rust
row + 1, column + 1  // Display
```

### 4. Backward Compatibility

For code that doesn't care about transformations:
```rust
// Simple case - just convert
location.to_source_map_info()

// Full case - with proper FileId
location.to_source_map_info_with_mapping(file_id)
```

## Benefits of This Design

1. **Single source of truth** for location tracking
2. **Automatic transformation support** - errors map back to original sources
3. **Smooth migration path** for all crates
4. **No duplication** of location types
5. **Future-proof** for YAML integration
6. **Incremental adoption** - can add location to errors gradually
7. **Serialization support** - all types are Serialize/Deserialize

## Testing Strategy

1. Test location rendering (with and without context)
2. Test transformation tracking (substring, concat)
3. Test conversion from pandoc::location
4. Test JSON serialization round-trip
5. Test display formatting (1-based vs 0-based)

## Documentation Updates Needed

1. quarto-error-reporting README - explain location support
2. Migration guide for adding locations to existing errors
3. Example of transformation-tracked error
4. API docs for to_text() with context parameter
