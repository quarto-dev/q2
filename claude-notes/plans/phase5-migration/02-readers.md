# Subplan 02: Pampa Readers

**Order:** 2nd
**Complexity:** MEDIUM
**Dependencies:** 01-core-types

## Files

| File | Usage | Changes Required |
|------|-------|------------------|
| `pampa/src/readers/qmd.rs` | MODERATE | Already uses ConfigValue (Phase 4)! |
| `pampa/src/readers/json.rs` | HEAVY | Return ConfigValue instead of MetaValueWithSourceInfo |

## Detailed Changes

### 1. `readers/qmd.rs` - Already Migrated!

**Good news:** Phase 4 already changed this file to use ConfigValue internally:
```rust
// Phase 4: Use rawblock_to_config_value then convert to MetaValueWithSourceInfo
let config_value = rawblock_to_config_value(&rb, &mut meta_diagnostics);
let meta_with_source = config_value_to_meta(&config_value);
```

**Change needed:** Remove the final conversion, return `ConfigValue` directly:
```rust
// Phase 5: Return ConfigValue directly
let config_value = rawblock_to_config_value(&rb, &mut meta_diagnostics);
outer_metadata = config_value;  // No conversion needed!
```

### 2. `readers/json.rs` - Major Rework

**Current functions:**
```rust
pub fn read_meta(json: &JsonValue) -> MetaValueWithSourceInfo
pub fn read_meta_value_with_source_info(json: &JsonValue) -> MetaValueWithSourceInfo
```

**Target functions:**
```rust
pub fn read_meta(json: &JsonValue) -> ConfigValue
pub fn read_meta_value(json: &JsonValue) -> ConfigValue  // Renamed
```

**Mapping changes:**

| JSON Type | Old Return | New Return |
|-----------|------------|------------|
| `"MetaString"` | `MetaValueWithSourceInfo::MetaString` | `ConfigValue { value: ConfigValueKind::Scalar(Yaml::String) }` |
| `"MetaBool"` | `MetaValueWithSourceInfo::MetaBool` | `ConfigValue { value: ConfigValueKind::Scalar(Yaml::Boolean) }` |
| `"MetaInlines"` | `MetaValueWithSourceInfo::MetaInlines` | `ConfigValue { value: ConfigValueKind::PandocInlines }` |
| `"MetaBlocks"` | `MetaValueWithSourceInfo::MetaBlocks` | `ConfigValue { value: ConfigValueKind::PandocBlocks }` |
| `"MetaList"` | `MetaValueWithSourceInfo::MetaList` | `ConfigValue { value: ConfigValueKind::Array }` |
| `"MetaMap"` | `MetaValueWithSourceInfo::MetaMap` | `ConfigValue { value: ConfigValueKind::Map }` |

**Entry handling:**
```rust
// OLD
MetaMapEntry { key, key_source, value }

// NEW
ConfigMapEntry { key, key_source, value }
```

**Key consideration:** JSON format uses `"MetaString"`, `"MetaMap"`, etc. as type discriminators. We need to:
- Keep reading the old format (backward compatibility)
- Potentially write new format later (or keep old for Pandoc compat)

## Migration Steps

```bash
# Step 1: Update qmd.rs - remove final conversion
# Already almost done from Phase 4

# Step 2: Update json.rs - change return types and construction
# This is the bigger change

# Step 3: Verify reading still works
cargo nextest run -p pampa --test test_json_roundtrip

# Step 4: Run broader tests
cargo nextest run -p pampa
```

## Completion Criteria

- [ ] `readers/qmd.rs` returns `ConfigValue` for `Pandoc.meta`
- [ ] `readers/json.rs` returns `ConfigValue` for metadata
- [ ] JSON roundtrip tests pass
- [ ] All pampa reader tests pass

## Notes

- JSON backward compatibility is important - users have existing JSON files
- The type discriminator strings in JSON (`"MetaString"`) are part of Pandoc's format
- Consider whether to update JSON format or maintain compatibility
