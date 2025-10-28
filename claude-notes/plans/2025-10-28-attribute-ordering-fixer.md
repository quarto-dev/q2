# Attribute Ordering Fixer for qmd-syntax-helper

**Issue**: qmd-7
**Date**: 2025-10-28

## Problem Statement

Our parser intentionally rejects attribute syntax where key-value pairs appear before class/id specifiers (e.g., `{key=value .class #id}`). This is stricter than Pandoc, which accepts any order. We want strict ordering for better roundtripping support.

However, 4/509 files in the quarto-web corpus have this issue. We need a qmd-syntax-helper rule to:
1. Detect these ordering violations
2. Automatically fix them by reordering attributes

## Challenge

The invalid syntax cannot be parsed by quarto-markdown-pandoc, so we can't use our normal AST-based approach. Instead, we'll use **Pandoc as a normalizer**.

## Pandoc Normalization Behavior

Pandoc accepts attributes in any order and outputs them in the canonical order: `#id .classes key="value"`

```bash
$ echo '[]{key=value .class #id}' | pandoc -t markdown
[]{#id .class key="value"}
```

This works for:
- Empty spans: `[]{attrs}`
- Spans with content: `[foo]{attrs}`
- Headers: `# Header {attrs}`
- Any other element with attributes

## Design

### Rule Name
`attribute-ordering`

### Error Detection

1. Parse file with `quarto-markdown-pandoc::readers::qmd::read()`
2. Look for diagnostic messages with title: `"Key-value Pair Before Class Specifier in Attribute"`
3. Extract location information (start_offset, end_offset) from each error
4. The error location points to the class/id specifier that appears after a key-value pair

Example error:
```json
{
  "kind": "error",
  "location": {
    "Original": {
      "start_offset": 49,
      "end_offset": 55,
      "file_id": 0
    }
  },
  "title": "Key-value Pair Before Class Specifier in Attribute"
}
```

### Conversion Strategy

For each violation:

1. **Locate the attribute block**:
   - Start from the error's `start_offset`
   - Search backward to find the opening `{`
   - Search forward to find the closing `}`
   - Extract the full attribute string (including braces)

2. **Normalize using Pandoc**:
   - Create a temporary input: `[]` + extracted_attrs
   - Run: `echo '[]{key=value .class}' | pandoc -t markdown`
   - Parse the output to extract the normalized attributes
   - Use regex to extract: `^\[\]\{(.+)\}$` → capture group 1

3. **Replace in source**:
   - Replace the original attribute block with the normalized version
   - Process violations in reverse order (bottom to top) to avoid offset invalidation

### Implementation Details

#### File Structure
```
crates/qmd-syntax-helper/src/conversions/attribute_ordering.rs
```

#### Key Functions

```rust
pub struct AttributeOrderingConverter {}

impl AttributeOrderingConverter {
    /// Get parse errors containing attribute ordering violations
    fn get_attribute_ordering_errors(&self, file_path: &Path)
        -> Result<Vec<AttributeOrderingViolation>>

    /// Find the full attribute block given an error location
    fn find_attribute_block(&self, content: &str, error_offset: usize)
        -> Result<(usize, usize)> // (start, end) byte offsets

    /// Normalize attributes using Pandoc
    fn normalize_with_pandoc(&self, attrs: &str)
        -> Result<String>

    /// Apply fixes to the content
    fn apply_fixes(&self, content: &str, violations: Vec<AttributeOrderingViolation>)
        -> Result<String>
}

struct AttributeOrderingViolation {
    start_offset: usize,  // Offset of '{'
    end_offset: usize,    // Offset of '}'
    original: String,     // Original attrs including braces
}
```

#### Algorithm for `find_attribute_block`

```rust
fn find_attribute_block(&self, content: &str, error_offset: usize) -> Result<(usize, usize)> {
    let bytes = content.as_bytes();

    // Search backward for '{'
    let mut start = error_offset;
    while start > 0 && bytes[start] != b'{' {
        start -= 1;
    }
    if bytes[start] != b'{' {
        return Err(anyhow!("Could not find opening brace"));
    }

    // Search forward for '}'
    let mut end = error_offset;
    while end < bytes.len() && bytes[end] != b'}' {
        end += 1;
    }
    if end >= bytes.len() || bytes[end] != b'}' {
        return Err(anyhow!("Could not find closing brace"));
    }

    Ok((start, end + 1)) // +1 to include the '}'
}
```

#### Algorithm for `normalize_with_pandoc`

```rust
fn normalize_with_pandoc(&self, attrs: &str) -> Result<String> {
    // Create input: []{ + attrs_content + }
    // attrs is already "{...}" so wrap with []
    let input = format!("[]{}", attrs);

    // Run pandoc
    let output = Command::new("pandoc")
        .arg("-t")
        .arg("markdown")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()?;

    output.stdin.write_all(input.as_bytes())?;
    let result = output.wait_with_output()?;
    let stdout = String::from_utf8(result.stdout)?;

    // Extract normalized attrs from "[]{...}"
    let re = Regex::new(r"^\[\]\{(.+)\}$")?;
    if let Some(caps) = re.captures(stdout.trim()) {
        Ok(format!("{{{}}}", &caps[1]))
    } else {
        Err(anyhow!("Unexpected pandoc output: {}", stdout))
    }
}
```

#### Algorithm for `apply_fixes`

```rust
fn apply_fixes(&self, content: &str, mut violations: Vec<AttributeOrderingViolation>)
    -> Result<String> {

    // Sort violations in reverse order to avoid offset invalidation
    violations.sort_by_key(|v| std::cmp::Reverse(v.start_offset));

    let mut result = content.to_string();

    for violation in violations {
        let normalized = self.normalize_with_pandoc(&violation.original)?;

        // Replace original with normalized
        result.replace_range(
            violation.start_offset..violation.end_offset,
            &normalized
        );
    }

    Ok(result)
}
```

### Integration with Rule Registry

Update `rule.rs`:

```rust
// In RuleRegistry::new()
registry.register(Arc::new(
    crate::conversions::attribute_ordering::AttributeOrderingConverter::new()?,
));
```

### Testing Strategy

1. **Unit tests**:
   - Test `find_attribute_block` with various error offsets
   - Test `normalize_with_pandoc` with different attribute orderings
   - Test `apply_fixes` with multiple violations

2. **Integration tests**:
   - Test files with single violation
   - Test files with multiple violations
   - Test files with violations on different element types (span, header, div)
   - Test that already-correct files are not modified

3. **Real-world validation**:
   - Run on the 4/509 files from quarto-web corpus
   - Verify the output parses successfully
   - Verify the attributes are functionally equivalent

### Example Test Cases

```markdown
# Input
[span]{key=value .class #id}

# Expected output
[span]{#id .class key="value"}
```

```markdown
# Input
# Header {key1=val1 .class key2=val2 #id}

# Expected output
# Header {#id .class key1="val1" key2="val2"}
```

```markdown
# Input with multiple violations
[first]{key=value .class}

Some text.

[second]{another=val .other #id}

# Expected output
[first]{.class key="value"}

Some text.

[second]{#id .other another="val"}
```

## Edge Cases to Handle

1. **Nested braces**: Unlikely in attributes, but should handle gracefully
2. **Malformed attributes**: If Pandoc fails, report error and skip
3. **Already-correct attributes**: Should detect and skip (no-op)
4. **Multiple violations in same file**: Process in reverse order
5. **Comments or code blocks with similar syntax**: Parser should only flag actual attribute blocks

## Dependencies

- `regex` crate for parsing Pandoc output
- `std::process::Command` for running Pandoc
- Pandoc must be installed and in PATH

## Error Handling

- If Pandoc is not installed: fail gracefully with helpful message
- If Pandoc normalization fails: log error, skip that violation
- If attribute block cannot be located: log error, skip
- Continue processing other violations even if one fails

## Success Criteria

1. ✅ Detect all attribute ordering violations
2. ✅ Correctly normalize attributes using Pandoc
3. ✅ Apply fixes without breaking file structure
4. ✅ Handle multiple violations in one file
5. ✅ Pass all test cases
6. ✅ Successfully fix the 4/509 files from quarto-web corpus
7. ✅ Fixed files parse successfully with quarto-markdown-pandoc

## Implementation Steps

1. Create `conversions/attribute_ordering.rs` skeleton
2. Implement `get_attribute_ordering_errors()`
3. Implement `find_attribute_block()`
4. Implement `normalize_with_pandoc()`
5. Implement `apply_fixes()`
6. Implement `Rule` trait methods
7. Register rule in `RuleRegistry`
8. Write unit tests
9. Write integration tests
10. Test on real quarto-web files
11. Document usage in qmd-syntax-helper help text

## Estimated Effort

- Implementation: 3-4 hours
- Testing: 2 hours
- Documentation: 1 hour
- **Total: 6-7 hours**
