# MappedString and AnnotatedParse Design for YAML in Quarto

## Executive Summary

This document presents a comprehensive design for location tracking in YAML parsing that handles three increasingly complex scenarios:
1. Standalone YAML files (_quarto.yml)
2. YAML metadata blocks in .qmd files
3. YAML in executable code cell options (the hardest case)

**Key Innovation**: We unify MappedString and SourceInfo into a single serializable system that can handle non-contiguous text extraction through explicit mapping strategies. This enables precise error reporting for code cell options like:

```r
#| echo: false
#| output: true
```

Where the YAML parser sees only `echo: false\noutput: true` but error messages must point to the correct line in the source file (including the `#| ` prefix).

**Recommendation**: ✅ Proceed with the unified SourceInfo design using the Concat strategy for cell options.

## Background: The Three YAML Scenarios

### Scenario 1: Standalone YAML Files (Simple)

**Example**: `_quarto.yml`
```yaml
project:
  type: website

format:
  html:
    theme: cosmo
```

**Characteristics**:
- Entire file is YAML
- yaml-rust2's Marker positions are already correct
- No transformation needed

**Implementation**:
```rust
let file_id = source_ctx.add_file("_quarto.yml".into(), Some(content));
let source_info = SourceInfo::original(file_id, Range::from_text(&content));
let annotated = parse_yaml_annotated(&content, source_info)?;

// Error at offset 42 in YAML maps directly to line/column in _quarto.yml
```

### Scenario 2: YAML Metadata Block in .qmd (Medium)

**Example**: `document.qmd`
```markdown
---
title: "My Document"
format:
  html: default
---

# Introduction
This is content.
```

**Characteristics**:
- YAML is embedded in markdown between `---` delimiters
- quarto-markdown parser extracts it as a CodeBlock
- yaml-rust2 receives string `title: "My Document"\nformat:\n  html: default\n`
- Offsets in YAML need adjustment by position of `---` block in .qmd

**Implementation**:
```rust
// quarto-markdown provides CodeBlock with source_info
let yaml_block: CodeBlock = ...; // Extracted from Pandoc AST

// yaml_block.source_info already points to correct range in .qmd
// But it includes the "---" delimiters, so we need to adjust

// Assume YAML content starts at offset 4 within the code block (skip "---\n")
let yaml_source_info = SourceInfo::substring(
    yaml_block.source_info.clone(),
    4,  // Start after "---\n"
    yaml_block.text.len() + 4
);

let annotated = parse_yaml_annotated(&yaml_block.text, yaml_source_info)?;

// Now errors map correctly: yaml offset 5 → code block offset 9 → document.qmd line X
```

**Key insight**: `SourceInfo::Substring` handles this by adding an offset to all positions.

### Scenario 3: YAML in Executable Code Cells (Complex)

**Example**: `analysis.qmd`
```markdown
Here's an analysis:

```{r}
#| echo: false
#| warning: false
#| fig-width: 8
#| fig-height: 6

library(ggplot2)
ggplot(data) + geom_point()
```
```

**Characteristics**:
- YAML is distributed across multiple lines
- Each line has a comment prefix (`#| ` or `%%| ` for other languages)
- Source text: `#| echo: false\n#| warning: false\n...`
- YAML parser sees: `echo: false\nwarning: false\n...`
- **Non-contiguous**: The YAML string is extracted from non-adjacent positions in source
- Error at YAML offset 6 (the 'f' in `false`) needs to map to source offset 13 (after `#| echo: `)

**Challenge**: yaml-rust2's Marker offsets are relative to the *concatenated YAML string*, but we need to map back to the *original source* where each line has a prefix.

**Solution**: Use `SourceInfo::Concat` with careful piece tracking.

## Detailed Design for Scenario 3

### Text Transformation Pipeline

1. **Original source** (what user sees):
```
    offset:  0         1         2         3
             0123456789012345678901234567890123456789
   content: "#| echo: false\n#| warning: false\n#| fig-width: 8\n"
```

2. **Extracted lines** (what we parse):
```
   Line 1: "#| echo: false"    (offsets 0-14 in source)
   Line 2: "#| warning: false" (offsets 15-31 in source)
   Line 3: "#| fig-width: 8"   (offsets 32-47 in source)
```

3. **Normalized YAML** (what yaml-rust2 sees):
```
   offset: 0         1         2
           012345678901234567890123456789012345
   yaml:   "echo: false\nwarning: false\nfig-width: 8\n"
```

4. **Mapping requirement**:
   - YAML offset 0 → source offset 3 (skip `#| `)
   - YAML offset 12 (newline) → source offset 14 (end of line 1)
   - YAML offset 13 → source offset 18 (skip `\n#| ` = 4 chars)
   - YAML offset 28 (newline) → source offset 31
   - YAML offset 29 → source offset 35 (skip `\n#| `)

### SourceInfo::Concat Design

```rust
/// Represents a piece of source text in a concatenation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourcePiece {
    /// Where this piece came from in its parent source
    pub source_info: SourceInfo,

    /// Where this piece starts in the concatenated result
    pub offset_in_concat: usize,

    /// Length of this piece in the concatenated result
    pub length: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SourceMapping {
    Original { file_id: FileId },
    Substring { parent: Box<SourceInfo>, offset: usize },

    /// Multiple pieces concatenated together
    Concat { pieces: Vec<SourcePiece> },

    // ... other variants
}
```

### Building SourceInfo for Cell Options

**Implementation**:
```rust
/// Extract YAML from code cell options
fn extract_cell_options_yaml(
    cell_source: &str,
    cell_source_info: SourceInfo,
) -> (String, SourceInfo) {
    let lines: Vec<&str> = cell_source.lines().collect();
    let mut yaml_parts = Vec::new();
    let mut pieces = Vec::new();
    let mut concat_offset = 0;
    let mut source_offset = 0;

    for line in lines {
        // Check for cell option prefix
        let prefix = if line.starts_with("#| ") {
            "#| "
        } else if line.starts_with("%%| ") {
            "%%| "
        } else {
            source_offset += line.len() + 1; // +1 for newline
            continue; // Not a cell option line
        };

        // Extract YAML content (after prefix)
        let yaml_content = &line[prefix.len()..];
        let content_start_in_source = source_offset + prefix.len();
        let content_len = yaml_content.len();

        // Add to YAML string
        yaml_parts.push(yaml_content);
        yaml_parts.push("\n");

        // Create SourcePiece for this line's content
        pieces.push(SourcePiece {
            source_info: SourceInfo::substring(
                cell_source_info.clone(),
                content_start_in_source,
                content_start_in_source + content_len,
            ),
            offset_in_concat: concat_offset,
            length: content_len,
        });
        concat_offset += content_len;

        // Add newline piece
        // Newline maps to end of this line in source
        pieces.push(SourcePiece {
            source_info: SourceInfo::substring(
                cell_source_info.clone(),
                source_offset + line.len(),
                source_offset + line.len() + 1,
            ),
            offset_in_concat: concat_offset,
            length: 1,
        });
        concat_offset += 1;

        source_offset += line.len() + 1;
    }

    let yaml_string = yaml_parts.concat();
    let yaml_source_info = SourceInfo {
        range: Range {
            start: Location { offset: 0, row: 0, column: 0 },
            end: Location { offset: yaml_string.len(), row: 0, column: 0 },
        },
        mapping: SourceMapping::Concat { pieces },
    };

    (yaml_string, yaml_source_info)
}
```

### Mapping Offsets Back

When yaml-rust2 reports an error at offset 16 in the YAML string:

```rust
impl SourceInfo {
    pub fn map_offset(&self, offset: usize, ctx: &SourceContext) -> Option<MappedLocation> {
        match &self.mapping {
            SourceMapping::Concat { pieces } => {
                // Find which piece contains this offset
                let piece = pieces.iter().find(|p| {
                    offset >= p.offset_in_concat &&
                    offset < p.offset_in_concat + p.length
                })?;

                // Map to piece's coordinate system
                let offset_in_piece = offset - piece.offset_in_concat;

                // Recursively map through the piece's source_info
                piece.source_info.map_offset(offset_in_piece, ctx)
            }

            SourceMapping::Substring { parent, offset: parent_offset } => {
                // Add offset and recurse to parent
                parent.map_offset(offset + parent_offset, ctx)
            }

            SourceMapping::Original { file_id } => {
                // Base case: convert offset to line/column in file
                let file = ctx.get_file(*file_id)?;
                let location = offset_to_location(file.content.as_ref()?, offset)?;
                Some(MappedLocation {
                    file_id: *file_id,
                    location,
                })
            }
        }
    }
}
```

**Example walkthrough**:
- YAML error at offset 16 (in "warning: false")
- Concat mapping finds piece: `SourcePiece { offset_in_concat: 13, length: 15, ... }`
- Offset in piece: 16 - 13 = 3
- Piece's source_info is `Substring { parent: cell_source_info, offset: 18 }`
- Recursively map: 3 + 18 = 21 in cell source
- Cell source_info maps to document
- Final result: "error in analysis.qmd at line 5, column 7" (pointing to 'w' in "#| warning: false")

## Integration with yaml-rust2 and AnnotatedParse

### AnnotatedYamlParser with SourceInfo

```rust
struct AnnotatedYamlParser {
    stack: Vec<PartialParse>,
    completed: Vec<AnnotatedParse>,
    source_info: SourceInfo,  // The SourceInfo for the YAML text
    source_text: String,
}

impl MarkedEventReceiver for AnnotatedYamlParser {
    fn on_event(&mut self, ev: Event, mark: Marker) {
        match ev {
            Event::Scalar(text, style, anchor, tag) => {
                let start = mark.index();  // Offset in YAML string
                let end = self.find_scalar_end(start, &text);

                // Create AnnotatedParse with SourceInfo that can map back
                let scalar_source_info = self.source_info.substring(start, end);

                let annotated = AnnotatedParse {
                    start,
                    end,
                    result: YamlValue::from_scalar(&text),
                    kind: YamlKind::Scalar,
                    source_info: scalar_source_info,
                    components: vec![],
                    errors: None,
                };

                self.push_completed(annotated);
            }

            Event::MappingStart(anchor, tag) => {
                let start = mark.index();
                self.stack.push(PartialParse {
                    start,
                    kind: YamlKind::Mapping,
                    value_so_far: PartialValue::Mapping {
                        pairs: vec![],
                        current_key: None,
                    },
                });
            }

            Event::MappingEnd => {
                let partial = self.stack.pop().unwrap();
                let end = mark.index();

                // ... build AnnotatedParse with source_info.substring(start, end)
            }

            // ... other events
        }
    }
}
```

### Error Reporting

```rust
// Validate YAML and report errors
fn validate_and_report(
    annotated: &AnnotatedParse,
    schema: &Schema,
    source_ctx: &SourceContext,
) -> Result<(), Vec<ValidationError>> {
    let errors = validate_yaml(annotated, schema)?;

    for error in &errors {
        // Error has offset in YAML, but source_info knows how to map back
        if let Some(mapped) = annotated.source_info.map_offset(error.start, source_ctx) {
            let file = source_ctx.get_file(mapped.file_id).unwrap();

            eprintln!(
                "Error: {} in {}:{}:{}",
                error.message,
                file.path,
                mapped.location.row,
                mapped.location.column
            );

            // Can also extract source context for pretty printing
            if let Some(content) = &file.content {
                let context = extract_source_context(
                    content,
                    mapped.location.offset,
                    3, // 3 lines of context
                );
                eprintln!("{}", context);
            }
        }
    }

    Ok(())
}
```

## Complete API Design

### Core Types

```rust
use serde::{Serialize, Deserialize};

/// Unified source location tracking
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
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourcePiece {
    pub source_info: SourceInfo,
    pub offset_in_concat: usize,
    pub length: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: Location,
    pub end: Location,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    pub offset: usize,
    pub row: usize,
    pub column: usize,
}

/// Context managing all source files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceContext {
    files: Vec<SourceFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// Result of mapping back to original source
#[derive(Debug, Clone)]
pub struct MappedLocation {
    pub file_id: FileId,
    pub location: Location,
}
```

### Construction API

```rust
impl SourceInfo {
    /// Create for original file position
    pub fn original(file_id: FileId, range: Range) -> Self {
        SourceInfo {
            range,
            mapping: SourceMapping::Original { file_id },
        }
    }

    /// Create for substring extraction
    pub fn substring(&self, start: usize, end: usize) -> Self {
        SourceInfo {
            range: Range {
                start: Location { offset: 0, row: 0, column: 0 },
                end: Location { offset: end - start, row: 0, column: 0 },
            },
            mapping: SourceMapping::Substring {
                parent: Box::new(self.clone()),
                offset: start,
            },
        }
    }

    /// Create for concatenated pieces
    pub fn concat(pieces: Vec<SourcePiece>) -> Self {
        let total_length: usize = pieces.iter().map(|p| p.length).sum();
        SourceInfo {
            range: Range {
                start: Location { offset: 0, row: 0, column: 0 },
                end: Location { offset: total_length, row: 0, column: 0 },
            },
            mapping: SourceMapping::Concat { pieces },
        }
    }

    /// Map offset back to original source location
    pub fn map_offset(&self, offset: usize, ctx: &SourceContext) -> Option<MappedLocation> {
        // Implementation as shown above
    }

    /// Map range back to original source locations
    pub fn map_range(&self, range: Range, ctx: &SourceContext)
        -> Option<(MappedLocation, MappedLocation)>
    {
        let start = self.map_offset(range.start.offset, ctx)?;
        let end = self.map_offset(range.end.offset, ctx)?;
        Some((start, end))
    }
}
```

### AnnotatedParse Integration

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotatedParse {
    pub start: usize,
    pub end: usize,
    pub result: YamlValue,
    pub kind: YamlKind,
    pub source_info: SourceInfo,  // Now handles all three scenarios!
    pub components: Vec<AnnotatedParse>,
    pub errors: Option<Vec<YamlError>>,
}

/// Parse YAML with source tracking
pub fn parse_yaml_annotated(
    yaml_text: &str,
    source_info: SourceInfo,
) -> Result<AnnotatedParse, Vec<YamlError>> {
    let mut parser_handler = AnnotatedYamlParser {
        stack: vec![],
        completed: vec![],
        source_info,
        source_text: yaml_text.to_string(),
    };

    let mut parser = Parser::new_from_str(yaml_text);

    match parser.load(&mut parser_handler, false) {
        Ok(_) => {
            if let Some(root) = parser_handler.completed.into_iter().next() {
                Ok(root)
            } else {
                Err(vec![YamlError {
                    start: 0,
                    end: 0,
                    message: "Empty YAML document".to_string(),
                }])
            }
        }
        Err(scan_error) => {
            Err(vec![YamlError {
                start: scan_error.marker().index(),
                end: scan_error.marker().index() + 1,
                message: scan_error.info().to_string(),
            }])
        }
    }
}
```

## Naming Considerations

The user asked about renaming MappedString. Given the unified design:

**Current name**: MappedString
**Proposed alternatives**:
1. **SourceInfo** (already used in quarto-markdown) - ✅ **RECOMMENDED**
2. TrackedString - emphasizes tracking
3. LocatedString - emphasizes location
4. SourceSpan - common in compiler literature
5. SourceRegion - emphasizes region tracking

**Recommendation**: Keep **SourceInfo** as the unified name. It's:
- Already established in quarto-markdown
- Clear and concise
- Standard in parsing/compiler literature
- Encompasses more than just strings (spans, regions, transformations)

The wrapper around SourceInfo for YAML specifically could be:
```rust
/// YAML text with source location tracking
pub struct YamlSource {
    pub text: String,
    pub source_info: SourceInfo,
}
```

But this is optional - we can just pass `&str` and `SourceInfo` separately.

## Implementation Plan

### Phase 1: SourceInfo Foundation (Week 1-2)
- [ ] Implement SourceInfo with Original, Substring, Concat variants
- [ ] Implement SourceContext and FileId system
- [ ] Implement map_offset() with full recursion
- [ ] Unit tests for all three scenarios
- [ ] Add serde derives and test serialization

**Deliverable**: Working SourceInfo that handles all three YAML scenarios

### Phase 2: AnnotatedParse with SourceInfo (Week 3-4)
- [ ] Implement AnnotatedYamlParser using MarkedEventReceiver
- [ ] Handle Scalar, Mapping, Sequence events
- [ ] Create SourceInfo for each AnnotatedParse node
- [ ] Test with simple YAML
- [ ] Test with scenario 2 (metadata block)

**Deliverable**: AnnotatedParse working for scenarios 1 and 2

### Phase 3: Cell Options Support (Week 5-6)
- [ ] Implement extract_cell_options_yaml()
- [ ] Build Concat SourceInfo correctly
- [ ] Handle different comment prefixes (#|, %%|, etc.)
- [ ] Test offset mapping for cell options
- [ ] Integration test with real Quarto documents

**Deliverable**: Full support for scenario 3 (cell options)

### Phase 4: Error Reporting Integration (Week 7)
- [ ] Implement ValidationError with SourceInfo
- [ ] Convert mapped locations to pretty error messages
- [ ] Extract source context for error display
- [ ] Test error messages point to correct locations
- [ ] Test with ariadne for visual errors

**Deliverable**: Beautiful error messages for all YAML scenarios

### Phase 5: quarto-markdown Integration (Week 8)
- [ ] Replace existing SourceInfo in quarto-markdown with new design
- [ ] Update AST construction to use new SourceInfo
- [ ] Update YAML extraction in quarto-markdown
- [ ] Update all AST consumers (LSP, validation)
- [ ] Integration tests with full rendering pipeline

**Deliverable**: Unified SourceInfo throughout Kyoto

### Phase 6: Optimization and Caching (Week 9-10)
- [ ] Benchmark SourceInfo size and performance
- [ ] Optimize map_offset() for deep chains
- [ ] Implement caching for frequently mapped positions
- [ ] Test serialization/deserialization performance
- [ ] Implement cache invalidation logic

**Deliverable**: Production-ready performance

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_scenario_1_standalone_yaml() {
    let yaml = "title: foo\nformat: html";
    let ctx = SourceContext::new();
    let file_id = ctx.add_file("_quarto.yml".into(), Some(yaml.into()));

    let source_info = SourceInfo::original(file_id, Range::from_text(yaml));
    let annotated = parse_yaml_annotated(yaml, source_info).unwrap();

    // Map offset 7 ('f' in 'foo') back to source
    let mapped = annotated.source_info.map_offset(7, &ctx).unwrap();
    assert_eq!(mapped.location.offset, 7);
    assert_eq!(mapped.location.row, 1);
    assert_eq!(mapped.location.column, 7);
}

#[test]
fn test_scenario_2_metadata_block() {
    let qmd = "---\ntitle: bar\n---\n\n# Content";
    let ctx = SourceContext::new();
    let file_id = ctx.add_file("doc.qmd".into(), Some(qmd.into()));

    // Extract YAML (skip "---\n", take until next "---")
    let yaml = "title: bar\n";
    let yaml_source_info = SourceInfo::original(file_id, Range::from_text(qmd))
        .substring(4, 15);  // Offsets of YAML content in qmd

    let annotated = parse_yaml_annotated(yaml, yaml_source_info).unwrap();

    // Map offset 7 ('b' in 'bar') back to source
    let mapped = annotated.source_info.map_offset(7, &ctx).unwrap();
    assert_eq!(mapped.location.offset, 11);  // 4 (skip ---\n) + 7
    assert_eq!(mapped.location.row, 2);
    assert_eq!(mapped.location.column, 7);
}

#[test]
fn test_scenario_3_cell_options() {
    let cell = "#| echo: false\n#| fig-width: 8\nprint('hi')";
    let ctx = SourceContext::new();
    let file_id = ctx.add_file("analysis.qmd".into(), Some(cell.into()));

    // Extract cell options
    let (yaml, yaml_source_info) = extract_cell_options_yaml(
        cell,
        SourceInfo::original(file_id, Range::from_text(cell)),
    );

    assert_eq!(yaml, "echo: false\nfig-width: 8\n");

    let annotated = parse_yaml_annotated(&yaml, yaml_source_info).unwrap();

    // Map offset 6 ('f' in 'false') back to source
    let mapped = annotated.source_info.map_offset(6, &ctx).unwrap();
    assert_eq!(mapped.location.offset, 9);  // "#| echo: " = 9 chars
    assert_eq!(mapped.location.row, 1);
    assert_eq!(mapped.location.column, 9);

    // Map offset 12 (start of 'fig-width') back to source
    let mapped = annotated.source_info.map_offset(12, &ctx).unwrap();
    assert_eq!(mapped.location.offset, 18);  // Start of line 2 + "#| " = 15 + 3
    assert_eq!(mapped.location.row, 2);
    assert_eq!(mapped.location.column, 3);
}
```

### Integration Tests

```rust
#[test]
fn test_error_reporting_cell_options() {
    let qmd = r#"
```{r}
#| echo: invalid_value
#| output: true
print("test")
```
"#;

    let ctx = SourceContext::new();
    let file_id = ctx.add_file("test.qmd".into(), Some(qmd.into()));

    // Parse document
    let (pandoc, _) = qmd::read(qmd.as_bytes(), ...)?;

    // Find code block
    let code_block = find_code_block(&pandoc)?;

    // Extract and parse cell options
    let (yaml, yaml_source_info) = extract_cell_options_yaml(
        &code_block.text,
        code_block.source_info,
    );

    let annotated = parse_yaml_annotated(&yaml, yaml_source_info)?;

    // Validate (echo must be boolean)
    let errors = validate_yaml(&annotated, &schema, &ctx)?;

    assert_eq!(errors.len(), 1);
    assert!(errors[0].message.contains("echo"));

    // Check error points to correct location
    let mapped = annotated.source_info.map_offset(errors[0].start, &ctx).unwrap();
    let file = ctx.get_file(mapped.file_id).unwrap();

    // Should point to "invalid_value" in "#| echo: invalid_value"
    let error_text = &file.content.as_ref().unwrap()
        [mapped.location.offset..mapped.location.offset + 13];
    assert_eq!(error_text, "invalid_value");
}
```

## Advantages of This Design

### 1. **Unified System** ✅
- Single SourceInfo type handles all scenarios
- No separate MappedString vs SourceInfo confusion
- Consistent API across codebase

### 2. **Serializable** ✅
- No closures, only data structures
- Can cache to disk for LSP performance
- Can send across thread boundaries

### 3. **Precise Error Reporting** ✅
- Scenario 1: Direct mapping
- Scenario 2: Automatic offset adjustment
- Scenario 3: Complex multi-piece mapping
- All work transparently through same API

### 4. **Composable** ✅
- Can nest: Original → Substring → Concat → Substring
- Each transformation is explicit and trackable
- Easy to debug mapping chains

### 5. **Efficient** ✅
- SourcePiece is small (~40 bytes)
- Box prevents exponential growth
- map_offset() is O(pieces) for Concat, typically <10 pieces

### 6. **Type Safe** ✅
- Rust compiler enforces correctness
- No runtime type errors
- Pattern matching ensures all cases handled

## Open Questions and Decisions

### Q1: Should newlines be separate pieces in Concat?

**Answer**: Yes, for accuracy.

**Rationale**:
- Newlines in source are part of the original text
- YAML errors can occur at newlines (e.g., indentation errors)
- Separate pieces allow precise mapping

**Implementation**: Each line contributes two pieces: content + newline

### Q2: How to handle end positions from yaml-rust2?

**Answer**: yaml-rust2 only provides start Markers. We need to compute end positions.

**Strategy**:
1. For scalars: Scan forward in source text to find end
2. For containers: Use the end event's Marker
3. Cache positions during parsing

**Implementation**:
```rust
fn find_scalar_end(&self, start: usize, scalar_value: &str, style: TScalarStyle) -> usize {
    match style {
        TScalarStyle::Plain => self.find_plain_scalar_end(start),
        TScalarStyle::SingleQuoted => self.find_quoted_scalar_end(start, '\''),
        TScalarStyle::DoubleQuoted => self.find_quoted_scalar_end(start, '"'),
        TScalarStyle::Literal | TScalarStyle::Folded => self.find_block_scalar_end(start),
    }
}
```

### Q3: What about language-specific comment prefixes?

**Answer**: Support all Quarto languages with a configurable prefix map.

**Implementation**:
```rust
const CELL_OPTION_PREFIXES: &[&str] = &["#|", "%%|", "//|", "--|"];

fn detect_cell_option_prefix(line: &str) -> Option<&str> {
    for prefix in CELL_OPTION_PREFIXES {
        if line.trim_start().starts_with(prefix) {
            return Some(prefix);
        }
    }
    None
}
```

### Q4: Should we support line-specific source info in Concat?

**Answer**: Yes, through the piece mechanism.

Each piece can have its own SourceInfo chain, so we automatically support:
- Different files (includes)
- Different transformations per line
- Mixed sources

## Comparison with TypeScript Implementation

| Aspect | TypeScript | Rust (Proposed) |
|--------|-----------|-----------------|
| **Core abstraction** | MappedString (closure-based) | SourceInfo (enum-based) |
| **Serializable** | ❌ No (closures) | ✅ Yes (data only) |
| **Multi-file** | ⚠️ Limited (filename string) | ✅ Yes (FileId system) |
| **Concat support** | ✅ Yes | ✅ Yes (more explicit) |
| **Cell options** | ✅ Yes (with mappedConcat) | ✅ Yes (with Concat variant) |
| **Debuggability** | ❌ Hard (opaque closures) | ✅ Easy (inspect enum) |
| **Performance** | ⚠️ Slower (recursive calls) | ✅ Faster (data access) |
| **Type safety** | ⚠️ Runtime checks | ✅ Compile-time checks |
| **Code size** | ~450 LOC | ~600 LOC (more explicit) |

## Success Criteria

✅ **Functionality**:
- [ ] All three YAML scenarios work correctly
- [ ] Error offsets map back to original source accurately
- [ ] Supports all Quarto languages (R, Python, Julia)
- [ ] Handles edge cases (multiline strings, comments, etc.)

✅ **Performance**:
- [ ] Parsing overhead <5% compared to no tracking
- [ ] map_offset() completes in <1μs for typical depths
- [ ] Serialization adds <10% to cache size

✅ **Quality**:
- [ ] 100% of unit tests pass
- [ ] Integration tests with real Quarto documents pass
- [ ] Error messages are accurate and helpful
- [ ] Code is well-documented

✅ **Integration**:
- [ ] Works with quarto-markdown SourceInfo
- [ ] Works with YAML validation
- [ ] Works with LSP error reporting
- [ ] Works with ariadne for visual errors

## Conclusion

The unified SourceInfo design with explicit Concat strategy provides a robust, serializable, and precise solution for location tracking in YAML parsing across all three scenarios in Quarto:

1. **Standalone files**: Direct Original mapping
2. **Metadata blocks**: Substring mapping
3. **Cell options**: Concat mapping with per-piece tracking

This design is:
- ✅ Serializable (no closures)
- ✅ Type-safe (Rust enums)
- ✅ Precise (handles non-contiguous text)
- ✅ Composable (can nest transformations)
- ✅ Efficient (small overhead)
- ✅ Debuggable (explicit data structures)

**Recommendation**: Proceed with implementation in the order specified in the Implementation Plan, starting with the SourceInfo foundation and building up to full cell options support.

## Next Steps

1. Review this design with stakeholders
2. Create prototype SourceInfo implementation
3. Write comprehensive unit tests
4. Integrate with yaml-rust2 for AnnotatedParse
5. Test with real Quarto documents
6. Optimize and benchmark
7. Document API and usage patterns
8. Migrate existing code to use new system
