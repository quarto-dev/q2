# Attr and Target Source Location Sideloading

**Date**: 2025-10-24
**Context**: Fixing quarto-markdown-pandoc to properly track source locations for tuple-based Pandoc structures
**Status**: Design phase

## Problem Statement

The current annotated Pandoc JSON format adds `s: number` fields to track source locations for AST nodes. However, **this approach fails for tuple-based structures** where we cannot add fields to plain strings inside arrays:

```json
// Current approach: ❌ Doesn't work for tuples
{
  "t": "Span",
  "c": [
    ["id", ["class1"], [["key", "value"]]],  // ← Can't add 's' to these strings!
    []
  ],
  "s": 42
}
```

### Affected Structures

1. **Attr**: `[string, string[], [string, string][]]`
   - `[id, classes, key-value attributes]`
   - Cannot add source IDs to id string, class strings, or attribute key/value strings

2. **Target**: `[string, string]`
   - `[url, title]`
   - Cannot add source IDs to url or title strings

## Solution: Parallel Sideloaded Fields

**Pattern**: For nodes containing tuple-based structures, add parallel `*S` fields that mirror the structure with source IDs.

### Example: Span with Attr

**Input markdown**:
```markdown
[content]{#my-id .class1 .class2 key1=value1 key2=value2}
```

**Output JSON** (annotated):
```json
{
  "t": "Span",
  "c": [
    ["my-id", ["class1", "class2"], [["key1", "value1"], ["key2", "value2"]]],
    [{"t": "Str", "c": "content", "s": 10}]
  ],
  "s": 0,
  "attrS": [1, [2, 3], [[4, 5], [6, 7]]]
}
```

**Source ID mapping**:
- `s: 0` → The entire `[content]{...}` span construct
- `attrS[0]: 1` → The id string `"my-id"`
- `attrS[1]: [2, 3]` → Classes `"class1"` and `"class2"`
- `attrS[2]: [[4, 5], [6, 7]]` → Keys `"key1"`, `"key2"` and values `"value1"`, `"value2"`

### Example: Link with Attr and Target

**Input markdown**:
```markdown
[link text](https://example.com "Title"){#link-id .external}
```

**Output JSON** (annotated):
```json
{
  "t": "Link",
  "c": [
    ["link-id", ["external"], []],
    [{"t": "Str", "c": "link text", "s": 10}],
    ["https://example.com", "Title"]
  ],
  "s": 0,
  "attrS": [1, [2], []],
  "targetS": [3, 4]
}
```

**Source ID mapping**:
- `s: 0` → The entire link construct
- `attrS[0]: 1` → The id `"link-id"`
- `attrS[1]: [2]` → The class `"external"`
- `targetS[0]: 3` → The URL `"https://example.com"`
- `targetS[1]: 4` → The title `"Title"`

### Null Values for Empty/Missing Strings

When an id is empty (`""`), or a title is not provided, use `null`:

```json
{
  "t": "Span",
  "c": [
    ["", ["class1"], []],
    [{"t": "Str", "c": "content", "s": 5}]
  ],
  "s": 0,
  "attrS": [null, [2], []]  // ← null for empty id
}
```

## Complete Type Inventory

### Category A: Inline Types with Attr

| Type | Structure | Needs attrS | Needs targetS |
|------|-----------|-------------|---------------|
| Code | `[Attr, string]` | ✓ | |
| Link | `[Attr, Inline[], Target]` | ✓ | ✓ |
| Image | `[Attr, Inline[], Target]` | ✓ | ✓ |
| Span | `[Attr, Inline[]]` | ✓ | |

**Total**: 4 inline types, all need `attrS`, 2 need `targetS`

### Category B: Block Types with Attr

| Type | Structure | Needs attrS |
|------|-----------|-------------|
| CodeBlock | `[Attr, string]` | ✓ |
| Header | `[Int, Attr, Inline[]]` | ✓ |
| Table | `[Attr, Caption, ...]` | ✓ |
| Figure | `[Attr, Caption, Block[]]` | ✓ |
| Div | `[Attr, Block[]]` | ✓ |

**Total**: 5 block types, all need `attrS`

### Category C: Table Components with Attr

Tables are particularly complex because **Attr appears at multiple nesting levels**:

| Type | Structure | Needs attrS |
|------|-----------|-------------|
| TableHead | `[Attr, Row[]]` | ✓ |
| TableBody | `[Attr, RowHeadColumns, Row[], Row[]]` | ✓ |
| TableFoot | `[Attr, Row[]]` | ✓ |
| Row | `[Attr, Cell[]]` | ✓ |
| Cell | `[Attr, Alignment, RowSpan, ColSpan, Block[]]` | ✓ |

**Total**: 5 table component types, all need `attrS`

**Example Table Structure** (showing nesting):
```
Table (has Attr + attrS)
├── Caption
├── ColSpec[]
├── TableHead (has Attr + attrS)
│   └── Row[] (each has Attr + attrS)
│       └── Cell[] (each has Attr + attrS)
├── TableBody[] (each has Attr + attrS)
│   ├── Row[] (head rows, each has Attr + attrS)
│   └── Row[] (body rows, each has Attr + attrS)
│       └── Cell[] (each has Attr + attrS)
└── TableFoot (has Attr + attrS)
    └── Row[] (each has Attr + attrS)
        └── Cell[] (each has Attr + attrS)
```

**Key insight**: A single table can have **dozens of Attr occurrences**, each requiring its own `attrS` field at the appropriate nesting level.

### Category D: Other Object-Based Structures

| Type | Field | Solution |
|------|-------|----------|
| Citation | `citationId: String` | Add `citationIdS: number` field directly to Citation object |

**Note**: Citation is an object type (not a tuple), so we can add fields directly without sideloading.

### Summary Table

| Category | Count | Pattern |
|----------|-------|---------|
| Inline types needing attrS | 4 | Add `attrS` field to node |
| Inline types needing targetS | 2 | Add `targetS` field to node |
| Block types needing attrS | 5 | Add `attrS` field to node |
| Table components needing attrS | 5 | Add `attrS` field to node |
| Object types needing field tracking | 1 | Add `citationIdS` field to object |
| **Total affected node types** | **~15** | |

## Type Definitions

### Rust Internal AST

In the Rust code, we need to extend the internal annotated AST types:

```rust
// Source information for Attr: [id, classes, key-value pairs]
pub struct AttrSourceInfo {
    pub id: Option<SourceId>,                    // Source ID for id string (None if empty)
    pub classes: Vec<Option<SourceId>>,          // Source IDs for each class
    pub attributes: Vec<(Option<SourceId>, Option<SourceId>)>, // Source IDs for keys and values
}

// Source information for Target: [url, title]
pub struct TargetSourceInfo {
    pub url: Option<SourceId>,    // Source ID for URL
    pub title: Option<SourceId>,  // Source ID for title
}

// Example: Annotated Span
pub struct AnnotatedSpan {
    pub attr: Attr,                        // Standard Pandoc Attr
    pub content: Vec<AnnotatedInline>,
    pub source: SourceId,                  // Source for the span itself
    pub attr_source: AttrSourceInfo,       // Source info for attr components
}

// Example: Annotated Link
pub struct AnnotatedLink {
    pub attr: Attr,
    pub content: Vec<AnnotatedInline>,
    pub target: Target,
    pub source: SourceId,
    pub attr_source: AttrSourceInfo,
    pub target_source: TargetSourceInfo,
}
```

### JSON Serialization Format

When serializing to JSON, the `*S` fields must mirror the structure of the original tuple:

```rust
// For Attr: [id, [classes...], [[key, val]...]]
// Serialize attrS as: [id_s, [class1_s, class2_s, ...], [[key1_s, val1_s], [key2_s, val2_s], ...]]

// For Target: [url, title]
// Serialize targetS as: [url_s, title_s]
```

**Important**: Use `null` in JSON for `None` values in Rust.

## Implementation Strategy for quarto-markdown-pandoc

### Phase 1: Extend Internal AST Types

1. Define `AttrSourceInfo` and `TargetSourceInfo` structs
2. Add `attr_source: AttrSourceInfo` field to all types with Attr (14 types)
3. Add `target_source: TargetSourceInfo` field to Link and Image
4. Add `citation_id_source: Option<SourceId>` to Citation

### Phase 2: Parser Changes

Track source locations during parsing for:

1. **Attr components**:
   - When parsing id: capture span of the id string
   - When parsing classes: capture span of each class string
   - When parsing key=value pairs: capture spans of both key and value strings

2. **Target components**:
   - When parsing URL: capture span of URL string
   - When parsing title: capture span of title string

3. **Citation components**:
   - When parsing citation id: capture span

**Example parsing pseudocode**:
```rust
fn parse_attr(&mut self) -> (Attr, AttrSourceInfo) {
    let id_span = self.capture_span_for_id();
    let id_source = if id.is_empty() { None } else { Some(self.create_source_id(id_span)) };

    let mut class_sources = Vec::new();
    for class in classes {
        let class_span = self.capture_span_for_class(class);
        class_sources.push(Some(self.create_source_id(class_span)));
    }

    let mut attr_sources = Vec::new();
    for (key, value) in attributes {
        let key_span = self.capture_span_for_key(key);
        let val_span = self.capture_span_for_value(value);
        attr_sources.push((
            Some(self.create_source_id(key_span)),
            Some(self.create_source_id(val_span))
        ));
    }

    let attr_source = AttrSourceInfo {
        id: id_source,
        classes: class_sources,
        attributes: attr_sources,
    };

    (attr, attr_source)
}
```

### Phase 3: JSON Writer Changes

Update JSON serialization to include `*S` fields:

```rust
fn serialize_span(&self, span: &AnnotatedSpan) -> JsonValue {
    json!({
        "t": "Span",
        "c": [
            serialize_attr(&span.attr),
            serialize_inlines(&span.content)
        ],
        "s": span.source,
        "attrS": serialize_attr_source(&span.attr_source)
    })
}

fn serialize_attr_source(attr_s: &AttrSourceInfo) -> JsonValue {
    json!([
        attr_s.id,  // Will be null if None
        attr_s.classes.iter().map(|s| s.as_ref()).collect::<Vec<_>>(),
        attr_s.attributes.iter().map(|(k, v)| vec![k.as_ref(), v.as_ref()]).collect::<Vec<_>>()
    ])
}

fn serialize_target_source(target_s: &TargetSourceInfo) -> JsonValue {
    json!([
        target_s.url,
        target_s.title
    ])
}
```

### Phase 4: Testing Strategy

Create test cases for each affected type:

1. **Empty/missing values**: `[]{.class}` (empty id) → `attrS: [null, [s1], []]`
2. **Multiple classes**: `[]{.c1 .c2 .c3}` → `attrS: [null, [s1, s2, s3], []]`
3. **Multiple attributes**: `[]{k1=v1 k2=v2}` → `attrS: [null, [], [[s1,s2], [s3,s4]]]`
4. **Links with targets**: `[text](url "title")` → needs both `attrS` and `targetS`
5. **Nested tables**: Test that each Row, Cell, etc. has its own `attrS`

## Verification Checklist

- [ ] All 14 node types with Attr have `attrS` field
- [ ] Link and Image have `targetS` field
- [ ] Citation has `citationIdS` field
- [ ] Empty strings serialize as `null`
- [ ] Array structures are properly nested (especially for attributes)
- [ ] Table components at all nesting levels have `attrS`
- [ ] JSON output matches expected format
- [ ] TypeScript types in annotated-qmd can deserialize the JSON

## Open Questions

1. **Optimization**: Should we omit `attrS` if all components are null? (e.g., `["", [], []]`)
   - **Recommendation**: Include it for consistency, even if all null
   - Simplifies deserialization logic
   - Makes structure predictable

2. **Source ID allocation**: How do we assign source IDs to the components?
   - **Answer**: Same as current approach - allocate during parsing when we have span information

3. **Table complexity**: Should we test with a "worst case" table (all Attrs populated)?
   - **Recommendation**: Yes - create a test with id, classes, and attributes on every table component

## Next Steps

1. Create beads issue for implementing AttrSourceInfo in quarto-markdown-pandoc
2. Start with Span (simplest case with just `attrS`)
3. Add Link/Image (introduces `targetS`)
4. Add Table components (tests nested `attrS`)
5. Update annotated-qmd TypeScript types to match
6. Verify round-trip: Rust → JSON → TypeScript deserialization

## References

- Previous approach: `claude-notes/2025-10-24-recursive-annotation-problem.md`
- Experimental work: `ts-packages/annotated-qmd/src/recursive-annotation-type-experiments.ts`
- Related beads issue: k-161 (recursive type annotation problem)
