# Subplan 04: Pampa Internal Modules

**Order:** 4th
**Complexity:** HIGH (most files, varied patterns)
**Dependencies:** 01-core-types, 02-readers, 03-writers

## Files

| File | Usage | Priority |
|------|-------|----------|
| `pampa/src/pandoc/meta.rs` | HEAVY | 1 - Core parsing |
| `pampa/src/template/config_merge.rs` | HEAVY | 2 - Already has conversions |
| `pampa/src/template/context.rs` | MODERATE | 3 |
| `pampa/src/template/render.rs` | LIGHT | 4 |
| `pampa/src/lua/readwrite.rs` | MODERATE | 5 - Lua compat |
| `pampa/src/lua/filter.rs` | MODERATE | 6 |
| `pampa/src/citeproc_filter.rs` | HEAVY | 7 - 35+ match arms |
| `pampa/src/filters.rs` | LIGHT | 8 |
| `pampa/src/json_filter.rs` | LIGHT | 9 |
| `pampa/src/pandoc/treesitter_utils/document.rs` | LIGHT | 10 |

## Detailed Changes

### 1. `pandoc/meta.rs` - Core Parsing (HEAVY)

This file has extensive MetaValueWithSourceInfo construction. Key functions:

**Functions to update:**
- `yaml_to_meta_with_source_info()` → Already have `yaml_to_config_value()` from Phase 2!
- `rawblock_to_meta_with_source_info()` → Already have `rawblock_to_config_value()` from Phase 4!
- Helper functions that construct MetaValueWithSourceInfo

**Strategy:**
- Keep the old functions temporarily (marked deprecated)
- New code uses the ConfigValue versions
- Remove old functions in Phase 6

### 2. `template/config_merge.rs` - Conversion Hub (HEAVY)

This file already has the conversion functions. Update to:
- Remove `config_value_to_meta()` calls (no longer needed when everything is ConfigValue)
- Simplify `meta_to_config_value()` (may be removable in Phase 6)
- Update tests to use ConfigValue directly

### 3. `template/context.rs` - Template Values (MODERATE)

**Current:**
```rust
fn meta_to_template_value(meta: &MetaValueWithSourceInfo) -> TemplateValue
```

**Target:**
```rust
fn config_to_template_value(config: &ConfigValue) -> TemplateValue
```

**Mapping:**
| ConfigValueKind | TemplateValue |
|-----------------|---------------|
| `Scalar(Yaml::String)` | `String` |
| `Scalar(Yaml::Boolean)` | `Bool` |
| `Scalar(Yaml::Integer)` | `Integer` |
| `PandocInlines` | `String` (rendered) |
| `Map` | `Object` |
| `Array` | `Array` |

### 4. `lua/readwrite.rs` - Lua Compatibility (MODERATE)

**Critical:** Lua expects old `MetaValue` format, not `MetaValueWithSourceInfo` or `ConfigValue`.

**Current:**
```rust
fn meta_with_source_to_lua(meta: &MetaValueWithSourceInfo) -> ...
fn lua_to_meta_with_source(val: Value) -> MetaValueWithSourceInfo
```

**Target:**
```rust
fn config_to_lua(config: &ConfigValue) -> ...
fn lua_to_config(val: Value) -> ConfigValue
```

**Note:** Internally, Lua filters use Pandoc's MetaValue format. The conversion chain:
- ConfigValue → MetaValue (via `to_meta_value()`) → Lua table
- Lua table → MetaValue → ConfigValue

### 5. `lua/filter.rs` - Filter Integration (MODERATE)

Update function signatures and pattern matches that handle metadata.

### 6. `citeproc_filter.rs` - Citation Processing (HEAVY)

**35+ match arms** extracting citation metadata. Update all patterns:

**Current:**
```rust
match meta {
    MetaValueWithSourceInfo::MetaString { value, .. } => Some(value.clone()),
    MetaValueWithSourceInfo::MetaInlines { content, .. } => extract_text(content),
    _ => None,
}
```

**Target:**
```rust
match &config.value {
    ConfigValueKind::Scalar(Yaml::String(s)) => Some(s.clone()),
    ConfigValueKind::PandocInlines(content) => extract_text(content),
    _ => None,
}
```

**Helper functions to update:**
- `get_meta_string()`
- `get_meta_bool()`
- `get_meta_string_list()`
- `extract_references()`

### 7-9. Remaining Files (LIGHT)

- `filters.rs` - Update metadata handling in filter chain
- `json_filter.rs` - Update JSON filter metadata
- `pandoc/treesitter_utils/document.rs` - Minor updates

## Migration Steps

```bash
# Work through files in priority order

# 1. pandoc/meta.rs
# Mark old functions deprecated, verify new ones used

# 2. template/config_merge.rs
# Simplify now that everything is ConfigValue

# 3. template/context.rs
# Update template value conversion

# 4-5. lua/readwrite.rs and lua/filter.rs
# Careful - maintain Lua compatibility

# 6. citeproc_filter.rs
# Many match arms to update

# 7-9. Remaining files
# Lighter changes

# After each major file:
cargo nextest run -p pampa
```

## Completion Criteria

- [ ] All 10 files compile with ConfigValue
- [ ] Lua filter tests pass
- [ ] Citeproc tests pass
- [ ] Template rendering tests pass
- [ ] All pampa tests pass (should be ~800+ tests)

## Notes

- Lua compatibility is the trickiest part - Lua uses old MetaValue internally
- The citeproc_filter has the most pattern matches to update
- Consider extracting common patterns into helper functions
