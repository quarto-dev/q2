# Source Location Reconciliation After Engine Execution

**Date**: 2025-12-15
**Issue**: k-6daf
**Status**: Design phase
**Related**: claude-notes/plans/2025-12-15-source-info-for-structured-formats.md

## Problem Statement

During the Quarto rendering pipeline, engine execution (Jupyter, knitr, etc.) produces markdown output that is fundamentally disconnected from the original source file's location information. The engines operate on text-in/text-out basis and are unaware of our source tracking infrastructure.

After engine execution, we have:
1. **Pre-engine PandocAST**: Parsed from the original `.qmd` with `SourceInfo` pointing to the original file
2. **Post-engine PandocAST**: Parsed from the engine's markdown output with `SourceInfo` pointing to an intermediate file

**Goal**: Reconcile these two ASTs so that:
- Elements that are "content-identical" retain their original source locations
- Elements that changed (code execution outputs, computed values) have source locations pointing to the intermediate engine output file

## Context: Current Pipeline Flow

```
Original .qmd
    ↓
Parse to PandocAST (AST-A, with original source locations)
    ↓
Engine Execution (text-in, text-out - source locations lost)
    ↓
Engine Output Markdown (intermediate file)
    ↓
Parse to PandocAST (AST-B, with intermediate file locations)
    ↓
[RECONCILIATION STEP - this design]
    ↓
Final PandocAST (with proper source locations)
```

## Prior Art: React's Virtual DOM Reconciliation

React's reconciliation algorithm provides useful concepts:

1. **Element identity**: Two elements of different types produce different trees
2. **Keys for lists**: Explicit keys help identify which elements moved/changed
3. **Depth-first comparison**: Compare children after parents match
4. **Bail-out optimization**: If a subtree is identical, skip deep comparison

However, our use case differs:
- We're not updating a real DOM, we're transferring source locations
- Our "elements" are AST nodes, not DOM elements
- We have rich structural information (block types, inline types)
- We need to handle cases where engines may normalize whitespace

## Design: AST Source Location Reconciliation

### Core Algorithm

```rust
/// Reconcile source locations between pre-engine and post-engine ASTs
pub fn reconcile_source_locations(
    original: &Pandoc,      // Pre-engine AST with original locations
    executed: &mut Pandoc,  // Post-engine AST to be updated
    engine_output_file_id: FileId,  // File ID for the engine output
) -> ReconciliationReport {
    let mut report = ReconciliationReport::new();

    // Reconcile metadata (YAML front matter)
    reconcile_meta(&original.meta, &mut executed.meta, &mut report);

    // Reconcile blocks
    reconcile_blocks(&original.blocks, &mut executed.blocks, &mut report);

    report
}
```

### Node Matching Strategy

#### 1. Structural Matching

Match nodes first by type, then by "identity signals":

```rust
enum MatchQuality {
    /// Exact content match - transfer source location
    Exact,
    /// Same structure but different content - keep executed location
    StructuralOnly,
    /// No match
    NoMatch,
}

fn match_blocks(original: &Block, executed: &Block) -> MatchQuality {
    // First, types must match
    if std::mem::discriminant(original) != std::mem::discriminant(executed) {
        return MatchQuality::NoMatch;
    }

    match (original, executed) {
        // Code blocks: match by attributes (language, id, classes)
        // Content may differ due to execution
        (Block::CodeBlock(a), Block::CodeBlock(b)) => {
            if code_block_attrs_match(&a.attr, &b.attr) {
                if a.text == b.text {
                    MatchQuality::Exact
                } else {
                    MatchQuality::StructuralOnly
                }
            } else {
                MatchQuality::NoMatch
            }
        }

        // Headers: match by level and content
        (Block::Header(a), Block::Header(b)) => {
            if a.level == b.level && inlines_content_eq(&a.content, &b.content) {
                MatchQuality::Exact
            } else {
                MatchQuality::NoMatch
            }
        }

        // Paragraphs, lists, etc: content-based matching
        (Block::Paragraph(a), Block::Paragraph(b)) => {
            if inlines_content_eq(&a.content, &b.content) {
                MatchQuality::Exact
            } else {
                MatchQuality::NoMatch
            }
        }

        // ... other block types
    }
}
```

#### 2. Content Equality

For content comparison, we need flexible equality that handles:
- Whitespace normalization (engines may normalize)
- Smart quote differences
- Trivial differences that don't affect semantics

```rust
/// Compare inlines for content equality, ignoring source locations
fn inlines_content_eq(a: &Inlines, b: &Inlines) -> bool {
    if a.len() != b.len() {
        return false;
    }

    a.iter().zip(b.iter()).all(|(ai, bi)| inline_content_eq(ai, bi))
}

fn inline_content_eq(a: &Inline, b: &Inline) -> bool {
    match (a, b) {
        (Inline::Str(a), Inline::Str(b)) => a.text == b.text,
        (Inline::Space(_), Inline::Space(_)) => true,
        (Inline::SoftBreak(_), Inline::SoftBreak(_)) => true,
        (Inline::Emph(a), Inline::Emph(b)) => inlines_content_eq(&a.content, &b.content),
        // ... other inline types
        _ => false,
    }
}
```

#### 3. List Alignment

For block lists (document body, list items, etc.), we need alignment:

```rust
/// Align two block sequences, finding best match
fn align_blocks(
    original: &[Block],
    executed: &[Block],
) -> Vec<AlignedPair> {
    // Option 1: Simple O(n) linear scan if documents are similar
    // Most engine outputs only differ in code cell outputs

    // Option 2: LCS-based alignment for heavily modified documents
    // More expensive but handles insertions/deletions

    // Start with Option 1, fall back to Option 2 if linear scan fails
    linear_align(original, executed)
        .unwrap_or_else(|| lcs_align(original, executed))
}

enum AlignedPair {
    /// Both original and executed present, matched
    Matched { original_idx: usize, executed_idx: usize, quality: MatchQuality },
    /// Only in original (deleted by engine - unusual)
    OriginalOnly { original_idx: usize },
    /// Only in executed (added by engine - code outputs)
    ExecutedOnly { executed_idx: usize },
}
```

### Special Cases

#### 1. Code Blocks

Code blocks are special because:
- Their content may be replaced by execution output
- Their attributes identify them (language, id, classes)
- Cell options in YAML should retain original locations

```rust
fn reconcile_code_block(original: &CodeBlock, executed: &mut CodeBlock) -> BlockReconciliation {
    // Attributes should match (these identify the cell)
    if !code_block_attrs_match(&original.attr, &executed.attr) {
        return BlockReconciliation::NoMatch;
    }

    // If content is identical, transfer all source info
    if original.text == executed.text {
        executed.source_info = original.source_info.clone();
        executed.attr_source = original.attr_source.clone();
        return BlockReconciliation::ExactMatch;
    }

    // Content differs - this is an executed cell
    // Keep executed.source_info (points to engine output)
    // But transfer attr_source from original (cell options came from original)
    executed.attr_source = original.attr_source.clone();
    BlockReconciliation::ContentChanged
}
```

#### 2. Inline Code Execution

Some engines support inline code execution (e.g., `` `r 1+1` `` becomes `2`):

```rust
fn reconcile_inline_code(original: &Code, executed: &Inline) -> InlineReconciliation {
    // If executed is also Code with same text, exact match
    if let Inline::Code(exec_code) = executed {
        if original.text == exec_code.text {
            return InlineReconciliation::ExactMatch;
        }
    }

    // If executed is a Str (inline code was evaluated), keep new location
    // The original Code node was replaced with its evaluation result
    InlineReconciliation::Replaced
}
```

#### 3. Figure/Table Outputs

Code cells may produce figure or table outputs that don't exist in original:

```rust
fn handle_execution_outputs(executed_blocks: &mut [Block], report: &mut ReconciliationReport) {
    for block in executed_blocks {
        if is_execution_output(block) {
            // Mark this block as engine-generated
            // Its source_info should point to engine output file
            report.add_generated_output(block);
        }
    }
}
```

### Reconciliation Report

Track what happened during reconciliation:

```rust
pub struct ReconciliationReport {
    /// Blocks that matched exactly - original source locations transferred
    exact_matches: usize,
    /// Blocks with same structure but different content
    content_changes: usize,
    /// Blocks only in original (deleted)
    deletions: usize,
    /// Blocks only in executed (added by engine)
    additions: usize,
    /// Detailed information for debugging
    details: Vec<ReconciliationDetail>,
}
```

### Integration with SourceContext

The engine output file needs to be registered:

```rust
impl SourceContext {
    /// Register an engine output file and get its FileId
    pub fn register_engine_output(
        &mut self,
        engine: &str,           // "jupyter", "knitr", etc.
        original_file: &Path,   // The original .qmd file
        content: String,        // The engine's markdown output
    ) -> FileId {
        let path = format!(
            "<engine-output:{}/{}::{}>",
            engine,
            original_file.file_name().unwrap().to_string_lossy(),
            /* unique id */
        );
        self.add_file(path, Some(content))
    }
}
```

## Implementation Plan

### Phase 1: Core Reconciliation Infrastructure

1. Add `reconcile` module to `quarto-pandoc-types`:
   - Content equality functions (ignoring source_info)
   - Block/Inline matching functions
   - List alignment algorithm

2. Implement `ReconciliationReport` for debugging/tracking

3. Add unit tests with synthetic ASTs

### Phase 2: Block Reconciliation

1. Implement block-level reconciliation for each Block variant
2. Special handling for CodeBlock (the most common case)
3. Handle nested blocks (BlockQuote, Div, lists)

### Phase 3: Inline Reconciliation

1. Implement inline-level reconciliation
2. Handle inline code execution replacement
3. Preserve source locations through inline containers (Emph, Strong, etc.)

### Phase 4: Integration

1. Add engine output file registration to SourceContext
2. Integrate reconciliation into the render pipeline
3. Add option to skip reconciliation (for debugging)

### Phase 5: Testing & Validation

1. Test with real Jupyter notebooks
2. Test with knitr documents
3. Verify error messages point to correct locations
4. Performance testing with large documents

## Open Questions

### Q1: How to handle whitespace normalization?

Engines may normalize whitespace differently. Options:
- **Strict**: Require exact whitespace match for content equality
- **Loose**: Normalize whitespace before comparison
- **Configurable**: Let users choose

**Recommendation**: Start strict, relax if real-world issues arise.

### Q2: Should we store both original and executed locations?

For some use cases (debugging), having both might be useful:

```rust
pub enum SourceInfo {
    // ... existing variants ...

    /// Location in post-engine output, with reference to original
    EngineOutput {
        executed_location: Box<SourceInfo>,
        original_location: Option<Box<SourceInfo>>,
    },
}
```

**Recommendation**: Defer this. Start by simply transferring or keeping locations.

### Q3: What about multi-pass engines?

Some workflows may involve multiple engine passes. How do we chain reconciliation?

**Recommendation**: Reconcile after each engine pass, always against the previous AST.

### Q4: Performance for large documents?

For documents with thousands of blocks, O(n^2) alignment is too slow.

**Recommendation**: Use linear scan with fallback. Real engine outputs rarely reorder content.

## Relationship to ipynb Source Tracking

This design complements the ipynb source tracking work:

1. **ipynb → qmd**: The previous plan handles source locations from notebook cells to qmd
2. **qmd → engine → qmd'**: This plan handles source locations through engine execution

Together, they provide end-to-end source tracking:
```
notebook.ipynb → qmd (with NotebookCell locations) → engine → qmd' (reconciled) → final output
```

## Summary

This design provides:
- A principled approach to reconciling source locations after engine execution
- Content-based matching that handles the typical case (mostly unchanged documents)
- Special handling for code blocks (the primary source of changes)
- A reconciliation report for debugging
- Integration path into the existing infrastructure

The key insight is that most engine outputs are nearly identical to the input, with only code block outputs differing. This allows a simple linear algorithm to handle most cases efficiently.
