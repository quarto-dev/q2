# Structural Hash-Based AST Reconciliation Design

**Date**: 2025-12-17
**Issue**: k-xvte
**Status**: Implemented (2025-12-18)
**Parent issue**: k-6daf (Good source location tracking after engine outputs)
**Related**: claude-notes/plans/2025-12-15-engine-output-source-location-reconciliation.md

## Executive Summary

This document designs a reconciliation algorithm for PandocAST nodes that enables "splicing in" only the actually-changed parts of a document after engine execution. The approach is directly inspired by React 15's reconciliation algorithm, adapted for our use case where we lack explicit id keys and instead use **structural hash values** computed for each AST node.

## Background: The Problem

During the Quarto rendering pipeline:

1. User provides `.qmd` input → parsed to AST-A with original source locations
2. Engine executes (Jupyter, knitr, etc.) → produces modified `.qmd` output
3. Engine output parsed → AST-B with source locations pointing to intermediate file
4. **We need to reconcile AST-A and AST-B**, producing a final AST with correct source locations

The key insight: **most of the document is unchanged by engine execution**. Code blocks produce outputs, but prose, headers, lists, etc. remain identical.

## The Core Operation: Selective Node Replacement

We do **NOT** "transfer source locations" between ASTs. Instead, we **selectively replace nodes** in AST-A with nodes from AST-B:

- **Unchanged nodes**: KEEP the AST-A node (preserving its original source location)
- **Changed nodes**: REPLACE with the AST-B node (which carries the engine output source location)

### Concrete Example

**Pre-engine (AST-A):**
```markdown
## Hello

foo.

```{python}
print("Hello world")
```

bar.
```

**Post-engine (AST-B):**
```markdown
## Hello

foo.

```
Hello world
```

bar.
```

**Reconciled result:**
```
Header("Hello")              <- KEPT from AST-A, source: original.qmd:1
Paragraph("foo.")            <- KEPT from AST-A, source: original.qmd:3
CodeBlock("Hello world")     <- REPLACED with AST-B node, source: engine-output.md:5-7
Paragraph("bar.")            <- KEPT from AST-A, source: original.qmd:9
```

The CodeBlock with Python code is replaced by the CodeBlock containing the output. When we query the source location of the output block, it correctly points to the engine output file. Everything else retains original source locations.

## Inspiration: React 15 Reconciliation

React's reconciliation algorithm achieves O(n) complexity (vs O(n³) for general tree diff) through two key heuristics:

### Heuristic 1: Type-Based Differentiation
> "Two elements of different types will produce different trees"

If a node changes from `Paragraph` to `Header`, React doesn't try to salvage any of the old subtree—it replaces entirely. This is sound because different types have different semantics.

### Heuristic 2: Keys for List Elements
> "Developer-provided keys help identify stable elements in lists"

When children are reordered or when items are inserted/deleted, keys enable React to match old and new elements without O(n²) comparison. Without keys, a prepended element causes all children to rematch by position.

### React's Recursion Strategy

1. Compare roots. If types differ → replace entire tree.
2. If types match → compare attributes/props, update changed ones.
3. Recurse into children using keys (or position if no keys).

## Adaptation: Structural Hashes as Virtual Keys

Our problem differs from React's:
- We don't have explicit keys—authors don't annotate their markdown
- We're not updating a DOM; we're selectively replacing nodes to preserve source locations
- Our "identity" is structural: a paragraph with text "Hello world" should match another paragraph with the same text

**Solution**: Compute a **structural hash** for each AST node that captures its semantic identity, excluding source locations.

```
hash(node) = hash(type_discriminant || hash(content) || hash(child_0) || hash(child_1) || ...)
```

If two nodes have the same structural hash, they are semantically identical (ignoring source locations). The hash serves as a "virtual key" for reconciliation.

## Structural Hash Design

### What Goes Into the Hash

**Include:**
- Node type discriminant (Block::Paragraph vs Block::Header)
- Leaf content (text strings, numeric values)
- Attributes (id, classes, key-value pairs) - but NOT source info within attrs
- Children hashes (recursively computed)
- Order of children (hash is position-sensitive)

**Exclude:**
- `source_info` fields
- `attr_source` fields
- `target_source` fields
- Any other location-tracking metadata

### Hash Function Choice

| Option | Pros | Cons |
|--------|------|------|
| SHA-256 | Collision-resistant | Slow, overkill |
| FxHash | Very fast, good for hash maps | Not cryptographic |
| xxHash | Fast, good distribution | External dependency |
| SipHash | Rust's default hasher | Moderate speed |

**Recommendation**: Use Rust's standard `std::hash::Hash` trait with `FxHasher` (from `rustc-hash` crate). It's fast, we're already hashing for hash maps elsewhere, and collision resistance isn't critical (we're not doing security, just matching).

### Hash Collision Mitigation

FxHash is not collision-resistant. While collisions are rare, they could cause incorrect source location assignments if two structurally different nodes happen to hash to the same value.

**Mitigation**: When hashes match, perform a structural equality check before emitting `KeepOriginal`:

```rust
if let Some(&orig_idx) = indices.iter().find(|&&i| !used_original.contains(&i)) {
    // Hash match found - verify with structural equality
    if structural_eq_block(&original[orig_idx], exec_block) {
        used_original.insert(orig_idx);
        alignments.push(BlockAlignment::KeepOriginal(orig_idx));
        continue;
    }
    // Hash collision! Fall through to type-based matching or UseExecuted
}
```

**Trade-offs**:
- **Pro**: Eliminates false positives from hash collisions
- **Con**: Structural equality is O(size of subtree), potentially expensive

**Recommendation**: Implement the structural equality check. In practice:
1. Most hash matches are true matches (no extra work after the check)
2. Hash collisions are rare, so the fallback path is seldom taken
3. Correctness is more important than micro-optimization

The `structural_eq_block` function compares nodes recursively, ignoring source_info fields—essentially the same traversal as hashing, but comparing instead of accumulating.

### Implementation Approach: Address-Based Memoization (Recommended)

The key insight is that the **pre-engine AST is immutable and lives for the entire reconciliation process**. This guarantees address stability—Rust won't move nodes while we hold a reference. We can use node memory addresses as cache keys for memoization, avoiding any cloning.

#### Why Not Clone-Based Approaches?

A naive approach would wrap each node with its hash:

```rust
struct HashedNode<T> {
    node: T,  // Requires Clone!
    hash: u64,
}
```

This doubles memory usage and requires cloning all string content, attributes, etc. For large documents, this is prohibitively expensive.

#### Why Not Index-Based Approaches?

We considered using pre-order traversal indices as node identifiers:

```rust
struct HashedAst {
    block_hashes: Vec<u64>,  // Index = pre-order position
}
```

**This doesn't work** because one of the main cases we handle is insertion of blocks into a `Blocks` vector. Insertions and deletions break all subsequent indices, making correlation between the two trees impossible.

#### The Address-Based Solution

Use the memory address of each node as a cache key. Since the original AST is borrowed immutably for the entire reconciliation, addresses are stable.

```rust
use std::marker::PhantomData;
use rustc_hash::FxHashMap;

/// Opaque pointer used as a cache key
#[derive(Hash, Eq, PartialEq, Clone, Copy)]
struct NodePtr(*const ());

/// Cache for structural hashes, tied to the lifetime of an AST
struct HashCache<'a> {
    cache: FxHashMap<NodePtr, u64>,
    _marker: PhantomData<&'a ()>,
}

impl<'a> HashCache<'a> {
    fn new() -> Self {
        Self {
            cache: FxHashMap::default(),
            _marker: PhantomData,
        }
    }

    /// Get or compute the structural hash for a block
    fn hash_block(&mut self, block: &'a Block) -> u64 {
        let ptr = NodePtr(block as *const Block as *const ());
        if let Some(&hash) = self.cache.get(&ptr) {
            return hash;
        }
        let hash = self.compute_block_hash(block);
        self.cache.insert(ptr, hash);
        hash
    }

    fn compute_block_hash(&mut self, block: &'a Block) -> u64 {
        let mut hasher = FxHasher::default();

        // Hash the discriminant
        std::mem::discriminant(block).hash(&mut hasher);

        // Hash content based on type (recursively)
        match block {
            Block::Paragraph(p) => {
                for inline in &p.content {
                    self.hash_inline(inline).hash(&mut hasher);
                }
            }
            Block::CodeBlock(cb) => {
                self.hash_attr(&cb.attr).hash(&mut hasher);
                cb.text.hash(&mut hasher);
            }
            Block::Div(d) => {
                self.hash_attr(&d.attr).hash(&mut hasher);
                for child in &d.content {
                    self.hash_block(child).hash(&mut hasher);
                }
            }
            // ... other variants (see implementation note below)
        }

        hasher.finish()
    }

    // Similar methods for hash_inline, hash_attr, etc.
}
```

**Implementation Note**: The examples above show only a few Block variants for brevity. The actual implementation must handle **all** Block and Inline variants:

- **Block variants** (16 total): Plain, Paragraph, LineBlock, CodeBlock, RawBlock, BlockQuote, OrderedList, BulletList, DefinitionList, Header, HorizontalRule, Table, Figure, Div, MetaBlock, NoteDefinitionPara, NoteDefinitionFencedBlock, CaptionBlock

- **Inline variants** (27 total): Str, Emph, Underline, Strong, Strikeout, Superscript, Subscript, SmallCaps, Quoted, Cite, Code, Space, SoftBreak, LineBreak, Math, RawInline, Link, Image, Note, Span, Shortcode, NoteReference, Attr, Insert, Delete, Highlight, EditComment

Each variant requires appropriate recursive handling in:
1. `compute_block_hash` / `compute_inline_hash`
2. `structural_eq_block` / `structural_eq_inline`
3. `is_container_block` / `is_container_inline`
4. `compute_container_plan` / `apply_container_reconciliation`

#### Why This Works

1. **Address stability**: Rust guarantees a value's address doesn't change while borrowed. Since we hold `&'a Pandoc`, all nodes have stable addresses for lifetime `'a`.

2. **Lifetime safety**: `PhantomData<&'a ()>` ties the cache to the AST's lifetime. The cache becomes invalid when the AST is dropped—the borrow checker enforces this.

3. **No unsafe code**: We use raw pointers only as opaque keys in a HashMap. No dereferencing, no unsafe blocks needed.

4. **Memory efficiency**: Only 16 bytes per cached node (8-byte pointer + 8-byte hash), and we only cache nodes we actually visit.

5. **Lazy computation**: If root hashes match (common case!), we compute very few hashes before bailing out.

#### Asymmetric Caching Strategy

We only cache hashes for the **original AST** (AST-A), not the executed AST (AST-B):

```rust
/// Compute hash without caching (for executed AST, single traversal)
fn compute_hash_fresh(block: &Block) -> u64 {
    // Same logic as HashCache::compute_block_hash, but no caching
}
```

**Rationale**:
- Original AST hashes are looked up repeatedly during alignment, so caching pays off.
- Executed AST is traversed exactly once; caching would add overhead without benefit.

#### Two-Phase API Design

The reconciliation is split into two distinct functions with clear responsibilities:

```rust
/// Phase 1: Compute the reconciliation plan (pure, no mutation)
/// Both ASTs are borrowed immutably - this is just analysis
fn compute_reconciliation(
    original: &Pandoc,
    executed: &Pandoc,
) -> ReconciliationPlan {
    // Uses address-based hash cache internally
    // Returns plan with indices and diagnostics
}

/// Phase 2: Apply the plan to produce the merged AST
/// Both ASTs are consumed - enables zero-copy moves
fn apply_reconciliation(
    original: Pandoc,
    executed: Pandoc,
    plan: &ReconciliationPlan,
) -> Pandoc {
    // MOVE from original for KeepOriginal
    // MOVE from executed for UseExecuted
    // Zero cloning!
}

/// Convenience wrapper
fn reconcile(original: Pandoc, executed: Pandoc) -> (Pandoc, ReconciliationPlan) {
    let plan = compute_reconciliation(&original, &executed);
    let result = apply_reconciliation(original, executed, &plan);
    (result, plan)
}
```

**Why this factoring?**

1. **Compute is pure**: No mutation, easy to test, can be called speculatively
2. **Plan is inspectable**: Useful for dry-run, debugging, driving other transformations
3. **Apply is optimal**: Both inputs are owned, so we MOVE everything (zero cloning!)
4. **Caller controls ownership**: If they need to keep an AST, they clone before calling

**The ReconciliationPlan:**

```rust
struct ReconciliationPlan {
    /// Block-level alignments for this scope
    block_alignments: Vec<BlockAlignment>,

    /// Nested plans for block containers (Div, BlockQuote, etc.)
    /// Key: index into block_alignments where alignment is RecurseIntoContainer
    block_container_plans: HashMap<usize, ReconciliationPlan>,

    /// Inline plans for blocks with inline content (Paragraph, Header, etc.)
    /// Key: index into block_alignments
    inline_plans: HashMap<usize, InlineReconciliationPlan>,

    /// Diagnostics
    stats: ReconciliationStats,
}

struct InlineReconciliationPlan {
    inline_alignments: Vec<InlineAlignment>,
    /// Nested plans for inline containers (Emph, Strong, Link, etc.)
    inline_container_plans: HashMap<usize, InlineReconciliationPlan>,
    /// For Note inlines, which contain Blocks
    note_block_plans: HashMap<usize, ReconciliationPlan>,
}

struct ReconciliationStats {
    blocks_kept: usize,
    blocks_replaced: usize,
    blocks_recursed: usize,
    inlines_kept: usize,
    inlines_replaced: usize,
    inlines_recursed: usize,
}
```

**Apply phase implementation:**

Since both inputs are owned, we can move nodes directly:

```rust
fn apply_reconciliation(
    mut original: Pandoc,
    mut executed: Pandoc,
    plan: &ReconciliationPlan,
) -> Pandoc {
    // Convert to Option<Block> so we can take ownership of individual blocks
    let mut orig_blocks: Vec<Option<Block>> =
        original.blocks.drain(..).map(Some).collect();
    let mut exec_blocks: Vec<Option<Block>> =
        executed.blocks.drain(..).map(Some).collect();

    let mut result_blocks = Vec::with_capacity(plan.block_alignments.len());

    for alignment in &plan.block_alignments {
        match alignment {
            BlockAlignment::KeepOriginal(orig_idx) => {
                // MOVE from original (no clone!)
                result_blocks.push(orig_blocks[*orig_idx].take().unwrap());
            }
            BlockAlignment::UseExecuted(exec_idx) => {
                // MOVE from executed (no clone!)
                result_blocks.push(exec_blocks[*exec_idx].take().unwrap());
            }
        }
    }

    Pandoc {
        meta: original.meta,  // Or reconcile metadata similarly
        blocks: result_blocks,
    }
}
```

**Memory efficiency**:
- 100% of nodes are MOVED, zero cloning
- Both input ASTs are consumed (caller clones if needed)
- Optimal for the common case where inputs aren't needed after reconciliation

## Reconciliation Algorithm

### Overview

```
compute_reconciliation(original: &Pandoc, executed: &Pandoc) -> ReconciliationPlan:

    1. Compute root hashes (cached for original, fresh for executed)
       If hashes match:
       - Trees are identical
       - Return plan with all KeepOriginal (early exit optimization)

    2. Compute block alignments:
       - Hash original.blocks (cached) and executed.blocks (fresh)
       - Build hash→[original indices] multimap
       - Align: for each executed block, look up by hash
       - Result: list of (KeepOriginal | UseExecuted) decisions

    3. Recurse into containers to build nested plans

    4. Compute metadata alignment similarly

    5. Return complete ReconciliationPlan


apply_reconciliation(original: Pandoc, executed: Pandoc, plan: &ReconciliationPlan) -> Pandoc:

    1. Drain both ASTs into Vec<Option<Block>> for selective moves

    2. Build result by iterating plan:
       - For KeepOriginal(i): MOVE from original[i]
       - For UseExecuted(i): MOVE from executed[i]

    3. Handle nested container plans recursively

    4. Return merged Pandoc
```

### Children Alignment Algorithm

This is the key insight from React: use keys (hashes) to align children efficiently.

```rust
fn compute_block_alignments(
    original: &[Block],
    original_hashes: &[u64],  // Pre-computed hashes for original blocks
    executed: &[Block],
    hash_cache: &mut HashCache<'_>,  // For computing nested container plans
) -> (Vec<BlockAlignment>, HashMap<usize, ReconciliationPlan>) {
    // Build hash -> indices map for original children
    // (multimap because duplicates are possible)
    let mut hash_to_indices: FxHashMap<u64, Vec<usize>> = FxHashMap::default();
    for (idx, &hash) in original_hashes.iter().enumerate() {
        hash_to_indices.entry(hash)
            .or_default()
            .push(idx);
    }

    let mut alignments = Vec::new();
    let mut container_plans = HashMap::new();
    let mut used_original: HashSet<usize> = HashSet::new();

    // For each executed child, find a matching original by hash
    for (exec_idx, exec_block) in executed.iter().enumerate() {
        let exec_hash = compute_hash_fresh(exec_block);

        // Step 1: Try exact hash match first
        if let Some(indices) = hash_to_indices.get(&exec_hash) {
            if let Some(&orig_idx) = indices.iter().find(|&&i| !used_original.contains(&i)) {
                used_original.insert(orig_idx);
                alignments.push(BlockAlignment::KeepOriginal(orig_idx));
                continue;
            }
        }

        // Step 2: No hash match - try type-based matching for containers
        // Look for an unused original with the same type (discriminant)
        let exec_discriminant = std::mem::discriminant(exec_block);
        let type_match = original.iter().enumerate()
            .filter(|(i, _)| !used_original.contains(i))
            .find(|(_, orig_block)| {
                std::mem::discriminant(*orig_block) == exec_discriminant
                    && is_container_block(orig_block)
            });

        if let Some((orig_idx, orig_block)) = type_match {
            // Container with same type but different hash: recurse
            used_original.insert(orig_idx);

            // Pre-compute the nested reconciliation plan for this container
            let nested_plan = compute_container_plan(orig_block, exec_block, hash_cache);
            container_plans.insert(alignments.len(), nested_plan);

            alignments.push(BlockAlignment::RecurseIntoContainer {
                original_idx: orig_idx,
                executed_idx: exec_idx,
            });
            continue;
        }

        // Step 3: No match at all - use executed node
        alignments.push(BlockAlignment::UseExecuted(exec_idx));
    }

    // Note: Original-only nodes (deleted by engine) are simply not included
    // in the result. This is intentional - code cells may produce no output.

    (alignments, container_plans)
}

/// Check if a block is a container (has children that need reconciliation)
fn is_container_block(block: &Block) -> bool {
    matches!(block,
        Block::Div(_) |
        Block::BlockQuote(_) |
        Block::OrderedList(_) |
        Block::BulletList(_) |
        Block::DefinitionList(_) |
        Block::Figure(_) |
        Block::Note(_)
    )
}

/// Compute a nested reconciliation plan for a container's children
fn compute_container_plan(
    orig_block: &Block,
    exec_block: &Block,
    hash_cache: &mut HashCache<'_>,
) -> ReconciliationPlan {
    // Extract children from both containers and recursively compute plan
    // Implementation varies by container type (Div, BlockQuote, lists, etc.)
    match (orig_block, exec_block) {
        (Block::Div(orig), Block::Div(exec)) => {
            compute_reconciliation_for_blocks(&orig.content, &exec.content, hash_cache)
        }
        (Block::BlockQuote(orig), Block::BlockQuote(exec)) => {
            compute_reconciliation_for_blocks(&orig.content, &exec.content, hash_cache)
        }
        // ... other container types
        _ => unreachable!("is_container_block should ensure matching types"),
    }
}
```

The algorithm has three steps:
1. **Exact hash match**: If hashes match, `KeepOriginal` - node is identical
2. **Type-based container match**: If types match and it's a container, `RecurseIntoContainer` - keep container's source location, reconcile children
3. **No match**: `UseExecuted` - new or transformed content from engine

This ensures containers preserve their original source locations even when children change, while still correctly reconciling the children themselves.

### Alignment Result Types

The alignment phase produces a simple decision for each position in the result:

```rust
enum BlockAlignment {
    /// Keep the original node (hashes matched exactly)
    /// Action: MOVE from original (preserves source location, zero-copy)
    KeepOriginal(usize),  // Index into original blocks

    /// Use the executed node (no match found)
    /// Action: MOVE from executed (gets engine output source location, zero-copy)
    UseExecuted(usize),   // Index into executed blocks

    /// Container with same type but different hash (children changed)
    /// Action: MOVE container from original, but recurse into children
    /// The nested ReconciliationPlan specifies how to reconcile children
    RecurseIntoContainer {
        original_idx: usize,
        executed_idx: usize,
    },
}
```

The alignment algorithm determines which decision applies to each position. Key cases:

- **Exact hash match**: `KeepOriginal` - the node is unchanged, keep it entirely
- **Type match, hash mismatch (container)**: `RecurseIntoContainer` - keep container's source location, reconcile children
- **Type match, hash mismatch (leaf)**: `UseExecuted` - content changed, use engine output
- **No type match**: `UseExecuted` - new or transformed node from engine
- **Original-only**: Node deleted by engine - simply not included in result

### Why No Source Location Transfer?

Unlike previous designs, we don't "transfer" source_info fields between nodes. Instead:

- **KeepOriginal**: The original node already has the correct source location—we MOVE it
- **UseExecuted**: The executed node already has the correct source location (engine output)—we MOVE it

Source locations are naturally correct because we're keeping/replacing **entire nodes**, not manipulating their fields.

## Special Cases

### Code Blocks

Code blocks are the most common case of changed content. The hash will differ because:
- Original: `CodeBlock({python}, "print('Hello')")`
- Executed: `CodeBlock({}, "Hello")`

Since hashes differ, we use `UseExecuted` → the output block (with engine output source location) replaces the input block.

**No special handling needed**: The general algorithm correctly handles this case.

### Inline Code Execution

Some engines evaluate inline code (e.g., `` `r 1+1` `` → `2`):
- Original inline: `Code("r 1+1")`
- Executed inline: `Str("2")`

Different types, different hashes → `UseExecuted`. The evaluated result carries its engine output source location.

### Nested Structures (Containers)

For containers (Div, BlockQuote, lists), we need to handle the case where the container itself is unchanged but its children differ. This is done via the `RecurseIntoContainer` alignment:

```rust
fn apply_alignment_to_block(
    orig_block: Block,      // MOVED from original
    exec_block: Block,      // MOVED from executed
    alignment: &BlockAlignment,
    nested_plan: Option<&ReconciliationPlan>,  // Pre-computed during compute phase
) -> Block {
    match alignment {
        BlockAlignment::KeepOriginal(_) => {
            // Exact hash match - use original entirely
            orig_block
        }
        BlockAlignment::UseExecuted(_) => {
            // No match - use executed entirely
            exec_block
        }
        BlockAlignment::RecurseIntoContainer { .. } => {
            // Type match but hash differs - keep container's source location,
            // but reconstruct children according to the nested plan
            let plan = nested_plan.expect("RecurseIntoContainer must have nested plan");
            apply_container_reconciliation(orig_block, exec_block, plan)
        }
    }
}

fn apply_container_reconciliation(
    orig_container: Block,
    exec_container: Block,
    plan: &ReconciliationPlan,
) -> Block {
    // Keep the original container's metadata (source_info, attr, attr_source),
    // but replace its children with the reconciled children
    match (orig_container, exec_container) {
        (Block::Div(mut orig), Block::Div(exec)) => {
            orig.content = apply_reconciliation_to_blocks(
                orig.content,
                exec.content,
                plan,
            );
            Block::Div(orig)
        }
        (Block::BlockQuote(mut orig), Block::BlockQuote(exec)) => {
            orig.content = apply_reconciliation_to_blocks(
                orig.content,
                exec.content,
                plan,
            );
            Block::BlockQuote(orig)
        }
        // ... other container types
        _ => unreachable!("type matching ensures same container types"),
    }
}
```

**Key insight**: The nested `ReconciliationPlan` is computed during the compute phase, not during apply. This keeps `compute_reconciliation` pure and `apply_reconciliation` simple—it just follows the pre-computed plan.

### Why RecurseIntoContainer?

When a container's hash changes because a child changed:

- Original Div: contains [Para("foo"), CodeBlock({py}, "...")]
- Executed Div: contains [Para("foo"), CodeBlock({}, "output")]

The Div's hash changes (because children hashes changed), but we want to:
1. Keep the Div's source location (it's the same Div structurally)
2. Keep Para("foo")'s source location (it hasn't changed)
3. Use the executed CodeBlock (it changed)

The `RecurseIntoContainer` alignment achieves this by:
1. Type-matching the Divs (same discriminant)
2. Pre-computing a nested plan for the children
3. During apply: moving the original Div but replacing its children per the nested plan

## Inline Reconciliation

Inline nodes need reconciliation using the same technique as blocks. When a Paragraph's hash differs between original and executed, we need to reconcile its inline content to preserve source locations for unchanged text.

### InlineAlignment Type

```rust
enum InlineAlignment {
    /// Keep the original inline (hashes matched exactly)
    KeepOriginal(usize),

    /// Use the executed inline (no match found)
    UseExecuted(usize),

    /// Container inline (Emph, Strong, Link, etc.) with changed children
    RecurseIntoContainer {
        original_idx: usize,
        executed_idx: usize,
    },
}
```

### Inline Container Types

The following inline types are containers that may need recursive reconciliation:

```rust
fn is_container_inline(inline: &Inline) -> bool {
    matches!(inline,
        Inline::Emph(_) |
        Inline::Strong(_) |
        Inline::Underline(_) |
        Inline::Strikeout(_) |
        Inline::Superscript(_) |
        Inline::Subscript(_) |
        Inline::SmallCaps(_) |
        Inline::Quoted(_) |
        Inline::Cite(_) |
        Inline::Link(_) |
        Inline::Image(_) |
        Inline::Span(_) |
        Inline::Note(_)  // Contains Blocks!
    )
}
```

### Special Case: Note Contains Blocks

`Inline::Note` is unusual because it contains `Blocks`, not `Inlines`. When reconciling a Note, we recursively apply block-level reconciliation to its content:

```rust
(Inline::Note(orig), Inline::Note(exec)) => {
    // Note contains Blocks, so use block reconciliation
    compute_reconciliation_for_blocks(&orig.content, &exec.content, hash_cache)
}
```

### Inline Code Evaluation

When engines evaluate inline code (e.g., `` `r 1+1` `` → `2`):

- Original: `Inline::Code("r 1+1")`
- Executed: `Inline::Str("2")`

Since types differ, there's no type-based match. The algorithm correctly emits `UseExecuted`, and the evaluated result carries its engine output source location.

### Extended ReconciliationPlan

The plan must include inline alignments for blocks that recurse:

```rust
struct ReconciliationPlan {
    /// Block-level alignments
    block_alignments: Vec<BlockAlignment>,

    /// Nested plans for block containers (Div, BlockQuote, etc.)
    block_container_plans: HashMap<usize, ReconciliationPlan>,

    /// Inline alignments for blocks with RecurseIntoContainer
    /// Key: index into block_alignments for blocks whose inlines need reconciliation
    inline_plans: HashMap<usize, InlineReconciliationPlan>,

    /// Diagnostics
    stats: ReconciliationStats,
}

struct InlineReconciliationPlan {
    inline_alignments: Vec<InlineAlignment>,
    /// Nested plans for inline containers (Emph, Strong, etc.)
    inline_container_plans: HashMap<usize, InlineReconciliationPlan>,
    /// For Note inlines, which contain Blocks
    note_block_plans: HashMap<usize, ReconciliationPlan>,
}
```

### When Blocks Need Inline Reconciliation

For `RecurseIntoContainer` blocks that contain inlines (like Paragraph), we also compute an inline reconciliation plan:

```rust
BlockAlignment::RecurseIntoContainer { original_idx, executed_idx } => {
    match (&original[original_idx], &executed[executed_idx]) {
        // Block containers with block children
        (Block::Div(orig), Block::Div(exec)) => {
            // Recurse into block children (already handled)
        }
        // "Leaf" blocks with inline content
        (Block::Paragraph(orig), Block::Paragraph(exec)) => {
            // Compute inline reconciliation plan
            let inline_plan = compute_inline_alignments(
                &orig.content, &exec.content, hash_cache
            );
            inline_plans.insert(alignment_idx, inline_plan);
        }
        // ... Header, Plain, etc. also have inline content
    }
}
```

**Note**: Paragraph is not a "container" for blocks, but it contains inlines. When a Paragraph's hash differs (because an inline changed), we type-match the Paragraphs and reconcile their inline content.

## Metadata Handling

The YAML front matter is stored in `Pandoc.meta` as `MetaValueWithSourceInfo`. For v1, we use a simple rule:

**If metadata changes, the executed metadata wins entirely.**

```rust
fn reconcile_metadata(
    original_meta: MetaValueWithSourceInfo,
    executed_meta: MetaValueWithSourceInfo,
) -> MetaValueWithSourceInfo {
    // Simple v1 approach: use executed metadata if different
    // This means source locations for changed metadata point to engine output
    executed_meta
}
```

### Rationale

1. **Engines can modify metadata**: Some engines (like knitr) can evaluate expressions in YAML front matter
2. **Structural diff is complex**: MetaValueWithSourceInfo has nested maps/lists that would need their own reconciliation
3. **Impact is limited**: Most YAML front matter doesn't change during engine execution

### Future Work

A future version could reconcile metadata at the mapping level, preserving source locations for unchanged fields while updating changed ones. This is tracked in issue **k-7bty**.

The recursive structure would be:
- For MetaMap: reconcile each key-value pair
- For MetaList: align items similar to block alignment
- For scalar values (MetaString, MetaBool, etc.): exact match or replace

## Handling Duplicates

When multiple nodes have the same hash (e.g., two identical paragraphs), we need a strategy.

### Example: Duplicate Paragraphs

```
Pre-engine:                    Post-engine:
[0] Paragraph("Hello.")        [0] Paragraph("Hello.")
[1] CodeBlock({py}, "1")       [1] CodeBlock({}, "1")
[2] Paragraph("Hello.")        [2] Paragraph("Hello.")
```

Both "Hello." paragraphs have the same hash (H1). The hash→indices map is:
```
H1 → [0, 2]
C1 → [1]
```

### How First-Come-First-Served Works

We iterate executed blocks **in order**, and for each, find the **first unused** original:

| exec_idx | exec_hash | Available originals | Chosen | Decision |
|----------|-----------|---------------------|--------|----------|
| 0 | H1 | [0, 2] | 0 | `KeepOriginal(0)` |
| 1 | C2 | — | — | `UseExecuted(1)` |
| 2 | H1 | [2] (0 used) | 2 | `KeepOriginal(2)` |

Result: Each "Hello." paragraph keeps its own original source location.

### Why This Works

Engine execution **preserves document order**—it doesn't shuffle paragraphs. So the first "Hello." in executed naturally corresponds to the first "Hello." in original.

### When Would This Fail?

Only if the engine **reorders** blocks with identical hashes. Then we'd assign wrong source locations. But:
1. Engines don't reorder content
2. Even if they did, the content would still be correct—just source locations swapped between identical nodes

### Alternative Strategies (Not Needed Yet)

**Option 2: Position-weighted matching**
- For duplicates, prefer the original closest in position to the executed block
- More complex but handles pathological cases better

**Option 3: LCS on same-hash groups**
- Use longest common subsequence within groups
- Most accurate but O(n²) in worst case

**Recommendation**: Start with first-come-first-served. It handles realistic scenarios correctly, and we can add complexity if real-world issues arise.

## Performance Analysis

### Time Complexity

1. **Hash computation**: O(n) - visit each node once
2. **Hash map construction**: O(n) - insert each original node's hash
3. **Alignment**: O(n) - iterate executed nodes, O(1) hash lookup
4. **Source transfer**: O(n) - visit matched pairs once

**Total: O(n)** where n is the number of nodes.

### Space Complexity

1. **Hash cache**: O(n) - 16 bytes per cached node (pointer + hash)
2. **Hash→indices map**: O(n) - one entry per original node at each recursion level
3. **Alignment list**: O(n) - one entry per node at each recursion level

**Total: O(n)** additional space, with small constants since we avoid cloning AST content.

### Comparison to Alternatives

| Approach | Time | Space | Notes |
|----------|------|-------|-------|
| Structural hash (this design) | O(n) | O(n) | Fast, handles insertions well |
| Naive tree diff | O(n³) | O(n²) | Too slow for large docs |
| Linear position matching | O(n) | O(1) | Fails on insertions |
| LCS-based alignment | O(n²) | O(n) | Handles reordering, slower |

## Implementation Plan

### Phase 1: Hash Infrastructure

1. Create `NodePtr` newtype for pointer-based cache keys
2. Implement `HashCache<'a>` with address-based memoization
3. Implement `compute_block_hash`, `compute_inline_hash`, etc. excluding source_info
4. Implement `compute_hash_fresh` (non-caching variant for executed AST)
5. Unit tests for hash determinism: same content → same hash, different content → different hash

### Phase 2: ReconciliationPlan Types

1. Define `BlockAlignment` enum (`KeepOriginal`, `UseExecuted`)
2. Define `ReconciliationPlan` struct with nested plans for containers
3. Define `ReconciliationStats` for diagnostics
4. Unit tests for plan construction

### Phase 3: Compute Phase

1. Implement `compute_reconciliation(&Pandoc, &Pandoc) -> ReconciliationPlan`
2. Implement `compute_block_alignments` with hash→indices multimap and first-come-first-served duplicate handling
3. Handle nested containers by building recursive plans
4. Early exit when root hashes match
5. Unit tests with synthetic AST pairs

### Phase 4: Apply Phase

1. Implement `apply_reconciliation(Pandoc, Pandoc, &ReconciliationPlan) -> Pandoc`
2. Use drain/Option pattern for zero-copy moves from both inputs
3. Handle nested container plans recursively
4. Unit tests verifying move semantics (no cloning)

### Phase 5: Convenience API

1. Implement `reconcile(Pandoc, Pandoc) -> (Pandoc, ReconciliationPlan)`
2. Handle metadata reconciliation
3. Comprehensive end-to-end tests

### Phase 6: Integration

1. Add `reconcile` module to appropriate crate (quarto-pandoc-types or quarto-core)
2. Hook into render pipeline after engine execution
3. Integration tests with real engine outputs (Jupyter, knitr)
4. Performance benchmarks with large documents

## Open Questions

### Q1: Should we hash attributes differently for code blocks?

Code block identity is primarily determined by their attributes (language, id, classes), not their content (which is replaced by execution). Should we use a separate "identity hash" for code blocks?

**Context**: When a code block is executed, its content changes completely:
- Original: `CodeBlock({python, id="cell-1"}, "print('Hello')")`
- Executed: `CodeBlock({}, "Hello")`

The hashes differ (different content AND attributes), so the algorithm emits `UseExecuted`. The executed block carries its engine output source location, which is correct for error reporting in the output.

**The question**: Should we try to match code blocks by attributes (id, language) even when content differs, similar to how we match containers by type? This could enable:
- Preserving `attr_source` from the original (source location of `{python, id="cell-1"}`)
- Creating a "hybrid" CodeBlock: original's attr_source + executed's content/source_info

**Current answer**: No special handling. CodeBlock is a leaf node (not a container), so when its hash differs, we use `UseExecuted` entirely. This is correct because:
1. The executed block's source_info correctly points to engine output
2. Preserving attr_source is a micro-optimization that adds complexity
3. Engine outputs may not even have the same attributes (e.g., output blocks often lack the language class)

**If we later find this insufficient**, we could add an `AttributeMatchLeaf` alignment type that keeps attr_source from original while taking content from executed. But this is deferred unless real-world use cases demand it.

**Resolved**: No special handling for v1. The current approach (UseExecuted for changed leaf nodes) is sufficient.

### Q2: How to handle whitespace normalization by engines?

Engines may normalize whitespace differently. If a Paragraph has `["Hello", Space, "world"]` in original but `["Hello world"]` in executed, they won't hash-match.

**Options**:
- Normalize whitespace before hashing (lossy)
- Accept that whitespace-only changes won't match (conservative)
- Special-case Space/SoftBreak in comparison (complex)

**Tentative answer**: Start conservative. Real-world testing will reveal if this is a problem.

### Q3: Should hashes be stored permanently or computed on-demand?

If we anticipate multiple reconciliation passes (e.g., multiple engine stages), storing hashes saves recomputation.

**Resolved**: Use address-based memoization with `HashCache<'a>`. Hashes are computed on-demand and cached using node addresses as keys. This gives us:
- Lazy computation (only hash what we visit)
- Automatic caching (repeated lookups are O(1))
- No cloning overhead
- Lifetime-safe via PhantomData

For the executed AST, we compute hashes fresh without caching since it's traversed only once.

### Q4: How does this interact with incremental parsing?

For LSP and IDE use cases, we may want incremental reconciliation as the user edits.

**Tentative answer**: This design focuses on the render pipeline. Incremental reconciliation is a separate (and more complex) problem.

## Design Decisions Summary

| Decision | Choice | Rationale |
|----------|--------|-----------|
| API structure | Separate compute/apply phases | Compute is pure; plan is inspectable; apply is optimal |
| Ownership model | Both inputs consumed in apply | Enables 100% moves, zero cloning |
| Core operation | Node replacement, not field transfer | Simpler; source locations naturally correct |
| Unchanged nodes | MOVE from original | Zero-copy; preserves source location |
| Changed nodes | MOVE from executed | Zero-copy; gets engine output source location |
| Container matching | Type-based + RecurseIntoContainer | Preserve container source locations when children change |
| Inline reconciliation | Same algorithm as blocks | Consistency; preserves source locations for unchanged text |
| Hash function | FxHash via `std::hash::Hash` | Fast, good enough for non-crypto use |
| Hash collision | Structural equality check on match | Correctness over micro-optimization |
| Hash storage | Address-based memoization | No cloning during hash phase; 16 bytes/node; lifetime-safe |
| Cache scope | Original AST only | Executed AST traversed once, caching adds overhead |
| Node identity | Memory address (`*const ()`) | Stable while borrowed; no index invalidation on insert/delete |
| Duplicate handling | First-come-first-served by document order | Simple; handles order-preserving engines correctly |
| Metadata handling | Executed wins entirely (v1) | Simple; future work (k-7bty) for field-level reconciliation |
| Nested plans | Pre-computed during compute phase | Keeps compute pure, apply simple |

## Test Resources

Test files for before/after engine execution are in `resources/ast-reconciliation-examples/`:

- `01.before.qmd` / `01.after.qmd`: Basic R code block execution
  - Before: `{r}` code block with `cat("Hello world")`
  - After: Wrapped in `::: {.cell}` div with output in `::: {.cell-output}` div

- `02.before.qmd` / `02.after.qmd`: Inline code + code block
  - Before: Inline `` `r 23 * 37` `` and `{r}` code block with `1 + 2`
  - After: Inline replaced with `851`, code block wrapped with output

If more examples of post-engine markdown are needed, ask the user.

## References

- React Reconciliation (Legacy): https://legacy.reactjs.org/docs/reconciliation.html
- FxHasher (rustc-hash): https://crates.io/crates/rustc-hash
- Parent plan: claude-notes/plans/2025-12-15-engine-output-source-location-reconciliation.md
