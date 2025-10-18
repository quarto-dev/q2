# Scaling Analysis: Linear vs Superlinear Growth

## Executive Summary

✅ **Overhead scales LINEARLY with data size** - no superlinear growth detected.

The overhead ratio stabilizes around 4-6x for realistic workloads, with only small variations (2-13%) as data size increases 100x.

## Test Results

### Test 1: Flat Array (10 → 1000 items)

```
Size    Raw        Tracked    Ratio
10      1,592      5,496      3.45x
50      6,840      26,536     3.88x
100     13,624     52,836     3.88x   ← Stabilizes
250     30,392     132,036    4.34x
500     60,728     264,036    4.35x
1000    121,400    528,036    4.35x   ← Stable
```

**Analysis**:
- Overhead ratio: 3.45x → 4.35x (26% change)
- Size increased: 100x
- Memory increased: Raw 76x, Tracked 96x
- **Verdict**: Small fixed cost at tiny sizes, then **linear** (ratio stabilizes at 4.35x)

### Test 2: Flat Hash (10 → 1000 key-value pairs)

```
Size    Raw        Tracked      Ratio
10      2,874      14,544       5.06x
50      12,618     70,288       5.57x
100     25,190     140,360      5.57x   ← Stabilizes
250     83,072     369,992      4.45x
500     166,998    740,168      4.43x
1000    334,850    1,480,520    4.42x   ← Stable
```

**Analysis**:
- Overhead ratio: 5.06x → 4.42x (12.6% change, actually *decreasing*)
- Size increased: 100x
- Memory increased: Raw 117x, Tracked 102x
- **Verdict**: **Linear** - ratio stabilizes, slight decrease due to amortization

### Test 3: Mixed Structure (5 → 100 sections, most realistic)

```
Size    Raw        Tracked    Ratio
5       7,005      42,860     6.12x
10      13,954     85,464     6.12x   ← Same!
20      27,862     170,722    6.13x
50      68,018     424,928    6.25x
100     135,990    849,650    6.25x   ← Stable
```

**Analysis**:
- Overhead ratio: 6.12x → 6.25x (**2.1% change** - excellent!)
- Size increased: 20x
- Memory increased: Raw 19.4x, Tracked 19.8x
- **Verdict**: ✅ **Perfectly linear!** This is closest to real Quarto configs

### Test 4: Nested Structures (depth=5, breadth 2 → 5)

```
Breadth  Total Nodes  Raw          Tracked       Ratio
2        32           18,010       146,128       8.11x
3        243          85,124       801,526       9.42x
4        1,024        434,836      3,597,208     8.27x
5        3,125        1,092,680    9,674,890     8.85x
```

**Analysis**:
- Overhead ratio: 8.11x → 8.85x (9.1% change)
- Nodes increased: 98x (32 → 3,125)
- **Verdict**: ✅ **Linear** even with deep nesting

## Why Flat Array Shows 26% Change?

The "26% change" in flat arrays is **not** superlinear growth. It's **fixed costs amortizing**:

### Small Size (10 items): 3.45x overhead
- Fixed overhead (YamlWithSourceInfo struct, Children enum, etc.) is significant
- Relative to tiny data size, fixed costs dominate

### Large Size (1000 items): 4.35x overhead
- Same fixed overhead, but now spread over 1000 items
- Per-item overhead dominates, fixed costs negligible
- **Ratio stabilizes** at 4.35x

This is **exactly what we want** - it means overhead is primarily per-item, not per-size-squared or worse.

## Mathematical Verification

For linear scaling, memory should follow: `M(n) = a + b·n`

Where:
- `a` = fixed overhead
- `b` = per-item overhead
- `n` = number of items

Looking at flat array results:

```
n=100:  M = 52,836
n=1000: M = 528,036

Per-item overhead: (528,036 - 52,836) / (1000 - 100) = 528 bytes/item
```

This matches the "528.0 bytes per item" reported at n=1000. ✅

## Practical Implications

### For Quarto Configs

Typical Quarto project config (~100 keys):
- Raw: ~136 KB
- Tracked: ~850 KB
- Overhead: 6.25x (stable ratio)

Large Quarto project (1000 keys) - unlikely but possible:
- Raw: ~1.3 MB
- Tracked: ~8.5 MB
- Overhead: Still 6.25x (same ratio!)

**No superlinear explosion** - memory grows proportionally.

### Worst Case: Deep Nesting

Even with pathological depth=5, breadth=5 (3,125 nodes):
- Raw: 1.1 MB
- Tracked: 9.7 MB
- Overhead: 8.85x

This is still linear - the higher ratio (8.85x vs 6.25x) is because hash entries are expensive (456 bytes each), but it doesn't grow superlinearly.

## Comparison to Alternatives

### If We Had O(n²) Scaling (hypothetical bad case):

```
Size    Linear (actual)  Quadratic (bad)
10      5,496           ~5,000
100     52,836          ~500,000       (10x worse!)
1000    528,036         ~50,000,000    (100x worse!)
```

We're seeing **linear**, not quadratic. 🎉

### If We Had O(n log n) Scaling:

```
Size    Linear (actual)  n log n (bad)
10      5,496           ~5,000
100     52,836          ~100,000       (2x worse)
1000    528,036         ~3,000,000     (6x worse)
```

We're not seeing this either - ratio stays constant.

## Why This Matters

### Memory Usage is Predictable

- 10 KB config → ~60 KB tracked (6x)
- 100 KB config → ~600 KB tracked (6x)
- 1 MB config → ~6 MB tracked (6x)

**Predictable scaling** means no surprises with large configs.

### No Performance Cliffs

With superlinear growth, you'd hit a "cliff" where:
- Small configs work fine
- Medium configs slow down noticeably
- Large configs become unusable

**Linear scaling** means smooth, predictable performance across all sizes.

### Validation for Design

The owned-data approach with parallel children structure:
- ✅ Scales linearly (verified)
- ✅ Predictable memory usage
- ✅ No pathological cases
- ✅ Simple implementation
- ✅ No lifetime complexity

## Detailed Scaling Behavior

### Per-Item Overhead by Structure Type

| Structure Type | Bytes per Item | Notes |
|---------------|----------------|-------|
| Flat Array | 528 | YamlWithSourceInfo + SourceInfo |
| Flat Hash | 1,480 | Includes YamlHashEntry (456 bytes!) |
| Mixed (realistic) | 8,497 | Nested hashes + arrays + scalars |
| Deep Nested | ~3,100 | More hash entries at each level |

Hash entries are expensive (456 bytes each) because they store:
- 2× YamlWithSourceInfo (288 bytes)
- 3× SourceInfo (168 bytes)

But even with expensive entries, scaling remains **linear**.

## Conclusion

✅ **Overhead scales linearly O(n)** - verified across multiple test cases:
- Flat arrays: Stable at 4.35x (after initial warmup)
- Flat hashes: Stable at 4.42x
- Mixed structures: **2.1% variation** (excellent!)
- Deep nesting: 9.1% variation (good)

✅ **No superlinear growth** - memory increases proportionally with data size

✅ **Predictable behavior** - can estimate memory usage for any config size

✅ **Design validated** - owned data approach works well at scale

**Recommendation**: The current implementation is production-ready. The linear scaling means we won't encounter performance cliffs or memory explosions with larger configs.

## Benchmark Tool

Run the scaling analysis:
```bash
cd crates/quarto-yaml
cargo bench --bench scaling_overhead
```

Tests:
- Flat arrays: 10 → 1000 items
- Flat hashes: 10 → 1000 pairs
- Mixed structures: 5 → 100 sections (realistic Quarto configs)
- Nested structures: depth=5, breadth 2→5 (3,125 nodes max)

All tests confirm **linear scaling**. 🚀
