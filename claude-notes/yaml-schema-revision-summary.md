# YAML Schema Design Revision Summary

**Date**: 2025-10-13
**Context**: Design revision for loading Quarto schemas from YAML files

## What Changed

### Original Design (Discarded)
- **Approach**: Use serde deserialization with custom `Deserialize` impl
- **Parser**: serde_yaml (YAML 1.1 via yaml-rust)
- **Source Tracking**: None
- **Status**: Partially implemented in quarto-yaml-validation/src/schema.rs (lines 269-572)

### Revised Design (Current)
- **Approach**: Manual parsing from `YamlWithSourceInfo`
- **Parser**: quarto-yaml (YAML 1.2 via yaml-rust2)
- **Source Tracking**: Full source location for all schema elements
- **Status**: Design complete, implementation pending

## Why The Change Was Necessary

### 1. YAML 1.2 Compatibility
**Problem**: User documents are parsed with YAML 1.2, but schemas would be parsed with YAML 1.1.

**Impact**: Inconsistent behavior. Example:
```yaml
author:
  orcid: no  # YAML 1.1: boolean false, YAML 1.2: string "no"
```

**Solution**: Use same parser (yaml-rust2 via quarto-yaml) for both documents and schemas.

### 2. Quarto Extensions Support
**Requirement**: Future Quarto extensions must be able to declare their own schemas using exactly the same infrastructure as core Quarto.

**Problem**: If we used serde_yaml, extensions would be stuck with YAML 1.1 limitations.

**Solution**: Extensions use quarto-yaml-validation with YAML 1.2 support out of the box.

### 3. Source Location Tracking
**Need**: Schema validation errors should point to exact locations in schema files.

**Problem**: Serde deserialization doesn't preserve source locations.

**Solution**: YamlWithSourceInfo provides source location for every element automatically.

## Architectural Comparison

| Aspect | Old (serde) | New (YamlWithSourceInfo) |
|--------|-------------|---------------------------|
| **Parsing** | `impl Deserialize for Schema` | `impl Schema { fn from_yaml() }` |
| **YAML Version** | 1.1 (yaml-rust) | 1.2 (yaml-rust2) |
| **Source Tracking** | No | Yes (SourceInfo for every element) |
| **Error Messages** | Generic serde errors | Custom errors with locations |
| **Extensibility** | Limited by serde | Full control |
| **Dependency** | serde_yaml | quarto-yaml |
| **Extension Support** | ❌ YAML 1.1 only | ✅ YAML 1.2 |

## Implementation Changes Required

### Files to Modify

**1. `/crates/quarto-yaml-validation/src/schema.rs`**
- Remove `impl<'de> Deserialize<'de> for Schema` (lines 269-572)
- Remove `SchemaVisitor` struct
- Remove serde deserialization tests (lines 608-919)
- Add dependency on quarto-yaml
- Implement `Schema::from_yaml(yaml: &YamlWithSourceInfo) -> Result<Schema, SchemaError>`
- Implement all helper methods and type-specific parsers

**2. `/crates/quarto-yaml-validation/src/error.rs` (new file)**
- Create `SchemaError` enum with location tracking

**3. `/crates/quarto-yaml-validation/src/schema_file.rs` (new file)**
- Implement `SchemaField` struct
- Implement `Description` enum
- Implement `load_schema_file()` using quarto_yaml::parse_file()

**4. `/crates/quarto-yaml-validation/Cargo.toml`**
- Add dependency: `quarto-yaml = { workspace = true }`

### Breaking Changes
- ❌ `serde_yaml::from_str()` no longer works for Schema
- ✅ New API: `Schema::from_yaml(&yaml_with_source_info)?`

## Timeline Impact

**Original estimate**: 2-3 weeks
**Revised estimate**: 2-3 weeks (same)

**Reason**: More manual code to write, but we save time not fighting with serde's limitations. Net neutral.

## Documentation Updates

**New files created:**
1. `/crates/quarto-yaml/YAML-1.2-REQUIREMENT.md` - Explains why we can't use serde_yaml
2. `/crates/quarto-yaml-validation/YAML-1.2-REQUIREMENT.md` - Impact on this crate
3. `/claude-notes/yaml-schema-from-yaml-design.md` - Updated with new design
4. `/claude-notes/yaml-schema-revision-summary.md` - This file

**Updated files:**
1. `/claude-notes/00-INDEX.md` - Added YAML and Validation section with links

## Next Steps

1. ✅ Design revision complete (this document)
2. ⏳ Get user approval on revised design
3. ⏳ Implement Step 0: Remove serde deserialization (Day 1)
4. ⏳ Implement Step 1: Schema::from_yaml() method (Days 1-3)
5. ⏳ Implement Step 2: SchemaField and file loading (Days 4-5)
6. ⏳ Implement Step 3: Schema registry (Week 2)
7. ⏳ Implement Step 4: Update validation (Week 2)
8. ⏳ Implement Step 5: validate-yaml binary (Week 2)
9. ⏳ Integration testing (Week 3)

## Key Quotes from Discussion

> "I still think we'll need to change, because we eventually will want to load these YAML schemas with source annotations as well."
> — User feedback that triggered this revision

> "One aspect of the design we haven't discussed is that we want future Quarto extensions to declare their own schemas. So Quarto's schemas need to work with exactly the same infrastructure."
> — User clarification on extensions support

> "Please update your notes accordingly, and write in the claude-notes/ directory of both quarto-yaml and quarto-yaml-validation that we cannot use serde_yaml until it can read Yaml 1.2."
> — User directive for documentation

## Benefits Summary

1. ✅ **YAML 1.2 Compatibility** - No ambiguous `yes`/`no` parsing
2. ✅ **Source Tracking** - Better error messages pointing to schema files
3. ✅ **Extensions Support** - Same infrastructure for core and extensions
4. ✅ **Full Control** - Not limited by serde's capabilities
5. ✅ **Consistency** - Same parser for documents and schemas
6. ✅ **Future-Proof** - No waiting for serde_yaml YAML 1.2 support

## References

- Design document: `/claude-notes/yaml-schema-from-yaml-design.md`
- YAML 1.2 requirement (quarto-yaml): `/crates/quarto-yaml/YAML-1.2-REQUIREMENT.md`
- YAML 1.2 requirement (validation): `/crates/quarto-yaml-validation/YAML-1.2-REQUIREMENT.md`
- Current implementation: `/crates/quarto-yaml-validation/src/schema.rs`
