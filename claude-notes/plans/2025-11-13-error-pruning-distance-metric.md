# Error Pruning Distance Metric Design

## Problem Analysis

Test file `008.qmd` is failing because the diagnostic is not overlapping with the ERROR node:

```
```{r eval=FALSE}
cat("hello")
```
```

- ERROR node range: offsets `4..5` (the space after `r`)
- Main diagnostic location: offsets `5..6` (start of `eval`)
- Detail location: offsets `6..10` (the `eval` itself)

Current overlap check fails:
```rust
if diag_start < err_end && diag_end > err_start {
    // diag_start=5 < err_end=5 is FALSE
```

## Data Structures

### ERROR Nodes
- Simple pairs: `(start_offset: usize, end_offset: usize)`

### DiagnosticMessage
- `location: Option<SourceInfo>` - main location of the diagnostic
- `details: Vec<DetailItem>` - each detail can have its own `location: Option<SourceInfo>`

### SourceInfo
- Has methods: `start_offset()`, `end_offset()`, `length()`
- Can be Original, Substring, or Concat (Concat is for merged sources)

## Design Requirements

1. **Never discard diagnostics** - All error diagnostics MUST be kept
2. **Assign each diagnostic to an ERROR node**:
   - First preference: Assign to outer ERROR node if overlapping
   - Second preference: Assign to closest ERROR node by distance
3. **For each ERROR node**: Keep only the earliest diagnostic (tiebreak with score)

## Distance Metric Design

### Proposed: Gap Distance

For ranges that don't overlap, measure the minimum gap between them:

```rust
fn range_gap_distance(
    range1_start: usize,
    range1_end: usize,
    range2_start: usize,
    range2_end: usize
) -> usize {
    if range1_end <= range2_start {
        // range1 is before range2
        range2_start - range1_end
    } else if range2_end <= range1_start {
        // range2 is before range1
        range1_start - range2_end
    } else {
        // Overlapping
        0
    }
}
```

This has nice properties:
- Returns 0 for overlapping ranges (desired for assignment)
- Measures actual byte distance in source
- Intuitive: closer errors should be associated together

### Handling Multiple Locations

A diagnostic can have:
- Main `location: Option<SourceInfo>`
- Detail locations in `details: Vec<DetailItem>` where each item has `location: Option<SourceInfo>`

**Strategy**: Use the minimum distance from ANY of these locations to the ERROR node.

Rationale: If any part of the diagnostic is close to an ERROR node, they're likely related.

## Algorithm

```rust
fn assign_diagnostics_to_error_nodes(
    diagnostics: Vec<DiagnosticMessage>,
    outer_error_nodes: &[(usize, usize)],
) -> BTreeMap<usize, Vec<usize>> {
    let mut assignments: BTreeMap<usize, Vec<usize>> = BTreeMap::new();

    for (diag_idx, diag) in diagnostics.iter().enumerate() {
        // Only process error diagnostics
        if diag.kind != DiagnosticKind::Error {
            continue;
        }

        // Collect all offsets from diagnostic (main + details)
        let diag_ranges = collect_all_location_ranges(diag);

        if diag_ranges.is_empty() {
            // No location info - can't assign
            continue;
        }

        // Try to find overlapping ERROR node
        let mut assigned = false;
        for (err_idx, &(err_start, err_end)) in outer_error_nodes.iter().enumerate() {
            if any_range_overlaps(&diag_ranges, err_start, err_end) {
                assignments.entry(err_idx).or_default().push(diag_idx);
                assigned = true;
                break; // Assign to first overlapping node
            }
        }

        // If no overlap, find closest ERROR node
        if !assigned {
            if let Some(closest_idx) = find_closest_error_node(&diag_ranges, outer_error_nodes) {
                assignments.entry(closest_idx).or_default().push(diag_idx);
            }
            // Note: If no ERROR nodes exist, diagnostic won't be assigned
            // But outer loop should catch this case earlier
        }
    }

    assignments
}

fn collect_all_location_ranges(diag: &DiagnosticMessage) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();

    // Add main location
    if let Some(loc) = &diag.location {
        ranges.push((loc.start_offset(), loc.end_offset()));
    }

    // Add detail locations
    for detail in &diag.details {
        if let Some(loc) = &detail.location {
            ranges.push((loc.start_offset(), loc.end_offset()));
        }
    }

    ranges
}

fn any_range_overlaps(
    ranges: &[(usize, usize)],
    err_start: usize,
    err_end: usize
) -> bool {
    ranges.iter().any(|&(start, end)| {
        // Ranges overlap if: start < err_end AND end > err_start
        start < err_end && end > err_start
    })
}

fn find_closest_error_node(
    diag_ranges: &[(usize, usize)],
    error_nodes: &[(usize, usize)]
) -> Option<usize> {
    if error_nodes.is_empty() {
        return None;
    }

    // Find ERROR node with minimum distance to ANY of the diagnostic's ranges
    error_nodes.iter().enumerate()
        .min_by_key(|(_, &(err_start, err_end))| {
            // Min distance from this ERROR node to any diagnostic range
            diag_ranges.iter()
                .map(|&(diag_start, diag_end)| {
                    range_gap_distance(err_start, err_end, diag_start, diag_end)
                })
                .min()
                .unwrap_or(usize::MAX)
        })
        .map(|(idx, _)| idx)
}
```

## Full Pruning Flow

```rust
pub fn prune_diagnostics_by_error_nodes(
    diagnostics: Vec<DiagnosticMessage>,
    error_nodes: &[(usize, usize)],
    outer_node_indices: &[usize],
) -> Vec<DiagnosticMessage> {
    // If no ERROR nodes, keep all diagnostics as fallback
    if outer_node_indices.is_empty() {
        return diagnostics;
    }

    // Build outer error ranges
    let outer_ranges: Vec<(usize, usize)> = outer_node_indices
        .iter()
        .map(|&idx| error_nodes[idx])
        .collect();

    // Assign diagnostics to ERROR nodes
    let assignments = assign_diagnostics_to_error_nodes(&diagnostics, &outer_ranges);

    // For each ERROR node, keep only the earliest diagnostic
    let mut kept_indices = Vec::new();

    for (_err_idx, diag_indices) in assignments.iter() {
        if diag_indices.is_empty() {
            continue;
        }

        // Find the earliest diagnostic (tiebreak with score)
        let best_idx = find_best_diagnostic(&diagnostics, diag_indices);
        kept_indices.push(best_idx);
    }

    // Add any error diagnostics that weren't assigned (defensive - shouldn't happen)
    for (idx, diag) in diagnostics.iter().enumerate() {
        if diag.kind == DiagnosticKind::Error &&
           !assignments.values().any(|v| v.contains(&idx)) {
            kept_indices.push(idx);
        }
    }

    kept_indices.sort();

    // Build result: kept error diagnostics + all non-error diagnostics
    let kept_set: HashSet<usize> = kept_indices.iter().copied().collect();

    diagnostics.into_iter().enumerate()
        .filter(|(idx, diag)| {
            kept_set.contains(idx) ||
            diag.kind != DiagnosticKind::Error
        })
        .map(|(_, diag)| diag)
        .collect()
}
```

## Testing Plan

1. **Test 008.qmd** - Should now work:
   - ERROR node at 4..5
   - Diagnostic at 5..6 (distance = 0 bytes gap)
   - Should assign and keep the diagnostic

2. **Test 003.qmd** - Should prune correctly:
   - Multiple errors about attribute parsing
   - Keep only earliest error per ERROR node

3. **Edge cases**:
   - Diagnostic with no location → skipped (can't assign)
   - Diagnostic with multiple detail locations → use minimum distance
   - Multiple diagnostics equidistant from ERROR node → all assigned, earliest kept
