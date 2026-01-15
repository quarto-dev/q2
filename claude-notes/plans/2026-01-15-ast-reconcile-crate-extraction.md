# Plan: Extract AST Reconciliation into Dedicated Crate

**Status:** Proposed
**Created:** 2026-01-15
**Issue:** kyoto-lko

## Summary

Refactor the tree reconciliation code from `quarto-pandoc-types/src/reconcile/` into a new dedicated crate `quarto-ast-reconcile`. This algorithm has grown to ~9,000 lines and is fundamental to how Rust Quarto preserves source locations through code execution.

## Motivation

1. **Size and complexity**: The reconciliation module is 9,028 lines across 6 files—larger than many standalone crates
2. **Clear boundaries**: The code has minimal dependencies and a well-defined API
3. **Independent evolution**: Reconciliation algorithm changes shouldn't require releasing quarto-pandoc-types
4. **Reusability**: Other tools may want to use reconciliation without pulling in all Pandoc types
5. **Testing isolation**: Property-based tests (~2,000 lines of generators) can be scoped to this crate

## Current State

### Files to Extract
```
crates/quarto-pandoc-types/src/reconcile/
├── mod.rs         (1,313 lines) - Entry points, tests
├── types.rs       (602 lines)   - Plan and alignment types
├── apply.rs       (1,285 lines) - Plan application phase
├── compute.rs     (1,888 lines) - Plan computation phase
├── hash.rs        (2,016 lines) - Structural hashing
└── generators.rs  (1,924 lines) - Property-based test generators
```

### Current Dependencies
- **Internal**: `quarto-source-map` (for `FileId`, `SourceInfo`)
- **From quarto-pandoc-types**: `Block`, `Inline`, `Pandoc`, `CustomNode`, `Slot`, `Table`
- **External**: `serde`, `serde_json`, `hashlink`, `rustc-hash`, `proptest` (dev)

### Current Consumers
1. `crates/pampa/src/bin/ast_reconcile.rs` - CLI tool for reconciliation
2. `crates/experiments/reconcile-viewer/src/main.rs` - Visualization tool

Note: `quarto-core/src/engine/reconcile.rs` has a separate, simpler implementation—not affected by this refactor.

## New Crate Structure

### Crate: `quarto-ast-reconcile`

```
crates/quarto-ast-reconcile/
├── Cargo.toml
├── src/
│   ├── lib.rs           # Public API exports
│   ├── types.rs         # Plan types, alignment enums
│   ├── compute.rs       # Plan computation logic
│   ├── apply.rs         # Plan application logic
│   ├── hash.rs          # Structural hashing and equality
│   └── generators.rs    # Property-based test generators (#[cfg(test)])
└── tests/
    └── reconcile_tests.rs   # Integration tests (moved from mod.rs)
```

### Public API

```rust
// crates/quarto-ast-reconcile/src/lib.rs

//! AST Reconciliation for Quarto
//!
//! Three-phase tree reconciliation algorithm that preserves source locations
//! when merging original and executed ASTs.

// Main entry points
pub fn reconcile(original: Pandoc, executed: Pandoc) -> (Pandoc, ReconciliationPlan);
pub fn compute_reconciliation(original: &Pandoc, executed: &Pandoc) -> ReconciliationPlan;
pub fn apply_reconciliation(original: Pandoc, executed: Pandoc, plan: &ReconciliationPlan) -> Pandoc;
pub fn apply_reconciliation_to_blocks(original: Vec<Block>, executed: Vec<Block>, plan: &ReconciliationPlan) -> Vec<Block>;

// Block-level computation (for custom pipelines)
pub fn compute_reconciliation_for_blocks<'a>(original: &'a [Block], executed: &[Block], cache: &mut HashCache<'a>) -> ReconciliationPlan;

// Alignment enums
pub enum BlockAlignment { KeepBefore, UseAfter, RecurseIntoContainer }
pub enum InlineAlignment { KeepBefore, UseAfter, RecurseIntoContainer }
pub enum ListItemAlignment { KeepOriginal, Reconcile, UseExecuted }

// Plan types (serializable)
pub struct ReconciliationPlan { /* ... */ }
pub struct InlineReconciliationPlan { /* ... */ }
pub struct CustomNodeSlotPlan { /* ... */ }
pub struct TableReconciliationPlan { /* ... */ }
pub struct ReconciliationStats { /* ... */ }
pub enum TableCellPosition { Head, BodyHead, BodyBody, Foot }

// Hashing utilities
pub struct HashCache<'a> { /* ... */ }
pub fn compute_block_hash_fresh(block: &Block) -> u64;
pub fn compute_inline_hash_fresh(inline: &Inline) -> u64;
pub fn compute_blocks_hash_fresh(blocks: &[Block]) -> u64;
pub fn structural_eq_block(a: &Block, b: &Block) -> bool;
pub fn structural_eq_blocks(a: &[Block], b: &[Block]) -> bool;
pub fn structural_eq_inline(a: &Inline, b: &Inline) -> bool;
```

### Cargo.toml

```toml
[package]
name = "quarto-ast-reconcile"
version = "0.1.0"
edition = "2021"
description = "Three-phase AST reconciliation for preserving source locations"
license = "MIT"

[dependencies]
quarto-pandoc-types = { path = "../quarto-pandoc-types" }
quarto-source-map = { path = "../quarto-source-map" }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
hashlink = { version = "0.10.0", features = ["serde_impl"] }
rustc-hash = "2.1"

[dev-dependencies]
proptest = "1.5"
```

## Migration Steps

### Step 1: Create New Crate
1. Create `crates/quarto-ast-reconcile/` directory
2. Create `Cargo.toml` with dependencies
3. Add crate to workspace in root `Cargo.toml`

### Step 2: Move Source Files
1. Copy `quarto-pandoc-types/src/reconcile/*.rs` to new crate's `src/`
2. Restructure: extract tests from `mod.rs` into `tests/reconcile_tests.rs`
3. Create `lib.rs` with public exports

### Step 3: Update Imports in New Crate
Change all internal references from `crate::` to `quarto_pandoc_types::`:
- `crate::Block` → `quarto_pandoc_types::Block`
- `crate::Inline` → `quarto_pandoc_types::Inline`
- `crate::Pandoc` → `quarto_pandoc_types::Pandoc`
- `crate::custom::{CustomNode, Slot}` → `quarto_pandoc_types::custom::{CustomNode, Slot}`
- `crate::table::Table` → `quarto_pandoc_types::table::Table`

### Step 4: Verify New Crate Builds
```bash
cargo build -p quarto-ast-reconcile
cargo nextest run -p quarto-ast-reconcile
```

### Step 5: Update Consumers

**pampa/Cargo.toml:**
```toml
quarto-ast-reconcile = { path = "../quarto-ast-reconcile" }
```

**pampa/src/bin/ast_reconcile.rs:**
```rust
// Change from:
use quarto_pandoc_types::reconcile::{...};
// To:
use quarto_ast_reconcile::{...};
```

**experiments/reconcile-viewer/Cargo.toml:**
```toml
quarto-ast-reconcile = { path = "../../crates/quarto-ast-reconcile" }
```

**experiments/reconcile-viewer/src/main.rs:**
```rust
// Change from:
use quarto_pandoc_types::reconcile::{...};
// To:
use quarto_ast_reconcile::{...};
```

### Step 6: Remove from quarto-pandoc-types
1. Delete `crates/quarto-pandoc-types/src/reconcile/` directory
2. Remove `pub mod reconcile;` from `lib.rs`

### Step 7: Final Verification
```bash
cargo build --workspace
cargo nextest run --workspace
```

## Dependency Graph After Extraction

```
┌──────────────┐     ┌──────────────┐     ┌─────────────────────┐
│   pampa      │     │ reconcile-   │     │ (future consumers)  │
│              │     │ viewer       │     │                     │
└──────┬───────┘     └──────┬───────┘     └──────────┬──────────┘
       │                    │                        │
       └────────────────────┼────────────────────────┘
                            │
                            ▼
              ┌─────────────────────────────┐
              │    quarto-ast-reconcile     │
              └─────────────┬───────────────┘
                            │
         ┌──────────────────┼──────────────────┐
         │                  │                  │
         ▼                  ▼                  ▼
┌─────────────────┐  ┌───────────────┐  ┌────────────────┐
│ quarto-pandoc-  │  │ quarto-source-│  │ serde,hashlink │
│ types           │  │ map           │  │ rustc-hash     │
└─────────────────┘  └───────────────┘  └────────────────┘
```

Note: `quarto-pandoc-types` has NO dependency on `quarto-ast-reconcile`. The arrow is one-way only.

## Considerations

### Crate Name
- `quarto-ast-reconcile` - Clear, specific, matches the main function name

### Test Organization
Property-based test generators (~2,000 lines) stay in the crate under `#[cfg(test)]`. They're not compiled in release builds and don't need separate packaging.

### Module vs Flat Structure
The new crate keeps the same module structure (`types.rs`, `compute.rs`, `apply.rs`, `hash.rs`) rather than flattening into one file. This preserves logical separation and makes the code easier to navigate.

## Success Criteria

- [ ] `crates/quarto-ast-reconcile/` exists with proper structure
- [ ] `cargo build -p quarto-ast-reconcile` succeeds
- [ ] `cargo nextest run -p quarto-ast-reconcile` passes all tests
- [ ] `cargo build --workspace` succeeds
- [ ] `cargo nextest run --workspace` passes
- [ ] `quarto-pandoc-types` no longer contains reconcile module
- [ ] No circular dependencies

## Follow-up Work

After this refactoring is complete, we should revisit `quarto-core/src/engine/reconcile.rs`. That module contains a simpler, linear-alignment-based `reconcile_source_locations()` function that predates the three-phase algorithm. The full reconciliation code handles:

- Content that moves position (hash-based matching)
- Nested containers (recursive reconciliation)
- Lists with changed item counts
- Tables (cell-by-cell reconciliation)
- CustomNodes with multiple slots

The simpler implementation only does positional matching, which loses source locations when content moves or containers change internally. Once `quarto-ast-reconcile` is extracted and stable, we should:

1. Update `quarto-core` to depend on `quarto-ast-reconcile`
2. Replace `engine/reconcile.rs` with calls to the full reconciliation API
3. Remove the duplicate simpler implementation

This ensures consistent, high-quality source location preservation throughout the pipeline.

**Issue:** kyoto-676 (blocked by kyoto-lko)

## Notes

The reconciliation algorithm is inspired by React 15's reconciler but adapted for AST merging:
- React uses keys; we use structural hashing
- React updates in place; we merge two immutable trees
- We preserve source locations from "original" tree where content matches

The three phases are:
1. **Exact hash matches** (any position) → `KeepBefore`
2. **Positional type matches** (same index, same type) → `RecurseIntoContainer`
3. **Fallback** → `UseAfter`
