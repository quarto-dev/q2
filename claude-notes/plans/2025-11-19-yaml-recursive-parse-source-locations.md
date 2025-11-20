# Fix Source Location Tracking in Recursive YAML Metadata Parsing

## Problem Analysis

### Observed Behavior
When parsing `~/today/dj_index.qmd`:
- File contains HTML element `<script>` on line 12 inside `include-in-header.text`
- Warning shows location as `dj_index.qmd:1:1` or `dj_index.qmd:3:9` (WRONG)
- Should show `dj_index.qmd:12:X` (CORRECT)

### Root Cause

In `/crates/quarto-markdown-pandoc/src/pandoc/meta.rs:237-260`, the function `parse_yaml_string_as_markdown`:

1. **Receives**:
   - `value: &str` - The YAML string value to parse (e.g., `<script src="...">`)
   - `source_info: &SourceInfo` - Where this value is in the original YAML document
   - `_context: &ASTContext` - The parent AST context (currently unused!)

2. **Creates a NEW isolated parse context** (line 247-253):
```rust
let result = readers::qmd::read(
    value.as_bytes(),    // Just the substring
    false,
    "<metadata>",        // Generic filename, not the real file!
    &mut output_stream,
    true,
);
```

3. **The new parse**:
   - Creates its own `ASTContext` with filename `"<metadata>"`
   - Adds `value` as a file with offsets starting at 0
   - All SourceInfo objects are relative to offset 0 of `value`
   - Warnings have offsets relative to the substring, not the original file

4. **Warnings are propagated incorrectly** (line 258-260):
```rust
for warning in warnings {
    diagnostics.add(warning);  // ⚠️ No offset adjustment!
}
```

### Why It's Wrong

The recursive parse creates SourceInfo that says:
- File: `<metadata>`
- Offset: relative to start of `value` string

But these diagnostics are added to the parent context which has:
- File: `dj_index.qmd`
- Offset: relative to start of entire file

The two coordinate systems don't align!

## Solution Approach

### Option 1: Adjust Diagnostic Locations (Simpler)

After the recursive parse returns, adjust the source locations in all diagnostics:

```rust
for mut warning in warnings {
    // Adjust the warning's source_info by wrapping it in a Substring
    // that maps back to the correct location in the parent file
    if let Some(location) = warning.location_in.as_mut() {
        *location = SourceInfo::substring(
            source_info.clone(),  // Parent location (where the YAML value is)
            0,                     // Start at beginning of the value
            value.len()            // Entire value
        );
    }
    diagnostics.add(warning);
}
```

**Pros**:
- Simple, localized fix
- Doesn't require changing the `read` API

**Cons**:
- Only fixes warnings, not the actual AST source info (but that might be OK since we're parsing metadata)
- Requires traversing diagnostic structure to find and adjust locations

### Option 2: Pass Parent Context (More Correct)

Modify `readers::qmd::read` to accept an optional parent SourceInfo:

```rust
pub fn read<T: Write>(
    input_bytes: &[u8],
    _loose: bool,
    filename: &str,
    mut output_stream: &mut T,
    prune_errors: bool,
    parent_source_info: Option<SourceInfo>,  // NEW PARAMETER
) -> Result<...> {
    // ...
    // When creating SourceInfo for nodes, if parent_source_info is Some,
    // wrap all SourceInfo in Substring relative to parent
}
```

**Pros**:
- Fixes both AST and diagnostic locations
- More architecturally correct

**Cons**:
- Requires API change and updates to all call sites
- More complex implementation

### Option 3: Use Parent Context's SourceContext (Hybrid)

Instead of creating a new `ASTContext`, reuse the parent's `SourceContext` and add the value as a substring:

```rust
// In parse_yaml_string_as_markdown:
let mut child_context = context.clone();  // Clone parent context

// Add the value as a substring in the parent's SourceContext
let value_file_id = child_context.source_context.add_file_with_info(
    format!("<metadata-value-at-{}:{}>", source_info.start_line(), source_info.start_col()),
    FileInformation::new(value)
);

// Create a Substring SourceInfo that maps to parent
let value_source_info = SourceInfo::substring(
    source_info.clone(),
    0,
    value.len()
);

// Now parse with this context...
// Diagnostics will have the correct context
```

**Pros**:
- Preserves context chain
- Diagnostics automatically have correct file references

**Cons**:
- Still need to handle SourceInfo wrapping
- More complex state management

## Recommended Solution

I recommend **Option 1 (Adjust Diagnostic Locations)** because:

1. **Localized change**: Only touches `parse_yaml_string_as_markdown`
2. **Metadata-specific**: The parsed AST from metadata values gets unwrapped anyway (we extract the Inlines/Blocks), so fixing diagnostic locations is sufficient
3. **Simpler**: Doesn't require API changes or complex context management

## Implementation Steps

1. Create a test case that reproduces the issue
2. Implement helper function to adjust SourceInfo in diagnostics
3. Apply adjustment to warnings before adding to parent diagnostics
4. Verify test passes
5. Test with original problematic file

## Test Case

```yaml
---
format:
  html:
    include-in-header:
      - text: |
          <script src="test.js"></script>
---
```

Expected: Warning at line 5 (where `<script>` is)
Actual (before fix): Warning at line 1 or 3
