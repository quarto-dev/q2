# Plan: Preserve YAML Tag Information in New API (k-62)

## Problem Statement

The old `rawblock_to_meta` API preserves YAML tag information (!path, !glob, !str) but the new `rawblock_to_meta_with_source_info` API loses it. Tags are currently discarded during YAML parsing in quarto-yaml.

## Additional Requirements (from code review)

1. **Single tag per node**: YAML spec only allows one tag per node (confirmed)
2. **Tag source location tracking**: Need to track where the tag itself appears in source for error reporting

## Root Cause Analysis

1. **yaml_rust2** provides tag information via `Event::Scalar(_value, _style, _anchor, tag)` during parsing
2. **quarto-yaml** receives this tag but ignores it (line 264 of parser.rs)
3. **YamlWithSourceInfo** doesn't have a field to store tag information
4. **yaml_to_meta_with_source_info** has a TODO comment about checking tags but can't access them

## Proposed Solution

### Phase 1: Extend quarto-yaml to Track Tags

1. **Add tag field to YamlWithSourceInfo**:
   ```rust
   pub struct YamlWithSourceInfo {
       pub yaml: Yaml,
       pub source_info: SourceInfo,  // Covers the entire node including tag
       pub tag: Option<(String, SourceInfo)>,  // NEW: Tag suffix + tag's source location
       pub children: YamlChildren,
   }
   ```

2. **Capture tag in parser** (parser.rs line 264):
   ```rust
   Event::Scalar(value, _style, _anchor_id, tag) => {
       // Compute source info for the entire node (from marker to end)
       let tag_info = tag.as_ref().map(|t| {
           // Tag appears at marker position
           // Format: !<suffix> where suffix is what we care about
           let tag_len = 1 + t.suffix.len(); // ! + suffix
           let tag_source_info = self.make_source_info(&marker, tag_len);
           (t.suffix.clone(), tag_source_info)
       });

       // For the value's source_info, we need to account for tag + space
       let value_offset = if tag.is_some() {
           // Rough estimate: tag length + space
           1 + tag.as_ref().unwrap().suffix.len() + 1
       } else {
           0
       };

       // Adjust marker for value position
       let value_marker = ... // Need to compute adjusted marker
       let len = self.compute_scalar_len(&value_marker, &value);
       let source_info = self.make_source_info(&value_marker, len);

       let yaml = parse_scalar_value(&value);
       let node = YamlWithSourceInfo::new_scalar_with_tag(yaml, source_info, tag_info);

       self.push_complete(node);
   }
   ```

3. **Add new constructor**:
   ```rust
   impl YamlWithSourceInfo {
       pub fn new_scalar_with_tag(
           yaml: Yaml,
           source_info: SourceInfo,
           tag: Option<(String, SourceInfo)>
       ) -> Self {
           // ...
       }
   }
   ```

4. **Update existing constructors** to pass `tag: None` for backwards compatibility

### Phase 2: Use Tags in quarto-markdown-pandoc

5. **Update yaml_to_meta_with_source_info** (meta.rs around line 265):
   - Check if `yaml.tag` is present
   - If tagged, wrap in Span with class "yaml-tagged-string" and tag attribute
   - If not tagged, parse as markdown (current behavior)
   - Store tag source location for potential error reporting

   ```rust
   match yaml_value {
       Yaml::String(s) => {
           // Check for YAML tag
           if let Some((tag_suffix, tag_source_info)) = yaml.tag {
               // Tagged string - bypass markdown parsing
               let mut attributes = HashMap::new();
               attributes.insert("tag".to_string(), tag_suffix.clone());

               let span = Span {
                   attr: (
                       String::new(),
                       vec!["yaml-tagged-string".to_string()],
                       attributes,
                   ),
                   content: vec![Inline::Str(Str {
                       text: s.clone(),
                       source_info: source_info.clone(),  // Value's source
                       source_info_qsm: None,
                   })],
                   source_info: tag_source_info,  // Tag's source (for error reporting on the tag itself)
               };
               MetaValueWithSourceInfo::MetaInlines {
                   content: vec![Inline::Span(span)],
                   source_info,  // Overall node source
               }
           } else {
               // Untagged - return as MetaString for later markdown parsing
               MetaValueWithSourceInfo::MetaString {
                   value: s,
                   source_info,
               }
           }
       }
       // ... other cases
   }
   ```

6. **Update parse_metadata_strings_with_source_info**:
   - Skip markdown parsing for MetaInlines that already contain yaml-tagged-string spans
   - Or better: tagged strings won't reach this function as MetaString, they'll already be MetaInlines

### Phase 3: Testing

7. **Verify test_yaml_tag_regression passes**
8. **Verify test_yaml_tagged_strings still passes with old API**
9. **Run full test suite**

## Impact Analysis

### Files to Modify

**quarto-yaml crate:**
- `src/yaml_with_source_info.rs`: Add `tag` field
- `src/parser.rs`: Capture and store tag from Event::Scalar

**quarto-markdown-pandoc crate:**
- `src/pandoc/meta.rs`: Update `yaml_to_meta_with_source_info` to handle tags
- `tests/test_yaml_tag_regression.rs`: Should pass after fix

### Breaking Changes

Adding a field to `YamlWithSourceInfo` is technically a breaking change, but:
- It's a public struct so we need to be careful
- We can make the field `pub` and use `#[serde(default)]` for backwards compatibility
- Most code uses the provided constructors/methods, not direct struct construction

### Risks

1. **Performance**: Storing `Option<(String, SourceInfo)>` for every node adds memory overhead
   - Mitigation: Most nodes won't have tags, so `None` is just a pointer-sized overhead

2. **Complexity**: Need to thread tag through all YAML construction code
   - Mitigation: Use helper methods to make this easier, provide constructors with default `tag: None`

3. **Tag position calculation**: Computing exact source position of tag + adjusting marker for value
   - Risk: Off-by-one errors, whitespace handling complexity
   - Mitigation: Start with simple implementation, add tests with various whitespace patterns

4. **Test coverage**: Need to ensure all tag types work correctly
   - Mitigation: Existing test_yaml_tagged_strings provides good coverage

## Alternative Approaches Considered

### Alternative 1: Store tags in a separate map
- Pro: Doesn't modify YamlWithSourceInfo structure
- Con: Harder to maintain consistency, lookup overhead

### Alternative 2: Only handle tags in meta.rs without quarto-yaml changes
- Pro: Smaller change scope
- Con: Impossible - tag information is already lost by the time meta.rs sees it

### Alternative 3: Use yaml-rust2 directly for tagged values
- Pro: No quarto-yaml changes needed
- Con: Loses source tracking for tagged values, inconsistent with overall design

## Recommendation

Proceed with the proposed solution. It's clean, maintains source tracking, and follows the existing architecture. The breaking change to `YamlWithSourceInfo` is acceptable for an internal crate.

## Implementation Order

1. Add `tag` field to `YamlWithSourceInfo` with default/None
2. Update quarto-yaml parser to capture tags
3. Update yaml_to_meta_with_source_info to use tags
4. Run tests and iterate
