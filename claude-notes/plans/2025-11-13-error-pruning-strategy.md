# Error Pruning Strategy for quarto-markdown-pandoc

**Date**: 2025-11-13
**Context**: We're emitting too many error diagnostics due to tree-sitter error recovery branching

## Problem Analysis

### Current Situation

The file `~/today/categorical-predictors.qmd` demonstrates the issue:
- **49 diagnostics** are generated
- **37 ERROR nodes** exist in the tree-sitter parse tree
- Only **2 outermost ERROR nodes**:
  1. ERROR node 1: 1 byte at row 475 → 1 diagnostic
  2. ERROR node 2: 16,531 bytes (row 520 to near EOF) → 48 diagnostics

### Key Findings

1. **Nested Structure**: ERROR nodes are nested. The huge outer ERROR node (16KB) contains 35 nested ERROR nodes that likely represent more specific parse errors.

2. **Redundant Diagnostics**: Multiple diagnostics are generated within the same error region due to tree-sitter's error recovery process trying different correction branches.

3. **Current Scoring**: `qmd_error_messages.rs:79-81` has a scoring function:
   ```rust
   fn diagnostic_score(diag: &DiagnosticMessage) -> usize {
       diag.hints.len() + diag.details.len() + diag.code.as_ref().map(|_| 1).unwrap_or(0)
   }
   ```
   More detailed diagnostics score higher.

## Proposed Solutions

### Strategy 1: Outermost ERROR Nodes (User's Original Proposal)

**Algorithm**:
1. Collect all ERROR nodes from tree
2. Filter to keep only non-nested (outermost) ERROR nodes
3. Group diagnostics by which outer ERROR node they overlap with
4. Keep only highest-scoring diagnostic per outer ERROR node
5. Discard diagnostics outside ERROR nodes

**Results for test file**:
- 49 diagnostics → 2 diagnostics (96% reduction)

**Pros**:
- Simple to implement
- Dramatically reduces noise
- Aligns with user's proposal

**Cons**:
- May be too aggressive when outer ERROR node is huge (16KB)
- Loses information about multiple distinct errors within large region

### Strategy 2: Leaf (Innermost) ERROR Nodes

**Algorithm**:
1. Collect all ERROR nodes
2. Filter to keep only leaf ERROR nodes (those with no ERROR children)
3. Group diagnostics by leaf ERROR node
4. Keep highest-scoring diagnostic per leaf ERROR node

**Expected Results**:
- Would likely keep more diagnostics than Strategy 1
- Better granularity for large error regions

**Pros**:
- More granular error reporting
- Better for large error regions

**Cons**:
- More complex to implement
- May still report too many errors

### Strategy 3: Size-Threshold Hybrid (Recommended)

**Algorithm**:
1. Collect all ERROR nodes
2. Identify outer ERROR nodes
3. For each outer ERROR node:
   - If size > threshold (e.g., 1000 bytes), recursively use its ERROR children
   - If size ≤ threshold, use this node
4. Result: non-overlapping ERROR nodes with reasonable sizes
5. Group diagnostics and keep highest-scoring per region

**Expected Results**:
- For test file: ERROR node 1 (1 byte) + ~35 inner nodes from ERROR node 2
- Would reduce 49 → ~25-30 diagnostics

**Pros**:
- Balances between too few and too many errors
- Adapts to document structure
- Maintains granularity where it matters

**Cons**:
- More complex implementation
- Threshold value needs tuning

### Strategy 4: Non-Overlapping Preference (Alternative)

**Algorithm**:
1. Collect all ERROR nodes sorted by size (smallest first)
2. Greedily select non-overlapping nodes:
   - Start with smallest nodes
   - Skip nodes that overlap with already-selected nodes
3. Group diagnostics by selected nodes
4. Keep highest-scoring per node

**Pros**:
- No arbitrary threshold
- Automatically prefers more specific errors

**Cons**:
- More complex algorithm
- May select too many small nodes

## Implementation Details

### Code Locations

1. **Error node collection**: Could reuse `src/errors.rs:accumulate_error_nodes()` or create new function
2. **Diagnostic production**: `src/readers/qmd.rs:125-130` calls `produce_diagnostic_messages()`
3. **Diagnostic filtering**: Would add new function in `src/readers/qmd_error_messages.rs`

### New Functions Needed

```rust
// In src/readers/qmd_error_messages.rs

/// Collect ERROR nodes from tree with position info
fn collect_error_node_ranges(tree: &MarkdownTree) -> Vec<(usize, usize)> {
    // Returns Vec of (start_offset, end_offset) for each ERROR node
}

/// Filter to outermost ERROR nodes
fn get_outer_error_nodes(error_nodes: &[(usize, usize)]) -> Vec<usize> {
    // Returns indices of non-nested nodes
}

/// Filter to leaf ERROR nodes
fn get_leaf_error_nodes(cursor: &mut MarkdownCursor, ...) -> Vec<(usize, usize)> {
    // Returns leaf ERROR nodes
}

/// Group diagnostics by error regions and keep best per region
fn prune_diagnostics_by_error_nodes(
    diagnostics: Vec<DiagnosticMessage>,
    error_regions: &[(usize, usize)]
) -> Vec<DiagnosticMessage> {
    // Groups diagnostics, scores them, keeps highest per region
}
```

### Integration Point

In `src/readers/qmd.rs:125-130`, after `produce_diagnostic_messages()`:

```rust
let diagnostics = produce_diagnostic_messages(...);

// NEW: Prune diagnostics based on ERROR nodes
let error_regions = collect_error_node_ranges(&tree);
let pruned_diagnostics = prune_diagnostics_by_error_nodes(diagnostics, &error_regions);

return Err(pruned_diagnostics);
```

## DECISION: Strategy 1 with First-Error Heuristic

**Approved approach**: Outermost ERROR Nodes with first-error selection

**Key Heuristic Change** (from user feedback):
- For each ERROR node range, pick the **EARLIEST** error (lowest start offset)
- Use scoring function only as **tiebreaker** when multiple errors at same location
- Rationale: The first error is what puts the parser in an error state

**Implementation Steps**:

1. ✅ Create test showing the analysis (done in `tests/error_node_analysis.rs`)
2. Add `collect_error_node_ranges()` function
3. Add `get_outer_error_nodes()` filter
4. Add `prune_diagnostics_by_error_nodes()` function with:
   - Sort diagnostics by start offset (earliest first)
   - Use scoring as tiebreaker for same offset
   - Keep first diagnostic per ERROR range
5. Integrate into `read()` function in `qmd.rs`
6. Add `--no-prune-errors` flag for debugging
7. Test with categorical-predictors.qmd
8. Run full test suite to ensure no regressions

## Testing Strategy

1. **Unit tests**: Test error node collection and grouping logic
2. **Integration test**: Use categorical-predictors.qmd (49 → 2 expected)
3. **Regression tests**: Ensure existing tests still pass
4. **New test cases**: Create tests with:
   - Multiple separate errors (should keep all)
   - Nested errors (should keep outer)
   - Single error (should keep 1)

## Decisions on Open Questions

1. **What to do with diagnostics outside ERROR nodes?**
   - ✅ **DECISION**: Discard ERROR diagnostics outside ERROR nodes
   - Future: When we have warnings, those should stay (warnings ≠ errors)
   - Rationale: If tree-sitter didn't create an ERROR node, it's likely a spurious diagnostic

2. **Should we add a flag to enable/disable pruning?**
   - ✅ **DECISION**: Yes, add `--no-prune-errors` flag
   - Useful for debugging parser issues
   - Default: pruning enabled

3. **What if there are no ERROR nodes but diagnostics exist?**
   - ✅ **DECISION**: Keep all diagnostics as fallback
   - This is a safety measure for edge cases

## References

- Error generation: `src/readers/qmd_error_messages.rs`
- Error node detection: `src/errors.rs`
- Test analysis: `tests/error_node_analysis.rs`
- Example file: `~/today/categorical-predictors.qmd`
