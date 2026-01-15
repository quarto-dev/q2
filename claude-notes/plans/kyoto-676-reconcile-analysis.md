# kyoto-676: Replace quarto-core engine/reconcile.rs with quarto-ast-reconcile

**Issue**: kyoto-676
**Status**: In Progress
**Blocked by**: kyoto-lko (completed)

## Executive Summary

`quarto-core/src/engine/reconcile.rs` contains a simpler, linear-alignment reconciliation algorithm that should be replaced with the full three-phase algorithm from `quarto-ast-reconcile`. This will enable better source location preservation in more complex editing scenarios.

## Current Implementation Analysis

### Location
`crates/quarto-core/src/engine/reconcile.rs` (~1023 lines)

### Algorithm: Linear Alignment with Limited Lookahead

The current algorithm uses a simple linear scan with lookahead of 5 blocks:

```rust
fn reconcile_blocks(original: &[Block], executed: &mut [Block], report: &mut ReconciliationReport) {
    let mut orig_idx = 0;
    let mut exec_idx = 0;

    while orig_idx < original.len() && exec_idx < executed.len() {
        let quality = match_blocks(&original[orig_idx], &executed[exec_idx]);

        match quality {
            MatchQuality::Exact => { /* transfer source info */ }
            MatchQuality::StructuralOnly => { /* reconcile children */ }
            MatchQuality::NoMatch => {
                // Try lookahead of 5 blocks in each direction
                if let Some(ahead) = find_match_ahead(original, executed, ...) { ... }
                else if let Some(ahead) = find_match_ahead_in_original(...) { ... }
                else { /* treat as addition/deletion pair */ }
            }
        }
    }
}
```

### Match Quality Levels

1. **Exact**: Content matches exactly (ignoring source locations) → transfer source location
2. **StructuralOnly**: Same type but different content → keep executed location, reconcile children
3. **NoMatch**: Different types or content → try lookahead, else treat as deletion + addition

### Limitations

1. **Limited lookahead (5 blocks)**: If content moves more than 5 positions, it will be treated as deletion + addition rather than movement

2. **No hash-based matching**: Cannot efficiently match content that has moved arbitrary distances

3. **List item handling**: Lists require same number of items for structural match:
   ```rust
   (Block::BulletList(a), Block::BulletList(b)) => {
       if lists_content_equal(&a.content, &b.content) {
           MatchQuality::Exact
       } else if a.content.len() == b.content.len() {
           MatchQuality::StructuralOnly  // Only if same length!
       } else {
           MatchQuality::NoMatch  // Different lengths = no match
       }
   }
   ```

4. **Table reconciliation skipped entirely**:
   ```rust
   (Block::Table(_), Block::Table(_)) => {
       // Tables are complex - for now, treat as structural match
       MatchQuality::StructuralOnly
   }
   // And in transfer_block_source_info:
   (Block::Table(_), Block::Table(_)) => {
       // Tables are complex - skip for now
   }
   ```

5. **CustomNode slots not reconciled**:
   ```rust
   (Block::Custom(_), Block::Custom(_)) => MatchQuality::StructuralOnly,
   // No slot-level reconciliation
   ```

6. **Mutates in place**: Takes `&mut executed` rather than consuming both ASTs, which limits optimization opportunities

### Usage Point

Used in `crates/quarto-core/src/stage/stages/engine_execution.rs:250`:

```rust
// Step 8: Reconcile source locations
let mut reconciled_ast = executed_ast;
let reconciliation_report = reconcile_source_locations(&doc_ast.ast, &mut reconciled_ast);
```

## Full Implementation (quarto-ast-reconcile)

### Algorithm: Three-Phase React 15-Inspired

The full implementation uses structural hashing as "virtual keys" and a three-phase approach:

**Phase 1 - Exact Hash Matches**: Hash-based matching for content that moved arbitrarily
**Phase 2 - Positional Type Matches**: Fall back to same-type positional matching
**Phase 3 - Fallback**: Replace with executed content

### Key Advantages

1. **Hash-based matching**: Content can move any distance and still be matched
2. **List item count changes handled**: Proper handling when items are added/removed
3. **Table cell reconciliation**: Deep reconciliation of table structure
4. **CustomNode slot reconciliation**: Each slot is reconciled independently
5. **Serializable plan**: The `ReconciliationPlan` can be inspected/debugged
6. **Comprehensive statistics**: Tracks kept, replaced, recursed counts
7. **Property-tested**: Extensive proptest coverage ensures correctness

### API Comparison

**Current (quarto-core)**:
```rust
pub fn reconcile_source_locations(
    original: &Pandoc,
    executed: &mut Pandoc,  // Mutated in place
) -> ReconciliationReport
```

**Full (quarto-ast-reconcile)**:
```rust
pub fn reconcile(
    original: Pandoc,   // Consumed
    executed: Pandoc,   // Consumed
) -> (Pandoc, ReconciliationPlan)  // New merged AST + plan
```

## Migration Path

### Option A: Direct Replacement (Recommended)

Replace the call in `engine_execution.rs` with:

```rust
use quarto_ast_reconcile::reconcile;

// Step 8: Reconcile source locations
let (reconciled_ast, reconciliation_plan) = reconcile(doc_ast.ast.clone(), executed_ast);

trace_event!(
    ctx,
    EventLevel::Debug,
    "reconciliation: {} kept, {} replaced, {} recursed",
    reconciliation_plan.stats.blocks_kept,
    reconciliation_plan.stats.blocks_replaced,
    reconciliation_plan.stats.blocks_recursed
);
```

**Considerations**:
- Requires cloning `doc_ast.ast` since `reconcile` consumes its arguments and we need `doc_ast` later (or restructure the code)
- Changes the trace output format (different stat names)
- Adds `quarto-ast-reconcile` as a dependency of `quarto-core`

### Option B: Wrapper for Compatibility

Create a wrapper that matches the current API:

```rust
pub fn reconcile_source_locations(
    original: &Pandoc,
    executed: &mut Pandoc,
) -> ReconciliationReport {
    let (result, plan) = quarto_ast_reconcile::reconcile(
        original.clone(),
        std::mem::take(executed),
    );
    *executed = result;

    // Convert stats to old format
    ReconciliationReport {
        exact_matches: plan.stats.blocks_kept,
        content_changes: plan.stats.blocks_recursed,
        deletions: 0,  // Not tracked the same way
        additions: plan.stats.blocks_replaced,
    }
}
```

**Considerations**:
- Maintains API compatibility
- Stat mapping is imprecise (different semantics)
- Extra clone overhead

### Recommendation

**Option A** is cleaner. The API change is localized to one file, and the new stats are more informative. The clone of `doc_ast.ast` can be avoided by restructuring the stage code slightly.

## Files to Modify

1. **`crates/quarto-core/Cargo.toml`**: Add `quarto-ast-reconcile` dependency
2. **`crates/quarto-core/src/engine/reconcile.rs`**: Delete (or keep for reference during migration)
3. **`crates/quarto-core/src/engine/mod.rs`**: Remove `pub mod reconcile;`
4. **`crates/quarto-core/src/stage/stages/engine_execution.rs`**:
   - Change import from `crate::engine::reconcile::reconcile_source_locations` to `quarto_ast_reconcile::reconcile`
   - Update the reconciliation call
   - Update trace output

## Testing Strategy

1. Run existing `quarto-core` tests to verify engine execution still works
2. Run full workspace test suite (`cargo nextest run --workspace`)
3. Manually test with actual engine execution (knitr/jupyter) if available
4. The fact that `quarto-ast-reconcile` is already property-tested provides confidence

## Risk Assessment

**Low Risk**:
- The full reconciliation algorithm is already well-tested (199 tests + property tests)
- The change is isolated to engine execution stage
- Markdown engine path (the common case) is a passthrough anyway

**Medium Risk**:
- Different reconciliation behavior may produce different source locations in edge cases
- This could affect error message line numbers for executed content

## Next Steps

1. Add `quarto-ast-reconcile` to `quarto-core/Cargo.toml`
2. Update `engine_execution.rs` to use `quarto_ast_reconcile::reconcile`
3. Delete `engine/reconcile.rs` and update `engine/mod.rs`
4. Run full test suite
5. Close kyoto-676
