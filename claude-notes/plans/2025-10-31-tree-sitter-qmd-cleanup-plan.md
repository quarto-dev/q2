# tree-sitter-qmd Cleanup Plan

**Date**: 2025-10-31
**Status**: Planning
**Priority**: HIGH (prevents confusion)

## Current Situation - Summary

### What We Found

1. **Two Grammar Directories Exist**:
   - `tree-sitter-markdown/` - Contains the **ACTIVE** unified grammar (both block AND inline)
   - `tree-sitter-markdown-inline/` - **LEGACY** unused inline grammar

2. **Both Are Still Compiled**:
   - `build.rs` lines 16-21 compile BOTH parser.c and scanner.c from both directories
   - This wastes build time and creates confusion

3. **Only Block Grammar Is Used**:
   - `MarkdownParser` (parser.rs line 156) only uses `LANGUAGE` (block grammar)
   - `MarkdownParser` no longer has an `inline_tree` field
   - `MarkdownTree` only contains `block_tree: Tree` (line 128)
   - The unified block grammar handles ALL content (block + inline)

4. **API Still Exposes Unused Grammar**:
   - `lib.rs` exports `INLINE_LANGUAGE`, `HIGHLIGHT_QUERY_INLINE`, `INJECTION_QUERY_INLINE`, `NODE_TYPES_INLINE`
   - These are never used by quarto-markdown-pandoc
   - This creates confusion for anyone reading the API

### How the Current System Works

```
Input QMD text
      ↓
MarkdownParser::parse()
      ↓
Uses ONLY tree-sitter-markdown (block grammar)
      ↓
Produces unified Tree with ALL nodes:
  - Block nodes: document, section, pandoc_paragraph, etc.
  - Inline nodes: pandoc_str, pandoc_emph, pandoc_code_span, etc.
      ↓
quarto-markdown-pandoc processes this single tree
```

### Evidence from Code

**parser.rs**:
```rust
pub struct MarkdownTree {
    block_tree: Tree,  // Only one tree!
}

impl Default for MarkdownParser {
    fn default() -> Self {
        let block_language = LANGUAGE.into();  // Only block language!
        // ...
    }
}
```

**Actual grammar** (`tree-sitter-markdown/grammar.js` line 531):
```javascript
pandoc_str: $ => /(?:[0-9A-Za-z%&()+-/]|\\.)(?:[0-9A-Za-z!%&()+,./;?:-]|\\.)*/,
// Inline content defined in "block" grammar!
```

**Usage** (readers/qmd.rs line 37):
```rust
let mut parser = MarkdownParser::default();  // Only uses block grammar
let tree = parser.parse(&input_bytes, None);  // Returns single tree
```

## Problems This Causes

### 1. Build Time Waste
- Compiling unused scanner.c and parser.c from tree-sitter-markdown-inline
- These are non-trivial C files that take time to compile

### 2. Confusion for Developers (like me!)
- API exports INLINE_LANGUAGE suggesting it's used
- Two grammar directories suggest dual-tree parsing
- I spent time looking at the wrong grammar.js file
- Documentation says "two grammars" but only one is used

### 3. Maintenance Burden
- Two sets of queries to maintain (highlights.scm, injections.scm)
- Two node-types.json files
- Potential for drift if someone accidentally modifies the wrong grammar

### 4. API Surface Clutter
- Exports 4+ unused items that consumers might try to use
- Tests verify unused grammar can be loaded (lines 79-85)

## Cleanup Plan

### Phase 1: Verify Nothing Uses Inline Grammar (Safety Check)

**Actions**:
1. Search entire workspace for `INLINE_LANGUAGE` usage
2. Search for `tree-sitter-markdown-inline` imports
3. Verify no external dependencies on the inline grammar

**Commands**:
```bash
# Search for INLINE_LANGUAGE usage
rg "INLINE_LANGUAGE" --type rust

# Search for inline grammar imports
rg "tree_sitter_markdown_inline" --type rust

# Search in Cargo.toml files
rg "tree-sitter-qmd.*inline" --type toml
```

**Expected Result**: Only finds definitions/exports in tree-sitter-qmd, no actual usage

### Phase 2: Remove Inline Grammar from Build

**File**: `crates/tree-sitter-qmd/bindings/rust/build.rs`

**Current** (lines 6-26):
```rust
fn main() {
    let block_dir = std::path::Path::new("tree-sitter-markdown").join("src");
    let inline_dir = std::path::Path::new("tree-sitter-markdown-inline").join("src");

    let mut c_config = cc::Build::new();
    c_config.std("c11").include(&block_dir);

    #[cfg(target_env = "msvc")]
    c_config.flag("-utf-8");

    for path in &[
        block_dir.join("parser.c"),
        block_dir.join("scanner.c"),
        inline_dir.join("parser.c"),  // REMOVE
        inline_dir.join("scanner.c"),  // REMOVE
    ] {
        c_config.file(path);
        println!("cargo:rerun-if-changed={}", path.to_str().unwrap());
    }

    c_config.compile("tree-sitter-markdown");
}
```

**New**:
```rust
fn main() {
    let block_dir = std::path::Path::new("tree-sitter-markdown").join("src");

    let mut c_config = cc::Build::new();
    c_config.std("c11").include(&block_dir);

    #[cfg(target_env = "msvc")]
    c_config.flag("-utf-8");

    for path in &[
        block_dir.join("parser.c"),
        block_dir.join("scanner.c"),
    ] {
        c_config.file(path);
        println!("cargo:rerun-if-changed={}", path.to_str().unwrap());
    }

    c_config.compile("tree-sitter-markdown");
}
```

### Phase 3: Remove Inline Grammar from Cargo.toml

**File**: `crates/tree-sitter-qmd/Cargo.toml`

**Current** (lines 14-25):
```toml
include = [
  "bindings/rust/*",
  "tree-sitter-markdown/src/*",
  "tree-sitter-markdown-inline/src/*",        # REMOVE
  "tree-sitter-markdown/grammar.js",
  "tree-sitter-markdown-inline/grammar.js",   # REMOVE
  "tree-sitter-markdown/queries/*",
  "tree-sitter-markdown-inline/queries/*",    # REMOVE
  "common/grammar.js",
  "common/html_entities.json",
]
```

**New**:
```toml
include = [
  "bindings/rust/*",
  "tree-sitter-markdown/src/*",
  "tree-sitter-markdown/grammar.js",
  "tree-sitter-markdown/queries/*",
  "common/common.js",
  "common/html_entities.json",
]
```

### Phase 4: Update lib.rs - Remove Inline Exports

**File**: `crates/tree-sitter-qmd/bindings/rust/lib.rs`

**Remove** (lines 24-61):
```rust
unsafe extern "C" {
    fn tree_sitter_markdown() -> *const ();
    fn tree_sitter_markdown_inline() -> *const ();  // REMOVE
}

pub const INLINE_LANGUAGE: LanguageFn =  // REMOVE entire constant
    unsafe { LanguageFn::from_raw(tree_sitter_markdown_inline) };

pub const HIGHLIGHT_QUERY_INLINE: &str =  // REMOVE
    include_str!("../../tree-sitter-markdown-inline/queries/highlights.scm");

pub const INJECTION_QUERY_INLINE: &str =  // REMOVE
    include_str!("../../tree-sitter-markdown-inline/queries/injections.scm");

pub const NODE_TYPES_INLINE: &str =  // REMOVE
    include_str!("../../tree-sitter-markdown-inline/src/node-types.json");
```

**Keep only**:
```rust
unsafe extern "C" {
    fn tree_sitter_markdown() -> *const ();
}

/// The tree-sitter [`LanguageFn`][LanguageFn] for the unified markdown grammar.
///
/// This grammar handles both block structure and inline content in a single parse tree.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_markdown) };

pub const HIGHLIGHT_QUERY: &str =
    include_str!("../../tree-sitter-markdown/queries/highlights.scm");

pub const INJECTION_QUERY: &str =
    include_str!("../../tree-sitter-markdown/queries/injections.scm");

pub const NODE_TYPES: &str =
    include_str!("../../tree-sitter-markdown/src/node-types.json");
```

**Note**: Rename exports to remove "_BLOCK" suffix since there's only one grammar now.

### Phase 5: Update lib.rs Documentation

**File**: `crates/tree-sitter-qmd/bindings/rust/lib.rs`

**Current** (lines 6-14):
```rust
//! This crate provides Markdown language support for the [tree-sitter][] parsing library.
//!
//! It contains two grammars: [`LANGUAGE`] to parse the block structure of markdown documents and
//! [`INLINE_LANGUAGE`] to parse inline content.
//!
//! It also supplies [`MarkdownParser`] as a convenience wrapper around the two grammars.
//! [`MarkdownParser::parse`] returns a [`MarkdownTree`] instread of a [`Tree`][Tree]. This struct
//! contains a block tree and an inline tree for each node in the block tree that has inline
//! content.
```

**New**:
```rust
//! This crate provides Quarto Markdown language support for the [tree-sitter][] parsing library.
//!
//! It contains a unified grammar ([`LANGUAGE`]) that parses both the block structure and inline
//! content of markdown documents in a single parse tree.
//!
//! It supplies [`MarkdownParser`] as a convenience wrapper around the grammar.
//! [`MarkdownParser::parse`] returns a [`MarkdownTree`] which contains the parsed syntax tree.
```

### Phase 6: Remove Test for Inline Grammar

**File**: `crates/tree-sitter-qmd/bindings/rust/lib.rs`

**Remove** (lines 79-85):
```rust
#[test]
fn can_load_inline_grammar() {  // REMOVE entire test
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&INLINE_LANGUAGE.into())
        .expect("Error loading Markdown inline grammar");
}
```

### Phase 7: Update MarkdownCursor and MarkdownTree Documentation

**File**: `crates/tree-sitter-qmd/bindings/rust/parser.rs`

**Update** (line 20-22):
```rust
/// A stateful object for walking a [`MarkdownTree`] efficiently.
///
/// This exposes the same methdos as [`TreeCursor`], but abstracts away the
/// double block / inline structure of [`MarkdownTree`].  // OUTDATED
```

**New**:
```rust
/// A stateful object for walking a [`MarkdownTree`] efficiently.
///
/// This is a thin wrapper around [`TreeCursor`] for the unified markdown tree.
```

### Phase 8: Consider Removing tree-sitter-markdown-inline Directory

**Decision Point**: Should we delete the entire directory?

**Option A: Delete Completely**
- Pros: Clean, no confusion
- Cons: Loses history if we ever need to reference old inline grammar

**Option B: Archive with README**
- Create `tree-sitter-markdown-inline/ARCHIVED.md`:
  ```markdown
  # ARCHIVED

  This grammar is no longer used as of 2025-10-31.

  The inline grammar has been merged into the unified grammar in
  `tree-sitter-markdown/grammar.js` which handles both block structure
  and inline content in a single parse tree.

  This directory is kept for historical reference only.
  ```
- Remove from build but keep files

**Recommendation**: Option B (archive) - safer approach, can delete later

### Phase 9: Update README.md

**File**: `crates/tree-sitter-qmd/README.md`

**Add note**:
```markdown
## Architecture

This crate uses a unified grammar (`tree-sitter-markdown/grammar.js`) that parses
both block structure and inline content in a single pass, producing one syntax tree.

Note: The `tree-sitter-markdown-inline/` directory is archived and no longer used.
```

## Testing Plan

After each phase:

1. **Build test**:
   ```bash
   cargo clean
   cargo build --release
   ```

2. **Unit tests**:
   ```bash
   cargo test -p tree-sitter-qmd
   ```

3. **Integration tests**:
   ```bash
   cargo test -p quarto-markdown-pandoc
   ```

4. **Smoke test**:
   ```bash
   echo "test *emph* here" | cargo run --bin quarto-markdown-pandoc --
   ```

5. **Verify tree structure**:
   ```bash
   echo "test *emph* here" | cargo run --bin quarto-markdown-pandoc -- --verbose
   ```

## Expected Benefits

### 1. Faster Builds
- ~50% reduction in tree-sitter compilation time (rough estimate)
- Fewer files to track for cargo rerun-if-changed

### 2. Clearer API
- Only exports what's actually used
- Documentation matches reality
- No confusion about which grammar to use

### 3. Easier Maintenance
- One grammar to maintain
- One set of queries (highlights, injections)
- One node-types.json to keep updated

### 4. Better Documentation
- README accurately describes architecture
- No misleading comments about "two grammars"
- Clear that it's a unified parsing approach

## Rollback Plan

If something breaks:

1. **Git revert**: All changes should be in atomic commits
2. **Keep branches**: Create cleanup branch, don't force push
3. **Document**: If rollback needed, document WHY in the commit message

## Time Estimate

- Phase 1 (verify): 15 minutes
- Phase 2-3 (build changes): 15 minutes
- Phase 4-6 (API cleanup): 30 minutes
- Phase 7 (docs update): 15 minutes
- Phase 8 (archive decision): 15 minutes
- Phase 9 (README): 15 minutes
- Testing after each phase: 30 minutes
- **Total**: ~2-2.5 hours

## Success Criteria

- ✅ Cargo build succeeds without errors
- ✅ All tests pass
- ✅ No mentions of INLINE_LANGUAGE except in git history
- ✅ Build is noticeably faster (time it before/after)
- ✅ API documentation is accurate
- ✅ No confusion for future developers
- ✅ All parsing functionality still works

## Future Cleanup (Out of Scope)

After this cleanup, we could also:
- Rename `LANGUAGE` to `QMD_LANGUAGE` for clarity
- Rename `tree-sitter-markdown` to `tree-sitter-qmd-unified` or similar
- But these are breaking changes to the public API

## Questions to Resolve

1. **Should we delete tree-sitter-markdown-inline/ or archive it?**
   - Recommendation: Archive with README.md explaining it's unused

2. **Should we rename exported constants?**
   - `HIGHLIGHT_QUERY_BLOCK` → `HIGHLIGHT_QUERY`
   - `INJECTION_QUERY_BLOCK` → `INJECTION_QUERY`
   - `NODE_TYPES_BLOCK` → `NODE_TYPES`
   - Recommendation: YES, removes "_BLOCK" suffix that's no longer meaningful

3. **Do we need deprecation warnings?**
   - This is an internal crate (publish = false)
   - No external consumers to worry about
   - Recommendation: No deprecation needed, just remove

## References

- Current code: `crates/tree-sitter-qmd/`
- Active grammar: `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`
- Parser wrapper: `crates/tree-sitter-qmd/bindings/rust/parser.rs`
- Build script: `crates/tree-sitter-qmd/bindings/rust/build.rs`
