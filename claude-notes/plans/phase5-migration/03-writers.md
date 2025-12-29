# Subplan 03: Pampa Writers

**Order:** 3rd
**Complexity:** MEDIUM
**Dependencies:** 01-core-types, 02-readers

## Files

| File | Usage | Changes Required |
|------|-------|------------------|
| `pampa/src/writers/json.rs` | HEAVY | Accept ConfigValue, serialize appropriately |
| `pampa/src/writers/qmd.rs` | MODERATE | Accept ConfigValue for YAML output |
| `pampa/src/writers/html.rs` | LIGHT | Minor metadata access changes |

## Detailed Changes

### 1. `writers/json.rs` - Heavy Rework

**Current functions:**
```rust
pub fn write_meta_value_with_source_info(meta: &MetaValueWithSourceInfo, ...) -> ...
pub fn write_meta(meta: &MetaValueWithSourceInfo, ...) -> ...
```

**Target functions:**
```rust
pub fn write_meta_value(config: &ConfigValue, ...) -> ...  // Renamed
pub fn write_meta(config: &ConfigValue, ...) -> ...
```

**Serialization mapping:**

| ConfigValueKind | JSON Type | Notes |
|-----------------|-----------|-------|
| `Scalar(Yaml::String)` | `"MetaString"` | For Pandoc compat |
| `Scalar(Yaml::Boolean)` | `"MetaBool"` | |
| `Scalar(Yaml::Integer)` | `"MetaString"` | Convert to string |
| `Scalar(Yaml::Real)` | `"MetaString"` | Convert to string |
| `Scalar(Yaml::Null)` | `"MetaString"` | Empty string? Or skip? |
| `PandocInlines` | `"MetaInlines"` | |
| `PandocBlocks` | `"MetaBlocks"` | |
| `Array` | `"MetaList"` | |
| `Map` | `"MetaMap"` | |
| `Path(s)` | `"MetaInlines"` | Wrap in Str |
| `Glob(s)` | `"MetaInlines"` | Wrap in Span |
| `Expr(s)` | `"MetaInlines"` | Wrap in Span |

**Important:** Keep JSON output format compatible with Pandoc's expectations.

### 2. `writers/qmd.rs` - Moderate Rework

**Current function:**
```rust
pub fn meta_value_with_source_info_to_yaml(meta: &MetaValueWithSourceInfo) -> ...
```

**Target function:**
```rust
pub fn config_value_to_yaml(config: &ConfigValue) -> ...  // Renamed
```

**YAML output mapping:**

| ConfigValueKind | YAML Output |
|-----------------|-------------|
| `Scalar(Yaml::String(s))` | `s` (plain) |
| `Scalar(Yaml::Boolean(b))` | `true`/`false` |
| `PandocInlines` | Rendered as string |
| `Map` | YAML mapping |
| `Array` | YAML sequence |
| `Path(s)` | `!path s` |
| `Glob(s)` | `!glob s` |
| `Expr(s)` | `!expr s` |

### 3. `writers/html.rs` - Light Changes

Check for any direct metadata access and update patterns:
```rust
// OLD
match &pandoc.meta {
    MetaValueWithSourceInfo::MetaMap { entries, .. } => ...
}

// NEW
match &pandoc.meta.value {
    ConfigValueKind::Map(entries) => ...
}
```

## Migration Steps

```bash
# Step 1: Update json.rs
# Biggest change - many match arms

# Step 2: Update qmd.rs
# Similar pattern changes

# Step 3: Update html.rs
# Minor changes

# Step 4: Run roundtrip tests
cargo nextest run -p pampa --test test_json_roundtrip

# Step 5: Run QMD writer tests
cargo nextest run -p pampa roundtrip

# Step 6: Full test suite
cargo nextest run -p pampa
```

## Completion Criteria

- [ ] `writers/json.rs` accepts `ConfigValue` and produces valid JSON
- [ ] `writers/qmd.rs` accepts `ConfigValue` and produces valid YAML
- [ ] `writers/html.rs` compiles and works
- [ ] JSON roundtrip tests pass
- [ ] QMD roundtrip tests pass

## Notes

- JSON format compatibility with Pandoc is critical
- The type discriminators (`"MetaString"`, etc.) must stay for Pandoc compat
- Consider adding JSON format tests that verify Pandoc compatibility
