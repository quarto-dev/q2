# Preview capture-splice: echo-cell output run (Variant B) — Implementation Plan

> **For agentic workers:** implement task-by-task with TDD. Steps use checkbox
> (`- [x]`) syntax. Write the failing test, watch it fail, implement, watch it
> pass, then run the regression set and end-to-end verification before committing.

**Goal:** Fix `q2 preview` dropping every marimo cell after the first `echo: true`
cell (symptom: `name 'mo' is not defined`), by letting the capture-splice derive
walk model a cell's output as a **run of blocks** (echoed source + output block)
instead of exactly one output block.

**Architecture:** Keep the current two-step splice design (derive a per-cell
`(hash, occurrence)` → output map from the `(A1, B1)` capture pair, then splice
onto the live `A2`). Two changes: (1) the derive walk skips a leading
*echoed-source* block — identified by a **content check** (`is_echo_of`: a plain,
unbraced `CodeBlock` whose text equals the current cell's source minus `#|`
directive lines) — while searching for a cell's output block, and (2) the map
value becomes `Vec<Block>` so the echoed source splices alongside the output
block, making preview match `q2 render` for `echo: true` cells.

This re-introduces one narrow, engine-shaped heuristic — "an echo is the cell's
own source re-emitted as a plain code block" — which is exactly the
cell-boundary reasoning the deferred three-way-merge design flags as the hard,
engine-coupled part (design doc §5, §7.1). The content check confines it to a
single named predicate and, because it only skips on a positive source match,
**preserves the module's fail-soft invariant**: an unmatched block is never
swallowed, so the splice still cannot emit wrong output — the worst case remains
raw source.

**Tech Stack:** Rust, `quarto-core`, `cargo nextest`. Unit tests live in
`crates/quarto-core/src/engine/capture_splice.rs` `#[cfg(test)] mod tests`.

## Execution outcome (2026-07-24) — DONE

Implemented and verified. Deviations from the task-by-task script, for honesty:

- **No separate RED-test commit** (Task 1 Step 4 skipped). The RED state *was*
  confirmed (`echo_cell_output_run_splices` failed with `left: 2, right: 3`, the
  cell falling through to raw `{python .marimo}` source) but not committed, to
  keep every commit in history green.
- **Tasks 2 and 3 landed as one change**, not the temporary single-element-run
  intermediate step. The final `Vec<Block>` map + `is_echo_of` + three-branch
  derive were applied together and verified in one pass.

Verification:
- `cargo nextest run -p quarto-core engine::capture_splice` — **18/18 pass**
  (4 new + 14 pre-existing, incl. Bug B `julia_first_fold…`, `two_engine_fold`,
  `cell_nested_in_figure_div`, `nested_and_top_level`, `plain_language_tag`).
- `cargo fmt` + `cargo clippy -p quarto-core --all-targets -- -D warnings` — clean.
- `cargo nextest run --workspace` — **10756/10756 pass**, 0 failed.
- **End-to-end** (WASM chain rebuilt: `build:wasm` → `build-q2-preview-spa` →
  `cargo build --bin q2`; `q2 preview index.qmd`; headless Chromium):
  **6 marimo-island elements** (was 1), islands session log `main (6 cells)`
  (was `main (1 cells)`), **no `name 'mo' is not defined`**. Symptom resolved.

## Background / why

Root cause (confirmed by dumping the recorded capture and tracing the walk on
`~/src/quarto-marimo/index.qmd`): an `echo: true` marimo cell emits **two**
sibling blocks in `B1` — a plain ` ```python ` `CodeBlock` (the echoed source),
then the island `RawBlock`. `derive_cell_outputs_walk` assumes each engine cell
maps to exactly **one** `is_engine_output_block` (a `.cell` Div or a `RawBlock`
island). At the first echo cell it finds the bare echo `CodeBlock`, which is
neither, so the engine-cell branch breaks without advancing the `B1` pointer,
the next `A1` block (a Header) diverges, and the fail-soft walk stops — dropping
every cell after it, including the `import marimo as mo` cell. With that cell's
island gone, the marimo islands runtime starts a 1-cell session that runs the
`mo`-using cell alone → `name 'mo' is not defined`.

This is **not** `bd-5m1ni9if` (that is a *no-output* cell followed by a
*user-authored* `RawBlock`). It is a distinct class: an echoed-source block
preceding the output block, reachable on any `echo: true` cell.

Full design context and the deferred general (three-way-merge) alternative:
`claude-notes/designs/2026-07-24-preview-capture-splice-three-way-merge.md`.

### Recorded-capture evidence (the shape the fix relies on)

Dumped from the live preview capture for `index.qmd` (gzipped JSON at
`<data_dir>/captures/<sha>.bin` → `.[0].result.markdown`, parsed to blocks). The
relevant `B1` slice around cells 1–2 (cell 1 `echo:false`, cell 2 `echo:true`):

```
B1[3]  RawBlock  html   <marimo-island …>            ← cell 1 output (no echo)
B1[4]  CodeBlock [python]  mo.md(f'''\n  # Hello …    ← cell 2 ECHOED SOURCE
B1[5]  RawBlock  html   <marimo-island …>            ← cell 2 output island
```

and the corresponding `A1` cell:

```
A1[4]  CodeBlock [{python},marimo]  #| echo: true\nmo.md(f'''\n  # Hello …
```

Two facts the fix depends on, both confirmed here: (1) the echo is a **plain,
unbraced `CodeBlock`** (classes `[python]`, so `engine_cell_lang` returns
`None`), *not* a `RawBlock`; and (2) it sits **immediately before** the island,
and its text equals the cell's source with the `#| echo: true` directive line
removed (`mo.md(f'''…').callout("info")` on both sides). Fact (2) is what makes
the content check in Task 3 exact for the real engine.

## Global Constraints

- Do **not** change the marimo engine or fixtures; this is a q2-side splice fix.
- Preserve the fail-soft guarantee: an unexpected shape leaves cells as raw
  source, never wrong output.
- Preserve behavior for wrapper engines (`.cell` Div, one per cell) and for the
  multi-engine fold (`bd-5oyk1xce` Bug B) — the discriminator below is chosen
  specifically so those paths are untouched.
- No `unwrap()`/`expect()` on the walk path. `cargo xtask verify --skip-hub-build`
  must pass (matches CI `-D warnings`); the final end-to-end step rebuilds the
  WASM preview chain.

## File Structure

- Modify: `crates/quarto-core/src/engine/capture_splice.rs`
  - `struct CellOutputMap` — value type `Block` → `Vec<Block>`.
  - `derive_cell_outputs_walk` — echo-run collection in the engine-cell branch.
  - `splice_blocks_walk` — `push` one block → `extend` with the run.
  - module + `CellOutputMap` doc comments — reflect the run.
  - `#[cfg(test)] mod tests` — new fixtures + tests.

There is exactly one file. Tasks are ordered so the crate compiles after each
implementation task; the map-type change (Task 2) and the derive change (Task 3)
are split only because they carry separate tests.

---

### Task 1: Add the failing test for the echo cell (RED)

**Files:**
- Test: `crates/quarto-core/src/engine/capture_splice.rs` (`mod tests`)

**Interfaces:**
- Consumes existing helpers: `pandoc_of(Vec<Block>) -> Pandoc`,
  `code_cell(lang, body) -> Block` (builds a **braced** `{lang}` engine cell),
  `raw_island(marker) -> Block` (a `{=html}` `<marimo-island>{marker}</…>`),
  `prose(text) -> Block`, `apply_capture_splice(a2, &a1, &b1, engine) -> Pandoc`.
- Produces a new helper other tests reuse:
  `echo_source(lang, body) -> Block` — a **plain** (unbraced) highlighted
  `CodeBlock`, the shape an engine emits for an `echo: true` cell's source.

- [x] **Step 1: Add the `echo_source` helper** (next to `raw_island` in `mod tests`)

```rust
/// A plain highlighted code block (classes = `[lang]`, no `{lang}` braces) —
/// the shape an engine emits for an `echo: true` cell's *source*, sitting
/// immediately before that cell's output block. `engine_cell_lang` returns
/// `None` for it (no braces), which is how the derive walk tells it apart
/// from a braced engine cell.
fn echo_source(lang: &str, body: &str) -> Block {
    Block::CodeBlock(CodeBlock {
        attr: (String::new(), vec![lang.to_string()], LinkedHashMap::new()),
        text: body.to_string(),
        source_info: SourceInfo::for_test(),
        attr_source: AttrSourceInfo::empty(),
    })
}

/// Extract the marker text from a `raw_island(...)` block, else `None`.
fn island_marker(block: &Block) -> Option<String> {
    let Block::RawBlock(rb) = block else { return None };
    let inner = rb.text.strip_prefix("<marimo-island>")?;
    Some(inner.strip_suffix("</marimo-island>")?.to_string())
}
```

- [x] **Step 2: Add the core echo-cell test**

```rust
#[test]
fn echo_cell_output_run_splices_echo_and_island() {
    // A1: an `echo: true` marimo cell, then prose.
    // B1: the engine emitted [echoed-source CodeBlock, island], then prose.
    // A2 == A1. The cell must map to the RUN [echo, island] — before the fix
    // the walk breaks on the echo CodeBlock and drops the island.
    let a1 = pandoc_of(vec![
        code_cell("python .marimo", "slider = mo.ui.slider()"),
        prose("after"),
    ]);
    let b1 = pandoc_of(vec![
        echo_source("python", "slider = mo.ui.slider()"),
        raw_island("ISL1"),
        prose("after"),
    ]);
    let a2 = a1.clone();

    let out = apply_capture_splice(a2, &a1, &b1, "marimo");

    // Expect: [echo CodeBlock, island ISL1, prose] — 3 blocks.
    assert_eq!(out.blocks.len(), 3, "blocks: {:?}", out.blocks);
    assert!(matches!(out.blocks[0], Block::CodeBlock(_)), "block0 not echo code");
    assert_eq!(island_marker(&out.blocks[1]).as_deref(), Some("ISL1"));
    assert!(matches!(out.blocks[2], Block::Paragraph(_)));
}
```

- [x] **Step 3: Run it, confirm RED**

Run: `cargo nextest run -p quarto-core echo_cell_output_run_splices`
Expected: FAIL — `out.blocks.len()` is 2 (cell fell through to raw source; no
island), so the length assertion fails.

- [x] **Step 4: Commit the RED test**

```bash
git add crates/quarto-core/src/engine/capture_splice.rs
git commit -m "test(capture-splice): failing test for echo:true cell output run (RED)"
```

---

### Task 2: Widen `CellOutputMap` to a run of blocks

**Files:**
- Modify: `crates/quarto-core/src/engine/capture_splice.rs`

**Interfaces:**
- Produces: `CellOutputMap.entries: HashMap<CellKey, Vec<Block>>` (was
  `HashMap<CellKey, Block>`). `len()`/`is_empty()` unchanged (count of keys).

- [x] **Step 1: Change the field type and doc comment**

Replace the struct doc + definition (currently around `capture_splice.rs:139-149`):

```rust
/// Per-engine-cell mapping derived from a capture pair `(A1, B1)`.
/// Each entry pairs a cell's `CellKey` with the **run** of B1 blocks the
/// engine emitted for it: an optional leading echoed-source `CodeBlock`
/// (`echo: true`) followed by the output block (a `.cell` wrapper Div or a
/// `RawBlock` island), or the echo run alone for an `echo: true` + no-output
/// cell. Most cells map to a one-element run. Cells with no output at all
/// (e.g. `include: false`) have no map entry; the splice falls through to
/// raw source for those.
#[derive(Debug, Default, Clone)]
pub struct CellOutputMap {
    entries: HashMap<CellKey, Vec<Block>>,
}
```

- [x] **Step 2: Update the splice side to extend with the run**

In `splice_blocks_walk` (currently `capture_splice.rs:361-365`):

```rust
                if let Some(replacement) = map.entries.get(&key) {
                    out.extend(replacement.iter().cloned());
                } else {
                    out.push(block);
                }
```

- [x] **Step 3: Make the derive side insert single-element runs (temporary, keeps it compiling)**

In `derive_cell_outputs_walk`, the output-found branch currently does
`map.entries.insert(key, b_blocks[j].clone());`. Change to a one-element run so
the crate compiles and all existing tests still pass **before** Task 3 adds the
echo-run logic:

```rust
                map.entries.insert(key, vec![b_blocks[j].clone()]);
```

- [x] **Step 4: Build + run the full existing suite, confirm still green (Task 1 test still RED)**

Run: `cargo nextest run -p quarto-core engine::capture_splice`
Expected: all pre-existing tests PASS; `echo_cell_output_run_splices` still FAILS
(the derive walk still can't get past the echo block — that's Task 3).

- [x] **Step 5: Commit**

```bash
git add crates/quarto-core/src/engine/capture_splice.rs
git commit -m "refactor(capture-splice): CellOutputMap value is a block run (no behavior change)"
```

---

### Task 3: Collect the echo run in the derive walk (GREEN)

**Files:**
- Modify: `crates/quarto-core/src/engine/capture_splice.rs`

**Interfaces:**
- Consumes: `engine_cell_lang(&Block) -> Option<&str>` (returns `Some` only for
  braced `{lang}` cells), `is_engine_output_block(&Block) -> bool`,
  `structural_eq_block_local(&Block, &Block) -> bool`, `compute_block_hash_fresh`.
- Produces: `is_echo_of(candidate: &Block, cell: &Block) -> bool` — the echo
  discriminator (module-private fn, used by the derive walk).

- [x] **Step 1: Add the `is_echo_of` discriminator** (module-level fn, near `engine_cell_lang`)

```rust
/// True when `candidate` is the *echoed source* of `cell`: the engine
/// re-emitted the cell's own source as a plain highlighting block (what
/// `echo: true` does). Both must be `CodeBlock`s; `candidate` must be plain
/// (unbraced — `engine_cell_lang` is `None`, so a braced engine cell is never
/// treated as an echo); and `candidate`'s text must equal `cell`'s source with
/// leading `#|` (Quarto directive) lines removed, trimmed.
///
/// This is a *positive* match: the derive walk skips a leading block only when
/// it is provably this cell's echo. An unrelated block (including the *next*
/// cell's echo, or a no-output cell's neighbour) fails the match and is left
/// alone, so the splice never swallows content it cannot attribute — the
/// fail-soft invariant holds.
fn is_echo_of(candidate: &Block, cell: &Block) -> bool {
    let (Block::CodeBlock(cand), Block::CodeBlock(src)) = (candidate, cell) else {
        return false;
    };
    if engine_cell_lang(candidate).is_some() {
        return false; // a braced engine cell is not an echo
    }
    let stripped: Vec<&str> = src
        .text
        .lines()
        .filter(|l| !l.trim_start().starts_with("#|"))
        .collect();
    stripped.join("\n").trim() == cand.text.trim()
}
```

- [x] **Step 2: Replace the engine-cell branch body**

Replace the whole engine-cell arm (currently `capture_splice.rs:214-271`, from
`if engine_cell_lang(a_block).is_some() {` through its closing `i += 1; }`) with:

```rust
        if engine_cell_lang(a_block).is_some() {
            let Block::CodeBlock(_) = a_block else {
                unreachable!()
            };

            // Collect any leading echoed-source blocks that precede this
            // cell's output block. An engine with `echo: true` (marimo) emits
            // the cell's source as a plain ```lang CodeBlock *before* the
            // output island. Skip such a block so the search reaches the real
            // output block, and fold it into the cell's output run (preview
            // must match render, which shows the echoed code).
            //
            // `is_echo_of` is a *positive* content match (this cell's source),
            // so it never skips a braced foreign cell (bd-5oyk1xce Bug B — it
            // falls to the no-output branch below), the *next* cell's echo, or
            // any other unattributable block. That keeps the walk fail-soft.
            let run_start = j;
            while j < b_blocks.len()
                && !is_engine_output_block(&b_blocks[j])
                && is_echo_of(&b_blocks[j], a_block)
            {
                j += 1;
            }

            let hash = compute_block_hash_fresh(a_block);
            let occurrence = occurrences.entry(hash).or_insert(0);
            let key = CellKey {
                hash,
                occurrence: *occurrence,
            };
            *occurrence += 1;

            if j < b_blocks.len() && is_engine_output_block(&b_blocks[j]) {
                // Output run = leading echo(es) (run_start..j) + output block (j).
                map.entries.insert(key, b_blocks[run_start..=j].to_vec());
                j += 1;
            } else if j > run_start {
                // Echo-only cell: `echo: true` with no output block
                // (e.g. eval: false). Map the collected echo run so preview
                // still shows the source, matching render.
                map.entries.insert(key, b_blocks[run_start..j].to_vec());
                // `j` already sits past the echo run.
            } else {
                // No echo, no output block: a genuine no-output cell, or a
                // foreign engine's un-executed cell (bd-5oyk1xce Bug B).
                // Advance `j` past a B1 block structurally equal to this A1
                // cell (a passthrough) so the walk stays aligned and reaches
                // this engine's later cells.
                if j < b_blocks.len() && structural_eq_block_local(a_block, &b_blocks[j]) {
                    j += 1;
                }
            }
            i += 1;
        } else if let (Block::Div(a_div), Some(Block::Div(b_div))) = (a_block, b_blocks.get(j)) {
```

Note: the `#[allow(clippy::never_loop)]` attribute on the old look-ahead loop is
removed (the loop now genuinely iterates). Ensure it is deleted.

- [x] **Step 3: Run the core test, confirm GREEN**

Run: `cargo nextest run -p quarto-core echo_cell_output_run_splices`
Expected: PASS.

- [x] **Step 4: Run the full capture_splice suite, confirm no regressions**

Run: `cargo nextest run -p quarto-core engine::capture_splice`
Expected: all PASS — especially the discriminator-sensitive guards
`julia_first_fold_preserves_julia_cell_after_foreign_marimo_cells` (Bug B),
`two_engine_fold_splices_both_engines_cells`, `cell_nested_in_figure_div_splices`,
`nested_and_top_level_cells_share_occurrence_ordering`, and
`plain_language_tag_without_braces_is_not_an_engine_cell`.

- [x] **Step 6: Commit**

```bash
git add crates/quarto-core/src/engine/capture_splice.rs
git commit -m "fix(capture-splice): map echo:true cell to its full output run (echo + island) (GREEN)"
```

---

### Task 4: Regression tests for adjacent cells + partial edit + echo-only

**Files:**
- Test: `crates/quarto-core/src/engine/capture_splice.rs` (`mod tests`)

**Interfaces:** consumes the same helpers as Task 1 plus `island_marker`,
`echo_source`.

- [x] **Step 1: Adjacent cells, no edit — all islands survive (the index.qmd shape)**

```rust
#[test]
fn adjacent_cells_echo_second_both_islands_survive() {
    // cell1 (echo:false) directly followed by cell2 (echo:true), no prose
    // between — the shape that dropped 5/6 islands on index.qmd. B1 =
    // [island1, echo2, island2]. A2 == A1. Both cells must map.
    let a1 = pandoc_of(vec![
        code_cell("python .marimo", "slider = mo.ui.slider()"),
        code_cell("python .marimo", "mo.md('x')"),
    ]);
    let b1 = pandoc_of(vec![
        raw_island("ISL1"),
        echo_source("python", "mo.md('x')"),
        raw_island("ISL2"),
    ]);
    let a2 = a1.clone();

    let out = apply_capture_splice(a2, &a1, &b1, "marimo");

    // [island1, echo2, island2] — 3 blocks.
    assert_eq!(out.blocks.len(), 3, "blocks: {:?}", out.blocks);
    assert_eq!(island_marker(&out.blocks[0]).as_deref(), Some("ISL1"));
    assert!(matches!(out.blocks[1], Block::CodeBlock(_)));
    assert_eq!(island_marker(&out.blocks[2]).as_deref(), Some("ISL2"));
}
```

- [x] **Step 2: Adjacent cells, second edited — first island kept, second falls through**

```rust
#[test]
fn adjacent_cells_edit_second_keeps_first_island() {
    // Proves the property the three-way-merge design struggled with is
    // handled by the two-step model: editing cell2 must NOT drop cell1's
    // island. Derive maps from the (unedited) capture; the splice keys on
    // the cell hash, so only the edited cell misses and falls through.
    let a1 = pandoc_of(vec![
        code_cell("python .marimo", "slider = mo.ui.slider()"),
        code_cell("python .marimo", "mo.md('x')"),
    ]);
    let b1 = pandoc_of(vec![
        raw_island("ISL1"),
        echo_source("python", "mo.md('x')"),
        raw_island("ISL2"),
    ]);
    // A2: cell2 edited (body changed) → hash miss → raw source.
    let a2 = pandoc_of(vec![
        code_cell("python .marimo", "slider = mo.ui.slider()"),
        code_cell("python .marimo", "mo.md('EDITED')"),
    ]);

    let out = apply_capture_splice(a2, &a1, &b1, "marimo");

    // [island1, edited cell2 raw source] — 2 blocks.
    assert_eq!(out.blocks.len(), 2, "blocks: {:?}", out.blocks);
    assert_eq!(island_marker(&out.blocks[0]).as_deref(), Some("ISL1"));
    if let Block::CodeBlock(cb) = &out.blocks[1] {
        assert!(cb.text.contains("EDITED"), "expected raw edited cell2");
    } else {
        panic!("expected raw CodeBlock, got {:?}", &out.blocks[1]);
    }
}
```

- [x] **Step 3: Echo-only cell (echo:true, no output island)**

```rust
#[test]
fn echo_only_cell_maps_echo_run() {
    // `echo: true` with no output (e.g. eval:false): B1 has the echoed
    // source but no island. The cell should map to the echo run so preview
    // shows the code (matching render), not fall through to the raw `{...}`
    // source cell.
    let a1 = pandoc_of(vec![
        code_cell("python .marimo", "import marimo as mo"),
        prose("after"),
    ]);
    let b1 = pandoc_of(vec![
        echo_source("python", "import marimo as mo"),
        prose("after"),
    ]);
    let a2 = a1.clone();

    let out = apply_capture_splice(a2, &a1, &b1, "marimo");

    assert_eq!(out.blocks.len(), 2, "blocks: {:?}", out.blocks);
    // block0 is the plain echo CodeBlock (no `{...}` braces), not the raw cell.
    if let Block::CodeBlock(cb) = &out.blocks[0] {
        assert!(
            cb.attr.1.iter().all(|c| !c.starts_with('{')),
            "expected plain echo block, got braced cell: {:?}",
            cb.attr.1
        );
    } else {
        panic!("expected CodeBlock, got {:?}", &out.blocks[0]);
    }
}
```

- [x] **Step 4: Run the three new tests + the whole suite**

Run: `cargo nextest run -p quarto-core engine::capture_splice`
Expected: all PASS.

- [x] **Step 5: Commit**

```bash
git add crates/quarto-core/src/engine/capture_splice.rs
git commit -m "test(capture-splice): adjacent-cell + partial-edit + echo-only regressions"
```

---

### Task 5: Update module docs to describe the output run

**Files:**
- Modify: `crates/quarto-core/src/engine/capture_splice.rs` (module `//!` header)

- [x] **Step 1: Amend the Algorithm section** (currently `capture_splice.rs:33-41`)

Update the bullet that says each cell "maps to exactly one B1 block" to describe
the run: a cell maps to an optional leading echoed-source `CodeBlock` plus the
output block (`.cell` Div or `RawBlock` island); the derive walk skips a leading
plain (non-braced) `CodeBlock` echo to reach the output block; a braced engine
cell is never skipped (Bug B). Keep the fail-soft description intact.

- [x] **Step 2: Build docs to confirm no broken intra-doc links**

Run: `cargo doc -p quarto-core --no-deps 2>&1 | grep -i warning || echo OK`
Expected: `OK` (or no new warnings).

- [x] **Step 3: Commit**

```bash
git add crates/quarto-core/src/engine/capture_splice.rs
git commit -m "docs(capture-splice): document the echo-cell output run"
```

---

### Task 6: Full verification + end-to-end preview check

**Files:** none (verification only).

- [x] **Step 1: Workspace build + tests**

Run: `cargo nextest run --workspace`
Expected: PASS (no regressions in `quarto-core` or downstream crates).

- [x] **Step 2: CI-strict verify (Rust-only leg is fine; this file feeds WASM, so run the WASM leg too)**

Run: `cargo xtask verify`
Expected: PASS. (`capture_splice.rs` is in `quarto-core`, which `wasm-quarto-hub-client` depends on, so the WASM build must be exercised.)

- [x] **Step 3: Rebuild the preview WASM chain (server exec alone is not enough — the splice runs in WASM)**

```bash
cd hub-client && npm run build:wasm
cd .. && cargo xtask build-q2-preview-spa
cargo build --bin q2
```

- [x] **Step 4: End-to-end preview of the real fixture**

```bash
cd /private/tmp/.../marimo-repro   # the scratch copy of quarto-marimo used in investigation
/Users/gordon/src/q2/.worktrees/ts-engine-extensions/target/debug/q2 preview index.qmd --port 7911 --no-browser &
```
Then drive headless Chromium (the investigation harness `preview_inspect2.cjs`,
run with `NODE_PATH=<worktree>/node_modules`) and assert:
- the doc frame has **6** `marimo-island` elements (baseline before fix: 1),
- no `name 'mo' is not defined` in the pane,
- the islands runtime log shows a 6-cell session, not "main (1 cells)".

Record the observed counts in the commit/PR description per the repo's
end-to-end verification rule.

- [x] **Step 5: Final commit if any doc/notes changed; otherwise stop.**

## Risks / edge cases (call these out in review)

1. **Mis-attribution hole — closed by the content check.** The original concern
   (a no-output cell immediately followed by the *next* cell's echo, no prose
   between) would, under a purely *structural* skip, attach the next cell's
   echo+island run to the no-output cell — emitting confidently-wrong output,
   which the current walk never does. `is_echo_of` closes this: it skips a
   leading block only when its text matches *this* cell's source, so the next
   cell's echo fails the match and is not skipped. The residual cost is the
   opposite, fail-soft direction: if an engine ever reformats echoed source so
   it no longer matches the cell verbatim, `is_echo_of` returns `false`, the
   echo is not skipped, and the cell falls through to raw source (never wrong
   output). For marimo the echo is verbatim source (see the recorded-capture
   evidence above), so the real path matches exactly.
2. **False green if the echo shape differs from the evidence.** The synthetic
   fixtures assume "plain `CodeBlock` echo immediately before the island." If a
   future marimo emits the echo as a `RawBlock`, or interposes a block between
   echo and island, unit tests stay green but preview breaks. Task 6's e2e
   6-island check is the real gate; the recorded-capture evidence above pins the
   shape at the time of writing.
3. **Discriminator coupling with Bug B.** The fix and `bd-5oyk1xce` must not
   fight: `is_echo_of` returns `false` for a *braced* engine cell
   (`engine_cell_lang` is `Some`), so Bug B's foreign passthrough cells never
   enter the skip and stay on the no-output branch. `julia_first_fold_…` is the
   guard.
4. **Occurrence-counter equivalence.** The refactor hoists the
   `occurrences.entry(hash)` increment ahead of the output/no-output branches.
   This is equivalent to today (both old branches incremented once per cell);
   `nested_and_top_level_cells_share_occurrence_ordering` and
   `repeated_cells_same_content_use_occurrence_index` guard it.
```
