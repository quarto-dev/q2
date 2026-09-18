# Make the concrete-tree depth guard cheaper

**Strand:** bd-t7i6oanu (related finding: bd-khect2gq)
**Branch:** `braid/bd-t7i6oanu-make-concrete-tree-depth`
**Status:** approved 2026-09-18. Approach B plus bd-khect2gq on the same branch, as two separate commits.

## Overview

`pampa::readers::qmd::read` (`crates/pampa/src/readers/qmd.rs:158`) calls
`crate::utils::concrete_tree_depth::concrete_tree_depth(&tree)` and rejects
the document if the result is `> 100`. The check exists only so that a
fuzzer-style, deeply nested document produces an error instead of a stack
overflow further down the pipeline. It is a whole-tree walk of its own, done
before `treesitter_to_pandoc` walks the same tree again.

The goal is to make the guard as close to free as we can while keeping it just
as strong: no document that passes today should fail, and every document that
fails today should still fail.

## Measurement (2026-09-18, main @ c6808f20)

- Build: `cargo build --profile release-perf --bin q2`
- Fixture: `~/repos/github/cscheid/q2-connect-docs/docs-quarto-2/api/index.qmd`
  (1.1 MB). The path in the original request, `docs-quarto-2/api.qmd`, does
  not exist; `api/index.qmd` is the large file. The whole project was copied to
  the session scratchpad (without `_site`) so the render doesn't write into the
  real checkout. Rendering the file on its own fails because it includes
  `api_codes.fragment.html` from the same directory.
- Command: `samply record --save-only --unstable-presymbolicate --rate 8000 -- q2 render api/index.qmd`
  (~1.3 s wall, 9.5k samples). The samples were aggregated with a small
  Python script over the samply JSON plus the `.syms.json` sidecar.

Direct callees of `pampa::readers::qmd::read` (share of **all** samples):

| callee                                          | share |
| ----------------------------------------------- | ----: |
| `treesitter_to_pandoc` (bottom-up tree walk + conversion) | 23.61% |
| `MarkdownParser::parse` (tree-sitter)           | 12.93% |
| `filters::topdown_traverse`                     |  3.00% |
| `print_whole_tree` into `io::sink()`            |  2.44% |
| **`concrete_tree_depth`**                       | **1.63%** |
| `ts_tree_delete`                                |  1.11% |

What `concrete_tree_depth` spends its 1.63% on:

| inside `concrete_tree_depth`          | share |
| ------------------------------------- | ----: |
| `ts_tree_cursor_goto_next_sibling`    | 1.05% |
| `ts_tree_cursor_goto_first_child`     | 0.35% |
| `ts_tree_cursor_current_node`         | 0.12% |
| traversal loop + closure (self time)  | 0.13% |

**What the numbers mean.** The closure is not the problem. It is a generic
`FnMut` that is monomorphized and inlined, and the whole Rust side (loop,
`Vec<usize>` state stack, closure body) costs 0.13%. About 90% of the time is
spent inside tree-sitter's C cursor code. Each `goto_next_sibling` /
`goto_first_child` also steps through the *hidden* subtrees that sit between
visible nodes, so the cost scales with the number of internal nodes, not the
number of visible ones. Rewriting the loop only removes the 0.13%. The real
saving comes from **not doing a separate cursor walk at all**.

## Candidate approaches

### A. Tight cursor loop (small win, very low risk)

Replace the closure plus state-machine traversal with a straight cursor loop
that uses a plain integer depth counter and never materializes a `Node`:

```rust
let mut cursor = tree.walk_cursor();
let (mut depth, mut max) = (1usize, 1usize);
loop {
    if cursor.goto_first_child() { depth += 1; max = max.max(depth); continue; }
    while !cursor.goto_next_sibling() {
        if !cursor.goto_parent() { return max; }
        depth -= 1;
    }
}
```

An early exit at the limit (`fn exceeds_depth(tree, limit) -> Option<usize>`)
costs nothing and helps adversarial inputs, but doesn't help normal ones,
which have to be walked in full anyway.

- Expected saving: roughly the 0.13% self time plus the 0.12% of
  `current_node` calls. **About 0.25%, not 1.6%.**
- Risk: none. The depth semantics can be kept exactly (see "Semantics to
  preserve").

### B. Fuse the guard into the bottom-up conversion walk (recommended)

`treesitter_to_pandoc` already walks every node with
`quarto_treesitter_ast::bottomup_traverse_concrete_tree`, which is
**iterative** (an explicit `Vec<BottomUpTraversePhase>` stack), so the walk
itself can't overflow the stack. The number of open `GoToSiblings` frames on
that stack *is* the current node's depth. Track it with a counter and stop as
soon as it goes over the limit:

- Add a depth-limited variant, e.g.
  `bottomup_traverse_concrete_tree_limited(cursor, visitor, input, ctx, max_depth) -> Result<(String, T), DepthExceeded>`,
  in `quarto-treesitter-ast`. Keep the existing function as a thin wrapper
  (`quarto-doctemplate` also uses it), or give doctemplate the same guard.
- The check happens on `Enter`, which is top-down. The walk stops on the
  first node deeper than the limit, *before* the visitor has run on anything
  under it. So no Pandoc structure deeper than the limit is ever built. That
  is the same protection as today: the stack-overflow risk is in the
  recursive code *after* conversion (filters, writers, recursive `Drop`), and
  none of it gets to run.
- `read` maps `DepthExceeded` to the same generic error. Delete
  `utils/concrete_tree_depth.rs` and its call site.

- Expected saving: **the whole ~1.6%**, since the separate walk goes away.
  The extra cost in the bottom-up loop is one increment/decrement and one
  compare per node, which is noise next to the `Vec` pushes it already does.
- Risk: low to moderate.
  - The error message changes. Today it prints the *true* maximum depth
    (`max depth: 137 > 100`). An early-exit walk only knows "went over 100",
    so the message would become e.g. "nested more than 100 levels deep".
    Nothing in the test suite asserts on this text (`grep "deeply nested"`
    only hits `qmd.rs`), but it is a user-visible string change.
  - Diagnostics that the visitor already pushed into `error_collector` before
    the stop are thrown away. That is fine: today we return before
    conversion even starts.
  - This couples a safety check to the conversion traversal. Anyone who later
    replaces that traversal has to keep the limit. A test (Phase 1) makes that
    coupling enforced rather than implicit.

### C. Considered and rejected

- **Bound depth by input length and skip the walk for small files.** Not
  sound: tree-sitter produces zero-width nodes (MISSING, zero-width
  tokens/continuations), so depth is not bounded by byte count. Also the win
  would be on small files, where the check is already cheap.
- **Check depth on the Pandoc AST after conversion instead.** This is still a
  full walk (a cheaper tree, but not free). It also runs *after* building the
  structure we are trying to guard against.
- **`Node`-based recursion / `children()` iteration.** Slower than the cursor
  (allocates a cursor per level, and recursion brings back the stack risk).

### Related, bigger finding: `print_whole_tree` into a sink (bd-khect2gq)

Right after the depth check, `read` calls `print_whole_tree(&mut tree.walk(),
&mut output_stream)` unconditionally. `quarto-core`'s `ParseDocumentStage`
passes `std::io::sink()`, so on every render we do a *third* full cursor walk.
For each node it allocates a `"  ".repeat(depth)` string and formats
`Node::kind()` and the `Node` `Debug` output, then throws the result away. That
costs **2.44%**, more than the depth check itself. It is only useful for
`pampa -v`. Gating it (for example with an explicit `verbose`/`dump_tree`
flag on `read`, or by having the caller opt in) is a separate change, but it
touches the same ten lines and gets the same kind of win. It is filed as
bd-khect2gq so it can be scheduled on its own or bundled here.

With B and bd-khect2gq together, `read` goes from three full cursor walks
before conversion to one, saving about **4%** of this render.

## Semantics to preserve

`concrete_tree_depth` starts at 1 and adds 1 on *every* `Enter`, including the
root's. So it returns `1 + (depth of the deepest node, with root = 1)`. The
check `depth > 100` therefore rejects any tree whose deepest node is at depth
≥ 100 (root = 1). Whichever approach we pick must keep that exact threshold,
so that no document that passes today starts failing and no document that
fails today starts passing. Phase 1 pins it with a boundary test.

## Decisions (2026-09-18)

1. **Approach B.** Fuse the guard into the bottom-up conversion walk.
2. **Changing the error message is fine.** The walk stops at the first node
   that goes over the limit, so the message states the limit rather than the
   true maximum depth.
3. **bd-khect2gq goes on the same branch, in a separate commit.**

### Design notes for B

- `quarto-treesitter-ast` gains
  `bottomup_traverse_concrete_tree_with_depth_limit(cursor, visitor, input,
  ctx, max_depth) -> Result<(String, T), DepthLimitExceeded>`. Depth counts
  the root as 1. The check runs on `Enter`, before the visitor has touched
  anything under the node that is too deep. The existing
  `bottomup_traverse_concrete_tree` becomes a wrapper that passes
  `usize::MAX`, so there is one implementation. `quarto-doctemplate` behaves
  exactly as before.
- **The threshold stays exactly where it is.** The old check rejected when
  `concrete_tree_depth > 100`, and `concrete_tree_depth` = 1 + (deepest node
  depth, with root = 1). So a document is accepted iff its deepest node is at
  depth ≤ 99. The new constant is therefore `99`, with a comment explaining
  why, and the message says "more than 99 levels", which matches what is
  enforced.
- The guard moves into `treesitter_to_pandoc`, so everyone who calls it
  directly (many tests) gets it too, not only `readers::qmd::read`.

### Design notes for bd-khect2gq

- `output_stream` is pampa's *verbose* channel. `pampa -v` sends it to
  stderr, and everything else sends it to a sink or a throwaway `Vec`.
  Production code pays for the dump too: `include_expansion.rs:335` passes a
  `Vec` that is never read, so every included file is walked and dumped, and
  the text is kept in memory.
- Fix: remove the dump from `read` and expose it as
  `readers::qmd::dump_concrete_tree(input, &mut impl Write)`. The pampa CLI
  calls it only when `-v` is set, so `-v` output (documented in CLAUDE.md)
  keeps the tree dump. Delete the unused copy of `print_whole_tree` in
  `crates/pampa/src/main.rs`.
- One visible difference under `-v`: the dump now also appears for documents
  that fail to parse. It is printed before `read` runs; before, it only
  appeared after a successful parse. That is more useful for debugging, not
  less.

## Phases

### Phase 1 — tests first (red)
- [x] `crates/pampa/tests/integration/test_nesting_depth_limit.rs`:
  - [x] Boundary test. An independent reference walk computes the CST depth.
        Nested blockquotes add exactly one level per `>` (depth = levels + 4),
        so 95 levels = depth 99 is accepted and 96 = depth 100 is rejected.
        Passes on `main`.
  - [x] Pathological input is rejected on a 2 MiB thread. It uses 5k nested
        `[…]{.c}` spans (CST depth 10005), because blockquotes and lists stop
        parsing somewhere between 100 and 200 levels, and 10k `>` gives a
        *parse error* that never reaches the depth guard. Teeth checked: with
        the guard disabled the thread overflows its stack (SIGABRT) and the
        boundary test fails.
  - [x] Message test: fails on `main` as expected ("max depth: 101 > 100").
- [x] `quarto-treesitter-ast` unit tests (dev-dep `tree-sitter-qmd`): exact
      boundary accept/reject, visitor never runs on a node past the limit,
      result identical to the unlimited traversal. Fail to compile (API
      missing) as expected.
- [x] `crates/pampa/tests/integration/test_concrete_tree_dump.rs`: `read`
      leaves the verbose stream empty (fails on `main`: it contains the dump);
      `dump_concrete_tree` output is exact (API missing); `pampa -v` shows the
      dump and plain `pampa` doesn't (both pass on `main`, pinning the CLI).

### Phase 2 — commit 1: fuse the depth guard (bd-t7i6oanu)
- [x] Limited traversal in `quarto-treesitter-ast`, plus the pampa wrapper.
      The unlimited `bottomup_traverse_concrete_tree` now delegates with
      `usize::MAX`, so there is one implementation. The pampa-side unlimited
      wrapper had a single caller and was replaced by the limited one.
- [x] Guard inside `treesitter_to_pandoc` (`MAX_CONCRETE_TREE_DEPTH = 99`);
      removed the check from `read`.
- [x] Deleted `crates/pampa/src/utils/concrete_tree_depth.rs` and its `mod`.
- [x] Tests green (workspace: 13961 passed), fmt, clippy/lints via verify
      step 1, commit `40eadc50`. `verify --skip-hub-build` still runs hub-client's
      `*.wasm.test.ts` against the existing WASM artifact, which was built
      Sep 11 (stale), and those fail. The full `verify` in Phase 4 rebuilds
      WASM and is the real gate.

### Phase 3 — commit 2: stop dumping the tree in `read` (bd-khect2gq)
- [x] Add `dump_concrete_tree`, remove the call from `read`, wire `pampa -v`,
      delete the dead copy in `main.rs`. (The copy was never flagged because
      `main.rs` has `#![allow(dead_code)]`; removing it made `io::Write`
      unused there, so the import went too.)
- [x] Tests green (workspace: 13965 passed), clippy `-D warnings` clean on
      pampa and quarto-treesitter-ast, commit. Nothing else in the repo
      matched the dump text (`{Node ` / `print_whole_tree`).

### Phase 4 — measure and verify
- [ ] Re-profile the same fixture with the same command; record the numbers
      before and after.
- [ ] `cargo nextest run --workspace`; full `cargo xtask verify`.
- [ ] End-to-end: `q2 render` the fixture and a too-deep fixture; `pampa -v`
      on a small file. Inspect the output.
