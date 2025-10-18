# YAML 1.2 Requirement

## Critical Constraint

**We CANNOT use `serde_yaml` until it supports YAML 1.2.**

## Background

### YAML Version Differences

- **YAML 1.1** (used by `yaml-rust` and `serde_yaml`): Older spec with ambiguous boolean parsing
  - `yes`, `no`, `on`, `off` are parsed as booleans
  - This breaks many real-world documents where `no` is meant to be a string

- **YAML 1.2** (used by `yaml-rust2` and `quarto-yaml`): Fixed ambiguities
  - Only `true`, `false` (and some case variants) are booleans
  - `yes`, `no`, `on`, `off` are strings by default
  - Much more predictable for users

### Why This Matters for Quarto

Quarto documents often contain YAML like:

```yaml
author:
  name: John Doe
  orcid: no  # Should be the string "no", not boolean false
```

With YAML 1.1 parsers, this would incorrectly parse `no` as `false`.

## Current State

- **quarto-yaml**: Uses `yaml-rust2` ✅ (YAML 1.2)
- **quarto-yaml-validation**: Uses `serde_yaml` ❌ (YAML 1.1) for Schema deserialization

## Problem

The current `Schema` deserialization in `quarto-yaml-validation/src/schema.rs` uses serde:

```rust
impl<'de> Deserialize<'de> for Schema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    // This uses serde_yaml, which only supports YAML 1.1
}
```

This means:
1. **User documents** are parsed with YAML 1.2 (correct)
2. **Schema files** are parsed with YAML 1.1 (incorrect)

This inconsistency is problematic because:
- Users expect consistent YAML parsing behavior
- Schema files may themselves contain ambiguous values like `no` in examples
- Quarto extensions will define their own schemas and expect YAML 1.2

## Solution

**Use `YamlWithSourceInfo` for loading schemas, not serde deserialization.**

Instead of:
```rust
// Current (WRONG - uses YAML 1.1)
let schema: Schema = serde_yaml::from_str(yaml_str)?;
```

Do:
```rust
// Correct (uses YAML 1.2)
let yaml = quarto_yaml::parse(yaml_str, Some(file_path))?;
let schema = Schema::from_yaml(&yaml)?;  // Manual conversion
```

Benefits:
1. ✅ Consistent YAML 1.2 parsing for both documents and schemas
2. ✅ Source location tracking for schema files (enables better error messages)
3. ✅ No dependency on `serde_yaml` (one less dependency)
4. ✅ Extensions can use the same infrastructure

Trade-offs:
- More manual code to convert `YamlWithSourceInfo` → `Schema`
- Cannot leverage serde's automatic deserialization
- But: More control over error messages and validation

## Implementation Plan

1. Remove `serde::Deserialize` implementation from `Schema` enum
2. Add `Schema::from_yaml(yaml: &YamlWithSourceInfo) -> Result<Schema, Error>` method
3. Add helper methods for parsing each schema type
4. Update tests to use `quarto_yaml::parse()` instead of `serde_yaml`
5. Add source location tracking to schema parsing errors

## Timeline

This should be done **before** implementing the `validate-yaml` binary, since it affects the fundamental architecture.

## Related Files

- `/crates/quarto-yaml-validation/src/schema.rs` - Schema deserialization (needs rewrite)
- `/claude-notes/yaml-schema-from-yaml-design.md` - Design document (needs revision)

## Future: serde_yaml YAML 1.2 Support

If `serde_yaml` ever adds YAML 1.2 support, we could:
1. Keep the `from_yaml()` approach for source tracking
2. Optionally add serde deserialization back as a convenience method
3. But `from_yaml()` should remain the primary API

## References

- yaml-rust2: https://docs.rs/yaml-rust2/ (YAML 1.2)
- serde_yaml: https://docs.rs/serde_yaml/ (YAML 1.1)
- YAML 1.2 spec: https://yaml.org/spec/1.2/spec.html
