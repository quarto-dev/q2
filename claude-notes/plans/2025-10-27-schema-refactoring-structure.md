# Schema Module Refactoring Structure

**Date**: 2025-10-27
**Issue**: k-243 - Refactor schema.rs into smaller modules
**Current Size**: 1299 lines in single file
**Target**: Multiple files, each <300 lines

## Problem

The `private-crates/quarto-yaml-validation/src/schema.rs` file is currently 1299 lines and will grow to ~1500+ lines with Phase 2 additions (arrayOf, maybeArrayOf, record, schema wrapper). This creates:
- High token usage when editing
- Difficulty navigating and maintaining
- Longer context windows for LLM assistance

## Proposed Module Structure

```
private-crates/quarto-yaml-validation/src/schema/
├── mod.rs                    (~150 lines)
│   - Schema enum definition
│   - Public API (from_yaml, type_name, annotations, etc.)
│   - SchemaRegistry
│   - Re-exports
│
├── types.rs                  (~250 lines)
│   - BooleanSchema
│   - NumberSchema
│   - StringSchema
│   - NullSchema
│   - EnumSchema
│   - AnySchema
│   - AnyOfSchema
│   - AllOfSchema
│   - ArraySchema
│   - ObjectSchema
│   - RefSchema
│   - All struct definitions
│
├── annotations.rs            (~100 lines)
│   - SchemaAnnotations struct
│   - parse_annotations()
│   - EMPTY_ANNOTATIONS static
│
├── parser.rs                 (~100 lines)
│   - from_yaml() entry point
│   - parse_short_form()
│   - parse_object_form() (dispatch)
│   - parse_inline_enum() (top-level array)
│
├── parsers/
│   ├── mod.rs                (~30 lines)
│   │   - Re-exports all parsers
│   │
│   ├── primitive.rs          (~200 lines)
│   │   - parse_boolean_schema()
│   │   - parse_number_schema()
│   │   - parse_string_schema()
│   │   - parse_null_schema()
│   │   - parse_any_schema()
│   │
│   ├── enum.rs               (~100 lines)
│   │   - parse_enum_schema()
│   │   - Handles both inline and explicit forms
│   │
│   ├── ref.rs                (~50 lines)
│   │   - parse_ref_schema()
│   │   - (Future: parse_resolveref_schema())
│   │
│   ├── combinators.rs        (~120 lines)
│   │   - parse_anyof_schema()
│   │   - parse_allof_schema()
│   │   - (Future: parse_maybe_arrayof_schema())
│   │
│   ├── arrays.rs             (~90 lines)
│   │   - parse_array_schema()
│   │   - (Future: parse_arrayof_schema())
│   │
│   ├── objects.rs            (~250 lines)
│   │   - parse_object_schema()
│   │   - (Future: parse_record_schema())
│   │   - (Future: super/baseSchema support)
│   │
│   └── wrappers.rs           (~50 lines)
│       - (Future: parse_schema_wrapper())
│       - (Future: parse_pattern_schema())
│
└── helpers.rs                (~200 lines)
    - get_hash_string()
    - get_hash_number()
    - get_hash_usize()
    - get_hash_bool()
    - get_hash_string_array()
    - get_hash_tags()
    - yaml_to_json_value()
```

## File Size Projections

### Current (as-is)
- schema.rs: 1299 lines

### After Refactoring (before Phase 2)
- mod.rs: ~150 lines
- types.rs: ~250 lines
- annotations.rs: ~100 lines
- parser.rs: ~100 lines
- parsers/primitive.rs: ~200 lines
- parsers/enum.rs: ~100 lines
- parsers/ref.rs: ~50 lines
- parsers/combinators.rs: ~120 lines
- parsers/arrays.rs: ~90 lines
- parsers/objects.rs: ~250 lines
- helpers.rs: ~200 lines
- **Total**: ~1610 lines (includes module overhead)
- **Largest file**: 250 lines (types.rs, objects.rs)

### After Phase 2 (with new parsers)
- parsers/combinators.rs: ~150 lines (+30 for maybeArrayOf)
- parsers/arrays.rs: ~140 lines (+50 for arrayOf)
- parsers/objects.rs: ~280 lines (+30 for record)
- parsers/wrappers.rs: ~50 lines (new file)
- **Total**: ~1720 lines
- **Largest file**: 280 lines (objects.rs)

## Benefits

1. **Reduced Token Usage**
   - Working on primitive parsers: read ~200 lines instead of 1299
   - Working on object parser: read ~250 lines instead of 1299
   - 80%+ reduction in context size for most edits

2. **Clear Organization**
   - Semantic grouping (primitives, combinators, arrays, objects)
   - Easy to find specific functionality
   - New parsers have obvious homes

3. **Better Testing**
   - Can test parser modules independently
   - Smaller test files co-located with parsers

4. **Improved Maintainability**
   - Clear separation of concerns
   - Less cognitive load when reading code
   - Easier to review PRs

5. **Scalability**
   - Prepared for future additions (Phase 3, Phase 4)
   - Can add new parser files without bloating existing ones

## Migration Strategy

### Phase 1: Create Module Structure (No Logic Changes)

1. Create `schema/` directory
2. Create all module files with proper structure
3. Move code from `schema.rs` to appropriate modules
4. Update imports and visibility
5. Ensure all tests still pass
6. Delete old `schema.rs`

### Phase 2: Verify and Clean

1. Run `cargo test` - all tests must pass
2. Run `cargo fmt` on all new files
3. Run `cargo clippy` - fix any warnings
4. Check that external API unchanged (pub exports)

### Phase 3: Documentation

1. Add module-level documentation to each file
2. Update lib.rs if needed
3. Verify examples still work

## Implementation Checklist

- [ ] Create `src/schema/` directory
- [ ] Create `mod.rs` with Schema enum and public API
- [ ] Create `types.rs` with all schema struct definitions
- [ ] Create `annotations.rs` with SchemaAnnotations
- [ ] Create `parser.rs` with from_yaml entry point
- [ ] Create `parsers/` subdirectory
- [ ] Create `parsers/mod.rs` with re-exports
- [ ] Create `parsers/primitive.rs` with 5 primitive parsers
- [ ] Create `parsers/enum.rs` with enum parser
- [ ] Create `parsers/ref.rs` with ref parser
- [ ] Create `parsers/combinators.rs` with anyOf/allOf
- [ ] Create `parsers/arrays.rs` with array parser
- [ ] Create `parsers/objects.rs` with object parser
- [ ] Create `parsers/wrappers.rs` (empty, for Phase 2)
- [ ] Create `helpers.rs` with all helper functions
- [ ] Update `lib.rs` to export from `schema/mod.rs`
- [ ] Run `cargo test` - verify all pass
- [ ] Run `cargo fmt` and `cargo clippy`
- [ ] Delete old `schema.rs`
- [ ] Commit changes

## Notes

- **No behavior changes**: This is purely a refactoring
- **Tests must pass**: All existing tests should pass without modification
- **Public API unchanged**: External users see no difference
- **Internal only**: This is an internal reorganization

## Dependencies

- **Blocks**: k-240 (Phase 2 implementation)
- **Blocks**: bd-8 (overall YAML schema work)

## Estimated Time

- Module creation and code movement: 1-2 hours
- Testing and verification: 30 minutes
- Documentation and cleanup: 30 minutes
- **Total**: 2-3 hours
