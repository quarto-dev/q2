# Subplan 01: Core Type Changes (quarto-pandoc-types)

**Order:** 1st (must complete before all others)
**Complexity:** HIGH
**Risk:** HIGH (breaks all dependents until fixed)

## Files

| File | Usage | Changes Required |
|------|-------|------------------|
| `quarto-pandoc-types/src/pandoc.rs` | Field type | Change `Pandoc.meta` type |
| `quarto-pandoc-types/src/meta.rs` | Definition | Keep temporarily, add deprecation |
| `quarto-pandoc-types/src/lib.rs` | Exports | Update exports |
| `quarto-pandoc-types/src/block.rs` | References | Check for any metadata refs |

## Detailed Changes

### 1. `pandoc.rs` - Change Pandoc.meta Type

**Current:**
```rust
pub struct Pandoc {
    pub api_version: (i32, i32, i32, i32),
    pub meta: MetaValueWithSourceInfo,
    pub blocks: Blocks,
}
```

**Target:**
```rust
pub struct Pandoc {
    pub api_version: (i32, i32, i32, i32),
    pub meta: ConfigValue,  // Changed!
    pub blocks: Blocks,
}
```

**Impact:** This single change will cause ~400 compilation errors across the codebase. That's expected and we fix them in subsequent subplans.

### 2. `meta.rs` - Keep MetaValueWithSourceInfo Temporarily

**Actions:**
- Keep the type definition (needed for Phase 6 cleanup)
- Add `#[deprecated]` attribute with migration message
- Keep helper methods (used by some code during migration)
- Keep conversion functions (`meta_from_legacy`, etc.)

**Add:**
```rust
#[deprecated(since = "0.x.x", note = "Use ConfigValue instead. See migration guide.")]
pub enum MetaValueWithSourceInfo { ... }
```

### 3. `lib.rs` - Update Exports

**Current exports to review:**
```rust
pub use meta::{MetaMapEntry, MetaValueWithSourceInfo, meta_from_legacy, meta_value_from_legacy};
```

**Keep all** during migration (consumers still reference them).

**Also ensure exported:**
```rust
pub use config_value::{ConfigValue, ConfigValueKind, ConfigMapEntry, MergeOp};
```

### 4. `block.rs` - Check References

Scan for any `MetaValueWithSourceInfo` references. Based on analysis, this file has light usage - verify and update if needed.

## Migration Steps

```bash
# Step 1: Change Pandoc.meta type
# Edit pandoc.rs

# Step 2: Verify it breaks (expected!)
cargo check 2>&1 | head -50
# Should see ~400 errors

# Step 3: Add deprecation to MetaValueWithSourceInfo
# Edit meta.rs

# Step 4: Verify exports
# Edit lib.rs if needed

# Step 5: Commit this breaking change
# Note: Code won't compile until other subplans done
```

## Completion Criteria

- [ ] `Pandoc.meta` is type `ConfigValue`
- [ ] `MetaValueWithSourceInfo` has `#[deprecated]` attribute
- [ ] All ConfigValue types exported from crate root
- [ ] `cargo check` on quarto-pandoc-types alone succeeds

## Notes

- This subplan creates a "broken" state - that's intentional
- Subsequent subplans fix the compilation errors
- Don't try to make everything compile here - just change the type
- The deprecation warning helps find remaining usages later
