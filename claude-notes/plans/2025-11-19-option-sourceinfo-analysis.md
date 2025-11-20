# Analysis: Is `Option<SourceInfo>` Sufficient?

## Proposed API Change

```rust
pub fn read<T: Write>(
    input_bytes: &[u8],
    _loose: bool,
    filename: &str,
    mut output_stream: &mut T,
    prune_errors: bool,
    parent_source_info: Option<SourceInfo>,  // NEW!
) -> Result<(Pandoc, ASTContext, Vec<DiagnosticMessage>), ...>
```

## Implementation Strategy

### 1. Add Field to ASTContext

```rust
pub struct ASTContext {
    pub filenames: Vec<String>,
    pub example_list_counter: Cell<usize>,
    pub source_context: SourceContext,
    pub parent_source_info: Option<SourceInfo>,  // NEW!
}
```

### 2. Modify `node_to_source_info_with_context`

In `/crates/quarto-markdown-pandoc/src/pandoc/source_map_compat.rs:61`:

```rust
pub fn node_to_source_info_with_context(node: &Node, ctx: &ASTContext) -> SourceInfo {
    let file_id = ctx.primary_file_id().unwrap_or(FileId(0));
    let base_info = node_to_source_info(node, file_id);

    // NEW: If we're in a recursive parse, wrap as Substring
    if let Some(parent) = &ctx.parent_source_info {
        SourceInfo::substring(
            parent.clone(),
            node.start_byte(),
            node.end_byte() - node.start_byte(),
        )
    } else {
        base_info
    }
}
```

### 3. Modify `read()` to Store parent_source_info

In `/crates/quarto-markdown-pandoc/src/readers/qmd.rs:100`:

```rust
let mut context = ASTContext::with_filename(filename.to_string());
context.parent_source_info = parent_source_info;  // NEW!
// ... rest of setup
```

### 4. Update Recursive Call in Metadata Parsing

In `/crates/quarto-markdown-pandoc/src/pandoc/meta.rs:247`:

```rust
let result = readers::qmd::read(
    value.as_bytes(),
    false,
    "<metadata>",
    &mut output_stream,
    true,
    Some(source_info.clone()),  // NEW! Pass parent location
);
```

### 5. Update All Other Call Sites

About 18 call sites in qmd-syntax-helper and 1 in pico-quarto-render need:

```rust
// Add None as the last parameter
readers::qmd::read(
    content.as_bytes(),
    false,
    &filename,
    &mut output_stream,
    true,
    None,  // NEW! No parent for top-level parses
)
```

## How It Works: Complete Flow

### Scenario: Parsing dj_index.qmd with Metadata

```yaml
---
format:
  html:
    include-in-header:
      - text: |
          <script src="test.js"></script>
---
```

### Step 1: Top-Level Parse

```rust
read(dj_index_content, "dj_index.qmd", parent_source_info: None)
├─ Create ASTContext with parent_source_info = None
├─ Add file to SourceContext: FileId(0) = "dj_index.qmd"
└─ Parse YAML metadata
   └─ Find include-in-header.text at offsets 500-550
```

### Step 2: Recursive Parse of Metadata Value

```rust
// In meta.rs, parse_yaml_string_as_markdown calls:
read(
    "<script src=\"test.js\"></script>",
    "<metadata>",
    parent_source_info: Some(Original(FileId(0), 500, 550))
)
├─ Create ASTContext
│  └─ parent_source_info = Some(Original(FileId(0), 500, 550))
├─ Add file to new SourceContext: FileId(0) = "<metadata>"
│  (This FileId(0) is in child context - different from parent!)
└─ Parse the string
   └─ Find <script> tag at bytes 0-40 in the substring
```

### Step 3: Create SourceInfo for the <script> Node

```rust
// node_to_source_info_with_context is called:
node.start_byte() = 0
node.end_byte() = 40

// Since ctx.parent_source_info is Some(...):
return SourceInfo::Substring {
    parent: Box::new(Original(FileId(0), 500, 550)),  // In PARENT context
    start_offset: 0,
    length: 40,
}
```

### Step 4: Create Warning with This SourceInfo

```rust
DiagnosticMessage {
    location: Some(Substring {
        parent: Original(FileId(0), 500, 550),  // Parent's FileId
        start_offset: 0,
        length: 40,
    }),
    ...
}
```

### Step 5: Return to Parent Parse

```rust
// In meta.rs:
Ok((pandoc, child_context, warnings)) => {
    //         ^^^^^^^^^^^^^ Discarded!
    for warning in warnings {
        diagnostics.add(warning);  // Warning has correct SourceInfo!
    }
}
```

The child's ASTContext (with its SourceContext) is discarded.
But that's OK because the warnings reference the PARENT's SourceContext!

### Step 6: Render the Warning

```rust
// Later, when rendering the diagnostic:
warning.to_text(&parent_context.source_context, ...)

// The SourceInfo mapping:
Substring { parent: Original(FileId(0), 500, 550), start: 0, len: 40 }
  └─ map_offset(0) →
     └─ Parent: Original(FileId(0), 500, 550).map_offset(0) →
        └─ Returns: MappedLocation(FileId(0), Location(offset: 500, row: 12, col: X))

// Render looks up FileId(0) in parent's SourceContext:
parent_context.source_context.get_file(FileId(0))
  └─ Returns file: "dj_index.qmd" with full content
     └─ Extracts line 12 ✓
        └─ Highlights correctly ✓
```

## Critical Insight: Why This Works

The key is that **Substring holds a pointer to parent SourceInfo**, not just offsets!

```rust
Substring {
    parent: Box<SourceInfo>,  // Carries the parent's FileId and context!
    start_offset: usize,
    length: usize,
}
```

When we wrap as Substring:
- Child node: offset 0 in `<metadata>` (child context, FileId(0) in child)
- Wrapped: Substring of parent (parent context, FileId(0) in parent)
- The parent's FileId is different from child's FileId - they're in different SourceContexts!
- But mapping works because Substring **chains to parent**, eventually reaching the parent's FileId

## Potential Issues & Solutions

### Issue 1: FileId Collision

**Problem**: Child and parent both use FileId(0), but in different SourceContexts.

**Why It's OK**:
- Child's FileId(0) is in child's SourceContext (discarded)
- Substring wraps with parent's SourceInfo (which has parent's FileId(0))
- When rendering, we use parent's SourceContext which has parent's FileId(0) ✓

### Issue 2: Child SourceContext Wasted

**Problem**: We create a SourceContext for `<metadata>` that's immediately discarded.

**Why It's OK**:
- Needed for parse-time diagnostics (if tree-sitter parsing fails)
- Small overhead (one file entry, some line break indices)
- Could optimize later if it matters

**Better**: Could skip adding to child context if parent_source_info is set, but adds complexity.

### Issue 3: Multiple Diagnostic Locations

**Problem**: DiagnosticMessage has `location`, and `details[].location`.

**Status**: Already handled! ✓
- Both use the same `node_to_source_info_with_context` path
- Both get wrapped as Substrings automatically
- No special handling needed

### Issue 4: Nested Recursive Parses

**Problem**: What if YAML metadata includes another file with metadata?

**Example**:
```yaml
# dj_index.qmd
---
include: other.qmd
---

# other.qmd (included)
---
format:
  html:
    include-in-header:
      text: <script>...</script>
---
```

**Flow**:
1. Parse dj_index.qmd → FileId(0)
2. Parse other.qmd → FileId(1)
3. Parse metadata in other.qmd → parent = Original(FileId(1), ...)
4. Wrap as Substring of parent ✓

**Status**: Works! Each level wraps relative to its immediate parent.

### Issue 5: Contract Between parent_source_info and input_bytes

**Problem**: parent_source_info must represent exactly the same content as input_bytes.

**Contract**:
```rust
// REQUIRED INVARIANT:
parent_source_info.length() == input_bytes.len()

// The content at parent_source_info in the parent file must be:
file_content[parent_source_info.start_offset()..parent_source_info.end_offset()]
  == input_bytes
```

**If Violated**: Offsets won't align, incorrect locations.

**Enforcement**: Document clearly, add debug assertions?

```rust
if let Some(parent) = &parent_source_info {
    debug_assert_eq!(
        parent.length(),
        input_bytes.len(),
        "parent_source_info length must match input_bytes length"
    );
}
```

### Issue 6: Range vs Length Semantics

**Question**: When creating Substring, do we use `node.end_byte()` or `node.end_byte() - node.start_byte()`?

**Answer**: `end_byte() - start_byte()` (the length).

```rust
SourceInfo::substring(
    parent,
    node.start_byte(),      // Offset within parent
    node.end_byte() - node.start_byte(),  // Length
)
```

But wait, let me check the Substring constructor...

Looking at quarto-source-map API, `SourceInfo::substring` takes:
- parent: SourceInfo
- start_offset: usize
- end_offset: usize  (NOT length!)

Actually, let me check the signature... From the earlier code I saw:
```rust
pub fn substring(parent: SourceInfo, start_offset: usize, end_offset: usize) -> Self
```

So it's `start_offset` and `end_offset`, not `start` and `length`.

So the correct wrapping is:
```rust
SourceInfo::substring(
    parent.clone(),
    node.start_byte(),
    node.end_byte(),  // end_offset, not length!
)
```

## Call Site Update Effort

Found ~18-20 call sites:
- 17+ in qmd-syntax-helper (various conversion modules)
- 1 in pico-quarto-render
- 1 recursive call in meta.rs (the important one)
- Plus internal recursive call in read() for newline handling

**Mechanical change**: Add `, None` to all existing calls.

Could create a helper function to reduce boilerplate:
```rust
pub fn read_top_level<T: Write>(
    input_bytes: &[u8],
    filename: &str,
    output_stream: &mut T,
) -> Result<...> {
    read(input_bytes, false, filename, output_stream, true, None)
}
```

## Summary: Is It Sufficient?

### ✅ YES, this approach is sufficient!

**It fixes:**
1. AST node SourceInfo ✓ (wrapped as Substrings)
2. Diagnostic SourceInfo ✓ (wrapped as Substrings)
3. Nested parses ✓ (chain through parent)
4. All detail locations ✓ (same wrapping path)

**It's correct because:**
1. Substring chains to parent SourceInfo with correct FileId
2. Parent's SourceContext is used for rendering
3. Offsets map correctly through the chain
4. No FileId conflicts (different contexts)

**It's maintainable because:**
1. Single wrapping point (node_to_source_info_with_context)
2. Automatic propagation to all SourceInfo creation
3. Clear separation: child context is ephemeral, parent context is permanent
4. Simple contract: parent_source_info represents input_bytes

**Caveats:**
1. ~20 call sites to update (mechanical but tedious)
2. Must maintain contract: parent_source_info.length() == input_bytes.len()
3. Small overhead: child SourceContext created but discarded
4. Need comprehensive testing for nested cases

## Recommendation

**Proceed with Option<SourceInfo> approach.**

This is the fundamentally correct fix that:
- Solves the problem completely
- Maintains semantic correctness
- Supports future use cases (metadata node SourceInfo)
- Has manageable implementation cost

The API change is justified by the correctness gain.
