# SourceInfo JSON Serialization Size Measurements

**Date**: 2025-10-19
**Related Issues**: k-50, k-44

## Summary

Current SourceInfo JSON serialization produces **30-55x blowup** in file size compared to original QMD source. The problem is parent chain duplication - each node serializes its complete parent chain.

**Key Findings**:
- **Worst case**: Siblings at same depth → **51-55x constant blowup** (each duplicates parent chain)
- **Best case**: Deep single-path nesting → **~30x blowup** (amortized over content size)
- **Balanced trees**: **30-50x blowup** depending on depth (O(n) serialization overhead)
- **Critical insight**: Sibling count has the biggest impact, not depth

## Measurements

### Test 1: Nested Depth Scaling

Created YAML with increasing nesting depth (level1 -> level2 -> level3 -> ...).

| Depth | QMD Size | JSON Size | Ratio   |
|-------|----------|-----------|---------|
| 1     | 48 B     | 2,461 B   | 51.27x  |
| 2     | 61 B     | 3,330 B   | 54.59x  |
| 3     | 76 B     | 4,199 B   | 55.25x  |
| 5     | 112 B    | 5,968 B   | 53.29x  |
| 10    | 238 B    | 10,381 B  | 43.62x  |
| 15    | 418 B    | 14,777 B  | 35.35x  |
| 20    | 648 B    | 19,173 B  | 29.59x  |

**Observation**: Ratio decreases with depth (better at deeper levels) because the actual content becomes a larger portion of the output.

### Test 2: Sibling Nodes Scaling

Created YAML with increasing number of sibling nodes at depth 3.

| Siblings | QMD Size | JSON Size  | Ratio  |
|----------|----------|------------|--------|
| 1        | 60 B     | 3,331 B    | 55.52x |
| 5        | 136 B    | 7,379 B    | 54.26x |
| 10       | 231 B    | 12,406 B   | 53.71x |
| 20       | 431 B    | 22,466 B   | 52.13x |
| 50       | 1,031 B  | 52,863 B   | 51.27x |
| 100      | 2,031 B  | 103,677 B  | 51.05x |

**Observation**: Ratio stays constant around **51-55x** - this is the worst case because each sibling duplicates the entire parent chain.

### Test 3: Binary Tree Scaling

Created a complete binary tree YAML structure with increasing depth (balanced tree where each node has left and right children).

| Depth | Nodes | QMD Size | JSON Size  | Ratio  |
|-------|-------|----------|------------|--------|
| 1     | 1     | 67 B     | 3,453 B    | 51.54x |
| 2     | 3     | 145 B    | 7,240 B    | 49.93x |
| 3     | 7     | 333 B    | 14,759 B   | 44.32x |
| 4     | 15    | 773 B    | 29,795 B   | 38.54x |
| 5     | 31    | 1,781 B  | 60,180 B   | 33.79x |
| 6     | 63    | 4,053 B  | 120,981 B  | 29.85x |

**Observation**: Similar pattern to Test 1 (nested depth) - ratio decreases as the tree grows because content becomes a larger portion of output. The binary tree structure shows that the serialization overhead is **O(n)** with respect to the number of nodes, with a baseline blowup factor around 30x for well-balanced structures.

### Test 4: Structure Analysis

Analyzed a moderately nested document (3 levels, 3 siblings):

```yaml
---
level1:
  level2:
    level3:
      item1: "value1"
      item2: "value2"
      item3: "value3"
---
```

**Results**:
- QMD size: 119 bytes
- JSON size: 6,231 bytes
- Ratio: **52.36x**
- Substring nodes: 24
- Original nodes: 12
- file_id occurrences: 12 (parent chain duplication count)

## Root Cause

Each SourceInfo with a `Substring` mapping serializes:
```json
{
  "mapping": {
    "Substring": {
      "offset": N,
      "parent": {
        "mapping": { ... },  // ENTIRE parent chain duplicated
        "range": { ... }
      }
    }
  },
  "range": { ... }
}
```

For 3 sibling nodes sharing the same parent, the parent chain appears **3 times** in the JSON.

## Implications

### Real-World Impact

A typical Quarto document with:
- 50 metadata fields
- Average nesting depth of 3
- Would produce JSON that's **50x+ larger** than the original QMD

For a 10KB QMD file with heavy metadata, the JSON could be **500KB+**.

### Where This Hurts

1. **WASM/TypeScript boundaries**: Shipping AST across process boundaries
2. **Network transfer**: Sending AST over HTTP
3. **Storage**: Caching parsed ASTs
4. **Memory**: Deserializing large JSON strings

### Where It Doesn't Hurt

- **In-memory Rust**: We use `Rc` to share parents (see k-43)
- **Compressed transfer**: gzip/brotli would deduplicate parent chains (needs measurement)

## Next Steps

See design proposals in separate notes. Options include:

1. **Custom serialization format**: ID-based parent references
2. **Flatten to absolute positions**: Store only (file_id, offset) for Substrings
3. **Compression-aware**: Accept the duplication, rely on compression
4. **Hybrid approach**: Different formats for different use cases

## Test Code

Test file: `crates/quarto-markdown-pandoc/tests/test_nested_yaml_serialization.rs`

Run tests:
```bash
cargo test --test test_nested_yaml_serialization -- --nocapture
```
