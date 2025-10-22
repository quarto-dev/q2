# Plan: YAML Tag-Based Markdown Parsing Behavior (2025-10-21)

## Problem Statement

Currently, when YAML metadata values fail to parse as markdown (e.g., `resources: images/*.png`), the parser wraps them in a Span with class `yaml-markdown-syntax-error`. This is silent - no warning is shown to the user.

We need to change the behavior based on YAML tags:
1. **`!str` or `!path` tag**: Emit the Str node without a syntax-error span (bypass markdown parsing)
2. **`!md` tag**: Fail the parse with a proper error message
3. **No tag**: Emit a visible warning (not just a silent span)

## Related Issues

- **qmd-8**: "Provide linting warnings for metadata strings that fail to parse as markdown"
  - Priority: 2, Feature, Open
  - Depends on qmd-9 (general linting support)
- **k-62**: "YAML tag information lost in new API with source tracking"
  - Status: Closed
  - Tags are now preserved in `YamlWithSourceInfo.tag` field

## Current Behavior Analysis

### Code Location: `meta.rs:532-624`

Two functions handle metadata string parsing:

#### 1. `parse_metadata_strings_with_source_info()` (lines 532-624)
- Called on `MetaValueWithSourceInfo` after YAML parsing
- For `MetaString` values: tries to parse as markdown
- On parse error: wraps in Span with class "yaml-markdown-syntax-error" (line 575-591)
- **Issue**: No warning emitted, just silent wrapping

#### 2. `parse_metadata_strings()` (lines 626-698) - Legacy version
- Similar behavior but works with old `MetaValue` type
- Also silently wraps errors (lines 664-678)

### Tag Handling: `yaml_to_meta_with_source_info()` (lines 206-339)

Located in `meta.rs:264-296`, this function already checks for YAML tags:

```rust
match yaml_value {
    Yaml::String(s) => {
        // Check for YAML tags (e.g., !path, !glob, !str)
        if let Some((tag_suffix, _tag_source_info)) = tag {
            // Tagged string - bypass markdown parsing
            // Wrap in Span with class "yaml-tagged-string" and tag attribute
            // ...
            MetaValueWithSourceInfo::MetaInlines { ... }
        } else {
            // Untagged string - return as MetaString for later markdown parsing
            MetaValueWithSourceInfo::MetaString { ... }
        }
    }
}
```

**Current tagged values**: `!path`, `!glob`, `!str` all bypass markdown parsing and become `MetaInlines` with `yaml-tagged-string` class.

## Proposed Changes

### Change 1: Handle `!str` and `!path` Tags

**Location**: `yaml_to_meta_with_source_info()` (meta.rs:264-296)

**Current behavior**: All tags wrap in Span with class "yaml-tagged-string"

**New behavior**: For `!str` and `!path`, emit plain Str node without wrapping:

```rust
if let Some((tag_suffix, _tag_source_info)) = tag {
    match tag_suffix.as_str() {
        "str" | "path" => {
            // Emit plain Str without markdown parsing, no error span
            MetaValueWithSourceInfo::MetaInlines {
                content: vec![Inline::Str(Str {
                    text: s.clone(),
                    source_info: source_info.clone(),
                })],
                source_info,
            }
        }
        "md" => {
            // Will be handled in parse_metadata_strings_with_source_info
            // Return as MetaString to trigger markdown parsing
            MetaValueWithSourceInfo::MetaString {
                value: s,
                source_info,
            }
        }
        _ => {
            // Other tags (like !glob, !expr): keep current behavior
            // Wrap in Span with yaml-tagged-string class
            // ... existing code ...
        }
    }
}
```

### Change 2: Add Error for `!md` Tag with Parse Failure

**Location**: `parse_metadata_strings_with_source_info()` (meta.rs:532-624)

**New behavior**: Check if the original value had `!md` tag, and if markdown parse fails, emit error instead of warning.

**Problem**: By the time we reach `parse_metadata_strings_with_source_info()`, we've lost tag information. The `MetaString` doesn't carry the tag.

**Solution Option A**: Add tag tracking to `MetaValueWithSourceInfo::MetaString` variant:
```rust
MetaString {
    value: String,
    source_info: SourceInfo,
    tag: Option<(String, SourceInfo)>,  // NEW
}
```

**Solution Option B**: Handle `!md` specially in `yaml_to_meta_with_source_info()`:
- When tag is `!md`, immediately try to parse as markdown
- If parse fails, emit error using quarto-error-reporting
- If parse succeeds, return MetaInlines

**Recommendation**: Option B - handle `!md` tag at YAML→Meta conversion time.

### Change 3: Emit Warning for Untagged Parse Failures

**Location**: `parse_metadata_strings_with_source_info()` (meta.rs:574-591)

**Current code**:
```rust
Err(_) => {
    // Markdown parse failed - wrap in Span with class "yaml-markdown-syntax-error"
    let span = Span { ... };
    MetaValueWithSourceInfo::MetaInlines {
        content: vec![Inline::Span(span)],
        source_info,
    }
}
```

**New code**:
```rust
Err(_) => {
    // Emit warning using quarto-error-reporting
    // TODO: Need access to diagnostic collector

    // Still wrap in span for graceful degradation
    let span = Span { ... };
    MetaValueWithSourceInfo::MetaInlines {
        content: vec![Inline::Span(span)],
        source_info,
    }
}
```

**Challenge**: `parse_metadata_strings_with_source_info()` doesn't have access to a diagnostic collector. We need to:
1. Thread a diagnostic collector through the function signatures, OR
2. Return warnings alongside the result, OR
3. Use a different mechanism

## Architecture Considerations

### Diagnostic Collection Challenge

The current call chain is:
1. `readers/qmd.rs` → `treesitter_to_pandoc()`
2. → `postprocess()` which calls `parse_metadata_strings_with_source_info()`
3. No diagnostic collector is threaded through

**Options**:

**Option A**: Thread diagnostic collector through signatures
- Add parameter to `parse_metadata_strings_with_source_info()`
- Add parameter to `postprocess()`
- Caller provides collector

**Option B**: Return warnings alongside result
- Change return type to `(MetaValueWithSourceInfo, Vec<DiagnosticMessage>)`
- Caller collects warnings

**Option C**: Use thread-local or global collector
- Less explicit, harder to test
- Not recommended

**Recommendation**: Option A - explicit parameter passing is clearest.

### Warning vs Error Infrastructure

`quarto-error-reporting` already supports warnings:
- `DiagnosticKind::Warning` exists (diagnostic.rs:14)
- `DiagnosticMessage::warning()` constructor (diagnostic.rs:214)
- `to_text()` and `to_json()` handle warnings (diagnostic.rs:288, 382)

**No changes needed to quarto-error-reporting** ✓

## Implementation Plan

### Phase 1: Extend yaml_to_meta_with_source_info for !md Tag

**Files**: `crates/quarto-markdown-pandoc/src/pandoc/meta.rs`

**Tasks**:
1. In `yaml_to_meta_with_source_info()`, match on tag suffix
2. For `"md"` tag: attempt markdown parse immediately
3. If parse fails: create `DiagnosticMessage::error()` and... (need collector access)
4. For `"str"` and `"path"`: emit plain Str without yaml-tagged-string wrapper

**Blocker**: Need access to diagnostic collector in `yaml_to_meta_with_source_info()`.

### Phase 2: Thread Diagnostic Collector Through Call Chain

**Files**:
- `crates/quarto-markdown-pandoc/src/pandoc/meta.rs`
- `crates/quarto-markdown-pandoc/src/postprocess.rs`

**Function signature changes**:

```rust
// meta.rs
pub fn yaml_to_meta_with_source_info(
    yaml: quarto_yaml::YamlWithSourceInfo,
    context: &crate::pandoc::ast_context::ASTContext,
    diagnostics: &mut Vec<quarto_error_reporting::DiagnosticMessage>,  // NEW
) -> MetaValueWithSourceInfo { ... }

pub fn parse_metadata_strings_with_source_info(
    meta: MetaValueWithSourceInfo,
    outer_metadata: &mut Vec<MetaMapEntry>,
    diagnostics: &mut Vec<quarto_error_reporting::DiagnosticMessage>,  // NEW
) -> MetaValueWithSourceInfo { ... }
```

**Callers to update**:
- `rawblock_to_meta_with_source_info()` in meta.rs
- `postprocess()` in postprocess.rs
- Any tests that call these functions

### Phase 3: Implement Tag-Based Behaviors

**3a. Handle `!str` and `!path`**:
```rust
match tag_suffix.as_str() {
    "str" | "path" => {
        MetaValueWithSourceInfo::MetaInlines {
            content: vec![Inline::Str(Str {
                text: s.clone(),
                source_info: source_info.clone(),
            })],
            source_info,
        }
    }
    // ...
}
```

**3b. Handle `!md` with error on failure**:
```rust
"md" => {
    // Try to parse as markdown immediately
    let mut output_stream = VerboseOutput::Sink(io::sink());
    let result = readers::qmd::read(
        s.as_bytes(),
        false,
        "<metadata>",
        &mut output_stream,
        None,
    );

    match result {
        Ok((mut pandoc, _context)) => {
            // Parse succeeded - return MetaInlines or MetaBlocks
            // ... existing logic ...
        }
        Err(_) => {
            // Parse failed - emit ERROR
            let error = DiagnosticMessageBuilder::error("Failed to parse metadata value as markdown")
                .with_code("Q-X-YYY")  // TODO: assign code
                .problem("The `!md` tag requires valid markdown syntax")
                .add_detail(format!("Could not parse: {}", s))
                .add_hint("Remove the `!md` tag or fix the markdown syntax?")
                .with_location(source_info.clone())
                .build();

            diagnostics.push(error);

            // Still return something for graceful degradation
            MetaValueWithSourceInfo::MetaString {
                value: s,
                source_info,
            }
        }
    }
}
```

**3c. Emit warning for untagged parse failures**:
```rust
Err(_) => {
    // Emit WARNING
    let warning = DiagnosticMessageBuilder::warning("Metadata value failed to parse as markdown")
        .with_code("Q-X-ZZZ")  // TODO: assign code
        .problem("String contains characters that cannot be parsed as markdown")
        .add_detail(format!("Value: {}", value))
        .add_info("Use YAML tags to specify the value type:")
        .add_info("  - `!str` for literal strings")
        .add_info("  - `!path` for file paths")
        .add_hint("Did you mean to use `!str` or `!path`?")
        .with_location(source_info.clone())
        .build();

    diagnostics.push(warning);

    // Still wrap in span for graceful degradation
    let span = Span { ... };
    MetaValueWithSourceInfo::MetaInlines {
        content: vec![Inline::Span(span)],
        source_info,
    }
}
```

### Phase 4: Wire Up Diagnostics to Output

**Files**: `crates/quarto-markdown-pandoc/src/readers/qmd.rs`

The `read()` function needs to output diagnostics collected during metadata parsing.

**Current behavior**: Returns `Result<(PandocAST, ASTContext), String>`

**Options**:
- Add diagnostics to ASTContext
- Change return type to include diagnostics
- Output diagnostics immediately as they're collected

**Need to investigate**: How are diagnostics currently handled in the reader?

### Phase 5: Testing

**Test files to create/update**:

1. **Test `!str` and `!path` behavior**:
   - Create `tests/yaml-tag-str-path.qmd`:
     ```yaml
     ---
     plain_path: !str images/*.png
     tagged_path: !path posts/*/index.qmd
     ---
     ```
   - Verify: No yaml-tagged-string wrapper, plain Str nodes

2. **Test `!md` error on failure**:
   - Create `tests/yaml-tag-md-error.qmd`:
     ```yaml
     ---
     bad_md: !md images/*.png
     ---
     ```
   - Verify: Error is emitted, not warning

3. **Test warning for untagged failures**:
   - Create `tests/yaml-untagged-warning.qmd`:
     ```yaml
     ---
     resources: images/*.png
     ---
     ```
   - Verify: Warning is emitted

4. **Update existing tests**:
   - `tests/test_meta.rs::test_yaml_tagged_strings` - may need updates
   - Any tests expecting yaml-tagged-string for !str or !path

### Phase 6: Error Code Assignment

Need to assign error codes in the Q-X-YYY format:
- `!md` parse failure: Maybe Q-1-X (YAML/metadata subsystem)
- Untagged parse warning: Q-1-Y

**Action**: Check catalog.rs for next available codes.

## Minimal Example Behavior

### Before Changes

**Input** (`example.qmd`):
```yaml
---
resources: images/*.png
---
```

**Output**:
- No warning shown
- Pandoc AST has Span with class "yaml-markdown-syntax-error"

### After Changes

**Input** (`example.qmd`):
```yaml
---
resources: images/*.png
---
```

**Output**:
```
Warning: Metadata value failed to parse as markdown
String contains characters that cannot be parsed as markdown
ℹ Value: images/*.png
ℹ Use YAML tags to specify the value type:
ℹ   - `!str` for literal strings
ℹ   - `!path` for file paths
? Did you mean to use `!str` or `!path`?
```

**With `!path` tag**:
```yaml
---
resources: !path images/*.png
---
```
- No warning
- Plain Str node, no yaml-tagged-string wrapper

**With `!md` tag**:
```yaml
---
title: !md **Bold* text
---
```
- **Error** (not warning): "Failed to parse metadata value as markdown"

## Open Questions

### Q1: Should `!glob` also bypass the yaml-tagged-string wrapper?

**Current behavior**: `!glob` wraps in Span with yaml-tagged-string class

**Consideration**: `!glob` is similar to `!path` - a file path pattern

**Recommendation**: Keep current behavior for now. User explicitly says `!glob`, so preserving the tag info seems useful.

### Q2: What about other tags like `!expr`?

**Current behavior**: Wrapped in yaml-tagged-string span

**Recommendation**: Keep current behavior. These need special processing downstream.

### Q3: Should warnings be emitted even in loose parsing mode?

**Context**: Parser has `--loose` flag for permissive parsing

**Recommendation**: Yes, emit warnings even in loose mode. Warnings don't block execution.

### Q4: How to handle nested metadata structures with mixed tags?

**Example**:
```yaml
format:
  html:
    theme: !md **bold**
```

**Answer**: The tag checking happens per-value during `yaml_to_meta_with_source_info`, so this works naturally.

## Success Criteria

1. ✅ `!str` and `!path` tags produce plain Str nodes without yaml-tagged-string wrapper
2. ✅ `!md` tag with invalid markdown produces ERROR (not warning)
3. ✅ Untagged strings that fail markdown parsing produce WARNING
4. ✅ Warning messages include helpful hints about using tags
5. ✅ All existing tests pass (with updates as needed)
6. ✅ New tests cover all three behaviors

## Dependencies

- **quarto-error-reporting**: Already supports warnings ✓
- **quarto-yaml**: Tag tracking already implemented (k-62) ✓
- **No new dependencies needed**

## Timeline Estimate

- **Phase 1**: 2 hours (extend yaml_to_meta for !md)
- **Phase 2**: 3 hours (thread diagnostic collector)
- **Phase 3**: 4 hours (implement tag-based behaviors)
- **Phase 4**: 2 hours (wire up diagnostics to output)
- **Phase 5**: 4 hours (comprehensive testing)
- **Phase 6**: 1 hour (error code assignment)

**Total**: ~16 hours (2 days)

## Related Beads Issues

- **qmd-8**: This plan directly addresses the core requirement
- **qmd-9**: General linting support - this could be part of that framework
- **k-62**: YAML tag preservation - already completed

## Next Steps

1. Confirm plan with user
2. Create beads issue for this work
3. Implement Phase 1
4. Test incrementally
5. Iterate through phases
