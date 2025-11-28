# Extract Pandoc AST Types to quarto-pandoc-types Crate

**Issue**: k-429 (discovered from k-422)
**Created**: 2025-11-27
**Status**: Completed (Phase 1)
**Completed**: 2025-11-27

## Problem Statement

quarto-citeproc needs to produce Pandoc AST output (`Vec<Inline>`) so that:
1. The production code path (`Output → Pandoc → HTML`) can be properly tested
2. quarto-markdown-pandoc can consume citeproc output directly
3. We avoid testing code paths that won't be used in production

Currently, the Pandoc AST types live in quarto-markdown-pandoc. To avoid circular dependencies:
- quarto-citeproc needs Pandoc types to produce output
- quarto-citeproc tests need quarto-markdown-pandoc's HTML writer
- quarto-markdown-pandoc will eventually use quarto-citeproc

## Solution

Extract all Pandoc AST types to a new `quarto-pandoc-types` crate.

### Architecture

```
quarto-pandoc-types (new crate)
├── Pandoc (top-level struct)
├── Block, Blocks (all block types)
├── Inline, Inlines (all inline types)
├── Attr, AttrSourceInfo, TargetSourceInfo
├── Meta, MetaValue
├── Table, Caption, Cell, Row, TableHead, TableBody, TableFoot
├── ListNumberStyle, ListNumberDelim
├── Shortcode (Quarto extension)
├── Citation, CitationMode
└── QuoteType, MathType, Target

quarto-markdown-pandoc
├── [depends on quarto-pandoc-types]
├── readers/ (QMD parser, JSON reader)
├── writers/ (HTML, JSON, native, plaintext, etc.)
├── treesitter_utils/ (parser internals)
├── ast_context.rs (AST building helpers)
├── location.rs (tree-sitter location helpers)
└── filters.rs, traversals.rs

quarto-citeproc
├── [depends on quarto-pandoc-types]
├── [dev-depends on quarto-markdown-pandoc]
├── Output AST (existing)
├── to_inlines() → Vec<Inline> (new)
└── CSL HTML writer (for tests, uses <b>/<i>)
```

### Dependency Graph

```
                quarto-pandoc-types
                    │
        ┌───────────┴───────────┐
        │                       │
        ▼                       ▼
quarto-citeproc          quarto-markdown-pandoc
        │                       │
        │    [dev-dependency]   │
        └───────────────────────┘
```

### What Moves to quarto-pandoc-types

From `quarto-markdown-pandoc/src/pandoc/`:

| File | Types to Move |
|------|---------------|
| `pandoc.rs` | `Pandoc` |
| `block.rs` | `Block`, `Blocks`, all block variant structs |
| `inline.rs` | `Inline`, `Inlines`, all inline variant structs, `Citation`, `CitationMode`, `QuoteType`, `MathType`, `Target` |
| `attr.rs` | `Attr`, `AttrSourceInfo`, `TargetSourceInfo`, `is_empty_attr()` |
| `meta.rs` | `Meta`, `MetaValue` |
| `table.rs` | Table-related types |
| `caption.rs` | `Caption` |
| `list.rs` | `ListNumberStyle`, `ListNumberDelim` |
| `shortcode.rs` | `Shortcode` |

### What Stays in quarto-markdown-pandoc

- `readers/` - All parsing logic
- `writers/` - All output writers (HTML, JSON, native, etc.)
- `treesitter_utils/` - Parser internals
- `ast_context.rs` - AST building context
- `location.rs` - tree-sitter location helpers
- `filters.rs` - AST traversal filters
- `traversals.rs` - AST traversal utilities
- `errors.rs` - Error types

### Dependencies

**quarto-pandoc-types:**
- `serde`, `serde_json` (serialization)
- `quarto-source-map` (SourceInfo type)
- `hashlink` (LinkedHashMap for Attr)

**quarto-markdown-pandoc** (after refactor):
- `quarto-pandoc-types`
- `tree-sitter`, `tree-sitter-qmd`
- Everything else it currently depends on

**quarto-citeproc:**
- `quarto-pandoc-types` (regular dependency)
- `quarto-markdown-pandoc` (dev-dependency, for HTML writer in tests)

## Implementation Plan

### Phase 1: Create quarto-pandoc-types crate ✅ COMPLETED

1. ✅ Create `crates/quarto-pandoc-types/` directory structure
2. ✅ Create `Cargo.toml` with minimal dependencies
3. ✅ Move type definitions from quarto-markdown-pandoc
4. ✅ Update re-exports in quarto-pandoc-types/src/lib.rs
5. ✅ Add quarto-pandoc-types dependency to quarto-markdown-pandoc
6. ✅ Update imports in quarto-markdown-pandoc
7. ✅ Verify quarto-markdown-pandoc builds and tests pass (428 tests)
8. ✅ Add quarto-pandoc-types dependency to quarto-citeproc (314 tests pass)

### Phase 2: Integrate with quarto-citeproc

1. Add quarto-pandoc-types dependency to quarto-citeproc
2. Implement `Output::to_inlines()` conversion
3. Add quarto-markdown-pandoc as dev-dependency
4. Create CSL HTML writer for tests (maps `Emph` → `<i>`, `Strong` → `<b>`)
5. Update test harness to use: `Output → to_inlines() → CSL HTML`
6. Verify existing tests still pass

### Phase 3: Enable more tests

1. Run CSL conformance tests through new pipeline
2. Fix any conversion issues discovered
3. Enable tests that were blocked by HTML format differences

## Key Design Decisions

### 1. Source Info for Generated Content

Citeproc-generated Inlines won't have meaningful source locations. Use `SourceInfo::empty()` for these - source info is primarily for error reporting, which doesn't apply to generated content.

### 2. CSL HTML Writer

The CSL test suite expects `<b>`/`<i>` tags, but Pandoc convention uses `<strong>`/`<em>`. Rather than modify 896 test expectations, we create a small CSL-specific HTML writer that:
- Takes `Vec<Inline>` (same as production)
- Outputs CSL-style HTML (`<b>`, `<i>`, `<sup>`, `<sub>`)
- Lives in quarto-citeproc (test code only)

This ensures the critical `Output → Vec<Inline>` conversion is tested, which is the code that runs in production.

### 3. Formatting Conversion

The `Output::to_inlines()` function handles:
- Basic formatting (bold, italic, superscript, subscript, small-caps)
- Links
- Quotes (using locale-appropriate quote characters)
- Text case transformations (traverse and modify Str nodes)
- Prefix/suffix
- Strip periods

Complex cases like flip-flop formatting (italic inside italic = normal) can be handled by tracking formatting state during conversion.

## Files to Create/Modify

### New Files
- `crates/quarto-pandoc-types/Cargo.toml`
- `crates/quarto-pandoc-types/src/lib.rs`
- `crates/quarto-pandoc-types/src/pandoc.rs`
- `crates/quarto-pandoc-types/src/block.rs`
- `crates/quarto-pandoc-types/src/inline.rs`
- `crates/quarto-pandoc-types/src/attr.rs`
- `crates/quarto-pandoc-types/src/meta.rs`
- `crates/quarto-pandoc-types/src/table.rs`
- (etc.)

### Modified Files
- `crates/quarto-markdown-pandoc/Cargo.toml` - add quarto-pandoc-types dep
- `crates/quarto-markdown-pandoc/src/pandoc/mod.rs` - update re-exports
- `crates/quarto-markdown-pandoc/src/**/*.rs` - update imports
- `crates/quarto-citeproc/Cargo.toml` - add dependencies
- `crates/quarto-citeproc/src/output.rs` - add `to_inlines()` method
- `crates/quarto-citeproc/tests/csl_conformance.rs` - use new rendering path

## Success Criteria

1. quarto-pandoc-types crate exists with full Pandoc AST
2. quarto-markdown-pandoc builds and all tests pass
3. quarto-citeproc can convert `Output` to `Vec<Inline>`
4. CSL conformance tests run through `Output → Inlines → CSL HTML` pipeline
5. No regression in existing test coverage

## References

- Pandoc citeproc (Haskell): `external-sources/citeproc/`
- Current Output AST: `crates/quarto-citeproc/src/output.rs`
- Current Pandoc types: `crates/quarto-markdown-pandoc/src/pandoc/`
- CSL conformance roadmap: `claude-notes/plans/2025-11-27-csl-conformance-roadmap.md`
