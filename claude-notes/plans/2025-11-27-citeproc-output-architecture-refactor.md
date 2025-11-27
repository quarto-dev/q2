# Citeproc Output Architecture Refactor

**Issue**: k-423 (child of k-422)
**Created**: 2025-11-27
**Status**: Complete

## Problem Statement

Our current `OutputBuilder` produces flat strings directly during evaluation. This prevents implementing advanced CSL features that require post-processing the output tree:

1. **Disambiguation** - requires finding names/dates in output to modify them
2. **Suppress-author/author-only** - requires locating author portions
3. **Year suffixes** - requires tagging dates for "2020a, 2020b" assignment
4. **Title hyperlinking** - requires identifying title portions
5. **Collapsing** - requires identifying year portions for "(Smith 2020a, b)"

Pandoc's citeproc uses a tagged AST (`Output a`) that preserves semantic information for post-processing, then renders to the final format as a separate step.

## Solution

Introduced an intermediate `Output` enum similar to Pandoc's approach:

```rust
pub enum Output {
    Formatted {
        formatting: Formatting,
        children: Vec<Output>,
    },
    Linked {
        url: String,
        children: Vec<Output>,
    },
    InNote(Box<Output>),
    Literal(String),
    Tagged {
        tag: Tag,
        child: Box<Output>,
    },
    Null,
}

pub enum Tag {
    Term(String),
    CitationNumber(i32),
    Title,
    Item { item_type: CitationItemType, item_id: String },
    Name(Name),
    Names { variable: String, names: Vec<Name> },
    Date(String),  // Variable name like "issued", "accessed"
    YearSuffix(i32),
    Locator,
    Prefix,
    Suffix,
}
```

## Implementation Plan

### Phase 1: Core Output Types ✅
- [x] Define `Output` enum in `output.rs`
- [x] Define `Tag` enum in `output.rs`
- [x] Implement `Output::is_null()` method
- [x] Implement `Output::render()` for plain text rendering
- [x] Add helper constructors: `Output::literal()`, `Output::formatted()`, `Output::tagged()`, `Output::sequence()`

### Phase 2: Update Evaluation Functions ✅
- [x] Change `evaluate_element()` to return `Output` instead of `OutputBuilder`
- [x] Change `evaluate_text()` to return `Output` with `Tag::Title` for titles, `Tag::Term` for terms
- [x] Change `evaluate_names()` to return `Output` with `Tag::Names`
- [x] Change `evaluate_date()` to return `Output` with `Tag::Date`
- [x] Change `evaluate_number()` to return `Output`
- [x] Change `evaluate_label()` to return `Output` with `Tag::Term`
- [x] Change `evaluate_group()` to return `Output`
- [x] Change `evaluate_choose()` to return `Output`

### Phase 3: Update Top-Level Functions ✅
- [x] Change `evaluate_citation()` to use `Output`
- [x] Change `evaluate_bibliography_entry()` to use `Output`
- [x] Add `Output::render()` method for final string conversion
- [x] Ensure all existing tests pass

### Phase 4: Add Tagging ✅
- [x] Tag names output with `Tag::Names`
- [x] Tag date output with `Tag::Date`
- [x] Tag title output with `Tag::Title`
- [x] Tag prefix/suffix with `Tag::Prefix`/`Tag::Suffix`
- [x] Tag terms with `Tag::Term`

### Phase 5: Cleanup
- [ ] Remove old `OutputBuilder` if no longer needed (kept for now as some code may still use it)
- [x] Run full test suite

## Key Design Decisions

### 1. Formatting Nesting

Formatting nests properly:
```rust
Output::Formatted {
    formatting: Formatting { font_style: Italic, .. },
    children: vec![
        Output::Literal("Title".into()),
    ],
}
```

### 2. Empty Handling

`Output::Null` represents truly empty output. `Output::literal("")` returns `Output::Null`.
`Output::formatted()` and `Output::sequence()` filter null children and return `Output::Null` if empty.

### 3. Tag Preservation

Tags survive through formatting operations:
```rust
Output::Formatted {
    formatting: some_formatting,
    children: vec![
        Output::Tagged {
            tag: Tag::Date("issued".to_string()),
            child: Box::new(Output::Literal("2020")),
        },
    ],
}
```

### 4. Rendering

Final rendering traverses the tree via `Output::render()`.

## Files Modified

1. `crates/quarto-citeproc/src/output.rs` - Added `Output` and `Tag` enums, helper methods, `join_outputs()`
2. `crates/quarto-citeproc/src/eval.rs` - All evaluation functions now return `Output`

## Testing Results

- All 155 tests pass (140 CSL conformance tests + 15 unit tests)
- Output tree preserves semantic information via Tags
- String rendering produces identical output to previous implementation

## Success Criteria

- [x] All tests pass (155 tests)
- [x] Output tree preserves semantic information via Tags
- [x] String rendering produces identical output to current implementation
- [x] Code is ready for disambiguation implementation

## Next Steps

The Output AST is now in place. Future work can:
1. Implement disambiguation by traversing the Output tree to find tagged Names/Dates
2. Implement suppress-author/author-only by filtering based on Tag::Names
3. Implement year suffixes by modifying Tag::Date nodes
4. Implement title hyperlinking by wrapping Tag::Title nodes in Output::Linked
5. Implement collapsing by grouping citations with same author
