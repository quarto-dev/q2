# quarto-source-map Compatibility with XML/CSL Parsing

**Date:** 2025-11-27
**Related Issue:** k-411 (quarto-xml)

## Executive Summary

**Conclusion: No changes needed to quarto-source-map.**

The existing architecture is well-suited for XML source tracking. The complexity lies in quarto-xml's implementation, not in the source tracking infrastructure.

---

## Investigation Scope

This report analyzes whether `quarto-source-map`'s architecture is suitable for tracking source locations in XML documents, specifically CSL (Citation Style Language) files. The analysis considers:

1. quarto-source-map's current capabilities
2. quarto-yaml's patterns for source tracking
3. Haskell citeproc's XML parsing approach
4. quick-xml's position tracking APIs
5. CSL file structure and tracking requirements

---

## quarto-source-map Architecture Review

### Core Types

```rust
pub enum SourceInfo {
    Original { file_id: FileId, start_offset: usize, end_offset: usize },
    Substring { parent: Rc<SourceInfo>, start_offset: usize, end_offset: usize },
    Concat { pieces: Vec<SourcePiece> },
}
```

**Key design points:**

1. **Byte offsets only** - Row/column computed on-demand via `map_offset()`
2. **Nested tracking** - `Substring` supports tracking content within content (e.g., YAML in .qmd)
3. **No format-specific assumptions** - Works with any text format

### quarto-yaml Patterns

quarto-yaml demonstrates how to use quarto-source-map with an event-based parser:

```rust
// From quarto-yaml/src/parser.rs
fn make_source_info(&self, marker: &Marker, len: usize) -> SourceInfo {
    let start_offset = marker.index();
    let end_offset = start_offset + len;

    if let Some(ref parent) = self.parent {
        SourceInfo::substring(parent.clone(), start_offset, end_offset)
    } else {
        SourceInfo::original(quarto_source_map::FileId(0), start_offset, end_offset)
    }
}
```

**Pattern: Stack-based construction**
- Push start positions onto stack when opening elements
- Pop and compute spans when closing elements
- Create parallel structure (`YamlWithSourceInfo`) alongside parsed data

---

## Haskell Citeproc Analysis

**Critical Finding: Haskell citeproc does NOT track source locations.**

From `Citeproc/Style.hs`:
```haskell
parseFailure :: String -> ElementParser a
parseFailure s = throwE (CiteprocParseError $ T.pack s)
```

The error type is just a `Text` message - no position information. This means:
- We are **adding a capability** that Haskell doesn't have
- No existing patterns to follow from the Haskell implementation
- We can design our error reporting from scratch

---

## quick-xml Position Tracking

quick-xml provides:

```rust
// Position after the last event's data
pub const fn buffer_position(&self) -> u64

// Position of the start of current markup (for error reporting)
pub const fn error_position(&self) -> u64
```

### Event-to-Position Mapping

| Event Type | Start Position | End Position |
|------------|---------------|--------------|
| `Start` | `error_position()` | `buffer_position()` |
| `End` | `error_position()` | `buffer_position()` |
| `Empty` | `error_position()` | `buffer_position()` |
| `Text` | Computed from `buffer_position() - text.len()` | `buffer_position()` |
| `CData` | `error_position()` | `buffer_position()` |

### Attribute Positions

Attributes provide positions **relative to tag start**:
```rust
// From events/attributes.rs
// Errors include: "position {}: attribute key must be..."
```

For absolute positions: `tag_start_offset + attribute.relative_position`

---

## CSL-Specific Tracking Requirements

### What We Need to Track

1. **Element spans** - For error messages like "macro 'foo' defined at style.csl:42 is circular"
2. **Attribute name positions** - For "unknown attribute 'xyz' at style.csl:42:15"
3. **Attribute value positions** - For "invalid variable 'abc' at style.csl:42:25"
4. **Text content positions** - For term definitions

### Example CSL with Annotations

```xml
<style xmlns="..." version="1.0">        <!-- style: bytes 0-200 -->
  <macro name="author">                  <!-- macro: bytes 50-150, name attr value: 63-69 -->
    <text variable="author"/>            <!-- text: bytes 75-100, variable attr value: 91-97 -->
  </macro>
</style>
```

---

## Compatibility Analysis

### ✅ What Works Perfectly

| Requirement | quarto-source-map Support |
|-------------|---------------------------|
| Byte offset tracking | `SourceInfo::Original` stores start/end offsets |
| Nested document tracking | `SourceInfo::Substring` chains through parents |
| On-demand row/column | `map_offset()` computes from file content |
| Multiple source locations | Can create separate `SourceInfo` per attribute |
| Error reporting integration | Works with `quarto-error-reporting` |

### ⚠️ Implementation Considerations

These are **not limitations** - just implementation details for quarto-xml:

1. **Stack management** - Must track element start positions on a stack (same as quarto-yaml)

2. **Attribute position computation** - quick-xml gives relative positions; quarto-xml must compute absolute:
   ```rust
   let attr_absolute_start = tag_start + attr.relative_start;
   let attr_absolute_end = tag_start + attr.relative_end;
   ```

3. **Text content position** - quick-xml's Text events require position calculation:
   ```rust
   let text_start = reader.buffer_position() as usize - text.len();
   let text_end = reader.buffer_position() as usize;
   ```

4. **Empty elements** - `<tag/>` needs special handling (single event, not start+end)

### ❌ Not an Issue

**Concern:** "XML attributes are embedded in tags, unlike YAML key-value pairs"

**Resolution:** This doesn't require quarto-source-map changes. We create separate `SourceInfo` objects:

```rust
pub struct XmlAttribute {
    pub name: String,
    pub name_source: SourceInfo,      // Points to "variable"
    pub value: String,
    pub value_source: SourceInfo,     // Points to "author"
}
```

This is analogous to `YamlHashEntry` having `key_span` and `value_span`.

---

## Proposed XmlWithSourceInfo Design

Following quarto-yaml patterns:

```rust
pub struct XmlWithSourceInfo {
    pub root: XmlElement,
    pub source_info: SourceInfo,      // Entire document span
}

pub struct XmlElement {
    pub name: String,
    pub name_source: SourceInfo,      // Element name span
    pub attributes: Vec<XmlAttribute>,
    pub children: XmlChildren,
    pub source_info: SourceInfo,      // Full element span (start tag to end tag)
}

pub struct XmlAttribute {
    pub name: String,
    pub name_source: SourceInfo,
    pub value: String,
    pub value_source: SourceInfo,
}

pub enum XmlChildren {
    Elements(Vec<XmlElement>),
    Text { content: String, source_info: SourceInfo },
    Mixed(Vec<XmlChild>),
    Empty,
}
```

---

## Implementation Strategy

### Phase 1: Basic Parsing with Element-Level Tracking

```rust
struct XmlParser<'a> {
    source: &'a str,
    reader: Reader<&'a [u8]>,
    stack: Vec<BuildNode>,
    file_id: FileId,
    parent: Option<SourceInfo>,
}

struct BuildNode {
    name: String,
    name_source: SourceInfo,
    start_offset: usize,          // From error_position() at Start event
    attributes: Vec<XmlAttribute>,
    children: Vec<XmlChild>,
}
```

### Phase 2: Add Attribute Position Tracking

Quick-xml's `Attributes` iterator provides:
- Attribute key bytes and value bytes
- Position information for error reporting

We compute absolute positions from tag start offset.

### Phase 3: CSL Integration

quarto-csl receives `XmlWithSourceInfo` and extracts semantic CSL types while preserving source tracking for validation and error reporting.

---

## Conclusion

**No modifications to quarto-source-map are required.**

The existing architecture handles all XML source tracking needs:

| Need | Solution |
|------|----------|
| Element positions | `SourceInfo::Original` with computed offsets |
| Attribute positions | Separate `SourceInfo` per attribute part |
| Nested documents | `SourceInfo::Substring` for embedded CSL |
| Error messages | Integrate with `quarto-error-reporting` |

The implementation work is entirely in quarto-xml:
1. Event-driven parsing with quick-xml
2. Stack-based position tracking
3. Parallel structure construction (`XmlWithSourceInfo`)

This mirrors the successful patterns established by quarto-yaml.

---

## References

- `crates/quarto-source-map/src/source_info.rs` - SourceInfo definition
- `crates/quarto-yaml/src/parser.rs` - Stack-based parsing pattern
- `external-sources/citeproc/src/Citeproc/Style.hs` - Haskell CSL parsing (no positions)
- `external-sources/quick-xml/src/reader/mod.rs` - Position APIs
