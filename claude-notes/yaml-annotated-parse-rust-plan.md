# Building AnnotatedParse in Rust with yaml-rust2

## Summary

**Conclusion**: ✅ **Yes, it is practical and feasible** to build an AnnotatedParse equivalent in Rust using yaml-rust2's `MarkedEventReceiver` API. The library provides position tracking (`Marker`) for all YAML events, which is exactly what we need.

## Background

### What is AnnotatedParse?

From TypeScript codebase:

```typescript
export interface AnnotatedParse {
  start: number;          // Start offset in source
  end: number;            // End offset in source
  result: JSONValue;      // Parsed value
  kind: string;           // YAML node type
  source: MappedString;   // Source with position mapping
  components: AnnotatedParse[];  // Nested structure
  errors?: Array<{...}>;  // Optional parse errors
}
```

**Purpose**: Preserve source positions for every YAML node to enable:
- Precise error reporting (maps validation errors back to source)
- IDE features (completions, hover, navigation in YAML)
- Schema validation with position-aware diagnostics

### Current TypeScript Approach

Uses **dual parsing**:
1. **tree-sitter-yaml** (lenient mode): Error recovery for IDE
2. **js-yaml** (strict mode): Compliant parsing for validation

Both parsers provide position information that builds AnnotatedParse trees.

## yaml-rust2 Capabilities

### Library: yaml-rust2

**Crate**: `yaml-rust2 = "0.10.4"`
**Repository**: https://github.com/Ethiraric/yaml-rust2
**Description**: Fully YAML 1.2 compliant parser

**Already used** in quarto-markdown-pandoc for frontmatter parsing.

### MarkedEventReceiver API

yaml-rust2 provides an **event-based parsing API** with position tracking:

```rust
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::Marker;

pub trait MarkedEventReceiver {
    fn on_event(&mut self, ev: Event, mark: Marker);
}

// Marker provides position information
pub struct Marker {
    index: usize,  // Byte offset
    line: usize,   // Line (1-indexed)
    col: usize,    // Column (1-indexed)
}

// Events emitted during parsing
pub enum Event {
    StreamStart,
    StreamEnd,
    DocumentStart,
    DocumentEnd,
    Scalar(String, TScalarStyle, anchor_id, Option<Tag>),
    SequenceStart(anchor_id, Option<Tag>),
    SequenceEnd,
    MappingStart(anchor_id, Option<Tag>),
    MappingEnd,
    Alias(anchor_id),
}
```

### Event Stream Example

For YAML:
```yaml
title: "My Document"
tags:
  - rust
  - yaml
```

Events emitted:
```
StreamStart
DocumentStart
MappingStart                    (mark: offset=0, line=1, col=1)
  Scalar("title", ...)          (mark: offset=0)
  Scalar("My Document", ...)    (mark: offset=7)
  Scalar("tags", ...)           (mark: offset=23)
  SequenceStart                 (mark: offset=28)
    Scalar("rust", ...)         (mark: offset=32)
    Scalar("yaml", ...)         (mark: offset=39)
  SequenceEnd
MappingEnd
DocumentEnd
StreamEnd
```

**Key insight**: We get `Marker` for **every event** including start/end of mappings and sequences!

## Proposed Rust Design

### AnnotatedParse Type

```rust
use serde::{Serialize, Deserialize};
use crate::unified_source_location::SourceInfo;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotatedParse {
    /// Start offset in immediate source
    pub start: usize,

    /// End offset in immediate source
    pub end: usize,

    /// Parsed YAML value
    pub result: YamlValue,

    /// YAML node type (scalar, sequence, mapping)
    pub kind: YamlKind,

    /// Source location (supports transformation chains via SourceInfo)
    pub source_info: SourceInfo,

    /// Nested components
    pub components: Vec<AnnotatedParse>,

    /// Parse errors (only at top level)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<YamlError>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum YamlValue {
    Null,
    Bool(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Array(Vec<YamlValue>),
    Object(indexmap::IndexMap<String, YamlValue>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum YamlKind {
    Scalar,
    Sequence,
    Mapping,
    Document,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct YamlError {
    pub start: usize,
    pub end: usize,
    pub message: String,
}
```

### Parser Implementation

```rust
use yaml_rust2::parser::{Event, MarkedEventReceiver, Parser};
use yaml_rust2::scanner::Marker;

struct AnnotatedYamlParser {
    /// Stack of in-progress AnnotatedParse nodes
    stack: Vec<PartialParse>,

    /// Completed nodes (used during construction)
    completed: Vec<AnnotatedParse>,

    /// Source info for the entire YAML text
    source_info: SourceInfo,

    /// Original YAML text (for extracting substrings)
    source_text: String,
}

/// A partially-constructed AnnotatedParse (while parsing)
struct PartialParse {
    start: usize,
    kind: YamlKind,
    value_so_far: PartialValue,
}

enum PartialValue {
    Scalar { text: String },
    Sequence { items: Vec<AnnotatedParse> },
    Mapping { pairs: Vec<(AnnotatedParse, AnnotatedParse)>, current_key: Option<AnnotatedParse> },
}

impl MarkedEventReceiver for AnnotatedYamlParser {
    fn on_event(&mut self, event: Event, mark: Marker) {
        match event {
            Event::StreamStart | Event::DocumentStart => {
                // Initialize top-level document
            }

            Event::MappingStart(_anchor, _tag) => {
                self.stack.push(PartialParse {
                    start: mark.index(),
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

                if let PartialValue::Mapping { pairs, .. } = partial.value_so_far {
                    let object: IndexMap<String, YamlValue> = pairs.into_iter().map(|(key, value)| {
                        // Extract key string from key AnnotatedParse
                        let key_str = self.extract_key_string(&key);
                        (key_str, value.result)
                    }).collect();

                    let annotated = AnnotatedParse {
                        start: partial.start,
                        end,
                        result: YamlValue::Object(object),
                        kind: YamlKind::Mapping,
                        source_info: self.source_info.substring(partial.start, end),
                        components: pairs.into_iter()
                            .flat_map(|(k, v)| vec![k, v])
                            .collect(),
                        errors: None,
                    };

                    self.push_completed(annotated);
                }
            }

            Event::SequenceStart(_anchor, _tag) => {
                self.stack.push(PartialParse {
                    start: mark.index(),
                    kind: YamlKind::Sequence,
                    value_so_far: PartialValue::Sequence { items: vec![] },
                });
            }

            Event::SequenceEnd => {
                let partial = self.stack.pop().unwrap();
                let end = mark.index();

                if let PartialValue::Sequence { items } = partial.value_so_far {
                    let array: Vec<YamlValue> = items.iter()
                        .map(|item| item.result.clone())
                        .collect();

                    let annotated = AnnotatedParse {
                        start: partial.start,
                        end,
                        result: YamlValue::Array(array),
                        kind: YamlKind::Sequence,
                        source_info: self.source_info.substring(partial.start, end),
                        components: items,
                        errors: None,
                    };

                    self.push_completed(annotated);
                }
            }

            Event::Scalar(text, _style, _anchor, _tag) => {
                let start = mark.index();
                // Find end by scanning forward in source text
                let end = self.find_scalar_end(start, &text);

                let value = parse_scalar_value(&text);

                let annotated = AnnotatedParse {
                    start,
                    end,
                    result: value,
                    kind: YamlKind::Scalar,
                    source_info: self.source_info.substring(start, end),
                    components: vec![],
                    errors: None,
                };

                self.push_completed(annotated);
            }

            Event::DocumentEnd | Event::StreamEnd => {
                // Finalize
            }

            Event::Alias(_anchor) => {
                // Handle YAML aliases
            }

            Event::Nothing => {}
        }
    }
}

impl AnnotatedYamlParser {
    fn push_completed(&mut self, annotated: AnnotatedParse) {
        // If we're inside a mapping or sequence, add to current context
        if let Some(partial) = self.stack.last_mut() {
            match &mut partial.value_so_far {
                PartialValue::Sequence { items } => {
                    items.push(annotated);
                }
                PartialValue::Mapping { pairs, current_key } => {
                    if let Some(key) = current_key.take() {
                        // This is a value, pair it with the key
                        pairs.push((key, annotated));
                    } else {
                        // This is a key, store it
                        *current_key = Some(annotated);
                    }
                }
                _ => {}
            }
        } else {
            // Top level
            self.completed.push(annotated);
        }
    }

    fn find_scalar_end(&self, start: usize, scalar_text: &str) -> usize {
        // yaml-rust2 gives us the parsed value but not the exact end position
        // We need to scan forward in the source to find where the scalar ends

        // Strategy 1: Simple case - literal match
        let expected_len = scalar_text.len();
        if self.source_text[start..].starts_with(scalar_text) {
            return start + expected_len;
        }

        // Strategy 2: Quoted string - scan for closing quote
        if self.source_text[start..].starts_with('"') || self.source_text[start..].starts_with('\'') {
            // Find matching quote
            return self.find_closing_quote(start);
        }

        // Strategy 3: Plain scalar - scan until whitespace or special char
        return self.find_plain_scalar_end(start);
    }

    fn extract_key_string(&self, key_parse: &AnnotatedParse) -> String {
        match &key_parse.result {
            YamlValue::String(s) => s.clone(),
            YamlValue::Integer(i) => i.to_string(),
            YamlValue::Float(f) => f.to_string(),
            YamlValue::Bool(b) => b.to_string(),
            _ => {
                // Complex key (unusual but valid YAML)
                self.source_text[key_parse.start..key_parse.end].to_string()
            }
        }
    }
}

fn parse_scalar_value(text: &str) -> YamlValue {
    // Parse scalar to appropriate type
    if text == "null" || text.is_empty() {
        YamlValue::Null
    } else if text == "true" {
        YamlValue::Bool(true)
    } else if text == "false" {
        YamlValue::Bool(false)
    } else if let Ok(i) = text.parse::<i64>() {
        YamlValue::Integer(i)
    } else if let Ok(f) = text.parse::<f64>() {
        YamlValue::Float(f)
    } else {
        YamlValue::String(text.to_string())
    }
}
```

### Public API

```rust
/// Parse YAML string into AnnotatedParse tree
pub fn parse_yaml_annotated(
    yaml_text: &str,
    source_info: SourceInfo,
) -> Result<AnnotatedParse, Vec<YamlError>> {
    let mut parser_handler = AnnotatedYamlParser {
        stack: vec![],
        completed: vec![],
        source_info: source_info.clone(),
        source_text: yaml_text.to_string(),
    };

    let mut parser = Parser::new_from_str(yaml_text);

    match parser.load(&mut parser_handler, false) {
        Ok(_) => {
            // Return the root document
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

/// Convenience: parse from MappedString (for compatibility)
pub fn parse_yaml_from_mapped_string(
    mapped: &MappedString,
) -> Result<AnnotatedParse, Vec<YamlError>> {
    parse_yaml_annotated(&mapped.value, mapped.source_info.clone())
}
```

## Integration with Existing Code

### quarto-markdown-pandoc

Current code already uses yaml-rust2 for frontmatter:

```rust
// In meta.rs (current)
impl MarkedEventReceiver for YamlEventHandler {
    fn on_event(&mut self, ev: Event, _mark: Marker) {
        // Builds Meta (Pandoc metadata) without position tracking
    }
}
```

**Enhancement**: Extend to build AnnotatedParse instead:

```rust
// Enhanced version
impl MarkedEventReceiver for AnnotatedYamlHandler {
    fn on_event(&mut self, ev: Event, mark: Marker) {
        // Build AnnotatedParse WITH position tracking
    }
}
```

### YAML Validation

```rust
use crate::yaml_annotated::parse_yaml_annotated;
use crate::yaml_validation::validate_yaml;

// Extract YAML from frontmatter
let yaml_source_info = SourceInfo::substring(
    frontmatter.source_info,
    0,
    frontmatter.text.len()
);

// Parse with positions
let annotated = parse_yaml_annotated(&frontmatter.text, yaml_source_info)?;

// Validate against schema
let errors = validate_yaml(&annotated, &schema, &source_context);

// Errors have precise source positions
for error in errors {
    let mapped_loc = error.source_info.map_offset(error.start, &source_context)?;
    eprintln!("Error at {}:{}:{}: {}",
        source_context.get_file(mapped_loc.file_id)?.path,
        mapped_loc.location.row,
        mapped_loc.location.column,
        error.message
    );
}
```

## Challenges and Solutions

### Challenge 1: Exact End Positions

**Problem**: yaml-rust2's `Marker` only points to the **start** of events. We need to determine end positions.

**Solution**: Track end markers by:
1. For containers (mappings, sequences): End event provides end marker
2. For scalars: Scan forward in source text to find end
3. Cache scalar positions during parsing

**Implementation**:
```rust
fn find_scalar_end(&self, start: usize, parsed_value: &str) -> usize {
    // Check for quoted strings
    if let Some(end) = self.find_quoted_scalar_end(start) {
        return end;
    }

    // Plain scalar: scan until delimiter
    self.find_plain_scalar_end(start)
}
```

### Challenge 2: Whitespace and Comments

**Problem**: YAML allows arbitrary whitespace and comments between elements.

**Solution**: Use source text directly for accurate ranges:
```rust
// Don't rely solely on Marker positions
// Cross-reference with actual source text
let actual_text = &self.source_text[start..end];
let trimmed = actual_text.trim();
// Adjust start/end to exclude leading/trailing whitespace
```

### Challenge 3: Multiline Strings

**Problem**: Literal (`|`) and folded (`>`) blocks span multiple lines.

**Solution**: yaml-rust2 gives us the **parsed** value. Extract raw source range:
```rust
Event::Scalar(parsed_text, TScalarStyle::Literal | TScalarStyle::Folded, ..) => {
    // Find the block scalar indicator in source
    let block_start = mark.index();
    // Scan forward to find end of block (based on indentation)
    let block_end = self.find_block_scalar_end(block_start);
}
```

### Challenge 4: Complex Keys

**Problem**: YAML allows complex keys (mappings as keys).

**Solution**: yaml-rust2 emits events for complex keys just like values:
```rust
// Events for: { [a, b]: c }
MappingStart
  SequenceStart    // Key is a sequence
    Scalar("a")
    Scalar("b")
  SequenceEnd
  Scalar("c")      // Value
MappingEnd

// Our parser handles this by treating first event after MappingStart as key
```

## Comparison with TypeScript

| Aspect | TypeScript | Rust (Proposed) |
|--------|-----------|-----------------|
| **Parser** | tree-sitter-yaml + js-yaml | yaml-rust2 |
| **Position tracking** | Both parsers provide positions | MarkedEventReceiver provides Marker |
| **Dual parsing** | Yes (lenient + strict) | Single parser (strict) |
| **Error recovery** | tree-sitter | Limited (would need tree-sitter-yaml) |
| **Performance** | Slower (JS) | Faster (Rust, compiled) |
| **Serializable** | No (MappedString has closures) | Yes (all data structures) |
| **Code size** | ~2,500 LOC | Est. ~1,500 LOC (simpler) |

## Future: Error Recovery (Optional)

For **lenient mode** (IDE editing with incomplete YAML), we can add tree-sitter-yaml:

```rust
// Optional dependency
tree-sitter-yaml = { version = "0.7", optional = true }

pub fn parse_yaml_lenient(
    yaml_text: &str,
    source_info: SourceInfo,
) -> AnnotatedParse {
    // Use tree-sitter-yaml for error recovery
    // Build AnnotatedParse from tree-sitter AST
    // Falls back to partial/invalid structures on errors
}
```

**Decision**: **Defer** lenient mode to Phase 2. Start with strict parsing (yaml-rust2 only).

## Implementation Plan

### Phase 1: Core AnnotatedParse (Week 1-2)

- [ ] Define AnnotatedParse, YamlValue, YamlKind types
- [ ] Implement AnnotatedYamlParser with MarkedEventReceiver
- [ ] Handle basic events: Scalar, Mapping, Sequence
- [ ] Implement position tracking for scalars
- [ ] Unit tests with simple YAML examples

**Deliverable**: Can parse simple YAML to AnnotatedParse

### Phase 2: Complex Cases (Week 2-3)

- [ ] Handle multiline strings (literal, folded)
- [ ] Handle anchors and aliases
- [ ] Handle complex keys
- [ ] Handle quoted strings with escapes
- [ ] Robust end position detection
- [ ] Test with real Quarto frontmatter

**Deliverable**: Can parse all common Quarto YAML patterns

### Phase 3: Error Handling (Week 3-4)

- [ ] Convert yaml-rust2 ScanError to YamlError with positions
- [ ] Preserve error positions in AnnotatedParse
- [ ] Pretty error formatting with source context
- [ ] Test with invalid YAML

**Deliverable**: Good error messages with source positions

### Phase 4: Integration (Week 4-5)

- [ ] Integrate with unified SourceInfo
- [ ] Update YAML validation to use AnnotatedParse
- [ ] Replace current Meta parsing in quarto-markdown-pandoc
- [ ] Test with quarto-cli YAML schemas
- [ ] Integration tests with full documents

**Deliverable**: YAML validation uses AnnotatedParse

### Phase 5: Optimization (Week 5-6)

- [ ] Benchmark parsing performance
- [ ] Optimize position calculations
- [ ] Memory profiling (reduce allocations)
- [ ] Cache frequently-parsed patterns
- [ ] Parallel parsing for multiple YAML blocks?

**Deliverable**: Performance meets targets (<50ms for typical YAML)

### Phase 6: Optional Lenient Mode (Week 6-7, if needed)

- [ ] Add tree-sitter-yaml dependency
- [ ] Implement lenient parser
- [ ] Error recovery for incomplete YAML
- [ ] Test in LSP with partial edits

**Deliverable**: IDE works with incomplete YAML

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_simple_mapping() {
    let yaml = "title: Hello\nauthor: World";
    let source_info = SourceInfo::original(file_id, Range::from_text(yaml));

    let annotated = parse_yaml_annotated(yaml, source_info).unwrap();

    assert_eq!(annotated.kind, YamlKind::Mapping);
    assert_eq!(annotated.components.len(), 4); // 2 keys + 2 values

    // Check positions
    assert_eq!(annotated.start, 0);
    assert!(annotated.end > 0);

    // Check nested structure
    let title_key = &annotated.components[0];
    assert_eq!(title_key.result, YamlValue::String("title".into()));
}

#[test]
fn test_sequence() {
    let yaml = "tags:\n  - rust\n  - yaml";
    // ... similar tests
}

#[test]
fn test_multiline_string() {
    let yaml = "description: |\n  Line 1\n  Line 2";
    // ... test block scalars
}
```

### Integration Tests

```rust
#[test]
fn test_quarto_frontmatter() {
    let qmd = r#"
---
title: "My Document"
format:
  html:
    toc: true
---

# Content
"#;

    // Parse document
    let (pandoc, ctx) = qmd::read(qmd.as_bytes(), ...)?;

    // Extract frontmatter
    let frontmatter = find_frontmatter(&pandoc)?;

    // Parse YAML
    let annotated = parse_yaml_annotated(&frontmatter.text, ...)?;

    // Validate that positions are correct
    assert!(annotated.components.iter().all(|c| {
        c.start < c.end && c.end <= frontmatter.text.len()
    }));
}
```

### Error Tests

```rust
#[test]
fn test_invalid_yaml_error_position() {
    let yaml = "title: foo\n  bad:indentation";

    let result = parse_yaml_annotated(yaml, source_info);

    assert!(result.is_err());
    let errors = result.unwrap_err();
    assert_eq!(errors.len(), 1);

    // Check error is at correct position
    let error = &errors[0];
    assert!(error.start >= "title: foo\n".len());
    assert!(error.message.contains("indentation"));
}
```

## Success Criteria

✅ **Functionality**:
- Parse all valid YAML to AnnotatedParse
- Preserve exact source positions for all nodes
- Support all YAML features used in Quarto
- Error reporting with correct positions

✅ **Performance**:
- Parse typical frontmatter (<50 lines) in <10ms
- Parse large YAML (>500 lines) in <100ms
- Memory usage proportional to YAML size

✅ **Quality**:
- All positions accurate (verified by tests)
- No regressions from TypeScript implementation
- Serializable (can cache to disk)

✅ **Integration**:
- Works with unified SourceInfo
- Integrates with YAML validation
- Used by LSP for completions/hover
- Used by CLI for error reporting

## Conclusion

**Recommendation**: ✅ **Proceed with yaml-rust2-based AnnotatedParse implementation**

**Rationale**:
1. yaml-rust2 provides exactly what we need (MarkedEventReceiver with position tracking)
2. Already integrated into quarto-markdown-pandoc
3. Event-based API is well-suited for building AnnotatedParse trees
4. Performance will be better than TypeScript
5. Fully serializable (enables disk caching)
6. Can add lenient mode later if needed (tree-sitter-yaml)

**Estimated effort**: 5-7 weeks for full implementation
**Risk**: Low (yaml-rust2 is mature, API is straightforward)
**Benefit**: High (enables YAML validation and IDE features in Rust)

## Next Steps

1. **Immediate**: Create prototype AnnotatedYamlParser
2. **Week 1**: Implement basic cases (scalar, mapping, sequence)
3. **Week 2**: Handle complex cases (multiline, anchors, etc.)
4. **Week 3**: Error handling and testing
5. **Week 4**: Integration with SourceInfo and validation
6. **Week 5**: Performance optimization
