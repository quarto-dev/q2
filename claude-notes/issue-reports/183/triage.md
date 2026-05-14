# Issue #183 — qmd writer emits list-table cell with multiple blocks as a broken bullet item

- **GitHub**: https://github.com/quarto-dev/q2/issues/183
- **Reporter**: @rundel (Colin Rundel), 2026-05-11
- **Triage date**: 2026-05-14
- **Worktree**: `.worktrees/issue-183` (branch `issue-183`, based on `main` @ `76b8fe3e`)
- **Beads issue**: bd-oxsr
- **Scope**: Writer bug in `write_list_table` (qmd writer). Related to #174 and #180 only in the broad category of "writer produces qmd that the reader rejects"; mechanism is independent.

## Summary

A `.list-table` div is parsed into a real `Table`. When any cell holds **more than one block** (e.g. a `Para` followed by a `CodeBlock`), the qmd writer emits the inner row-marker line with **no inline content** (`  - \n`) and then writes every block in the cell at 4-space indent with **no blank-line separation between blocks**. The resulting shape is rejected by the qmd reader, so the writer output fails to round-trip and any document containing this pattern cannot be regenerated. Reproduced at HEAD `76b8fe3e`. Root cause is local to the multi-block / non-Plain-non-Para path in `write_list_table` in `crates/pampa/src/writers/qmd.rs:1075-1115`. Fix scope is small and contained.

## Reproduction

Repro file: `claude-notes/issue-reports/183/repro.qmd`

```qmd
::: {.list-table}
- - foo
  - Add values:

    ```python
    x
    ```
:::
```

### Parse (Pandoc AST)

```
$ cargo run --bin pampa -- claude-notes/issue-reports/183/repro.qmd
[ Table … [Row … [Cell … [Para [Str "foo"]],
                  Cell … [Para [Str "Add", Space, Str "values:"],
                          CodeBlock ( "" , ["python"] , [] ) "x"]] …] ]
```

Two cells; the second cell holds **two blocks** — a `Para` and a `CodeBlock`.

### Writer output (observed)

`claude-notes/issue-reports/183/observed-output.qmd`:

```qmd
::: {.list-table}

* - foo
  -
    Add values:
    ```python
    x
    ```
:::
```

Two distinct defects:

1. **Empty inner-marker line** — `  - ` (followed by a trailing space and newline) instead of putting the first `Para` inline with the marker.
2. **No blank line between blocks** — `Add values:` (a `Para`) is followed immediately by the opening `` ```python `` fence on the next indented line, with no blank line separating them.

### Re-parse of writer output (observed failure)

```
$ cargo run --bin pampa -- claude-notes/issue-reports/183/observed-output.qmd
Error: Parse error  (line 5, col 5 — `Add`)
```

The reader does not accept the writer's emitted shape.

### Hand-fixed shape (round-trips cleanly)

`claude-notes/issue-reports/183/expected-output.qmd`:

```qmd
::: {.list-table}

* - foo
  - Add values:

    ```python
    x
    ```
:::
```

Re-parsing this hand-fixed shape yields the same AST as the original input:

```
[ Table … [Row … [Cell … [Para [Str "foo"]],
                  Cell … [Para [Str "Add", Space, Str "values:"],
                          CodeBlock ( "" , ["python"] , [] ) "x"]] …] ]
```

So the reader is fine; only the writer is wrong.

## Bug surface — what triggers it

Three cell shapes were exercised. Two trigger the bug, one does not:

| Cell content                         | Writer emits empty marker line | Roundtrips? | Path in writer |
|---                                   |---                             |---          |---             |
| Single `Plain` / `Para`              | no                             | yes         | line 1078-1089 |
| Single non-`Plain`/non-`Para` block (e.g. `CodeBlock`) | **yes**       | **no**      | line 1090-1100 (`other` arm) |
| Multiple blocks                      | **yes**                        | **no**      | line 1102-1113 (`else` arm) |

Both broken paths share the same two defects:

- They `writeln!(buf)?` immediately after writing `- `, so the marker line ends up empty.
- They emit each block back-to-back with no blank-line separator between blocks within the cell.

Fixture for the single-non-Para case: `claude-notes/issue-reports/183/exp-single-codeblock.qmd`.
Fixture for the two-paragraph case: `claude-notes/issue-reports/183/exp-two-paras.qmd`.

The two-paragraph fixture also fails to round-trip and confirms that the missing blank line — not anything code-block-specific — is the second defect: any two consecutive blocks at indent-4 collapse into a single paragraph or otherwise misparse.

## Localization

**File**: `crates/pampa/src/writers/qmd.rs`
**Function**: `write_list_table` (introduced at line 937)

The two broken arms are:

```rust
// crates/pampa/src/writers/qmd.rs:1078-1115 (excerpted)

if cell.content.len() == 1 {
    match &cell.content[0] {
        Block::Plain(plain) => { /* inline — OK */ }
        Block::Paragraph(para) => { /* inline — OK */ }
        other => {
            writeln!(buf)?;          // ← writes "- \n" — defect #1 (empty marker line)
            let mut block_buf = Vec::<u8>::new();
            write_block(other, &mut block_buf, ctx)?;
            let content = String::from_utf8_lossy(&block_buf);
            for line in content.lines() {
                writeln!(buf, "    {}", line)?;
            }
            continue;
        }
    }
} else {
    // Multiple blocks — write on new lines with indentation
    writeln!(buf)?;                  // ← defect #1 again
    for block in &cell.content {
        let mut block_buf = Vec::<u8>::new();
        write_block(block, &mut block_buf, ctx)?;
        let content = String::from_utf8_lossy(&block_buf);
        for line in content.lines() {
            writeln!(buf, "    {}", line)?;
        }
        // ← defect #2: no blank-line separator between consecutive blocks
    }
    continue;
}
```

The analogous *working* model — first block inline with the marker, blank line, then remaining blocks indented — is how regular markdown loose bullet lists handle multi-block items. The single-`Para`/single-`Plain` arms already do the first half (inline emission); the rest of the fix is to extend that treatment into the other two arms.

## Fix sketch (for the beads issue)

In `write_list_table`'s cell-emission loop:

1. **Multi-block case**: if the first block is `Plain` or `Paragraph`, emit its inlines on the marker line. Then for each subsequent block (or for all blocks if the first wasn't `Plain`/`Paragraph`):
   - emit a blank line
   - emit the block at 4-space indent
2. **Single non-Plain/non-Para case**: same shape — empty inline content on the marker line is OK only if it's `[]`-marked, otherwise write a blank line first, then the indented block. (The hand-fixed fixture shows the blank-line variant reparses cleanly; the `[]` variant should be checked but is likely unnecessary if defect #2 is also fixed.)

In both cases, between any two consecutive blocks inside one cell, emit a blank line.

This mirrors what the writer already does for top-level loose bullet lists (and what issues #174/#180 are about for *those* writers — but those bugs are separate).

## Open questions — resolved during triage

**Q1**: Is the reader at fault? Maybe it should accept the writer's current shape.
**Experiment**: Hand-fixed the writer output and re-parsed it (`expected-output.qmd`). It produces the original AST. So the reader is correct for the canonical loose-list-item shape; the writer is the one producing an off-spec form.
**Conclusion**: Writer-side fix only.

**Q2**: Is this purely a `.list-table` problem, or does it affect regular nested bullet lists too?
**Experiment**: Inspected the writer code path. `write_list_table` is the only caller of this particular multi-block / 4-space-indent emission. Regular `BulletList` emission lives elsewhere (and has its own loose/tight bugs — see #174 — but the mechanism is independent).
**Conclusion**: Scoped to `write_list_table`. #174 is related in *symptom* but is in a different code path.

**Q3**: Does the bug fire for a single non-`Plain`/non-`Para` block (e.g. a row with one cell that's just a code block)?
**Experiment**: `exp-single-codeblock.qmd`.
**Conclusion**: Yes — the `other` arm at line 1090 has defect #1 too. Both the single-non-Para arm and the multi-block arm need the same fix.

**Q4**: Should the writer prefer a pipe table for these tables that don't need list-table features?
**Conclusion**: Out of scope. The `should_use_pipe_table` decision (line 874) already opts cells with `CodeBlock`/multi-block content into list-table, which is correct. The bug is that the list-table writer emits a broken shape for the cases where list-table is the only option.

## Outcome / recommended next step

Filed bd-oxsr (see § Cross-references). Fix is small, single-file (`crates/pampa/src/writers/qmd.rs`), TDD-able through `tests/roundtrip_tests/qmd-json-qmd` with the three fixtures already captured under `claude-notes/issue-reports/183/`.

### Fix applied — 2026-05-14

Fix landed on this branch. Plan: `claude-notes/plans/2026-05-14-list-table-multiblock-cell-fix.md`.

Summary of the change in `crates/pampa/src/writers/qmd.rs`:

- Replaced the cell-emission block in `write_list_table` (~lines 1069-1116 of the pre-fix file) with a uniform three-shape algorithm: empty cell / first block is `Plain`/`Paragraph` / first block is anything else. Subsequent blocks (2nd … nth) within any cell are emitted as blank-line-separated 4-space-indented stanzas.
- Added two helpers: `write_cell_block_on_marker_line` (for the first block when it is not `Plain`/`Paragraph` — puts its first line on the marker line, indents continuation lines) and `write_cell_block_indented` (for every subsequent block).

One refinement vs. the triage's original fix sketch: the case where the first block is non-`Plain`/non-`Paragraph` (e.g. a `CodeBlock`-only cell) does **not** leave the marker line empty followed by a blank line — that shape introduced a phantom empty `Paragraph` in the reparsed AST. Instead the block's first line continues the marker line, mirroring how a regular CommonMark list item with non-`Plain` content looks. Verified by probing the reader before committing.

Validated:

- `test_qmd_roundtrip_consistency` (was the regression sentinel): all three new fixtures green.
- `cargo nextest run --workspace`: 8859/8859 pass.
- End-to-end CLI repro: writer output for the bug-report input round-trips back to a byte-identical AST.
- No snapshot deltas: the change does not affect any existing `list-table-*` snapshot (all use single-`Plain` cells, an unchanged path).

## Verification commands used

```bash
gh issue view 183 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments

# Pre-flight (in main checkout)
cargo xtask verify --skip-hub-build

# In the worktree
cargo xtask verify --skip-hub-build --skip-hub-tests

# Reproduce
cargo run --bin pampa -- claude-notes/issue-reports/183/repro.qmd
cargo run --bin pampa -- -t qmd claude-notes/issue-reports/183/repro.qmd
cargo run --bin pampa -- -t qmd claude-notes/issue-reports/183/repro.qmd \
  | tee claude-notes/issue-reports/183/observed-output.qmd
cargo run --bin pampa -- claude-notes/issue-reports/183/observed-output.qmd  # parse error

# Side fixtures
cargo run --bin pampa -- claude-notes/issue-reports/183/exp-single-codeblock.qmd
cargo run --bin pampa -- -t qmd claude-notes/issue-reports/183/exp-single-codeblock.qmd
cargo run --bin pampa -- claude-notes/issue-reports/183/exp-two-paras.qmd
cargo run --bin pampa -- -t qmd claude-notes/issue-reports/183/exp-two-paras.qmd

# Hand-fixed shape
cargo run --bin pampa -- claude-notes/issue-reports/183/expected-output.qmd
```

## Cross-references

- **bd-oxsr** — beads issue filed from this triage. Carries the fix-scope description and TDD plan.
- GitHub #174 — loose-bullet-list writer drops looseness when a nested sublist is present. Related symptom (round-trip failure caused by writer output), independent code path.
- GitHub #180 — Figure-then-Para spacing bug. CLOSED. Same family ("writer emits qmd the reader rejects"), independent code path.
- `crates/pampa/CLAUDE.md` — mandatory TDD checklist for any fix in this crate.
- `crates/pampa/src/writers/qmd.rs:937` — `write_list_table` (target of the fix).
- `crates/pampa/src/writers/qmd.rs:1124-1128` — incremental-writer coupling note: tables are always fully rewritten, so the fix does not need to handle incremental splicing.
- Reporter-cited quarto-web usages (real-world hits):
  - `docs/extensions/lua-api.qmd:292`
  - `docs/blog/posts/2026-03-24-1.9-release/index.qmd:91`
  - `docs/blog/posts/2026-03-24-1.9-release/index.qmd:114`
