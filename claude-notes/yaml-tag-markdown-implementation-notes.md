# YAML Tag-Based Markdown Parsing Implementation Notes

## Overview

Implementing tag-based behavior for YAML metadata markdown parsing (k-90).

**Goal**: Change how metadata strings are parsed based on YAML tags:
- `!str` or `!path`: Emit plain Str nodes (no yaml-tagged-string wrapper)
- `!md`: Fail with ERROR if markdown parse fails
- No tag: Emit WARNING if markdown parse fails

## Beads Issues Created

- **k-90**: Main feature (priority 1)
- **k-91**: Thread diagnostic collector through metadata parsing
- **k-92**: Implement !str and !path behavior
- **k-93**: Implement !md error behavior
- **k-94**: Emit warning for untagged failures
- **k-95**: Add comprehensive tests

Related to: **qmd-8** (linting warnings for metadata)

## Key Files

### Primary Implementation
- `crates/quarto-markdown-pandoc/src/pandoc/meta.rs`
  - `yaml_to_meta_with_source_info()` (lines 206-339): Tag checking happens here
  - `parse_metadata_strings_with_source_info()` (lines 532-624): Where warnings need to be emitted

### Callers to Update
- `crates/quarto-markdown-pandoc/src/pandoc/meta.rs`:
  - `rawblock_to_meta_with_source_info()` (line 468)
- `crates/quarto-markdown-pandoc/src/postprocess.rs`:
  - `postprocess()` function

### Tests
- `crates/quarto-markdown-pandoc/tests/test_meta.rs`
- `crates/quarto-markdown-pandoc/tests/yaml-tagged-strings.qmd`

## Implementation Strategy

### Phase 1: Thread Diagnostic Collector (k-91)

**Challenge**: Currently no access to diagnostic collector in metadata parsing functions.

**Solution**: Add `diagnostics: &mut Vec<DiagnosticMessage>` parameter to:

1. `yaml_to_meta_with_source_info()`:
```rust
pub fn yaml_to_meta_with_source_info(
    yaml: quarto_yaml::YamlWithSourceInfo,
    context: &crate::pandoc::ast_context::ASTContext,
    diagnostics: &mut Vec<quarto_error_reporting::DiagnosticMessage>,  // NEW
) -> MetaValueWithSourceInfo
```

2. `parse_metadata_strings_with_source_info()`:
```rust
pub fn parse_metadata_strings_with_source_info(
    meta: MetaValueWithSourceInfo,
    outer_metadata: &mut Vec<MetaMapEntry>,
    diagnostics: &mut Vec<quarto_error_reporting::DiagnosticMessage>,  // NEW
) -> MetaValueWithSourceInfo
```

3. `rawblock_to_meta_with_source_info()`:
```rust
pub fn rawblock_to_meta_with_source_info(
    block: &RawBlock,
    context: &crate::pandoc::ast_context::ASTContext,
    diagnostics: &mut Vec<quarto_error_reporting::DiagnosticMessage>,  // NEW
) -> MetaValueWithSourceInfo
```

**Callers**:
- `postprocess()` in postprocess.rs needs to create and pass diagnostics vec
- Tests need to be updated

### Phase 2: Implement Tag Behaviors

#### A. !str and !path (k-92)

In `yaml_to_meta_with_source_info()`, around line 267:

```rust
match yaml_value {
    Yaml::String(s) => {
        if let Some((tag_suffix, _tag_source_info)) = tag {
            match tag_suffix.as_str() {
                "str" | "path" => {
                    // NEW: Emit plain Str without wrapper
                    MetaValueWithSourceInfo::MetaInlines {
                        content: vec![Inline::Str(Str {
                            text: s.clone(),
                            source_info: source_info.clone(),
                        })],
                        source_info,
                    }
                }
                "md" => {
                    // NEW: Return as MetaString to trigger markdown parsing
                    // We'll handle errors in parse_metadata_strings_with_source_info
                    MetaValueWithSourceInfo::MetaString {
                        value: s,
                        source_info,
                    }
                }
                _ => {
                    // Existing behavior for !glob, !expr, etc.
                    // Keep yaml-tagged-string wrapper
                    // ... existing code ...
                }
            }
        } else {
            // Untagged - existing behavior
            MetaValueWithSourceInfo::MetaString {
                value: s,
                source_info,
            }
        }
    }
    // ... other cases
}
```

**Issue**: With this approach, we lose the `!md` tag information by the time we get to `parse_metadata_strings_with_source_info()`.

**Better approach**: Add an optional tag field to MetaValueWithSourceInfo::MetaString:

```rust
MetaString {
    value: String,
    source_info: SourceInfo,
    tag: Option<(String, SourceInfo)>,  // Track original tag
}
```

Then in `parse_metadata_strings_with_source_info()`, check if tag was `!md` and emit error instead of warning.

**Alternative**: Handle `!md` parsing immediately in `yaml_to_meta_with_source_info()` to avoid adding tag field to MetaString.

#### B. !md error handling (k-93)

**Option 1**: Add tag field to MetaString (see above)

**Option 2**: Parse !md immediately in yaml_to_meta_with_source_info():

```rust
"md" => {
    // Parse markdown immediately
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
            // Success - extract inlines/blocks
            // ... existing markdown parse logic ...
        }
        Err(_) => {
            // ERROR for !md tag
            let error = DiagnosticMessageBuilder::error("Failed to parse !md tagged value")
                .with_code("Q-1-XXX")  // Assign code later
                .problem("The `!md` tag requires valid markdown syntax")
                .add_detail(format!("Could not parse: {}", s))
                .with_location(source_info.clone())
                .build();

            diagnostics.push(error);

            // Return MetaString for graceful degradation
            MetaValueWithSourceInfo::MetaString {
                value: s,
                source_info,
            }
        }
    }
}
```

**Recommendation**: Option 2 (parse immediately) to avoid changing MetaValueWithSourceInfo structure.

#### C. Untagged warnings (k-94)

In `parse_metadata_strings_with_source_info()`, around line 574:

```rust
Err(_) => {
    // NEW: Emit warning for untagged parse failures
    let warning = DiagnosticMessageBuilder::warning("Metadata value failed to parse as markdown")
        .with_code("Q-1-YYY")  // Assign code later
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
    let span = Span {
        attr: (
            String::new(),
            vec!["yaml-markdown-syntax-error".to_string()],
            HashMap::new(),
        ),
        content: vec![Inline::Str(Str {
            text: value.clone(),
            source_info: quarto_source_map::SourceInfo::default(),
        })],
        source_info: quarto_source_map::SourceInfo::default(),
    };
    MetaValueWithSourceInfo::MetaInlines {
        content: vec![Inline::Span(span)],
        source_info,
    }
}
```

### Phase 3: Wire Up to Output (k-94)

Need to ensure diagnostics collected during metadata parsing are output to user.

**Investigation needed**: How does `postprocess()` currently handle diagnostics?

Looking at postprocess.rs, it takes an `error_collector` parameter. Need to:
1. Check if error_collector can handle DiagnosticMessage
2. Or convert DiagnosticMessage to error_collector format
3. Or output diagnostics separately

### Phase 4: Testing (k-95)

**Test files to create**:

1. `tests/yaml-tag-str.qmd`:
```yaml
---
plain: !str images/*.png
path: !path posts/*/index.qmd
---
```

2. `tests/yaml-tag-md-error.qmd`:
```yaml
---
bad_md: !md images/*.png
---
```

3. `tests/yaml-untagged-warning.qmd`:
```yaml
---
resources: images/*.png
---
```

**Test assertions**:
- !str and !path produce plain Str (no yaml-tagged-string class)
- !md with invalid markdown emits error
- Untagged with invalid markdown emits warning
- All have proper source location info

## Error Codes to Assign

Need to check `crates/quarto-error-reporting/src/catalog.rs` for available codes.

Likely subsystem 1 (YAML/metadata):
- Q-1-XXX: !md parse failure (error)
- Q-1-YYY: Untagged parse failure (warning)

## Questions to Resolve

### Q1: Should we modify MetaValueWithSourceInfo::MetaString to track tag?

**Pros**:
- Can handle !md errors in parse_metadata_strings_with_source_info
- Cleaner separation of concerns

**Cons**:
- Changes public API structure
- More complex

**Recommendation**: Parse !md immediately in yaml_to_meta_with_source_info to avoid API change.

### Q2: How to output diagnostics collected during metadata parsing?

**Options**:
- Integrate with existing error_collector in postprocess
- Add separate diagnostic output path
- Return diagnostics alongside PandocAST

**Need to investigate**: Current error handling in readers/qmd.rs

### Q3: Should !glob also get plain Str treatment?

**Current plan**: Keep yaml-tagged-string wrapper for !glob

**Rationale**: !glob has special meaning for downstream processing

### Q4: What about !expr tags?

**Current plan**: Keep existing yaml-tagged-string wrapper

**Rationale**: Needs special handling by R/Python/Julia evaluator

## Next Steps

1. ✅ Create beads issues
2. ✅ Write implementation notes
3. Start with Phase 1: Thread diagnostic collector
4. Implement Phase 2: Tag behaviors
5. Investigate diagnostic output (Phase 3)
6. Write tests (Phase 4)
7. Assign error codes

## References

- Plan: `claude-notes/plans/2025-10-21-yaml-tag-markdown-warning.md`
- Previous tag work: `claude-notes/plans/2025-10-20-yaml-tag-preservation.md`
- Issue qmd-8: Linting warnings for metadata
- Issue k-62: YAML tag preservation (completed)
