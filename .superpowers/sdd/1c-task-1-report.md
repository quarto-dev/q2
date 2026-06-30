# Task 1 Report — Engine-contribution data types + static-claim → LanguageClaim conversion

## Status

DONE

## Files Changed

- `crates/quarto-core/src/extension/types.rs` — sole file modified

## What Was Added

### New types
- `EngineContribution` enum (`External { path, name, claims, file_extensions, claims_files }` + `Reorder { name }`)
- `StaticLanguageClaim` struct (`kind`, `priority`, `when_class`)
- `ClaimKind` enum (`Primary`, `Interop`, `Fallback`)

### New field on `Contributes`
```rust
pub engines: Vec<EngineContribution>,
```
All three existing `Contributes { .. }` literals already used `..Default::default()`, so no manual updates were needed:
- `crates/quarto-core/src/filter_resolve.rs:488`
- `crates/quarto-core/src/transforms/shortcode_resolve.rs:2048`
- `crates/quarto-core/src/stage/stages/metadata_merge.rs:1630`

### New functions
- `static_claim_to_language_claim(claim, first_class) -> LanguageClaim`
- `lookup_static_claim(claims, language, first_class) -> LanguageClaim`

### `LanguageClaim` derives
Already had `#[derive(Debug, Clone, Copy, PartialEq, Eq)]` at `engine/mod.rs:104` — no changes needed.

## TDD Sequence

**RED** — wrote tests with two stubs:
- `static_claim_to_language_claim`: converted correctly but ignored `when_class` (always converted)
- `lookup_static_claim`: always returned `Primary(1)` ignoring map contents

Ran `cargo nextest run -p quarto-core -E 'test(extension::types::tests::static_claim) or test(extension::types::tests::lookup)'`:
- 5 PASS (positive conversion cases — stubs handled those correctly)
- 4 FAIL (the required RED cases):
  - `static_claim_when_class_mismatch_returns_none`: got `Primary(1)`, expected `None`
  - `static_claim_when_class_mismatch_no_first_class_returns_none`: got `Primary(1)`, expected `None`
  - `lookup_absent_language_returns_none`: got `Primary(1)`, expected `None`
  - `lookup_present_mismatched_when_class_returns_none`: got `Primary(1)`, expected `None`

**GREEN** — replaced stubs with correct implementations:

`static_claim_to_language_claim`: added `when_class` guard before the `match`:
```rust
if let Some(ref required) = claim.when_class {
    if first_class != Some(required.as_str()) {
        return LanguageClaim::None;
    }
}
```

`lookup_static_claim`: proper absent-check + delegation:
```rust
match claims.get(language) {
    None => crate::engine::LanguageClaim::None,
    Some(claim) => static_claim_to_language_claim(claim, first_class),
}
```

## Test Results

### New tests (15 total in `extension::types::tests`)
```
cargo nextest run -p quarto-core -E 'test(extension::types)'
Summary [0.062s] 15 tests run: 15 passed, 2556 skipped
```

Tests added (9 new, 6 pre-existing):
1. `static_claim_primary_no_when_class_default_priority` — `Primary(1)` default ✓
2. `static_claim_primary_no_when_class_explicit_priority` — `Primary(5)` explicit ✓
3. `static_claim_interop_and_fallback_default_priority` — `Interop(0)`, `Fallback(0)` ✓
4. `static_claim_when_class_match_converts` — `"marimo"` == `"marimo"` → `Primary(1)` ✓
5. `static_claim_when_class_mismatch_returns_none` — `"marimo"` != `"python"` → `None` ✓ (P1-14 binding)
6. `static_claim_when_class_mismatch_no_first_class_returns_none` — `"marimo"` != `None` → `None` ✓ (P1-14 binding)
7. `lookup_absent_language_returns_none` — absent key → `None` ✓
8. `lookup_present_matching_when_class_converts` — present + match → converts ✓
9. `lookup_present_mismatched_when_class_returns_none` — present + mismatch → `None` ✓

Pre-existing tests also updated:
- `test_contributes_default`: added `assert!(c.engines.is_empty())` ✓

### Broader regression check
```
cargo nextest run -p quarto-core -E 'test(extension::) or test(engine::)'
Summary [6.699s] 460 tests run: 460 passed, 2111 skipped
```

### Build verification
```
cargo build -p quarto-core
Finished `dev` profile [optimized + debuginfo] target(s) in 2.78s
```
No warnings, no errors. All three existing `Contributes` literals compile correctly via `..Default::default()`.

## Notes

- No `serde` derives added (Task 2 owns YAML parsing).
- No changes to `parse_contributes`, `TsEngine`, or resolution code.
- `LanguageClaim` needed no derive additions.
