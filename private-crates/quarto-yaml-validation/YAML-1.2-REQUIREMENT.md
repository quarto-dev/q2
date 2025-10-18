# YAML 1.2 Requirement

## Critical Constraint

**We CANNOT use `serde_yaml` until it supports YAML 1.2.**

See `/crates/quarto-yaml/YAML-1.2-REQUIREMENT.md` for full background.

## Impact on This Crate

The `Schema` enum in `src/schema.rs` currently implements `serde::Deserialize`, which uses `serde_yaml` under the hood. This is **incorrect** because:

1. User documents are parsed with YAML 1.2 (via `quarto-yaml`)
2. Schema files are parsed with YAML 1.1 (via `serde_yaml`)
3. This inconsistency breaks user expectations

## Current Status

**TEMPORARY**: The current serde implementation is acceptable for initial development and testing, but must be replaced before production use.

The implementation includes this comment (line 264-267):

```rust
// Note: This uses serde_yaml which supports YAML 1.1 (via yaml-rust).
// For user YAML documents, we use yaml-rust2 (YAML 1.2) via quarto-yaml.
// This is acceptable because schema definitions are simpler and don't
// typically use YAML 1.2-specific features. User documents get full YAML 1.2 support.
```

**This comment is now outdated** - we need YAML 1.2 for schemas too.

## Required Changes

Replace serde deserialization with manual parsing from `YamlWithSourceInfo`:

**Before (current)**:
```rust
impl<'de> Deserialize<'de> for Schema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(SchemaVisitor)
    }
}
```

**After (required)**:
```rust
impl Schema {
    pub fn from_yaml(yaml: &YamlWithSourceInfo) -> Result<Schema, Error> {
        // Manual parsing from YamlWithSourceInfo
        // This uses yaml-rust2 (YAML 1.2) via quarto-yaml
    }
}
```

## Benefits of This Approach

1. ✅ Consistent YAML 1.2 parsing
2. ✅ Source location tracking for better error messages
3. ✅ No serde_yaml dependency
4. ✅ Extensions can use same infrastructure

## Quarto Extensions

One design goal is that **Quarto extensions can declare their own schemas** using the same infrastructure as core Quarto. This means:

- Extensions define schemas in YAML files
- Extensions use `quarto-yaml-validation` to validate their documents
- Everything uses YAML 1.2 consistently

If we used `serde_yaml`, extensions would be stuck with YAML 1.1 limitations.

## Implementation Priority

This change should happen **before** implementing the `validate-yaml` binary, as it affects the fundamental architecture.

See `/claude-notes/yaml-schema-from-yaml-design.md` for the revised implementation plan.
