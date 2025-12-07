# Error Handling Strategy for Config Merging

**Date**: 2025-12-07
**Issue**: k-os6h (child of k-zvzm)
**Status**: Design proposal
**Parent Plan**: `claude-notes/plans/2025-12-07-config-merging-design.md`

## Problem Statement

The config merging system needs to handle various error conditions gracefully. The parent design document identified four areas needing definition:

1. YAML parsing failures in one layer
2. Syntactically invalid tags
3. Circular includes
4. Memory/stack overflow from deeply nested configs

This document expands on each concern, explores design options, and proposes solutions.

## Context: Existing Error Handling Patterns

### DiagnosticCollector Pattern

The codebase uses `DiagnosticCollector` to accumulate errors and warnings without immediately failing:

```rust
pub struct DiagnosticCollector {
    diagnostics: Vec<DiagnosticMessage>,
}

impl DiagnosticCollector {
    pub fn add(&mut self, diagnostic: DiagnosticMessage) { ... }
    pub fn has_errors(&self) -> bool { ... }  // Warnings don't count
    pub fn warn_at(&mut self, message: impl Into<String>, location: SourceInfo) { ... }
    pub fn error_at(&mut self, message: impl Into<String>, location: SourceInfo) { ... }
}
```

This pattern enables:
- Collecting multiple errors in one pass
- Distinguishing warnings (continue) from errors (may need to stop)
- Deferred rendering (text or JSON)

### Error Codes

Error codes follow the `Q-X-Y` pattern:
- `X` = subsystem (0=internal, 1=yaml, 2=markdown, etc.)
- `Y` = specific error within subsystem

For config merging, we'll use subsystem 1 (yaml) since config merging is YAML-adjacent.

### Existing Related Codes

From `quarto-yaml-validation`:
- `Q-1-10` through `Q-1-20`: Validation errors
- `Q-1-99`: Generic validation error

From the parent design document (D5):
- `Q-1-21`: Unknown YAML tag component
- `Q-1-22`: Completely unrecognized YAML tag

---

## Error Condition 1: YAML Parsing Failures in One Layer

### The Problem

When merging multiple config layers, one layer might fail to parse:

```
project/_quarto.yml      # Valid YAML
project/doc/_metadata.yml # Invalid YAML (syntax error)
project/doc/paper.qmd    # Valid YAML in frontmatter
```

Should the entire merge fail, or should we skip the broken layer?

### Design Options

#### Option A: Fail Fast

Stop immediately when any layer fails to parse.

```rust
pub fn merge_configs(layer_sources: &[&str]) -> Result<MergedConfig, ConfigError> {
    let mut layers = Vec::new();
    for source in layer_sources {
        let config = parse_config(source)?;  // Early return on error
        layers.push(config);
    }
    Ok(MergedConfig::new(layers))
}
```

**Pros:**
- Simple, predictable behavior
- User gets clear feedback: "fix this file before continuing"
- No risk of proceeding with incomplete config

**Cons:**
- Can't show multiple parse errors at once
- May block user from seeing other issues

#### Option B: Skip Layer with Warning

Skip unparseable layers and continue with remaining layers.

```rust
pub fn merge_configs(
    layer_sources: &[&str],
    diagnostics: &mut DiagnosticCollector,
) -> MergedConfig {
    let mut layers = Vec::new();
    for source in layer_sources {
        match parse_config(source) {
            Ok(config) => layers.push(config),
            Err(e) => {
                diagnostics.warn_at(
                    format!("Skipping config layer due to parse error: {}", e),
                    e.location(),
                );
            }
        }
    }
    MergedConfig::new(layers)
}
```

**Pros:**
- Graceful degradation
- Can show all parse errors at once
- Useful for LSP/IDE scenarios (show as many errors as possible)

**Cons:**
- User might not notice a layer was skipped
- Resulting config might be incomplete in subtle ways
- "Warning" might be too weak for a parse failure

#### Option C: Collect Errors, Then Decide

Collect all parse errors, but don't produce a result if any layer failed.

```rust
pub fn merge_configs(
    layer_sources: &[&str],
    diagnostics: &mut DiagnosticCollector,
) -> Result<MergedConfig, ()> {
    let mut layers = Vec::new();
    let mut had_errors = false;

    for source in layer_sources {
        match parse_config(source) {
            Ok(config) => layers.push(config),
            Err(e) => {
                diagnostics.error_at(
                    format!("Failed to parse config: {}", e),
                    e.location(),
                );
                had_errors = true;
            }
        }
    }

    if had_errors {
        Err(())  // Errors already in diagnostics
    } else {
        Ok(MergedConfig::new(layers))
    }
}
```

**Pros:**
- Shows all parse errors at once
- Doesn't proceed with incomplete data
- Caller can decide what to do (has access to diagnostics)

**Cons:**
- More complex API
- Caller must handle the error case

### Recommendation: Option C (Collect Errors, Then Decide)

This matches the validation pattern in `quarto-yaml-validation` and gives the best user experience:
- All errors shown at once
- No silent failures
- Caller has control

**Proposed error code**: `Q-1-23` - "Config layer parse failure"

---

## Error Condition 2: Syntactically Invalid Tags

### The Problem

Tags can be malformed in several ways:

```yaml
# Valid
title: !prefer "My Title"
items: !concat,path ["./a", "./b"]

# Invalid: empty component
title: !prefer, "oops"      # trailing comma
items: !,md "what"          # leading comma

# Invalid: whitespace
title: !prefer ,md "space"  # space before comma

# Already covered by D5: unknown components
title: !prefre "typo"       # Q-1-21
title: !custom "unknown"    # Q-1-22
```

### Sub-cases

#### 2a: Empty tag components

Input: `!prefer,` or `!,md` or `!prefer,,md`

**Options:**
1. **Error**: Reject as malformed
2. **Warning + ignore**: Strip empty components, continue
3. **Silent ignore**: Strip empty components silently

**Recommendation**: Error (Q-1-24). Be strict—everything not explicitly allowed is an error. The user should fix their syntax.

#### 2b: Whitespace in tags

Input: `!prefer ,md` or `! prefer`

YAML parsers typically handle this at the lexer level, so we may not even see this case. If we do:

**Recommendation**: Error (Q-1-25). This is likely a user mistake and we shouldn't guess at intent.

#### 2c: Invalid tag characters

Input: `!prefer@md` or `!prefer/md`

**Recommendation**: Error (Q-1-26). Unknown separator character, can't parse.

### Proposed Tag Parsing Error Codes

| Code | Description | Severity |
|------|-------------|----------|
| Q-1-21 | Unknown tag component | Warning |
| Q-1-22 | Unrecognized tag | Warning |
| Q-1-24 | Empty tag component | Error |
| Q-1-25 | Whitespace in tag | Error |
| Q-1-26 | Invalid tag character | Error |
| Q-1-28 | Conflicting merge operations | Error |

### Implementation Sketch

```rust
fn parse_tag(
    tag_str: &str,
    tag_source: &SourceInfo,
    diagnostics: &mut DiagnosticCollector,
) -> Result<ParsedTag, ()> {
    let mut result = ParsedTag::default();

    // Check for invalid characters (only alphanumeric and comma allowed)
    if tag_str.contains(|c: char| !c.is_alphanumeric() && c != ',') {
        diagnostics.error_at(
            format!("Invalid character in tag '!{}' (Q-1-26)", tag_str),
            tag_source.clone(),
        );
        return Err(());
    }

    for (i, component) in tag_str.split(',').enumerate() {
        // Empty component check (strict: this is an error)
        if component.is_empty() {
            diagnostics.error_at(
                format!("Empty component in tag '!{}' (Q-1-24)", tag_str),
                tag_source.clone(),
            );
            return Err(());
        }

        // Whitespace check
        if component != component.trim() {
            diagnostics.error_at(
                format!("Whitespace in tag component '!{}' (Q-1-25)", tag_str),
                tag_source.clone(),
            );
            return Err(());
        }

        match component {
            "prefer" => {
                if result.merge_op.is_some() {
                    diagnostics.error_at(
                        format!("Conflicting merge operations in tag '!{}' (Q-1-28)", tag_str),
                        tag_source.clone(),
                    );
                    return Err(());
                }
                result.merge_op = Some(MergeOp::Prefer);
            }
            "concat" => {
                if result.merge_op.is_some() {
                    diagnostics.error_at(
                        format!("Conflicting merge operations in tag '!{}' (Q-1-28)", tag_str),
                        tag_source.clone(),
                    );
                    return Err(());
                }
                result.merge_op = Some(MergeOp::Concat);
            }
            "md" => result.interpretation = Some(Interpretation::Markdown),
            "str" => result.interpretation = Some(Interpretation::PlainString),
            "path" => result.interpretation = Some(Interpretation::Path),
            "glob" => result.interpretation = Some(Interpretation::Glob),
            "expr" => result.interpretation = Some(Interpretation::Expr),
            unknown => {
                // Unknown components are warnings, not errors
                diagnostics.warn_at(
                    format!("Unknown tag component '{}' in '!{}' (Q-1-21)", unknown, tag_str),
                    tag_source.clone(),
                );
            }
        }
    }

    Ok(result)
}
```

---

## Error Condition 3: Circular Includes

### The Problem

Config files can include other config files, potentially creating cycles:

```
# project/_quarto.yml
metadata-files:
  - _shared.yml

# project/_shared.yml
metadata-files:
  - _quarto.yml  # Circular!
```

### Scope Clarification

Per the Non-Goals section of the parent design:
> **Circular include detection**: Detecting and handling circular `_metadata.yml` includes is a project-level concern, not a merge-level concern.

The `quarto-config` crate handles *merging* of already-loaded `ConfigValue` trees. The *loading* of config files (including resolving `metadata-files` references) happens at a higher level.

### Where This Should Be Handled

```
┌─────────────────────────────────────────────────┐
│  Project Layer (quarto-core or similar)         │
│  - Discovers config files                       │
│  - Tracks include graph                         │
│  - Detects cycles                               │◄── Circular include detection HERE
│  - Loads ConfigValue trees                      │
└─────────────────────────────────────────────────┘
                        │
                        │ Vec<&ConfigValue>
                        ▼
┌─────────────────────────────────────────────────┐
│  Config Layer (quarto-config)                   │
│  - Merges ConfigValue trees                     │
│  - Applies !prefer/!concat semantics            │◄── This crate
│  - Preserves source locations                   │
└─────────────────────────────────────────────────┘
```

### Recommendation

**Document the boundary**: The `quarto-config` crate does NOT handle circular includes. It receives already-loaded `ConfigValue` references and merges them.

The caller (project layer) is responsible for:
1. Tracking which files have been loaded
2. Detecting cycles in the include graph
3. Emitting appropriate errors (e.g., `Q-7-XX` for CLI/project errors)

**No error codes needed in quarto-config** for circular includes.

---

## Error Condition 4: Memory/Stack Overflow from Deeply Nested Configs

### The Problem

Malicious or buggy configs could cause resource exhaustion:

```yaml
# Deeply nested structure
a:
  b:
    c:
      d:
        e:
          f:
            # ... 1000 levels deep
```

Or via array concatenation:
```yaml
# Layer 1
items: [1, 2, 3, ..., 1000000]

# Layer 2
items: !concat [more, items, ...]
```

### Sub-cases

#### 4a: Deep nesting (stack overflow risk)

Navigation through `MergedConfig` is iterative (path-based), not recursive, so deep nesting shouldn't cause stack overflow in the merge layer.

However, `materialize()` is recursive. With very deep nesting (thousands of levels), this could overflow the stack.

**Mitigation options:**
1. **Depth limit**: Track depth during materialization, error if exceeded
2. **Iterative materialization**: Rewrite to use explicit stack
3. **Accept the risk**: Very deep configs are rare; let it crash

**Recommendation**: Option 1 (depth limit). Add a configurable depth limit (default: 256 levels) with a clear error message.

**Proposed error code**: `Q-1-27` - "Config nesting too deep"

```rust
const MAX_NESTING_DEPTH: usize = 256;

impl<'a> MergedCursor<'a> {
    fn materialize_with_depth(&self, depth: usize) -> Result<ConfigValue, ConfigError> {
        if depth > MAX_NESTING_DEPTH {
            return Err(ConfigError::NestingTooDeep {
                max_depth: MAX_NESTING_DEPTH,
                path: self.path.clone(),
            });
        }

        // ... recursive calls pass depth + 1
    }
}
```

#### 4b: Large arrays (memory exhaustion)

Array concatenation could create very large arrays:

```yaml
# If each layer adds 1M items...
items: !concat [...]  # Layer 1: 1M items
items: !concat [...]  # Layer 2: 2M items total
items: !concat [...]  # Layer 3: 3M items total
```

**Mitigation options:**
1. **Item count limit**: Error if merged array exceeds N items
2. **Memory limit**: Track approximate memory usage
3. **Accept the risk**: Let the OS handle memory limits

**Recommendation**: Option 3 (accept the risk) for now. Large arrays are a legitimate use case, and OS-level memory limits provide a safety net. We can add limits later if abuse becomes a problem.

#### 4c: Large maps (memory exhaustion)

Similar concern for maps with many keys.

**Recommendation**: Same as arrays—accept the risk for now.

### Summary of Resource Limits

| Resource | Limit | Error Code | Default |
|----------|-------|------------|---------|
| Nesting depth | Yes | Q-1-27 | 256 |
| Array items | No | - | - |
| Map keys | No | - | - |
| Total memory | No | - | - |

---

## Error Codes Summary

| Code | Description | Severity | Condition |
|------|-------------|----------|-----------|
| Q-1-21 | Unknown tag component | Warning | `!prefre` (typo) |
| Q-1-22 | Unrecognized tag | Warning | `!custom` (unknown) |
| Q-1-23 | Config layer parse failure | Error | YAML syntax error in layer |
| Q-1-24 | Empty tag component | Error | `!prefer,` or `!,md` |
| Q-1-25 | Whitespace in tag | Error | `!prefer ,md` |
| Q-1-26 | Invalid tag character | Error | `!prefer@md` |
| Q-1-27 | Config nesting too deep | Error | >256 levels |
| Q-1-28 | Conflicting merge operations | Error | `!prefer,concat` |

---

## API Design

### MergeConfig Construction

```rust
/// Result of attempting to merge config layers
pub struct MergeResult<'a> {
    /// The merged config (may be partial if some layers failed)
    pub config: Option<MergedConfig<'a>>,
    /// Diagnostics collected during merging
    pub diagnostics: Vec<DiagnosticMessage>,
}

impl<'a> MergedConfig<'a> {
    /// Merge config layers, collecting diagnostics
    ///
    /// Returns MergeResult which contains:
    /// - `config`: Some if all layers parsed successfully, None if any failed
    /// - `diagnostics`: All warnings and errors encountered
    pub fn merge_with_diagnostics(
        layers: Vec<(&'a ConfigValue, &SourceInfo)>,
        diagnostics: &mut DiagnosticCollector,
    ) -> Option<MergedConfig<'a>> {
        // ... implementation
    }
}
```

### Tag Parsing

```rust
/// Parse result for a YAML tag
pub struct ParsedTag {
    pub merge_op: Option<MergeOp>,
    pub interpretation: Option<Interpretation>,
    /// True if any errors occurred (not just warnings)
    pub had_errors: bool,
}

/// Parse a tag string, collecting diagnostics
///
/// Returns ParsedTag with whatever could be parsed.
/// Check `had_errors` to determine if the tag should be rejected.
pub fn parse_tag(
    tag_str: &str,
    tag_source: &SourceInfo,
    diagnostics: &mut DiagnosticCollector,
) -> ParsedTag {
    // ... implementation
}
```

### Materialization

```rust
/// Options for materialization
pub struct MaterializeOptions {
    /// Maximum nesting depth (default: 256)
    pub max_depth: usize,
}

impl Default for MaterializeOptions {
    fn default() -> Self {
        Self { max_depth: 256 }
    }
}

impl<'a> MergedConfig<'a> {
    /// Materialize with options
    pub fn materialize_with_options(
        &self,
        options: &MaterializeOptions,
    ) -> Result<ConfigValue, ConfigError> {
        // ... implementation
    }

    /// Materialize with default options
    pub fn materialize(&self) -> Result<ConfigValue, ConfigError> {
        self.materialize_with_options(&MaterializeOptions::default())
    }
}
```

---

## Resolved Questions

The following questions were discussed and resolved:

### Q1: Should tag errors be recoverable?

**Decision**: Be strict. Everything not explicitly allowed is an error. Only unknown tag *components* (like `!prefre` typo) are warnings; all syntax issues (empty components, whitespace, invalid chars, conflicting ops) are errors.

### Q2: Should we support "strict mode"?

**Decision**: Not at this layer. The `quarto-config` crate collects diagnostics; the caller (pampa or other libraries) decides how to handle them based on context (e.g., treating warnings as errors in CI).

### Q3: How should we handle conflicting merge ops?

**Decision**: Error (Q-1-28). If a tag contains both `!prefer` and `!concat`, it's rejected as invalid. The user's intent is unclear.

---

## Implementation Plan

1. **Add error codes to error_catalog.json**:
   - Q-1-23 through Q-1-28

2. **Implement tag parsing with diagnostics**:
   - Update `parse_tag()` to take `DiagnosticCollector`
   - Handle all malformed tag cases

3. **Implement layer merge with diagnostics**:
   - Update `MergedConfig::new()` to collect parse errors
   - Return `Option` or `Result` based on error presence

4. **Implement depth-limited materialization**:
   - Add `MaterializeOptions` struct
   - Track depth during recursive materialization
   - Error on depth exceeded

5. **Write tests** for:
   - Each error condition
   - Edge cases (empty tags, whitespace, etc.)
   - Depth limit enforcement

---

## Relationship to Parent Design

This document expands on OQ2 from the parent design:

- **Clarified**: Circular includes are out of scope (project layer concern)
- **Added**: Specific error codes for tag parsing issues
- **Added**: Depth limit for materialization
- **Added**: API design for diagnostic collection
- **Deferred**: Memory limits for arrays/maps (accept OS limits for now)
