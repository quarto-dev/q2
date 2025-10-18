# quarto-source-map Implementation Plan

**Date**: 2025-10-13
**Context**: Creating a unified source mapping crate to replace ad-hoc solutions in quarto-yaml and support ariadne integration in quarto-error-reporting

## Executive Summary

We need to extract and enhance source mapping functionality into a standalone `quarto-source-map` crate that:
1. Replaces the simple `SourceInfo` in `quarto-yaml`
2. Provides the foundation for ariadne integration in `quarto-error-reporting`
3. Eventually replaces `location.rs` in `quarto-markdown-pandoc`
4. Supports transformation chains (YAML extraction, concatenation, normalization)
5. Is fully serializable for disk caching (LSP support)

## Current State Analysis

### quarto-yaml/src/source_info.rs (CURRENT)
```rust
pub struct SourceInfo {
    pub file: Option<String>,  // ❌ String duplication, not indexed
    pub offset: usize,         // ✅ Byte offset
    pub line: usize,           // ✅ 1-based line
    pub col: usize,            // ✅ 1-based column
    pub len: usize,            // ✅ Length in bytes
}
```

**Problems**:
- No transformation tracking (can't trace YAML → QMD)
- String duplication for filenames (memory inefficient)
- No multi-file support
- Comment says "will be replaced by unified SourceInfo"

### quarto-markdown-pandoc/src/pandoc/location.rs (CURRENT)
```rust
pub struct SourceInfo {
    pub filename_index: Option<usize>,  // ✅ Indexed filenames
    pub range: Range,                   // ✅ Start/end locations
}

pub struct Range {
    pub start: Location,  // offset, row, column
    pub end: Location,
}

pub struct ASTContext {
    pub filenames: Vec<String>,  // ✅ Deduplicated storage
}
```

**Problems**:
- No transformation tracking
- Only supports original positions
- Can't represent extracted/concatenated content

### Unified Design (unified-source-location-design.md)
- ✅ **SourceMapping enum**: Original, Substring, Concat, Transformed
- ✅ **FileId system**: Indexed, deduplicated
- ✅ **Serializable**: All data, no closures
- ✅ **Multi-file**: Full support
- ✅ **Transformation chains**: Preserved history

## Implementation Plan

### Phase 1: Create quarto-source-map Crate (Week 1-2)

#### Task 1.1: Create Crate Structure
```bash
cargo new --lib quarto-source-map
```

**Files to create**:
- `src/lib.rs` - Public API exports
- `src/types.rs` - Core types (Location, Range, FileId)
- `src/source_info.rs` - SourceInfo and SourceMapping
- `src/context.rs` - SourceContext for file management
- `src/mapping.rs` - Position mapping logic
- `src/utils.rs` - Utility functions
- `Cargo.toml` - Dependencies

**Dependencies**:
```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }

[dev-dependencies]
serde_json = "1.0"  # For testing serialization
```

#### Task 1.2: Implement Core Types (`src/types.rs`)
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Location {
    pub offset: usize,  // Byte offset from start
    pub row: usize,     // 0-indexed row
    pub column: usize,  // 0-indexed column
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Location,
    pub end: Location,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub usize);
```

**Tests**:
- Range creation
- Location ordering
- Serialization round-trip

#### Task 1.3: Implement SourceInfo (`src/source_info.rs`)
```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceInfo {
    pub range: Range,
    pub mapping: SourceMapping,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SourceMapping {
    Original { file_id: FileId },
    Substring { parent: Box<SourceInfo>, offset: usize },
    Concat { pieces: Vec<SourcePiece> },
    Transformed { parent: Box<SourceInfo>, mapping: Vec<RangeMapping> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourcePiece {
    pub source_info: SourceInfo,
    pub offset_in_concat: usize,
    pub length: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RangeMapping {
    pub from_start: usize,
    pub from_end: usize,
    pub to_start: usize,
    pub to_end: usize,
}
```

**API Methods**:
```rust
impl SourceInfo {
    pub fn original(file_id: FileId, range: Range) -> Self;
    pub fn substring(parent: SourceInfo, start: usize, end: usize) -> Self;
    pub fn concat(pieces: Vec<(SourceInfo, usize)>) -> Self;
    pub fn transformed(parent: SourceInfo, mapping: Vec<RangeMapping>) -> Self;
}
```

**Tests**:
- Create each SourceMapping variant
- Serialization round-trip for all variants
- Memory layout (ensure Box prevents exponential growth)

#### Task 1.4: Implement SourceContext (`src/context.rs`)
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceContext {
    files: Vec<SourceFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,  // Optional for serialization
    pub metadata: FileMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub file_type: Option<String>,
    pub modified: Option<SystemTime>,
}

impl SourceContext {
    pub fn new() -> Self;
    pub fn add_file(&mut self, path: String, content: Option<String>) -> FileId;
    pub fn get_file(&self, id: FileId) -> Option<&SourceFile>;
    pub fn without_content(&self) -> Self;  // For serialization
}
```

**Tests**:
- Add files and retrieve
- Serialization with/without content
- FileId stability

#### Task 1.5: Implement Position Mapping (`src/mapping.rs`)
```rust
#[derive(Debug, Clone)]
pub struct MappedLocation {
    pub file_id: FileId,
    pub location: Location,
}

impl SourceInfo {
    pub fn map_offset(&self, offset: usize, ctx: &SourceContext) -> Option<MappedLocation>;
    pub fn map_range(&self, range: Range, ctx: &SourceContext) -> Option<(MappedLocation, MappedLocation)>;
}
```

**Implementation** (recursive, following unified-source-location-design.md):
- Original: Direct lookup
- Substring: Add offset and recurse
- Concat: Find piece and recurse
- Transformed: Interpolate and recurse

**Tests**:
- Map through Original
- Map through Substring
- Map through Concat
- Map through nested chains (Substring of Concat)
- Map through Transformed

#### Task 1.6: Utility Functions (`src/utils.rs`)
```rust
/// Convert offset to (line, column) in source text
pub fn offset_to_location(source: &str, offset: usize) -> Option<Location>;

/// Convert (line, column) to offset in source text
pub fn line_col_to_offset(source: &str, line: usize, col: usize) -> Option<usize>;

/// Calculate range from byte offsets
pub fn range_from_offsets(start: usize, end: usize) -> Range;
```

**Tests**:
- Offset/location conversion with various inputs
- Unicode handling (multi-byte characters)
- Line endings (LF, CRLF)

#### Task 1.7: Documentation and Examples
- Module-level documentation
- Example: YAML extraction from QMD
- Example: Multi-file includes
- Example: Concatenated cell options

### Phase 2: Integrate into quarto-yaml (Week 3)

#### Task 2.1: Update Dependencies
**quarto-yaml/Cargo.toml**:
```toml
[dependencies]
quarto-source-map = { path = "../quarto-source-map" }
```

#### Task 2.2: Replace SourceInfo
**Migration steps**:
1. Add `use quarto_source_map::*;`
2. Replace `SourceInfo` with `quarto_source_map::SourceInfo`
3. Add `SourceContext` to YamlBuilder
4. Update `from_marker()` to use FileId
5. Update tests

**Breaking changes**:
- `SourceInfo.file` → requires `SourceContext` to resolve
- `SourceInfo.line/col` → access via `range.start`
- Need to pass `SourceContext` through parsing

#### Task 2.3: Update YamlBuilder
```rust
struct YamlBuilder<'a> {
    _source: &'a str,
    filename: Option<String>,
    stack: Vec<BuildNode>,
    root: Option<YamlWithSourceInfo>,
    source_context: SourceContext,  // NEW
    file_id: FileId,                // NEW
}
```

#### Task 2.4: Update Tests
- Fix compilation errors
- Update assertions for new API
- Add transformation tests

### Phase 3: Integrate into quarto-error-reporting (Week 4)

#### Task 3.1: Add Ariadne Support
**quarto-error-reporting/Cargo.toml**:
```toml
[dependencies]
quarto-source-map = { path = "../quarto-source-map" }
ariadne = "0.4"
```

#### Task 3.2: Update DiagnosticMessage
```rust
pub struct DiagnosticMessage {
    // ... existing fields
    pub source_spans: Vec<SourceSpan>,  // NEW
}

pub struct SourceSpan {
    pub source_info: quarto_source_map::SourceInfo,  // NEW
    pub label: Option<String>,
    pub color: SpanColor,
}
```

#### Task 3.3: Implement Rendering Module
**quarto-error-reporting/src/render.rs** (NEW):
- `render_diagnostic()` - Main entry point
- `render_simple_text()` - Existing behavior
- `render_ariadne()` - NEW: Ariadne with source context
- `render_json()` - JSON export

#### Task 3.4: Update Builder API
```rust
impl DiagnosticMessageBuilder {
    pub fn with_source_span(
        mut self,
        source_info: quarto_source_map::SourceInfo,
        label: impl Into<String>,
        color: SpanColor,
    ) -> Self;
}
```

#### Task 3.5: Update validate-yaml
```rust
// Enhanced conversion with source spans
fn validation_error_to_diagnostic(
    error: &ValidationError,
    source_ctx: &SourceContext,
) -> DiagnosticMessage {
    let mut builder = DiagnosticMessageBuilder::error("YAML Validation Failed")
        .with_code(infer_error_code(error))
        .problem(error.message.clone());

    // If we have YAML node with source info, add span
    if let Some(node) = &error.yaml_node {
        builder = builder.with_source_span(
            node.source_info.clone(),
            "error occurred here",
            SpanColor::Red,
        );
    }

    builder.build()
}
```

### Phase 4: Future Integration (Week 5+)

#### Task 4.1: quarto-markdown-pandoc Migration (FUTURE)
**Not part of initial implementation, but designed for**:
1. Replace `location.rs` with quarto-source-map
2. Update `ASTContext` to use `SourceContext`
3. Migrate all Pandoc AST nodes to new SourceInfo
4. Update tests

#### Task 4.2: LSP Integration (FUTURE)
1. Convert SourceInfo to LSP Location
2. Use SourceContext in DocumentCache
3. Map diagnostics to original sources

## Testing Strategy

### Unit Tests (Per Phase)
- Each module: 90%+ coverage
- Serialization: Round-trip all types
- Mapping: All SourceMapping variants
- Edge cases: Empty files, unicode, large offsets

### Integration Tests
1. **YAML Extraction**: QMD → YAML with source tracking
2. **Multi-file**: Main doc + includes
3. **Cell Options**: Concatenated YAML from cell comments
4. **Error Reporting**: validate-yaml with ariadne
5. **Serialization**: Cache and reload SourceContext

### Performance Benchmarks
- `map_offset()` speed (should be <1μs for typical chains)
- Serialization size (should be <10KB for typical docs)
- Memory overhead (SourceInfo should be ~100 bytes max)

## Dependencies and Compatibility

### New Crate: quarto-source-map
```toml
[dependencies]
serde = { version = "1.0", features = ["derive"] }

[dev-dependencies]
serde_json = "1.0"
criterion = "0.5"  # For benchmarks
```

### Updated Crates
1. **quarto-yaml**: Add quarto-source-map dependency
2. **quarto-error-reporting**: Add quarto-source-map + ariadne
3. **validate-yaml**: Use new rendering API

### Compatibility
- **Rust version**: 1.70+ (for serde derives)
- **Breaking changes**: Yes, for quarto-yaml SourceInfo
- **Migration path**: Semver major version bump

## Risks and Mitigations

### Risk 1: Box Overhead
**Concern**: Nested Box<SourceInfo> could cause performance issues

**Mitigation**:
- Benchmark early
- Consider Arc<SourceInfo> if sharing is common
- Profile memory usage in real documents

### Risk 2: Complex Mapping Logic
**Concern**: Recursive map_offset() might be slow or error-prone

**Mitigation**:
- Comprehensive tests for all paths
- Add tracing/debugging mode
- Consider caching mapped locations

### Risk 3: Breaking Changes
**Concern**: Changing SourceInfo breaks existing code

**Mitigation**:
- Clear migration guide
- Compatibility layer for quarto-yaml?
- Version bump with changelog

### Risk 4: Serialization Size
**Concern**: Nested SourceInfo might serialize large

**Mitigation**:
- Benchmark serialization size
- Consider compression for disk cache
- Skip content in serialized SourceContext

## Success Criteria

### Phase 1 (quarto-source-map)
- ✅ All core types implemented
- ✅ All tests passing (90%+ coverage)
- ✅ Serialization works for all variants
- ✅ Documentation complete with examples
- ✅ Benchmarks show acceptable performance

### Phase 2 (quarto-yaml)
- ✅ quarto-yaml uses new SourceInfo
- ✅ All existing tests pass
- ✅ New transformation tests added
- ✅ No performance regression

### Phase 3 (quarto-error-reporting)
- ✅ Ariadne rendering works with source spans
- ✅ validate-yaml shows visual errors
- ✅ Backward compatibility maintained (simple text still works)
- ✅ Example errors look good

## Timeline Estimate

| Phase | Duration | Tasks |
|-------|----------|-------|
| Phase 1: quarto-source-map | 2 weeks | 7 tasks (1.1-1.7) |
| Phase 2: quarto-yaml | 1 week | 4 tasks (2.1-2.4) |
| Phase 3: quarto-error-reporting | 1 week | 5 tasks (3.1-3.5) |
| **Total** | **4 weeks** | **16 tasks** |

## Next Steps

1. **Review this plan** with user
2. **Confirm design decisions**:
   - SourceMapping enum variants sufficient?
   - Serialization strategy acceptable?
   - Breaking changes in quarto-yaml OK?
3. **Start Phase 1**: Create quarto-source-map crate
4. **Track progress**: Use TodoWrite for each task

## Questions for User

1. Should we implement all SourceMapping variants in Phase 1, or start with Original + Substring?
2. Is 0-indexed (row, column) OK, or should we use 1-indexed like quarto-yaml currently does?
3. Should SourceContext.content be Option<String> or Arc<String> for sharing?
4. Do we need Transformed variant immediately, or can it be added later?
5. Should Phase 4 (quarto-markdown-pandoc) be planned in detail now, or wait until Phase 3 complete?

## Appendix: Task Checklist

### Phase 1: quarto-source-map
- [ ] 1.1: Create crate structure
- [ ] 1.2: Implement core types (Location, Range, FileId)
- [ ] 1.3: Implement SourceInfo and SourceMapping
- [ ] 1.4: Implement SourceContext
- [ ] 1.5: Implement position mapping
- [ ] 1.6: Implement utility functions
- [ ] 1.7: Documentation and examples

### Phase 2: quarto-yaml Integration
- [ ] 2.1: Update dependencies
- [ ] 2.2: Replace SourceInfo
- [ ] 2.3: Update YamlBuilder
- [ ] 2.4: Update tests

### Phase 3: quarto-error-reporting Integration
- [ ] 3.1: Add ariadne support
- [ ] 3.2: Update DiagnosticMessage
- [ ] 3.3: Implement rendering module
- [ ] 3.4: Update builder API
- [ ] 3.5: Update validate-yaml

### Phase 4: Future (Not Immediate)
- [ ] 4.1: quarto-markdown-pandoc migration
- [ ] 4.2: LSP integration

---

**Status**: Awaiting approval to begin implementation
**Next Action**: Review and answer questions, then start Task 1.1
