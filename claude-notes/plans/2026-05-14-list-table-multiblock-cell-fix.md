# Fix: qmd writer emits list-table cell with multiple blocks as broken bullet item

- **Issue**: GitHub #183
- **Beads**: bd-oxsr
- **Worktree**: `.worktrees/issue-183` (branch `issue-183`)
- **Triage**: `claude-notes/issue-reports/183/triage.md`
- **Started**: 2026-05-14

## Overview

`write_list_table` in `crates/pampa/src/writers/qmd.rs:937` produces qmd that the reader rejects whenever a `.list-table` cell contains either (a) multiple blocks or (b) a single block that is not `Plain`/`Paragraph`. Two defects in the cell-emission paths (lines 1090-1100 and 1102-1113):

1. The `-` marker line is left empty (`writeln!(buf)?` runs immediately after the marker is written).
2. Consecutive blocks within a cell are emitted at indent-4 with no blank-line separator.

The canonical valid shape — first `Plain`/`Paragraph` inline with the marker, blank line, then subsequent blocks at indent-4 — round-trips cleanly (`claude-notes/issue-reports/183/expected-output.qmd`).

## TDD ground rules (from `crates/pampa/CLAUDE.md`)

1. Write the failing test.
2. Confirm it fails as expected.
3. Implement the fix.
4. Confirm the test passes.
5. Run `cargo nextest run --workspace` for regressions.
6. End-to-end exercise the binary on the original repro.

## Work Items

### Phase 1 — Failing tests

- [x] **1.1** Copy `repro.qmd` (multi-block: Para + CodeBlock) into the round-trip suite as `list_table_cell_para_then_codeblock.qmd`.
- [x] **1.2** Copy `exp-single-codeblock.qmd` (single non-Para/non-Plain block) into the round-trip suite as `list_table_cell_single_codeblock.qmd`.
- [x] **1.3** Copy `exp-two-paras.qmd` (two consecutive Paragraphs in one cell) into the round-trip suite as `list_table_cell_two_paragraphs.qmd`.
- [x] **1.4** Run `cargo nextest run -p pampa test_qmd_roundtrip_consistency` — confirmed `test_qmd_roundtrip_consistency` panics on `list_table_cell_para_then_codeblock.qmd` because the regenerated qmd fails to parse. Test driver aborts on first failure, so the other two were verified via CLI.
- [x] **1.5** CLI repros for all three confirm the same observed shape (empty marker line + no blank-line separator → parse error on re-read).

### Phase 2 — Implementation

- [x] **2.1** Refactor the cell-emission block in `write_list_table` to handle three shapes uniformly. One change vs. the original plan: for the "first block is non-Plain/non-Para" case, the block's **first line goes on the marker line** (e.g. `  - \`\`\`python`) rather than leaving the marker line empty and using a blank line. Probing showed that the empty-marker + blank-line shape introduces a phantom empty `Para` in the parsed AST (mismatching the original), whereas the first-line-on-marker shape round-trips cleanly. Implementation uses two helpers: `write_cell_block_on_marker_line` (first block, non-Plain/non-Para) and `write_cell_block_indented` (every subsequent block).
- [x] **2.2** Fixtures from Phase 1 pass — `test_qmd_roundtrip_consistency` green.
- [x] **2.3** `table_list_colspan.qmd` still passes — Plain-content cells unaffected.
- [x] **2.4** Surveyed all `list-table-*.qmd` snapshots — every existing one uses single-Plain cells; none touch the code paths the fix changed.

### Phase 3 — Verification

- [x] **3.1** `cargo nextest run -p pampa` — 3687 tests, all pass.
- [x] **3.2** `cargo nextest run --workspace` — 8859 tests, all pass (qmd-syntax-helper green: 93/93).
- [x] **3.3** `cargo xtask verify --skip-hub-build --skip-hub-tests` — one unrelated pre-existing failure (`tree-sitter-qmd` GFM example 209); confirmed by stashing the change and re-running on `main`. Not introduced by this fix.
- [x] **3.4** End-to-end through pampa CLI: writer output for the original repro round-trips back to the byte-identical AST as the original input. Captured in transcript.
- [x] **3.5** No snapshot diffs (`git status` shows only the writer source file + new fixtures + plan doc). All existing list-table snapshots use single-Plain cells and are untouched.

### Phase 4 — Close-out

- [ ] **4.1** Update the triage doc with the actual fix.
- [ ] **4.2** Stage and commit on `issue-183` branch.
- [ ] **4.3** Update bd-oxsr (close with summary).
- [ ] **4.4** Sync beads, commit JSONL on `main`.
- [ ] **4.5** Wait for user approval before pushing.

## Design notes

### Cell shape after the fix

```rust
match cell.content.split_first() {
    None                                       => /* empty cell */,
    Some((Block::Plain(_) | Block::Paragraph(_), rest)) => {
        emit_inlines_on_marker_line();
        for block in rest {
            emit_blank_line();
            emit_indented_block(block);
        }
    }
    Some((first, rest)) => {
        // no inlines on marker line — leave it as just "- "
        // emit first as the first indented stanza
        emit_blank_line();
        emit_indented_block(first);
        for block in rest {
            emit_blank_line();
            emit_indented_block(block);
        }
    }
}
```

A single function `emit_indented_block(block)` keeps the indentation logic in one place.

### Why a blank line for the all-non-Para case

CommonMark loose-list rule: a list item with non-Plain block content needs a blank line before that content for the indented block to be recognized as belonging to the item. The reader currently enforces this — `expected-output.qmd` round-trips, the observed-output does not.

### `[]` placeholder for non-Para first blocks?

An alternative shape for the "non-Para first block" case would be `* - []` (empty span placeholder) followed by blank line and indented block. The current code only uses `[]` when no attributes are needed AND the cell is empty. Adding `[]` for non-Para first blocks would be a stylistic preference, not a correctness one. The blank-line-before-indented-block shape is what the canonical hand-fixed fixture uses and what TS Pandoc would round-trip cleanly. Stick with that.

### What this fix does NOT touch

- `should_use_pipe_table` decision logic (untouched).
- Loose/tight bullet list writer (#174) — different code path.
- Incremental writer registry (tables are always fully rewritten — see comment at line 1124).

## Risk / blast radius

- Single function (`write_list_table`).
- Touched by:
  - `tests/roundtrip_tests/qmd-json-qmd/table_list_colspan.qmd` (existing fixture — single Plain content).
  - Any snapshot using `.list-table` in `crates/pampa/tests/snapshots/`.
- Downstream consumer `qmd-syntax-helper` may have grid-table → list-table tests with multi-block cells; flag for review.
