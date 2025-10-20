# Memory Overhead Analysis

## Executive Summary

**Measured overhead: 6.38x** (not the 3x estimated)

However, this is still **acceptable** for Quarto's use case:
- Typical config files are <10KB
- 10KB × 6.38 = ~64KB total memory
- Provides precise error reporting and LSP support
- Memory is cheap, developer time is expensive

## Benchmark Results

### Base Type Sizes

```
yaml_rust2::Yaml:        56 bytes
YamlWithSourceInfo:     144 bytes  (2.57x larger)
SourceInfo:              56 bytes
YamlHashEntry:          456 bytes  (!!!)
```

### Test Cases

| Test Case | Raw Yaml | YamlWithSourceInfo | Overhead |
|-----------|----------|---------------------|----------|
| Simple scalar | 67 bytes | 267 bytes | **3.99x** |
| Small hash (3 items) | 772 bytes | 4,424 bytes | **5.73x** |
| Small array (5 items) | 809 bytes | 2,866 bytes | **3.54x** |
| Nested structure | 4,402 bytes | 27,924 bytes | **6.34x** |
| Quarto document | 4,991 bytes | 32,175 bytes | **6.45x** |
| Quarto project | 8,275 bytes | 55,576 bytes | **6.72x** |
| **TOTAL** | **19,316 bytes** | **123,232 bytes** | **6.38x** |

## Why Higher Than Expected?

### 1. YamlHashEntry is Heavy (456 bytes!)

Each hash entry contains:
- `key: YamlWithSourceInfo` (144 bytes)
- `value: YamlWithSourceInfo` (144 bytes)
- `key_span: SourceInfo` (56 bytes)
- `value_span: SourceInfo` (56 bytes)
- `entry_span: SourceInfo` (56 bytes)

**Total: 456 bytes per entry**

### 2. Recursive Duplication

`YamlWithSourceInfo` contains:
- `yaml: Yaml` (56 bytes) - the original tree
- `source_info: SourceInfo` (56 bytes)
- `children: Children` (enum with Vec)

The `children` field duplicates the entire tree structure, creating recursive overhead.

### 3. SourceInfo is Not Small

At 56 bytes, `SourceInfo` is as large as `Yaml` itself:
- `file: Option<String>` (24 bytes)
- `offset: usize` (8 bytes)
- `line: usize` (8 bytes)
- `col: usize` (8 bytes)
- `len: usize` (8 bytes)

### 4. Overhead Increases with Nesting

Deeper structures have higher overhead because each level duplicates:
- The Yaml value
- SourceInfo for the node
- Children structure with more YamlWithSourceInfo nodes

## Is This A Problem?

### No, for several reasons:

#### 1. Absolute Numbers Are Small

Even "large" Quarto project configs:
- Raw: 8KB → With tracking: 56KB
- Still fits in L1 cache on modern CPUs
- Negligible compared to typical application memory usage

#### 2. Temporary Data Structure

Config parsing is a one-time operation:
- Parse → Validate → Extract values → Drop YamlWithSourceInfo
- Not held in memory throughout application lifetime
- Only kept for error reporting context

#### 3. Value Proposition

The overhead buys us:
- ✅ Precise error messages with line/col
- ✅ LSP hover showing where config came from
- ✅ Config merging with source tracking
- ✅ Validation errors pointing to exact location
- ✅ "Jump to definition" for config values

#### 4. Proven At Scale

rust-analyzer uses similar approach:
- Owned SyntaxNode with refcounting
- Handles entire Rust codebases (100K+ LOC)
- Memory overhead acceptable

## Optimization Opportunities

If we needed to reduce overhead (we don't), we could:

### 1. Remove Redundant SourceInfo from YamlHashEntry

Currently:
```rust
pub struct YamlHashEntry {
    pub key: YamlWithSourceInfo,     // has source_info
    pub value: YamlWithSourceInfo,   // has source_info
    pub key_span: SourceInfo,        // duplicate!
    pub value_span: SourceInfo,      // duplicate!
    pub entry_span: SourceInfo,
}
```

Could just use:
```rust
pub struct YamlHashEntry {
    pub key: YamlWithSourceInfo,     // use key.source_info
    pub value: YamlWithSourceInfo,   // use value.source_info
    pub entry_span: SourceInfo,      // only this is unique
}
```

**Savings**: 112 bytes per hash entry → ~30% reduction for hashes

### 2. Box SourceInfo

```rust
pub struct YamlWithSourceInfo {
    pub yaml: Yaml,
    pub source_info: Box<SourceInfo>,  // 8 bytes pointer vs 56 bytes struct
    children: Children,
}
```

**Savings**: 48 bytes per node, but adds indirection (slower access)

### 3. Interned Filenames

Instead of `file: Option<String>` in every SourceInfo:
```rust
pub struct SourceInfo {
    pub file_id: Option<u32>,  // index into global string table
    // ...
}
```

**Savings**: ~16 bytes per node with filename

### 4. Compact SourceInfo

```rust
#[repr(C)]
pub struct CompactSourceInfo {
    pub file_id: u16,     // 65K files should be enough
    pub offset: u32,      // 4GB should be enough
    pub line: u16,        // 65K lines should be enough
    pub col: u16,         // 65K columns should be enough
    pub len: u16,         // 65K byte spans should be enough
}
// Total: 12 bytes vs 56 bytes
```

**Savings**: 44 bytes per node → ~70% reduction in SourceInfo overhead

### 5. Single Allocation for Tree

Like rust-analyzer's arena allocation:
- Allocate entire tree in single Vec
- Use indices instead of pointers
- Better cache locality

**Savings**: Reduces allocator overhead, improves cache performance

## Recommendation

**Do nothing.** The current overhead is acceptable because:

1. **Absolute cost is low** (~60KB for typical configs)
2. **Temporary data** (parsed, used, dropped)
3. **High value** (precise error reporting, LSP support)
4. **Simple implementation** (no lifetime complexity)
5. **Proven approach** (rust-analyzer does similar)

If we later discover memory pressure (unlikely), we have clear optimization paths.

## Updating Documentation

Need to update these claims:

### Before
"~3x memory overhead (acceptable for configs <10KB)"

### After
"~6x memory overhead, but still acceptable:
- 10KB config → ~60KB in memory
- Temporary data structure (parse, validate, drop)
- Provides precise error reporting and LSP support"

## Conclusion

The **6.38x overhead is higher than estimated but still acceptable** for Quarto's use case.

The owned data approach remains the right choice:
- ✅ Simple API (no lifetime parameters)
- ✅ Config merging across different lifetimes
- ✅ LSP caching support
- ✅ Memory cost is negligible for typical configs
- ✅ Follows rust-analyzer precedent

**Status**: No changes needed. Ship it! 🚢
