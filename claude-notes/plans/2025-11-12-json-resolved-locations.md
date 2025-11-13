# JSON Writer: Add Resolved Source Locations

## Goal
Add an optional 'l' (location) field to JSON output containing fully resolved source position information for each node.

## Background

### Current State
- JSON writer uses pool-based serialization for `SourceInfo` efficiency (93% size reduction)
- Nodes reference source info via integer ID in 's' field: `{"t": "Str", "c": "hello", "s": 0}`
- Source info pool contains compact format: `{"r": [0, 5], "t": 0, "d": 0}`
- No configuration mechanism exists in JSON writer

### Desired State
Add optional 'l' field with resolved locations:
```json
{
  "t": "Str",
  "c": "hello",
  "s": 0,
  "l": {
    "b": {"o": 0, "l": 1, "c": 1},
    "e": {"o": 5, "l": 1, "c": 6}
  }
}
```

Where:
- `b` = begin position, `e` = end position
- `o` = offset (unicode characters from file start)
- `l` = line (1-based)
- `c` = column (1-based)

## Technical Details

### Source Map Resolution
From quarto-source-map crate:
- `SourceInfo::map_offset(offset, ctx)` → `Option<MappedLocation>`
- `SourceInfo::map_range(start, end, ctx)` → `Option<(MappedLocation, MappedLocation)>`
- `MappedLocation` has `file_id` and `Location` (offset, row, column)
- **Internal representation is 0-indexed**, must convert to 1-indexed for output
- Resolution uses O(log n) binary search on pre-computed line breaks

### Current Serialization Flow
1. `write_pandoc()` creates `SourceInfoSerializer` with pool
2. Each node calls `serializer.to_json_ref(&source_info)` → returns ID as JSON number
3. Pool is finalized and added to `astContext.sourceInfoPool`

## Implementation Plan

### 1. Add Configuration Structure
**File**: `crates/quarto-markdown-pandoc/src/writers/json.rs`

Add config struct (similar to ansi.rs pattern):
```rust
pub struct JsonConfig {
    pub include_inline_locations: bool,
}

impl Default for JsonConfig {
    fn default() -> Self {
        Self { include_inline_locations: false }
    }
}
```

Add new entry point:
```rust
pub fn write_with_config<W: std::io::Write>(
    pandoc: &Pandoc,
    context: &ASTContext,
    writer: &mut W,
    config: &JsonConfig,
) -> std::io::Result<()>
```

Update existing `write()` to delegate to `write_with_config()` with default config.

### 2. Thread Config Through Serializer
**File**: `crates/quarto-markdown-pandoc/src/writers/json.rs`

Modify `SourceInfoSerializer`:
```rust
struct SourceInfoSerializer<'a> {
    pool: Vec<SerializableSourceInfo>,
    id_map: HashMap<*const SourceInfo, usize>,
    context: &'a ASTContext,  // Need this for resolution
    config: &'a JsonConfig,    // For conditional behavior
}
```

### 3. Add Location Resolution Helper
**File**: `crates/quarto-markdown-pandoc/src/writers/json.rs`

Add helper function:
```rust
fn resolve_location(source_info: &SourceInfo, context: &ASTContext) -> Option<Value> {
    // Get the range from source_info
    let (start_offset, end_offset) = (source_info.start_offset(), source_info.end_offset());

    // Map both offsets to resolved locations
    let (start_mapped, end_mapped) = source_info.map_range(start_offset, end_offset, context.source_context())?;

    // Convert from 0-indexed (internal) to 1-based (output)
    Some(json!({
        "b": {
            "o": start_mapped.location.offset,
            "l": start_mapped.location.row + 1,
            "c": start_mapped.location.column + 1
        },
        "e": {
            "o": end_mapped.location.offset,
            "l": end_mapped.location.row + 1,
            "c": end_mapped.location.column + 1
        }
    }))
}
```

### 4. Modify Serialization Methods
**File**: `crates/quarto-markdown-pandoc/src/writers/json.rs`

Update `to_json_ref()` to conditionally add location:
```rust
fn to_json_ref(&mut self, source_info: &SourceInfo) -> Value {
    let id = self.intern(source_info);

    if self.config.include_inline_locations {
        if let Some(location) = resolve_location(source_info, self.context) {
            return json!({
                "s": id,
                "l": location
            });
        }
    }

    json!(id)  // Fallback: just the ID
}
```

**WAIT**: This changes the return type from `Value` (integer) to `Value` (object).

**Better approach**: Return an object consistently when config is enabled:
- Instead of `"s": 0`, always produce `"s": {"id": 0, "l": {...}}`
- OR: Keep 's' as ID, add separate 'l' field at node level

**Decision needed**: Which structure?
- Option A: `"s": {"id": 0, "l": {...}}` - location nested under 's'
- Option B: `"s": 0, "l": {...}` - location as sibling field (user's request)

**User specified**: "an additional field 'l'" - so Option B (sibling field).

This means we can't just modify `to_json_ref()`. We need to change how nodes are serialized.

### 4. (REVISED) Change Node Serialization Pattern
**Problem**: Currently `serializer.to_json_ref()` returns just the ID, which gets assigned to 's'.

**Solution**: Make `to_json_ref()` return an `Option<(id, location)>`, then let each node builder decide how to structure the JSON.

**Better solution**: Create two methods:
- `to_json_id(&source_info)` → just the ID (current behavior)
- `to_json_location(&source_info)` → optional location data

Then update each node serialization:
```rust
fn write_inline(inline: &Inline, serializer: &mut SourceInfoSerializer) -> Value {
    match inline {
        Inline::Str(s) => {
            let mut obj = json!({
                "t": "Str",
                "c": s.text,
                "s": serializer.to_json_id(&s.source_info)
            });

            if let Some(location) = serializer.to_json_location(&s.source_info) {
                obj["l"] = location;
            }

            obj
        },
        // ... repeat for all variants
    }
}
```

**Problem**: This requires changing ~50+ match arms across write_inline, write_block, write_table_part, etc.

**Better approach**: Create a helper macro or function to build node JSON with optional location:

```rust
fn node_json(
    serializer: &mut SourceInfoSerializer,
    t: &str,
    c: Value,
    source_info: &SourceInfo
) -> Value {
    let mut obj = json!({"t": t, "c": c});
    obj["s"] = serializer.to_json_id(source_info);

    if let Some(location) = serializer.to_json_location(source_info) {
        obj["l"] = location;
    }

    obj
}
```

Then nodes become:
```rust
Inline::Str(s) => node_json(serializer, "Str", json!(s.text), &s.source_info)
```

**Issue**: Not all nodes have the same structure (some have multiple fields in 'c', some have attrs, etc.)

### 4. (FINAL APPROACH) Post-process JSON objects
Since there are so many node types, the cleanest approach is:
1. Keep current serialization mostly intact
2. Create a helper that adds 'l' field to existing JSON objects
3. Apply it conditionally based on config

```rust
fn add_location_if_needed(
    mut node: Value,
    source_info: &SourceInfo,
    context: &ASTContext,
    config: &JsonConfig
) -> Value {
    if config.include_inline_locations {
        if let Some(location) = resolve_location(source_info, context) {
            node["l"] = location;
        }
    }
    node
}
```

Then wrap node creation:
```rust
Inline::Str(s) => add_location_if_needed(
    json!({
        "t": "Str",
        "c": s.text,
        "s": serializer.to_json_ref(&s.source_info)
    }),
    &s.source_info,
    serializer.context,
    serializer.config
)
```

**Still requires updating all ~50+ match arms.**

### 5. Alternative: Extend SourceInfoSerializer API
Add method to serializer that handles both 's' and 'l':
```rust
impl SourceInfoSerializer {
    fn source_fields(&mut self, source_info: &SourceInfo) -> Vec<(&'static str, Value)> {
        let id = self.intern(source_info);
        let mut fields = vec![("s", json!(id))];

        if self.config.include_inline_locations {
            if let Some(location) = resolve_location(source_info, self.context) {
                fields.push(("l", location));
            }
        }

        fields
    }
}
```

Then use `json!` macro's object merging:
```rust
Inline::Str(s) => {
    let mut obj = json!({"t": "Str", "c": s.text});
    for (key, value) in serializer.source_fields(&s.source_info) {
        obj[key] = value;
    }
    obj
}
```

**This still requires touching every match arm.**

## Decision Point
Given that we need to touch many match arms regardless, the cleanest approach is:

1. Add helper method `serializer.add_source_info(&mut json_obj, &source_info)`
2. Update each node creation to call this helper
3. Helper adds both 's' and optionally 'l' fields

This keeps the logic centralized while being explicit at each call site.

## Implementation Steps

### Phase 1: Setup (Config and Infrastructure)
1. Add `JsonConfig` struct with `include_inline_locations` field
2. Add `write_with_config()` function
3. Update existing `write()` to call `write_with_config()` with default config
4. Thread config through to `SourceInfoSerializer`
5. Add `ASTContext` reference to `SourceInfoSerializer`

### Phase 2: Location Resolution
6. Add `resolve_location()` helper function
7. Add test for resolution logic (verify 0→1 index conversion)
8. Add `add_source_info()` method to `SourceInfoSerializer`

### Phase 3: Update Serialization
9. Update `write_inline()` - all Inline variants (~25 variants)
10. Update `write_block()` - all Block variants (~20 variants)
11. Update `write_meta_value()`, `write_caption()`, `write_table_*()` as needed
12. Update `write_attr()` if it has source info

### Phase 4: Integration
13. Add command-line flag to enable feature (in main.rs)
14. Add WASM entry point with config option (in wasm_entry_points/)
15. Update documentation

### Phase 5: Testing
16. Create test file with known source positions
17. Verify JSON output has correct 'l' fields
18. Verify 1-based indexing is correct
19. Verify config flag toggles behavior correctly
20. Test with complex transformations (substring, concat)

## Design Decisions (RESOLVED)

1. **Performance**: Users opt-in and accept the cost. No premature optimization.

2. **Failures**: If `map_range()` returns `None`, omit 'l' field silently.

3. **File ID**: YES - include file_id as 'f' field in the location object.

4. **Naming**: Intentional for JSON size optimization. Keep terse names.

5. **CLI Flag**: Add `--json-source-location=full` option (disabled by default).

## Final Output Format

```json
{
  "t": "Str",
  "c": "hello",
  "s": 0,
  "l": {
    "f": 0,
    "b": {"o": 0, "l": 1, "c": 1},
    "e": {"o": 5, "l": 1, "c": 6}
  }
}
```

Where 'f' is the file_id from MappedLocation.

## Acceptance Criteria

- [ ] JSON output can optionally include 'l' field on nodes
- [ ] 'l' field has correct structure: `{b: {o, l, c}, e: {o, l, c}}`
- [ ] Line and column numbers are 1-based (not 0-based)
- [ ] Offsets are in unicode characters (not bytes)
- [ ] Feature is opt-in via `JsonConfig`
- [ ] Existing behavior unchanged when config is disabled
- [ ] Command-line flag added to enable feature
- [ ] Tests verify correctness of resolved locations
- [ ] Documentation updated
