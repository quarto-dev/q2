# Subplan 06: Tests and comrak-to-pandoc

**Order:** 6th (last)
**Complexity:** LOW
**Dependencies:** All previous subplans

## Files

### Test Files
| File | Changes Required |
|------|------------------|
| `pampa/tests/test_json_roundtrip.rs` | Update fixtures and assertions |
| `pampa/tests/test_metadata_source_tracking.rs` | Update pattern matches |
| `pampa/tests/test_yaml_tag_regression.rs` | Update assertions |
| `pampa/tests/test_rawblock_to_config_value.rs` | Already uses ConfigValue! |
| `pampa/tests/test_template_integration.rs` | Update fixtures |

### comrak-to-pandoc Files
| File | Usage | Changes Required |
|------|-------|------------------|
| `comrak-to-pandoc/src/block.rs` | LIGHT | Default metadata construction |
| `comrak-to-pandoc/src/normalize.rs` | LIGHT | Metadata normalization |
| `comrak-to-pandoc/src/tests/generators.rs` | LIGHT | Test fixtures |
| `comrak-to-pandoc/src/tests/debug.rs` | LIGHT | Debug utilities |

## Detailed Changes

### Test Files

#### 1. `test_json_roundtrip.rs`

**Update fixture construction:**
```rust
// OLD
let meta = MetaValueWithSourceInfo::MetaMap {
    entries: vec![
        MetaMapEntry {
            key: "title".to_string(),
            key_source: SourceInfo::default(),
            value: MetaValueWithSourceInfo::MetaString { ... }
        }
    ],
    source_info: SourceInfo::default()
};

// NEW
let meta = ConfigValue {
    value: ConfigValueKind::Map(vec![
        ConfigMapEntry {
            key: "title".to_string(),
            key_source: SourceInfo::default(),
            value: ConfigValue { ... }
        }
    ]),
    source_info: SourceInfo::default(),
    merge_op: MergeOp::Concat,
};
```

**Update assertions:**
```rust
// OLD
assert!(matches!(meta, MetaValueWithSourceInfo::MetaString { .. }));

// NEW
assert!(matches!(meta.value, ConfigValueKind::Scalar(Yaml::String(_))));
```

#### 2. `test_metadata_source_tracking.rs`

Already updated in Phase 4 for known limitations. Verify pattern matches still work:
```rust
// Current (from Phase 4)
if let MetaValueWithSourceInfo::MetaMap { ref entries, .. } = pandoc.meta { ... }

// Target
if let ConfigValueKind::Map(ref entries) = pandoc.meta.value { ... }
```

#### 3. `test_yaml_tag_regression.rs`

Update assertions for YAML tag handling.

#### 4. `test_rawblock_to_config_value.rs`

**Already uses ConfigValue!** This was created in Phase 4. May need minor updates if helper functions changed.

#### 5. `test_template_integration.rs`

Update any metadata fixtures used in template tests.

### comrak-to-pandoc Files

#### 1. `block.rs`

**Default metadata construction:**
```rust
// OLD
pub fn default_meta() -> MetaValueWithSourceInfo {
    MetaValueWithSourceInfo::MetaMap {
        entries: vec![],
        source_info: SourceInfo::default()
    }
}

// NEW
pub fn default_meta() -> ConfigValue {
    ConfigValue {
        value: ConfigValueKind::Map(vec![]),
        source_info: SourceInfo::default(),
        merge_op: MergeOp::Concat,
    }
}
```

#### 2. `normalize.rs`

Update any metadata normalization patterns.

#### 3-4. Test Files

Update test fixtures and debug utilities to use ConfigValue.

## Migration Steps

```bash
# 1. Update pampa test files
# These should largely compile once pampa modules are updated

# 2. Run pampa tests
cargo nextest run -p pampa

# 3. Update comrak-to-pandoc files

# 4. Run comrak-to-pandoc tests
cargo nextest run -p comrak-to-pandoc

# 5. Full test suite
cargo nextest run
```

## Completion Criteria

- [ ] All pampa test files compile and pass
- [ ] All comrak-to-pandoc files compile
- [ ] All comrak-to-pandoc tests pass
- [ ] Full test suite passes (2668+ tests)

## Notes

- Test files are updated last because they depend on the implementation changes
- `test_rawblock_to_config_value.rs` is already ConfigValue-native
- comrak-to-pandoc is auxiliary - lower priority
- This subplan confirms the migration is complete
