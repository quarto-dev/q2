# Configuration Merging Analysis: Source-Location-Aware merge_configs

## Executive Summary

This document analyzes how to implement Quarto's configuration merging system (`mergeConfigs`) in Rust while maintaining source location information for validation. The current TypeScript implementation uses eager merging with lodash's `mergeWith`, which loses source provenance. For the Rust port, we need a strategy that preserves the origin of every configuration value to enable precise error reporting.

**Recommendation**: Implement **eager merging with source-tracked AnnotatedParse trees**, creating a new merged AnnotatedParse where each node's `source_info` points to its origin layer.

## Background

### The Problem

Quarto merges configuration from multiple sources in a hierarchical manner:

```
default format configs
  ↓ (merge)
extension format configs
  ↓ (merge)
project-level configs (_quarto.yml)
  ↓ (merge)
directory-level configs (_metadata.yml)
  ↓ (merge)
document-level configs (frontmatter)
  ↓ (merge)
command-line flags
  ↓
final configuration
```

**Current limitation**: After merging in TypeScript, we lose track of *where* each value came from. This makes it impossible to report validation errors at the correct source location.

**Rust port requirement**: Maintain source location information through the merge, so that:
- `theme: "invalid-value"` can report an error pointing to the exact file and line where it was defined
- Validation can distinguish between project defaults and user overrides
- IDE features can provide accurate diagnostics

### Example Scenario

**_quarto.yml** (project config):
```yaml
format:
  html:
    theme: cosmo
    toc: true
```

**document.qmd** (frontmatter):
```yaml
format:
  html:
    toc: false
    number-sections: 3.5  # ERROR: should be boolean
```

**After merge** (logical result):
```yaml
format:
  html:
    theme: cosmo           # from _quarto.yml:3
    toc: false             # from document.qmd:3 (overrides)
    number-sections: 3.5   # from document.qmd:4 (ERROR)
```

**Validation requirement**: When validating `number-sections`, the error must point to `document.qmd:4`, not to `_quarto.yml` or a generic "merged config" location.

## Current TypeScript Implementation

### Core mergeConfigs (src/core/config.ts)

```typescript
export const mergeConfigs = makeTimedFunction(
  "mergeConfigs",
  function mergeConfigs<T>(config: T, ...configs: Array<unknown>): T {
    // copy all configs so we don't mutate them
    config = ld.cloneDeep(config);
    configs = ld.cloneDeep(configs);

    return ld.mergeWith(
      config,
      ...configs,
      mergeArrayCustomizer,
    );
  },
);

export function mergeArrayCustomizer(objValue: unknown, srcValue: unknown) {
  if (ld.isArray(objValue) || ld.isArray(srcValue)) {
    // handle nulls
    if (!objValue) {
      return srcValue;
    } else if (!srcValue) {
      return objValue;
    } else {
      // coerce scalars to array
      if (!ld.isArray(objValue)) {
        objValue = [objValue];
      }
      if (!ld.isArray(srcValue)) {
        srcValue = [srcValue];
      }
    }

    // concatenate and deduplicate
    const combined = (objValue as Array<unknown>).concat(srcValue as Array<unknown>);
    return ld.uniqBy(combined, (value: unknown) => {
      if (typeof value === "function") {
        return globalThis.crypto.randomUUID();
      } else {
        return JSON.stringify(value);
      }
    });
  }
}
```

**Key behaviors**:
1. **Deep cloning**: Prevents mutation of input configs
2. **Deep merging**: Recursively merges nested objects
3. **Array concatenation**: Arrays are concatenated and deduplicated (not replaced)
4. **Later wins**: When a key exists in multiple configs, the later one wins
5. **No source tracking**: Result is plain JSON with no origin metadata

### Customized Variants

**mergeConfigsCustomized** (src/config/metadata.ts):
```typescript
export function mergeConfigsCustomized<T>(
  customizer: (objValue: unknown, srcValue: unknown, key: string) => unknown | undefined,
  config: T,
  ...configs: Array<T>
) {
  config = ld.cloneDeep(config);
  configs = ld.cloneDeep(configs);

  return ld.mergeWith(
    config,
    ...configs,
    (objValue: unknown, srcValue: unknown, key: string) => {
      const custom = customizer(objValue, srcValue, key);
      if (custom !== undefined) {
        return custom;
      } else {
        return mergeArrayCustomizer(objValue, srcValue);
      }
    },
  );
}
```

**Special merge behaviors**:
- **mergeFormatMetadata**: Special handling for:
  - `kTblColwidths`: Last value wins (unmergeable array)
  - `kVariant`: Pandoc variants are merged specially (combining +/- extensions)
  - `kCodeLinks`, `kOtherLinks`: Boolean `false` disables arrays
- **mergeProjectMetadata**: String keys that normally expand to arrays are replaced

### Usage Patterns

**Hierarchical format merging** (render-contexts.ts:507-511):
```typescript
const userFormat = mergeFormatMetadata(
  projFormat || {},
  directoryFormat || {},
  inputFormat || {},
);
```

**Multi-layer format resolution** (render-contexts.ts:549-555):
```typescript
mergedFormats[format] = mergeFormatMetadata(
  defaultWriterFormat(formatDesc.formatWithVariants),
  extensionMetadata[formatDesc.baseFormat]?.format || {},
  userFormat,
);
```

**Metadata with includes** (render-contexts.ts:105-109):
```typescript
const allMetadata = mergeQuartoConfigs(
  metadata,
  included.metadata,
  flags?.metadata || {},
);
```

## Rust Port Requirements

### 1. Source Location Tracking

Every configuration value must maintain its origin:
- **File ID**: Which file it came from (FileId in SourceContext)
- **Position**: Line and column in that file (Range in SourceInfo)
- **Transformation**: How it was extracted/normalized (SourceMapping chain)

### 2. Validation Integration

The merged configuration must be directly validatable:
- Pass merged config to validator
- Validator extracts SourceInfo for each validated node
- Errors report original file:line:column

### 3. Multiple Merge Semantics

Support different merge behaviors per key:
- **Standard merge**: Later values override earlier ones
- **Array concatenation**: Merge arrays element-wise with deduplication
- **Array replacement**: Some arrays should replace, not merge
- **Special merges**: Pandoc variants, disableable arrays, etc.

### 4. Performance

Configuration merging happens frequently:
- Every document render
- Every format resolution
- Multiple layers (5-7 merges per document)
- Must be fast enough for interactive use (LSP)

### 5. Serialization

Merged configurations should be cacheable:
- For LSP performance (disk cache)
- For incremental builds
- Must preserve source location information

## Implementation Strategies

### Strategy 1: Eager Merging (Current Approach)

**Description**: Immediately create a new merged object, copying all values.

**TypeScript pseudo-code**:
```typescript
function mergeConfigs(base, override) {
  const result = {};
  for (const key in base) result[key] = base[key];
  for (const key in override) result[key] = override[key];  // Later wins
  return result;
}
```

**Pros**:
- ✅ Simple to implement
- ✅ Fast access (no indirection)
- ✅ Standard JavaScript approach

**Cons**:
- ❌ Loses source location information
- ❌ Can't answer "where did this value come from?"
- ❌ Not suitable for Rust port with validation requirements

**Verdict**: ❌ **Not viable** for Rust port (no source tracking)

---

### Strategy 2: Lazy/Proxy Resolution

**Description**: Don't merge immediately. Instead, create a proxy object that resolves values on-demand by checking layers in order.

**Rust pseudo-code**:
```rust
struct MergedConfig {
    layers: Vec<AnnotatedParse>,  // Ordered: first = lowest priority
}

impl MergedConfig {
    fn get(&self, path: &[&str]) -> Option<(&YamlValue, &SourceInfo)> {
        // Check layers in reverse order (highest priority first)
        for layer in self.layers.iter().rev() {
            if let Some(value) = layer.get_path(path) {
                return Some((&value.result, &value.source_info));
            }
        }
        None
    }
}
```

**Example**:
```rust
// Merge project config + document config
let merged = MergedConfig {
    layers: vec![project_config, document_config],
};

// Get "format.html.theme"
let (value, source_info) = merged.get(&["format", "html", "theme"])?;
// Returns value from document_config if present, else project_config
// source_info points to the layer it came from
```

**Pros**:
- ✅ Perfect source tracking (no information loss)
- ✅ Memory efficient (no duplication)
- ✅ Can query "where did this value come from?"
- ✅ Lazy evaluation (only access what's needed)

**Cons**:
- ❌ Complex implementation (proxy mechanics)
- ❌ Performance overhead (multiple layer lookups per access)
- ❌ Array merging is ambiguous (which layer do array elements come from?)
- ❌ Doesn't integrate cleanly with AnnotatedParse validation
- ❌ Difficult to serialize (layers may reference different files)
- ❌ Deep nesting requires recursive layer checks

**Verdict**: ⚠️ **Possible but not recommended** (complexity outweighs benefits)

---

### Strategy 3: Eager Merging with Value Wrappers

**Description**: Eagerly merge like Strategy 1, but wrap each value with its source information.

**Rust pseudo-code**:
```rust
struct TrackedValue {
    value: YamlValue,
    source_info: SourceInfo,
}

struct TrackedConfig {
    fields: HashMap<String, TrackedValue>,
}

fn merge_configs(base: TrackedConfig, override: TrackedConfig) -> TrackedConfig {
    let mut result = base.clone();
    for (key, tracked_value) in override.fields {
        result.fields.insert(key, tracked_value);  // Preserves source_info
    }
    result
}
```

**Example**:
```rust
// Project config
let project = TrackedConfig {
    fields: {
        "theme" => TrackedValue {
            value: YamlValue::String("cosmo"),
            source_info: SourceInfo::original(project_file_id, Range::at(3, 10)),
        },
        "toc" => TrackedValue {
            value: YamlValue::Bool(true),
            source_info: SourceInfo::original(project_file_id, Range::at(4, 10)),
        },
    }
};

// Document config (override)
let document = TrackedConfig {
    fields: {
        "toc" => TrackedValue {
            value: YamlValue::Bool(false),
            source_info: SourceInfo::original(doc_file_id, Range::at(3, 10)),
        },
    }
};

// Merged result
let merged = merge_configs(project, document);
// merged["theme"] -> source_info points to project_file_id:3:10
// merged["toc"] -> source_info points to doc_file_id:3:10 (overridden)
```

**Pros**:
- ✅ Preserves source information for all values
- ✅ Fast access (no proxy indirection)
- ✅ Serializable (wrapper is plain data)
- ✅ Integrates with validation (validator sees TrackedValue)

**Cons**:
- ⚠️ More memory usage (wrapper overhead)
- ⚠️ Need wrapper for every configuration value
- ⚠️ Nested objects require recursive wrapping
- ❌ Array merging is complex (which elements came from which layer?)
- ❌ Doesn't integrate naturally with existing AnnotatedParse

**Verdict**: ⚠️ **Workable but not ideal** (reinvents AnnotatedParse)

---

### Strategy 4: AnnotatedParse Merge ⭐ **RECOMMENDED**

**Description**: Leverage the existing `AnnotatedParse` structure, which already tracks source locations for YAML trees. Merge two AnnotatedParse trees to create a new AnnotatedParse where each node's `source_info` points to its origin.

**Key insight**: Configuration is already parsed as AnnotatedParse! We just need to merge AnnotatedParse trees while preserving their source information.

**Rust pseudo-code**:
```rust
pub fn merge_annotated_parse(
    base: &AnnotatedParse,
    override_layer: &AnnotatedParse,
) -> AnnotatedParse {
    match (&base.result, &override_layer.result) {
        // Both are objects: merge recursively
        (YamlValue::Object(base_map), YamlValue::Object(override_map)) => {
            let mut merged_map = base_map.clone();
            let mut merged_components = Vec::new();

            // Add all base components
            for component in &base.components {
                merged_components.push(component.clone());
            }

            // Override/add from override layer
            for (key, override_value) in override_map {
                if let Some(base_value) = merged_map.get(key) {
                    // Key exists in both: recursively merge
                    let base_component = base.components.iter()
                        .find(|c| c.kind == YamlKind::Scalar && c.result == YamlValue::String(key.clone()))
                        .expect("component for key");

                    let override_component = override_layer.components.iter()
                        .find(|c| c.kind == YamlKind::Scalar && c.result == YamlValue::String(key.clone()))
                        .expect("component for key");

                    let merged_value = merge_annotated_parse(base_component, override_component);
                    merged_map.insert(key.clone(), merged_value.result.clone());

                    // Replace component with merged one
                    merged_components.retain(|c| {
                        !(c.kind == YamlKind::Scalar && c.result == YamlValue::String(key.clone()))
                    });
                    merged_components.push(merged_value);
                } else {
                    // Key only in override: use override's value and source
                    merged_map.insert(key.clone(), override_value.clone());

                    if let Some(override_component) = override_layer.components.iter()
                        .find(|c| c.kind == YamlKind::Scalar && c.result == YamlValue::String(key.clone())) {
                        merged_components.push(override_component.clone());
                    }
                }
            }

            // Create merged AnnotatedParse
            AnnotatedParse {
                start: 0,
                end: 0,  // Will be recalculated
                result: YamlValue::Object(merged_map),
                kind: YamlKind::Mapping,
                source_info: SourceInfo::concat(vec![
                    (base.source_info.clone(), base.end - base.start),
                    (override_layer.source_info.clone(), override_layer.end - override_layer.start),
                ]),
                components: merged_components,
                errors: None,
            }
        }

        // Arrays: concatenate and deduplicate
        (YamlValue::Array(base_arr), YamlValue::Array(override_arr)) => {
            let mut combined = base_arr.clone();
            combined.extend(override_arr.iter().cloned());

            // Deduplicate by JSON representation
            let mut seen = HashSet::new();
            combined.retain(|item| {
                let key = serde_json::to_string(item).unwrap();
                seen.insert(key)
            });

            AnnotatedParse {
                start: 0,
                end: combined.len(),
                result: YamlValue::Array(combined),
                kind: YamlKind::Sequence,
                source_info: SourceInfo::concat(vec![
                    (base.source_info.clone(), base.end - base.start),
                    (override_layer.source_info.clone(), override_layer.end - override_layer.start),
                ]),
                components: base.components.iter().chain(override_layer.components.iter()).cloned().collect(),
                errors: None,
            }
        }

        // Scalar override: use override's value and source
        (_, _) => override_layer.clone(),
    }
}
```

**Example**:
```rust
// Project config as AnnotatedParse
let project_config = parse_yaml_annotated(
    r#"
    theme: cosmo
    toc: true
    "#,
    SourceInfo::original(project_file_id, Range::from_text("...")),
)?;

// Document config as AnnotatedParse
let document_config = parse_yaml_annotated(
    r#"
    toc: false
    number-sections: 3.5
    "#,
    SourceInfo::original(doc_file_id, Range::from_text("...")),
)?;

// Merge
let merged_config = merge_annotated_parse(&project_config, &document_config);

// Access merged values
// merged_config.components[0] = "theme: cosmo" with source_info -> project_file_id
// merged_config.components[1] = "toc: false" with source_info -> doc_file_id (overridden!)
// merged_config.components[2] = "number-sections: 3.5" with source_info -> doc_file_id

// Validate
let errors = validate_yaml(&merged_config, &schema, &source_context);
// Error on "number-sections" will point to doc_file_id:3 (correct source!)
```

**Pros**:
- ✅ Perfect source tracking (leverages existing SourceInfo)
- ✅ Integrates naturally with existing systems (AnnotatedParse, validation)
- ✅ Serializable (AnnotatedParse already derives Serialize)
- ✅ Fast validation (validator already works with AnnotatedParse)
- ✅ No new data structures needed (reuses AnnotatedParse)
- ✅ Array merging is well-defined (components track individual elements)
- ✅ Supports all merge semantics (customizer function can handle special cases)

**Cons**:
- ⚠️ Moderate implementation complexity (recursive merge logic)
- ⚠️ Memory usage for merged tree (but necessary for validation anyway)

**Verdict**: ⭐ **RECOMMENDED** (best fit for requirements)

---

## Detailed Design: AnnotatedParse Merge

### Core API

```rust
/// Merge two AnnotatedParse trees, preserving source location information
pub fn merge_annotated_parse(
    base: &AnnotatedParse,
    override_layer: &AnnotatedParse,
) -> AnnotatedParse {
    merge_annotated_parse_with_customizer(base, override_layer, &default_customizer)
}

/// Merge with custom merge behavior for specific keys
pub fn merge_annotated_parse_with_customizer(
    base: &AnnotatedParse,
    override_layer: &AnnotatedParse,
    customizer: &dyn MergeCustomizer,
) -> AnnotatedParse {
    // Implementation in next section
}

/// Merge multiple layers (variadic)
pub fn merge_annotated_parse_all(
    layers: &[AnnotatedParse],
) -> AnnotatedParse {
    layers.iter().fold(
        AnnotatedParse::empty(),
        |acc, layer| merge_annotated_parse(&acc, layer)
    )
}
```

### Merge Customizer Trait

```rust
pub trait MergeCustomizer {
    /// Customize merge behavior for a specific key
    /// Return Some(value) to override default merge, None to use default
    fn customize(
        &self,
        key: &str,
        base_value: &AnnotatedParse,
        override_value: &AnnotatedParse,
    ) -> Option<AnnotatedParse>;
}

struct DefaultCustomizer;

impl MergeCustomizer for DefaultCustomizer {
    fn customize(&self, _key: &str, _base: &AnnotatedParse, _override: &AnnotatedParse) -> Option<AnnotatedParse> {
        None  // Use default merge behavior
    }
}

fn default_customizer() -> impl MergeCustomizer {
    DefaultCustomizer
}
```

### Format Metadata Customizer

```rust
struct FormatMetadataCustomizer;

impl MergeCustomizer for FormatMetadataCustomizer {
    fn customize(
        &self,
        key: &str,
        base_value: &AnnotatedParse,
        override_value: &AnnotatedParse,
    ) -> Option<AnnotatedParse> {
        match key {
            // Unmergeable keys: last value wins
            "tbl-colwidths" => {
                Some(override_value.clone())
            }

            // Pandoc variants: merge specially
            "variant" => {
                Some(merge_pandoc_variant(base_value, override_value))
            }

            // Disableable arrays: false disables
            "code-links" | "other-links" => {
                Some(merge_disableable_array(base_value, override_value))
            }

            _ => None,  // Use default merge
        }
    }
}

pub fn merge_format_metadata(
    base: &AnnotatedParse,
    override_layer: &AnnotatedParse,
) -> AnnotatedParse {
    merge_annotated_parse_with_customizer(base, override_layer, &FormatMetadataCustomizer)
}
```

### Implementation Details

#### Object Merging

```rust
fn merge_objects(
    base: &AnnotatedParse,
    override_layer: &AnnotatedParse,
    customizer: &dyn MergeCustomizer,
) -> AnnotatedParse {
    let base_map = match &base.result {
        YamlValue::Object(m) => m,
        _ => panic!("Expected object"),
    };
    let override_map = match &override_layer.result {
        YamlValue::Object(m) => m,
        _ => panic!("Expected object"),
    };

    let mut merged_map = IndexMap::new();
    let mut merged_components = Vec::new();

    // Process all keys from both maps
    let all_keys: HashSet<String> = base_map.keys()
        .chain(override_map.keys())
        .cloned()
        .collect();

    for key in all_keys {
        let base_val = get_component_by_key(&base.components, &key);
        let override_val = get_component_by_key(&override_layer.components, &key);

        let merged_val = match (base_val, override_val) {
            (Some(b), Some(o)) => {
                // Key exists in both: try customizer first
                if let Some(custom) = customizer.customize(&key, b, o) {
                    custom
                } else {
                    // Default: recursively merge
                    merge_annotated_parse_with_customizer(b, o, customizer)
                }
            }
            (Some(b), None) => {
                // Key only in base: keep base value
                b.clone()
            }
            (None, Some(o)) => {
                // Key only in override: use override value
                o.clone()
            }
            (None, None) => unreachable!(),
        };

        merged_map.insert(key.clone(), merged_val.result.clone());
        merged_components.push(merged_val);
    }

    AnnotatedParse {
        start: 0,
        end: 0,
        result: YamlValue::Object(merged_map),
        kind: YamlKind::Mapping,
        source_info: SourceInfo::concat(vec![
            (base.source_info.clone(), 1),
            (override_layer.source_info.clone(), 1),
        ]),
        components: merged_components,
        errors: None,
    }
}
```

#### Array Merging

```rust
fn merge_arrays(
    base: &AnnotatedParse,
    override_layer: &AnnotatedParse,
) -> AnnotatedParse {
    let base_arr = match &base.result {
        YamlValue::Array(a) => a,
        _ => panic!("Expected array"),
    };
    let override_arr = match &override_layer.result {
        YamlValue::Array(a) => a,
        _ => panic!("Expected array"),
    };

    // Concatenate
    let mut combined = base_arr.clone();
    combined.extend(override_arr.iter().cloned());

    // Deduplicate by JSON representation
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    let mut deduped_components = Vec::new();

    for (i, item) in combined.iter().enumerate() {
        let key = serde_json::to_string(item).unwrap_or_else(|_| format!("{:?}", item));
        if seen.insert(key) {
            deduped.push(item.clone());

            // Track which component this came from
            if i < base.components.len() {
                deduped_components.push(base.components[i].clone());
            } else {
                deduped_components.push(override_layer.components[i - base.components.len()].clone());
            }
        }
    }

    AnnotatedParse {
        start: 0,
        end: deduped.len(),
        result: YamlValue::Array(deduped),
        kind: YamlKind::Sequence,
        source_info: SourceInfo::concat(vec![
            (base.source_info.clone(), base.components.len()),
            (override_layer.source_info.clone(), override_layer.components.len()),
        ]),
        components: deduped_components,
        errors: None,
    }
}
```

#### Scalar Override

```rust
fn merge_scalars(
    _base: &AnnotatedParse,
    override_layer: &AnnotatedParse,
) -> AnnotatedParse {
    // Scalars: last value wins
    override_layer.clone()
}
```

#### Utility: Get Component by Key

```rust
fn get_component_by_key(components: &[AnnotatedParse], key: &str) -> Option<&AnnotatedParse> {
    // In an object's components, keys and values are interleaved
    // component[0] = key, component[1] = value, component[2] = key, component[3] = value, ...
    for i in (0..components.len()).step_by(2) {
        if let YamlValue::String(k) = &components[i].result {
            if k == key {
                return components.get(i + 1);
            }
        }
    }
    None
}
```

### Special Merge Functions

#### Merge Pandoc Variant

```rust
fn merge_pandoc_variant(
    base: &AnnotatedParse,
    override_layer: &AnnotatedParse,
) -> AnnotatedParse {
    let base_str = match &base.result {
        YamlValue::String(s) => s,
        _ => return override_layer.clone(),
    };
    let override_str = match &override_layer.result {
        YamlValue::String(s) => s,
        _ => return override_layer.clone(),
    };

    if base_str == override_str {
        return override_layer.clone();
    }

    // Parse variants: "+extension1-extension2+extension3"
    let mut extensions: HashMap<String, bool> = HashMap::new();

    for variant_str in &[base_str, override_str] {
        let re = regex::Regex::new(r"([+-])([a-z_]+)").unwrap();
        for cap in re.captures_iter(variant_str) {
            let enabled = &cap[1] == "+";
            let name = cap[2].to_string();
            extensions.insert(name, enabled);
        }
    }

    // Reconstruct variant string
    let merged_variant: String = extensions.iter()
        .map(|(name, enabled)| format!("{}{}", if *enabled { "+" } else { "-" }, name))
        .collect::<Vec<_>>()
        .join("");

    AnnotatedParse {
        start: 0,
        end: merged_variant.len(),
        result: YamlValue::String(merged_variant.clone()),
        kind: YamlKind::Scalar,
        source_info: SourceInfo::concat(vec![
            (base.source_info.clone(), base.end - base.start),
            (override_layer.source_info.clone(), override_layer.end - override_layer.start),
        ]),
        components: vec![],
        errors: None,
    }
}
```

#### Merge Disableable Array

```rust
fn merge_disableable_array(
    base: &AnnotatedParse,
    override_layer: &AnnotatedParse,
) -> AnnotatedParse {
    // If override is false, return empty array
    if let YamlValue::Bool(false) = override_layer.result {
        return AnnotatedParse {
            start: 0,
            end: 0,
            result: YamlValue::Array(vec![]),
            kind: YamlKind::Sequence,
            source_info: override_layer.source_info.clone(),
            components: vec![],
            errors: None,
        };
    }

    // Otherwise, merge as arrays
    match (&base.result, &override_layer.result) {
        (YamlValue::Array(_), YamlValue::Array(_)) => {
            merge_arrays(base, override_layer)
        }
        _ => {
            // Coerce to arrays and merge
            let base_arr = AnnotatedParse {
                result: YamlValue::Array(vec![base.result.clone()]),
                ..base.clone()
            };
            let override_arr = AnnotatedParse {
                result: YamlValue::Array(vec![override_layer.result.clone()]),
                ..override_layer.clone()
            };
            merge_arrays(&base_arr, &override_arr)
        }
    }
}
```

## Integration with Validation

### Validation Flow

```rust
// 1. Parse configs as AnnotatedParse
let project_config = parse_yaml_annotated(
    &project_yaml,
    SourceInfo::original(project_file_id, Range::from_text(&project_yaml)),
)?;

let document_config = parse_yaml_annotated(
    &document_yaml,
    SourceInfo::original(doc_file_id, Range::from_text(&document_yaml)),
)?;

// 2. Merge configs
let merged_config = merge_format_metadata(&project_config, &document_config);

// 3. Validate against schema
let schema = get_frontmatter_schema();
let errors = validate_yaml(&merged_config, &schema, &source_context);

// 4. Report errors with correct source locations
for error in errors {
    let mapped_loc = error.source_info.map_offset(error.start, &source_context)?;
    let file = source_context.get_file(mapped_loc.file_id)?;

    eprintln!(
        "{}:{}:{}: {}",
        file.path,
        mapped_loc.location.row,
        mapped_loc.location.column,
        error.message
    );
}
```

### Example: Validation Error Tracing

**Scenario**:
```yaml
# _quarto.yml
format:
  html:
    theme: cosmo

# document.qmd
format:
  html:
    number-sections: "yes"  # ERROR: should be boolean
```

**Validation process**:
1. Parse both configs as AnnotatedParse with source tracking
2. Merge: `merged["format"]["html"]["number-sections"]` has `source_info` pointing to document.qmd:3
3. Validate: Schema expects boolean, finds string
4. Error creation: Uses `source_info` from the violating node
5. Error reporting: Maps to document.qmd:3:21 (correct location!)

**Error output**:
```
document.qmd:3:21: Expected boolean for 'number-sections', found string "yes"
```

## Performance Considerations

### Benchmarking Targets

- **Single merge**: < 1ms for typical config (50-100 keys)
- **Multi-layer merge** (5 layers): < 5ms
- **Large config** (500+ keys): < 50ms
- **Validation after merge**: < 10ms

### Optimization Strategies

1. **Lazy component reconstruction**: Only rebuild components when accessed
2. **Memoization**: Cache merged results for repeated merges (same inputs)
3. **Shallow cloning**: Use Rc/Arc for immutable subtrees
4. **Parallel merging**: Independent keys can be merged in parallel
5. **Incremental validation**: Only validate changed subtrees

### Memory Profile

**Typical config size**:
- AnnotatedParse tree: ~2-5 KB per config
- Merged config: ~5-10 KB (includes source info)
- Source context: ~1 KB per file

**Total memory for document render**:
- 5 config layers × 5 KB = 25 KB
- Merged result: 10 KB
- Source context: 5 KB
- **Total: ~40 KB per document** (acceptable)

## Migration Path

### Phase 1: Implement Core Merge (Week 1)

- [ ] Implement `merge_annotated_parse` for objects
- [ ] Implement array merging with concatenation
- [ ] Implement scalar override
- [ ] Unit tests for basic merging
- [ ] Verify source info preservation

### Phase 2: Customizer System (Week 1-2)

- [ ] Define `MergeCustomizer` trait
- [ ] Implement `merge_annotated_parse_with_customizer`
- [ ] Port TypeScript customizers:
  - [ ] Format metadata customizer
  - [ ] Project metadata customizer
  - [ ] Pandoc variant merge
  - [ ] Disableable array merge
- [ ] Integration tests with real configs

### Phase 3: Integration (Week 2)

- [ ] Update config loading to use AnnotatedParse
- [ ] Replace all TypeScript mergeConfigs calls
- [ ] Update validation to work with merged AnnotatedParse
- [ ] Update error reporting to use mapped locations
- [ ] End-to-end tests with multi-layer configs

### Phase 4: Optimization (Week 3)

- [ ] Profile merge performance
- [ ] Implement memoization
- [ ] Optimize hot paths
- [ ] Benchmark against targets
- [ ] Memory profiling and optimization

### Phase 5: LSP Integration (Week 3-4)

- [ ] Add caching for merged configs
- [ ] Serialize/deserialize merged AnnotatedParse
- [ ] Incremental validation on config change
- [ ] IDE diagnostics with source locations
- [ ] Performance testing in VS Code

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_merge_objects() {
    let base = parse_yaml_annotated("a: 1\nb: 2", source_info_1).unwrap();
    let override = parse_yaml_annotated("b: 3\nc: 4", source_info_2).unwrap();

    let merged = merge_annotated_parse(&base, &override);

    assert_eq!(get_field(&merged, "a"), Some(&YamlValue::Integer(1)));
    assert_eq!(get_field(&merged, "b"), Some(&YamlValue::Integer(3)));  // Overridden
    assert_eq!(get_field(&merged, "c"), Some(&YamlValue::Integer(4)));

    // Check source tracking
    assert_source(&merged, "a", source_info_1);
    assert_source(&merged, "b", source_info_2);  // From override
    assert_source(&merged, "c", source_info_2);
}

#[test]
fn test_merge_arrays() {
    let base = parse_yaml_annotated("items: [1, 2]", source_info_1).unwrap();
    let override = parse_yaml_annotated("items: [2, 3]", source_info_2).unwrap();

    let merged = merge_annotated_parse(&base, &override);

    let items = get_field(&merged, "items").unwrap();
    assert_eq!(items, &YamlValue::Array(vec![
        YamlValue::Integer(1),
        YamlValue::Integer(2),  // Deduplicated
        YamlValue::Integer(3),
    ]));
}

#[test]
fn test_deep_merge() {
    let base = parse_yaml_annotated("format:\n  html:\n    theme: cosmo", source_info_1).unwrap();
    let override = parse_yaml_annotated("format:\n  html:\n    toc: true", source_info_2).unwrap();

    let merged = merge_annotated_parse(&base, &override);

    let html = get_nested_field(&merged, &["format", "html"]).unwrap();
    assert_eq!(get_field(html, "theme"), Some(&YamlValue::String("cosmo".into())));
    assert_eq!(get_field(html, "toc"), Some(&YamlValue::Bool(true)));

    // Check source tracking
    assert_source_nested(&merged, &["format", "html", "theme"], source_info_1);
    assert_source_nested(&merged, &["format", "html", "toc"], source_info_2);
}
```

### Integration Tests

```rust
#[test]
fn test_multi_layer_merge() {
    // Simulate: default -> extension -> project -> directory -> document
    let default_config = load_default_format("html");
    let extension_config = load_extension_format("my-extension");
    let project_config = load_yaml_file("_quarto.yml");
    let directory_config = load_yaml_file("_metadata.yml");
    let document_config = load_yaml_file("document.qmd");

    let merged = merge_annotated_parse_all(&[
        default_config,
        extension_config,
        project_config,
        directory_config,
        document_config,
    ]);

    // Verify final values
    assert_eq!(get_nested(&merged, &["format", "html", "theme"]), Some("journal"));

    // Verify source tracking
    let theme_source = get_source_info_nested(&merged, &["format", "html", "theme"]);
    assert_eq!(theme_source.file_id, extension_file_id);
}

#[test]
fn test_validation_with_merged_config() {
    let project_config = parse_yaml("theme: cosmo\ntoc: true", project_file_id);
    let document_config = parse_yaml("number-sections: invalid", doc_file_id);

    let merged = merge_annotated_parse(&project_config, &document_config);

    let schema = get_html_format_schema();
    let errors = validate_yaml(&merged, &schema, &source_context);

    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].message, "Expected boolean for 'number-sections'");

    // Verify error points to correct source
    let error_loc = errors[0].source_info.map_offset(errors[0].start, &source_context).unwrap();
    assert_eq!(error_loc.file_id, doc_file_id);
    assert_eq!(error_loc.location.row, 1);
}
```

### Regression Tests

- Port all existing TypeScript mergeConfigs tests
- Test all special merge cases (variants, disableable arrays, etc.)
- Test error reporting accuracy (compare to TypeScript output)

## Open Questions

### Q1: How to handle SourceInfo for merged containers?

**Question**: When merging two objects, the merged object itself doesn't exist in any source file. What should its `source_info` be?

**Options**:
1. Use `SourceInfo::concat()` to represent "came from merging these two sources"
2. Use the override layer's source_info (most recent)
3. Use a special `SourceInfo::Merged { sources: Vec<SourceInfo> }` variant

**Recommendation**: Option 1 (SourceInfo::concat) - already supported by unified design

---

### Q2: How to handle components in merged AnnotatedParse?

**Question**: When merging objects, how do we construct the `components` vector? Components represent the structure of the original YAML.

**Options**:
1. Interleave components from both layers (preserves structure)
2. Build new components from merged result (loses some structure)
3. Keep separate component lists per layer (complex)

**Recommendation**: Option 1 - interleave, but track which layer each component came from via its source_info

---

### Q3: Performance of deep merging?

**Question**: Recursive merging can be slow for deeply nested configs. How to optimize?

**Options**:
1. Lazy evaluation (don't merge until accessed)
2. Memoization (cache merge results)
3. Shallow merging (only merge top levels, resolve deeper on demand)

**Recommendation**: Start with eager merging, profile, optimize as needed (likely memoization)

---

### Q4: Caching strategy for LSP?

**Question**: LSP needs to quickly re-validate configs on document edit. How to cache merged configs?

**Options**:
1. Cache merged AnnotatedParse to disk (with source context)
2. Cache individual layer AnnotatedParse, merge on demand
3. Incremental merge (only re-merge changed layers)

**Recommendation**: Option 3 (incremental) with Option 2 as fallback

---

### Q5: Handling circular includes?

**Question**: What if _metadata.yml includes another file that includes _metadata.yml?

**Options**:
1. Detect cycles and error
2. Detect cycles and skip
3. Allow limited depth (max 10 includes)

**Recommendation**: Option 1 (error on cycles) - TypeScript likely does this already

## Comparison: Strategies Summary

| Strategy | Source Tracking | Performance | Complexity | Integration | Serializable | Verdict |
|----------|----------------|-------------|------------|-------------|--------------|---------|
| **1. Eager (current)** | ❌ None | ✅ Fast | ✅ Simple | ✅ Easy | ✅ Yes | ❌ Not viable |
| **2. Lazy/Proxy** | ✅ Perfect | ⚠️ Slower | ❌ Complex | ⚠️ Difficult | ⚠️ Challenging | ⚠️ Not recommended |
| **3. Value Wrappers** | ✅ Good | ✅ Fast | ⚠️ Moderate | ⚠️ Reinvents wheel | ✅ Yes | ⚠️ Workable |
| **4. AnnotatedParse Merge** | ✅ Perfect | ✅ Fast | ⚠️ Moderate | ✅ Natural | ✅ Yes | ⭐ **Recommended** |

## Conclusion

**Recommendation**: ⭐ **Implement Strategy 4: AnnotatedParse Merge**

**Rationale**:
1. **Leverages existing infrastructure**: AnnotatedParse already exists and is used throughout the system
2. **Perfect source tracking**: Every value maintains its origin via SourceInfo
3. **Natural validation integration**: Validator already works with AnnotatedParse
4. **Serializable**: Enables disk caching for LSP performance
5. **Proven pattern**: Similar to how other compilers track source locations through transformations

**Key advantages over alternatives**:
- No need for proxy objects (simpler than Strategy 2)
- No need for custom wrapper types (simpler than Strategy 3)
- Reuses existing AnnotatedParse type (less code)
- Integrates seamlessly with YAML parsing and validation
- Supports all required merge semantics via customizer trait

**Implementation plan**: 6-8 weeks
- Week 1: Core merge implementation
- Week 2: Customizer system and special merge cases
- Week 2-3: Integration with config loading and validation
- Week 3: Performance optimization
- Week 4: LSP integration and caching

**Next steps**:
1. Review this design with team
2. Create prototype of core merge_annotated_parse
3. Test with real Quarto configs
4. Iterate based on performance and usability findings
