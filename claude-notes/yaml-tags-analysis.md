# YAML Tags Analysis: Preserving Tag Information in Rust Port

## Executive Summary

YAML tags (like `!expr`) are a critical feature of Quarto's configuration system, allowing values to be marked for special processing (e.g., R expressions to be evaluated at runtime). The current TypeScript implementation preserves tag information by transforming tagged values into objects with `{ value: "...", tag: "!expr" }` structure.

**Good news**: yaml-rust2 provides full support for YAML tags through its Event API, including tag information for scalars, sequences, and mappings. We can integrate this seamlessly with our AnnotatedParse design to preserve tag information throughout the configuration pipeline.

**Recommendation**: Extend AnnotatedParse to include optional tag information, and handle tagged values as special objects (similar to TypeScript) to maintain compatibility with existing validation and processing logic.

## Background

### What are YAML Tags?

YAML tags are type annotations that can be applied to values:

```yaml
# Standard YAML tags
number: !!int 42
text: !!str "hello"

# Custom Quarto tags
fig-cap: !expr paste("Air", "Quality")
eval: !expr knitr::is_html_output()
```

Tags tell the YAML processor how to interpret or process the value. In Quarto:
- `!expr` marks R expressions that should be evaluated at runtime
- Future tags could support other languages or transformations

### Current TypeScript Implementation

#### Custom Schema Definition (js-yaml-schema.ts)

```typescript
export const QuartoJSONSchema = new Schema({
  implicit: [_null, bool, int, float],
  include: [failsafe],
  explicit: [
    new Type("!expr", {
      kind: "scalar",
      construct(data: any): Record<string, unknown> {
        const result: string = data !== null ? data : "";
        return {
          value: result,
          tag: "!expr",
        };
      },
    }),
  ],
});
```

**Key behavior**: Values with `!expr` tag are transformed into objects:
```javascript
// Input YAML:
fig-cap: !expr paste("Air", "Quality")

// Parsed result:
{
  "fig-cap": {
    value: "paste(\"Air\", \"Quality\")",
    tag: "!expr"
  }
}
```

#### Tree-Sitter Parsing (annotated-yaml.ts:351-362)

Tree-sitter YAML parser also recognizes tags:

```typescript
const annotateTag = (
  innerParse: AnnotatedParse,
  tagNode: TreeSitterNode,
  outerNode: TreeSitterNode,
): AnnotatedParse => {
  const tagParse = annotate(tagNode, tagNode.text, []);
  const result = annotate(outerNode, {
    tag: tagNode.text,
    value: innerParse.result,
  }, [tagParse, innerParse]);
  return result;
};
```

#### Validation Integration (errors.ts:257-278)

Validation has special handling for `!expr` tags:

```typescript
function ignoreExprViolations(
  error: LocalizedError,
  _parse: AnnotatedParse,
  _schema: Schema,
): LocalizedError | null {
  const { result } = error.violatingObject;
  if (
    typeof result !== "object" ||
    Array.isArray(result) ||
    result === null ||
    error.schemaPath.slice(-1)[0] !== "type"
  ) {
    return error;
  }

  if (result.tag === "!expr" && typeof result.value === "string") {
    // assume that this validation error came from !expr, drop the error.
    return null;
  } else {
    return error;
  }
}
```

**Rationale**: Values with `!expr` should skip type validation because they'll be evaluated later by R/Python/Julia.

### Usage Examples

#### Example 1: Figure Caption with R Expression

**document.qmd**:
```yaml
---
title: "Analysis"
---

```{r}
#| label: fig-plot
#| fig-cap: !expr paste("Air", "Quality")
plot(airquality)
```
```

**Parsed metadata**:
```json
{
  "label": "fig-plot",
  "fig-cap": {
    "value": "paste(\"Air\", \"Quality\")",
    "tag": "!expr"
  }
}
```

**Processing flow**:
1. YAML parser recognizes `!expr` tag
2. Value is wrapped as `{ value: "...", tag: "!expr" }`
3. Validation skips type checks (would expect string, got object)
4. R engine evaluates the expression at runtime
5. Result replaces the tagged object

#### Example 2: Conditional Evaluation

**document.qmd**:
```yaml
```{r}
#| eval: !expr knitr::is_html_output()
print("Only for HTML")
```
```

**Purpose**: Conditionally execute code based on output format.

#### Example 3: Multiple Tags in Project Config

**_quarto.yml**:
```yaml
format:
  html:
    theme: !expr if(Sys.getenv("DARK_MODE") == "1") "darkly" else "cosmo"
    toc: true
```

## yaml-rust2 Tag Support

### Event API

yaml-rust2 provides full tag support through its Event API:

```rust
pub enum Event {
    Nothing,
    StreamStart,
    StreamEnd,
    DocumentStart,
    DocumentEnd,
    Alias(usize),
    Scalar(String, TScalarStyle, usize, Option<Tag>),   // ← Tag info here
    SequenceStart(usize, Option<Tag>),                   // ← Tag info here
    SequenceEnd,
    MappingStart(usize, Option<Tag>),                    // ← Tag info here
    MappingEnd,
}
```

**Tag structure**:
```rust
pub struct Tag {
    /// Handle of the tag (`!` included).
    pub handle: String,
    /// The suffix of the tag.
    pub suffix: String,
}
```

### Tag Representation Examples

**Local tag** (`!expr`):
```rust
Tag {
    handle: "!".to_string(),
    suffix: "expr".to_string(),
}
```

**Standard YAML tag** (`!!str`):
```rust
Tag {
    handle: "!!".to_string(),
    suffix: "str".to_string(),
}
```

**Named handle tag** (`!my-app!custom`):
```rust
Tag {
    handle: "!my-app!".to_string(),
    suffix: "custom".to_string(),
}
```

### MarkedEventReceiver Integration

The current meta.rs implementation uses MarkedEventReceiver but **ignores tags**:

```rust
impl MarkedEventReceiver for YamlEventHandler {
    fn on_event(&mut self, ev: Event, _mark: yaml_rust2::scanner::Marker) {
        match ev {
            // ... other cases ...
            Event::Scalar(s, ..) => {  // ← ..(ignore) style, tag
                match self.stack.last_mut() {
                    Some(ContextFrame::Map(_, key_slot @ None)) => {
                        *key_slot = Some(s.to_string());
                    }
                    Some(ContextFrame::Map(_, Some(_))) | Some(ContextFrame::List(_)) => {
                        let value = self.parse_scalar(&s);
                        self.push_value(value);
                    }
                    _ => {}
                }
            },
            // ...
        }
    }
}
```

**To support tags**, we need to capture the tag parameter:

```rust
Event::Scalar(s, _style, _anchor, tag) => {
    // Now we have access to tag: Option<Tag>
    // ...
}
```

## Rust Port Design

### Updated AnnotatedParse Type

Extend AnnotatedParse to include optional tag information:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnnotatedParse {
    pub start: usize,
    pub end: usize,
    pub result: YamlValue,
    pub kind: YamlKind,
    pub source_info: SourceInfo,
    pub components: Vec<AnnotatedParse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<YamlTag>,  // ← NEW: Tag information
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Vec<YamlError>>,
}
```

### YamlTag Type

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct YamlTag {
    /// The tag handle (e.g., "!", "!!", "!my-app!")
    pub handle: String,

    /// The tag suffix (e.g., "expr", "str", "custom")
    pub suffix: String,
}

impl YamlTag {
    /// Get the full tag representation (e.g., "!expr")
    pub fn full_tag(&self) -> String {
        format!("{}{}", self.handle, self.suffix)
    }

    /// Check if this is a specific tag
    pub fn is_tag(&self, tag_str: &str) -> bool {
        self.full_tag() == tag_str
    }

    /// Create from yaml-rust2 Tag
    pub fn from_yaml_rust2(tag: &yaml_rust2::parser::Tag) -> Self {
        YamlTag {
            handle: tag.handle.clone(),
            suffix: tag.suffix.clone(),
        }
    }
}
```

### Tagged Value Representation

To maintain compatibility with TypeScript, represent tagged scalars as special objects:

```rust
/// When a scalar has a tag, wrap it in a TaggedValue
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaggedValue {
    pub value: YamlValue,
    pub tag: String,  // Full tag string like "!expr"
}

impl TaggedValue {
    pub fn new(value: YamlValue, tag: &YamlTag) -> Self {
        TaggedValue {
            value,
            tag: tag.full_tag(),
        }
    }

    pub fn to_yaml_value(self) -> YamlValue {
        // Convert to object representation for compatibility
        let mut map = IndexMap::new();
        map.insert("value".to_string(), self.value);
        map.insert("tag".to_string(), YamlValue::String(self.tag));
        YamlValue::Object(map)
    }
}
```

### Updated Parser Implementation

#### Handling Tagged Scalars

```rust
impl MarkedEventReceiver for AnnotatedYamlParser {
    fn on_event(&mut self, event: Event, mark: Marker) {
        match event {
            Event::Scalar(text, _style, _anchor, tag) => {
                let start = mark.index();
                let end = self.find_scalar_end(start, &text);

                // Parse the scalar value
                let base_value = parse_scalar_value(&text);

                // If there's a tag, wrap the value
                let (result, yaml_tag) = if let Some(ref t) = tag {
                    let yaml_tag = YamlTag::from_yaml_rust2(t);
                    let tagged_value = TaggedValue::new(base_value, &yaml_tag);
                    (tagged_value.to_yaml_value(), Some(yaml_tag))
                } else {
                    (base_value, None)
                };

                let annotated = AnnotatedParse {
                    start,
                    end,
                    result,
                    kind: YamlKind::Scalar,
                    source_info: self.source_info.substring(start, end),
                    components: vec![],
                    tag: yaml_tag,  // Preserve tag info
                    errors: None,
                };

                self.push_completed(annotated);
            }
            // ... other cases ...
        }
    }
}
```

#### Handling Tagged Collections

Tags can also apply to sequences and mappings:

```yaml
# Tagged sequence
tags: !special-list
  - rust
  - yaml

# Tagged mapping
config: !environment
  debug: true
  prod: false
```

**Implementation**:

```rust
Event::SequenceStart(_anchor, tag) => {
    let yaml_tag = tag.as_ref().map(|t| YamlTag::from_yaml_rust2(t));
    self.stack.push(PartialParse {
        start: mark.index(),
        kind: YamlKind::Sequence,
        tag: yaml_tag,  // Store tag info
        value_so_far: PartialValue::Sequence { items: vec![] },
    });
}

Event::MappingStart(_anchor, tag) => {
    let yaml_tag = tag.as_ref().map(|t| YamlTag::from_yaml_rust2(t));
    self.stack.push(PartialParse {
        start: mark.index(),
        kind: YamlKind::Mapping,
        tag: yaml_tag,  // Store tag info
        value_so_far: PartialValue::Mapping {
            pairs: vec![],
            current_key: None,
        },
    });
}
```

### Validation Integration

Update validation to handle tagged values:

```rust
/// Check if a value is a tagged expression (e.g., !expr)
pub fn is_expr_tag(value: &YamlValue) -> bool {
    if let YamlValue::Object(map) = value {
        if let (Some(YamlValue::String(tag_str)), Some(_value)) =
            (map.get("tag"), map.get("value"))
        {
            return tag_str == "!expr";
        }
    }
    false
}

/// Error handler: Ignore type violations for !expr tags
fn ignore_expr_violations(
    error: &LocalizedError,
) -> bool {
    // Check if error is a type mismatch
    if error.schema_path.last() != Some(&"type".to_string()) {
        return false;
    }

    // Check if the violating object is a tagged !expr value
    is_expr_tag(&error.violating_object.result)
}
```

**Integration with error handler pipeline**:

```rust
impl ErrorPipeline {
    pub fn process(&self, mut errors: Vec<LocalizedError>) -> Vec<LocalizedError> {
        errors.retain(|error| {
            // Filter out errors for !expr tags
            !ignore_expr_violations(error)
        });

        // Apply other handlers
        for error in &mut errors {
            for handler in &self.handlers {
                if let Some(modified) = handler.handle(error) {
                    *error = modified;
                }
            }
        }

        errors
    }
}
```

### Configuration Merging with Tags

When merging configurations, preserve tag information:

```rust
pub fn merge_annotated_parse(
    base: &AnnotatedParse,
    override_layer: &AnnotatedParse,
) -> AnnotatedParse {
    match (&base.result, &override_layer.result) {
        // When merging objects
        (YamlValue::Object(base_map), YamlValue::Object(override_map)) => {
            // ... merge logic ...

            AnnotatedParse {
                start: 0,
                end: 0,
                result: YamlValue::Object(merged_map),
                kind: YamlKind::Mapping,
                source_info: SourceInfo::concat(vec![...]),
                components: merged_components,
                tag: override_layer.tag.clone().or_else(|| base.tag.clone()),
                errors: None,
            }
        }

        // Scalar override: use override's value and tag
        (_, _) => override_layer.clone(),
    }
}
```

**Example merge**:

```yaml
# _quarto.yml
format:
  html:
    theme: cosmo

# document.qmd
format:
  html:
    theme: !expr get_theme()
```

**After merge**:
```json
{
  "format": {
    "html": {
      "theme": {
        "value": "get_theme()",
        "tag": "!expr"
      }
    }
  }
}
```

The override completely replaces the base value, including tag information.

## Implementation Strategy

### Phase 1: Extend AnnotatedParse (Week 1)

- [ ] Add `tag: Option<YamlTag>` field to AnnotatedParse
- [ ] Define YamlTag type with from_yaml_rust2 conversion
- [ ] Define TaggedValue type for wrapped values
- [ ] Add is_expr_tag utility function
- [ ] Update AnnotatedParse serialization tests

### Phase 2: Parser Integration (Week 1-2)

- [ ] Update AnnotatedYamlParser to capture tag from Event::Scalar
- [ ] Wrap tagged scalars in TaggedValue -> Object representation
- [ ] Handle tagged sequences (Event::SequenceStart with tag)
- [ ] Handle tagged mappings (Event::MappingStart with tag)
- [ ] Unit tests for parsing tagged values

### Phase 3: Validation Updates (Week 2)

- [ ] Implement ignore_expr_violations error handler
- [ ] Add handler to error pipeline
- [ ] Test validation with !expr tagged values
- [ ] Ensure type mismatches are properly ignored

### Phase 4: Configuration Merging (Week 2-3)

- [ ] Update merge_annotated_parse to preserve tags
- [ ] Test merging configs with tagged values
- [ ] Ensure override behavior is correct
- [ ] Test multi-layer merges with tags

### Phase 5: Integration Testing (Week 3)

- [ ] Port TypeScript yaml.test.ts tests with !expr
- [ ] Test with real Quarto documents using !expr
- [ ] Test cell options with !expr (fig-cap, eval, etc.)
- [ ] Test project configs with !expr
- [ ] Verify error messages for tagged values

### Phase 6: Future Tag Support (Week 4+)

- [ ] Design extensible tag handler system
- [ ] Support for custom application tags
- [ ] Tag handler registration API
- [ ] Documentation for custom tags

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_parse_expr_tag() {
    let yaml = r#"
fig-cap: !expr paste("Air", "Quality")
"#;

    let source_info = SourceInfo::original(file_id, Range::from_text(yaml));
    let annotated = parse_yaml_annotated(yaml, source_info).unwrap();

    // Check result is wrapped object
    let fig_cap = get_field(&annotated, "fig-cap").unwrap();
    assert!(matches!(fig_cap, YamlValue::Object(_)));

    // Check tag and value fields
    if let YamlValue::Object(map) = fig_cap {
        assert_eq!(map.get("tag"), Some(&YamlValue::String("!expr".into())));
        assert_eq!(map.get("value"), Some(&YamlValue::String("paste(\"Air\", \"Quality\")".into())));
    }

    // Check AnnotatedParse has tag info
    let fig_cap_parse = get_component_by_key(&annotated.components, "fig-cap").unwrap();
    assert!(fig_cap_parse.tag.is_some());
    assert_eq!(fig_cap_parse.tag.as_ref().unwrap().full_tag(), "!expr");
}

#[test]
fn test_validation_ignores_expr_tags() {
    let yaml = r#"
number-sections: !expr TRUE
"#;

    let annotated = parse_yaml_annotated(yaml, source_info).unwrap();
    let schema = get_html_format_schema();

    // number-sections expects boolean, but we have object with !expr
    // Validation should NOT error
    let errors = validate_yaml(&annotated, &schema, &source_context);
    assert_eq!(errors.len(), 0);
}

#[test]
fn test_merge_preserves_tags() {
    let base = parse_yaml_annotated("theme: cosmo", source_info_1).unwrap();
    let override_yaml = parse_yaml_annotated("theme: !expr get_theme()", source_info_2).unwrap();

    let merged = merge_annotated_parse(&base, &override_yaml);

    let theme = get_field(&merged, "theme").unwrap();
    // Should use override's tagged value
    assert!(is_expr_tag(theme));
}

#[test]
fn test_tagged_sequence() {
    let yaml = r#"
items: !special
  - a
  - b
"#;

    let annotated = parse_yaml_annotated(yaml, source_info).unwrap();
    let items_parse = get_component_by_key(&annotated.components, "items").unwrap();

    assert!(items_parse.tag.is_some());
    assert_eq!(items_parse.tag.as_ref().unwrap().suffix, "special");
}
```

### Integration Tests

```rust
#[test]
fn test_quarto_document_with_expr() {
    let qmd = r#"
---
title: "Test"
---

```{r}
#| fig-cap: !expr paste("Air", "Quality")
plot(airquality)
```
"#;

    let (pandoc, ctx) = qmd::read(qmd.as_bytes(), ...)?;

    // Extract code block metadata
    let code_block = find_code_block(&pandoc, "r")?;
    let options = extract_cell_options(&code_block)?;

    // Check fig-cap has !expr tag
    let fig_cap = options.get("fig-cap").unwrap();
    assert!(is_expr_tag(fig_cap));
}

#[test]
fn test_project_config_with_expr() {
    let yaml = r#"
format:
  html:
    theme: !expr if(Sys.getenv("DARK") == "1") "darkly" else "cosmo"
"#;

    let config = parse_project_config(yaml)?;

    // Validate entire config
    let schema = get_project_config_schema();
    let errors = validate_yaml(&config, &schema, &source_context);

    // Should have no errors (theme !expr should be ignored)
    assert_eq!(errors.len(), 0);
}
```

## Compatibility Considerations

### Backward Compatibility

**JSON Representation**:
```json
{
  "fig-cap": {
    "value": "paste(\"Air\", \"Quality\")",
    "tag": "!expr"
  }
}
```

This matches TypeScript exactly, ensuring:
- Existing R/Python/Julia evaluation code works unchanged
- Validation logic ports cleanly
- Error messages reference correct locations

### Forward Compatibility

**Extensibility for future tags**:

```rust
pub enum CustomTag {
    Expr,          // !expr - R/Python/Julia expression
    Env,           // !env - Environment variable substitution
    Template,      // !template - Template string
    Include,       // !include - Include external file
    Custom(String),  // User-defined tags
}

impl CustomTag {
    pub fn from_yaml_tag(tag: &YamlTag) -> Option<Self> {
        match tag.full_tag().as_str() {
            "!expr" => Some(CustomTag::Expr),
            "!env" => Some(CustomTag::Env),
            "!template" => Some(CustomTag::Template),
            "!include" => Some(CustomTag::Include),
            other => Some(CustomTag::Custom(other.to_string())),
        }
    }
}
```

## Open Questions

### Q1: Should we support other YAML tags beyond !expr?

**Current usage**: Only `!expr` is used in Quarto CLI.

**Future possibilities**:
- `!env` for environment variable substitution
- `!include` for including external files
- `!template` for template strings

**Recommendation**: Start with `!expr` support only, but design system to be extensible.

---

### Q2: How to handle tag information in merged configs?

**Scenario**:
```yaml
# Base
theme: cosmo

# Override
theme: !expr get_theme()
```

**Behavior**: Override completely replaces base, including tag.

**Merged result**:
```json
{
  "theme": {
    "value": "get_theme()",
    "tag": "!expr"
  }
}
```

**Recommendation**: Always use override's tag (or lack thereof) when merging.

---

### Q3: Should tags be preserved in AnnotatedParse.components?

**Current design**: Tags stored in both:
1. Top-level `AnnotatedParse.tag` field
2. Wrapped in result as `{ value: ..., tag: "..." }` object

**Rationale**:
- Top-level tag: For parser/AST navigation
- Wrapped result: For compatibility with validation/evaluation

**Recommendation**: Keep both representations.

---

### Q4: How to serialize/deserialize tagged values?

**JSON serialization** (for cache):
```json
{
  "start": 0,
  "end": 20,
  "result": {
    "value": "paste(\"Air\", \"Quality\")",
    "tag": "!expr"
  },
  "kind": "Scalar",
  "source_info": {...},
  "tag": {
    "handle": "!",
    "suffix": "expr"
  }
}
```

**Recommendation**: Use serde's skip_serializing_if for optional tags, full serialization otherwise.

---

### Q5: Should we validate tag syntax?

**Valid tags**:
- `!expr` - local tag
- `!!str` - standard YAML tag
- `!my-app!custom` - named handle tag

**Invalid usage**:
```yaml
# Invalid: tag with no value
invalid: !expr
```

**Recommendation**: yaml-rust2 handles tag syntax validation. We only need to validate that tagged values have appropriate structure.

---

## Performance Considerations

### Memory Impact

**Additional memory per tagged value**:
- YamlTag struct: ~48 bytes (2 Strings + overhead)
- Optional wrapper: 8 bytes (Option<YamlTag>)

**Typical case**: Very few tagged values in a config (< 10)
**Total overhead**: < 1 KB per document

**Verdict**: ✅ Negligible impact

### Parsing Performance

**Additional operations per tagged scalar**:
1. Check if tag is present: O(1)
2. Create YamlTag: O(1) string clones
3. Wrap value in object: O(1)

**Impact**: < 1% slowdown for typical YAML

**Verdict**: ✅ Acceptable

### Validation Performance

**Tagged values skip some validation**:
- Type checks skipped for !expr
- Faster validation for expressions

**Net impact**: Neutral to slightly positive

**Verdict**: ✅ No performance concern

## Comparison with TypeScript

| Aspect | TypeScript | Rust (Proposed) |
|--------|-----------|-----------------|
| **Tag representation** | `{ value: ..., tag: "..." }` | `{ value: ..., tag: "..." }` |
| **Parser support** | js-yaml custom schema | yaml-rust2 Event API |
| **AnnotatedParse storage** | No separate tag field | `tag: Option<YamlTag>` field |
| **Validation** | ignoreExprViolations handler | ignore_expr_violations handler |
| **Merging** | Override wins | Override wins |
| **Extensibility** | Add to schema | Tag handler system |
| **Serializable** | No (closures in MappedString) | Yes (all data structures) |

## Conclusion

**Recommendation**: ✅ **Implement full YAML tag support with AnnotatedParse integration**

**Key advantages**:
1. **yaml-rust2 provides complete tag support** - No library limitations
2. **Compatible representation** - Matches TypeScript behavior exactly
3. **Extensible design** - Easy to add new tags in future
4. **Validation integration** - Error handlers cleanly skip !expr tags
5. **Merge compatibility** - Tags preserved through configuration pipeline
6. **Minimal overhead** - Negligible memory and performance impact

**Implementation timeline**: 3-4 weeks
- Week 1: AnnotatedParse extension and parser integration
- Week 2: Validation and merging updates
- Week 3: Integration testing with real Quarto docs
- Week 4: Polish and edge cases

**Risk**: Low (yaml-rust2 provides all necessary primitives)

**Benefit**: High (critical feature for Quarto, enables R/Python expression evaluation)

## Next Steps

1. Create prototype AnnotatedParse with tag field
2. Update AnnotatedYamlParser to capture tags from Event::Scalar
3. Test with simple !expr examples
4. Port TypeScript validation handlers
5. Integrate with config merging
6. Test with real Quarto documents

## References

- **yaml-rust2 Event API**: https://github.com/Ethiraric/yaml-rust2/blob/master/src/parser.rs
- **Current TypeScript implementation**:
  - src/core/lib/yaml-intelligence/js-yaml-schema.ts
  - src/core/lib/yaml-intelligence/annotated-yaml.ts
  - src/core/lib/yaml-validation/errors.ts (ignoreExprViolations)
- **YAML 1.2 Tag Specification**: https://yaml.org/spec/1.2/spec.html#id2764295
- **Quarto !expr usage**: tests/docs/yaml/test-tag-expr.qmd
