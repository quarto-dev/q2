# Phase 5: Migrate Consumers - Master Plan

**Parent Issue:** k-2tu9
**Phase:** 5 of 7
**Status:** Planning

## Overview

This phase changes `Pandoc.meta` from `MetaValueWithSourceInfo` to `ConfigValue` and updates all consumer code. Based on codebase analysis:

- **441 total occurrences** of `MetaValueWithSourceInfo`
- **33 files** need updates
- **Heavy usage** in parsing, serialization, and template rendering

## Migration Strategy

### Approach: Gradual Type Migration

1. **Change the type definition** in `Pandoc` struct
2. **Keep `MetaValueWithSourceInfo`** available during migration (for intermediate conversions)
3. **Update consumers in dependency order** (core → readers → writers → filters → transforms → tests)
4. **Remove legacy type** in Phase 6

### Why Not Big Bang?

The types are structurally different:
- `MetaValueWithSourceInfo` is an enum with 6 variants
- `ConfigValue` is a struct with `value: ConfigValueKind`

A big-bang change would require updating 33 files simultaneously. Gradual migration allows:
- Smaller, reviewable PRs
- Ability to test after each batch
- Rollback safety

## Subplans

| Subplan | Files | Complexity | Order |
|---------|-------|------------|-------|
| [01-core-types](./01-core-types.md) | 4 | HIGH | 1st |
| [02-readers](./02-readers.md) | 3 | MEDIUM | 2nd |
| [03-writers](./03-writers.md) | 3 | MEDIUM | 3rd |
| [04-pampa-internal](./04-pampa-internal.md) | 8 | HIGH | 4th |
| [05-quarto-core](./05-quarto-core.md) | 7 | MEDIUM | 5th |
| [06-tests-comrak](./06-tests-comrak.md) | 8 | LOW | 6th |

## Dependency Graph

```
quarto-pandoc-types (01-core-types)
    │
    ├── pampa/readers (02-readers)
    │       │
    │       └── pampa/writers (03-writers)
    │               │
    │               └── pampa/internal (04-pampa-internal)
    │                       │
    │                       └── quarto-core (05-quarto-core)
    │                               │
    │                               └── tests/comrak (06-tests-comrak)
    │
    └── All crates depend on types
```

## Change Categories

### Pattern Changes Required

1. **Enum matching → Struct matching**
   ```rust
   // OLD
   match meta {
       MetaValueWithSourceInfo::MetaString { value, .. } => ...
       MetaValueWithSourceInfo::MetaMap { entries, .. } => ...
   }

   // NEW
   match &config.value {
       ConfigValueKind::Scalar(Yaml::String(s)) => ...
       ConfigValueKind::Map(entries) => ...
   }
   ```

2. **Variant construction → Struct construction**
   ```rust
   // OLD
   MetaValueWithSourceInfo::MetaString {
       value: "hello".into(),
       source_info: SourceInfo::default()
   }

   // NEW
   ConfigValue {
       value: ConfigValueKind::Scalar(Yaml::String("hello".into())),
       source_info: SourceInfo::default(),
       merge_op: MergeOp::Concat,
   }
   ```

3. **MetaMapEntry → ConfigMapEntry**
   ```rust
   // OLD
   MetaMapEntry { key, key_source, value }

   // NEW
   ConfigMapEntry { key, key_source, value }
   ```

4. **Type changes in signatures**
   ```rust
   // OLD
   fn process(meta: &MetaValueWithSourceInfo) -> ...

   // NEW
   fn process(config: &ConfigValue) -> ...
   ```

## Completion Criteria

Each subplan has its own completion criteria. Overall Phase 5 is complete when:

1. [ ] `Pandoc.meta` is type `ConfigValue`
2. [ ] All 33 files compile without errors
3. [ ] All 2668+ tests pass
4. [ ] No remaining references to `MetaValueWithSourceInfo` in active code paths
5. [ ] JSON roundtrip still works
6. [ ] Lua filter compatibility maintained

## Risk Mitigation

1. **Compile after each file** - Catch errors early
2. **Test after each subplan** - Verify no regressions
3. **Keep conversion functions** - Use `config_value_to_meta()` as escape hatch
4. **Parallel test runs** - Run tests frequently via `cargo nextest run`

## Notes

- The existing `meta_to_config_value()` and `config_value_to_meta()` functions provide a safety net
- Phase 4 already proved the ConfigValue path works in `qmd.rs`
- Lua compatibility requires special handling (uses old `MetaValue` format internally)
