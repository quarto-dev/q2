# Issue #152 — Table caption attributes are dropped by qmd writer

- **GitHub**: https://github.com/quarto-dev/q2/issues/152
- **Reporter**: @rundel (Colin Rundel), 2026-05-03
- **Triage date**: 2026-05-03
- **Worktree**: `.worktrees/issue-152` (branch `issue-152`, based on `main` @ `132c13c8`)
- **Beads issue**: bd-f3pl ("qmd writer drops table caption attributes (issue #152)", priority 1, bug)
- **Scope**: this report covers only the **second** issue in #152 ("Table caption attributes are not written"). The first issue (old-style code-block options being mangled in qmd output) is being deprecated upstream and is intentionally not addressed here.

## Summary

When a table caption carries a Quarto attribute block (`: caption {tbl-colwidths="[30,70]"}`), the parser correctly attaches the attribute to the `Table` node, but the **pipe-table branch** of the qmd writer drops it on output. The list-table branch already handles this correctly. Round-tripping is therefore lossy for any pipe-formatted table that has attributes.

## Reproduction

Fixture: `claude-notes/issue-reports/152/repro.qmd`

```
| A | B |
|---|---|
| 1 | 2 |

: ABCD {tbl-colwidths="[30,70]"}
```

### Native AST output (parser is fine)

```
$ cargo run --bin pampa -- < claude-notes/issue-reports/152/repro.qmd
[ Table ( "" , [] , [("tbl-colwidths", "[30,70]")] )
        (Caption Nothing [ Plain [Str "ABCD"] ])
        ... ]
```

`Table.attr.2` (the keyvals slot) correctly contains `("tbl-colwidths", "[30,70]")`. The desugaring contract documented in `docs/syntax/desugaring/table-captions.qmd` is honored by the reader path.

### Round-tripped qmd output (writer drops the attr)

```
$ cargo run --bin pampa -- -t qmd < claude-notes/issue-reports/152/repro.qmd
| A   | B   |
| --- | --- |
| 1   | 2   |

: ABCD                      ← attribute block missing
```

Expected output:

```
| A   | B   |
| --- | --- |
| 1   | 2   |

: ABCD {tbl-colwidths="[30,70]"}
```

## Localization

**File**: `crates/pampa/src/writers/qmd.rs`

There are two table-writer code paths:

| Branch | Function | Lines | Handles `table.attr`? |
|--------|----------|-------|------------------------|
| pipe table | `write_table` | 1120–1239 | **No** — `table.attr` is never read; only the caption's inline content is emitted at lines 1217–1235. |
| list-table sugar | `write_list_table` | 928–1118 | **Yes** — id at lines 983–987, classes at 935, keyvals at 977–980 (copied into the div attribute block). |

The bug therefore lives in the pipe-table branch: it writes `: <caption>\n` (lines 1228–1234) but never emits the table's id/classes/keyvals.

The companion JSON parser snapshot at `crates/pampa/tests/snapshots/json/table-caption-attr.qmd` exercises exactly this fixture for the read path, but there is no corresponding round-trip test in `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/`. Adding that fixture (and any related ones) is part of the fix.

## Surface syntax

Per `docs/syntax/desugaring/table-captions.qmd`, table-caption attributes are extracted from the caption inline content during postprocessing and merged into `Table.attr` (id, classes, keyvals). The writer must reverse this: when emitting the caption line, append `{<attr-block>}` if `Table.attr` is non-empty.

The existing `write_attr` helper in `qmd.rs` (line 396) already handles the formatting for inline attribute blocks, so no new formatting machinery is needed — only a call site at the end of the pipe-table caption.

## Suggested fix scope

1. **Test first** (per `crates/pampa/CLAUDE.md`): add a failing round-trip fixture under `tests/roundtrip_tests/qmd-json-qmd/`. Candidate files (split per CLAUDE.md "many small fixtures" guidance):
   - `table-caption-with-keyval.qmd` — `: cap {tbl-colwidths="[30,70]"}`
   - `table-caption-with-id.qmd` — `: cap {#tbl-foo}`
   - `table-caption-with-classes.qmd` — `: cap {.striped .hover}`
   - `table-caption-with-mixed-attrs.qmd` — id + classes + keyvals together (mirrors the docs example)
2. Confirm each new fixture **fails** the round-trip equality check before any code change.
3. In `write_table` (pipe-table branch, around lines 1228–1234), after the caption text is written but before the trailing newline, emit ` {...}` via `write_attr(&table.attr, buf, ctx)?` if `!is_empty_attr(&table.attr)`. The exact placement (` {…}\n` vs. `\n: caption {…}\n`) should match what the parser accepts and what `table-caption-attr.qmd`'s native AST round-trips to.
4. Run the new fixtures and the full pampa suite. Confirm no existing snapshots regress (`table-caption.qmd` should be unchanged because that fixture has empty `Table.attr`).
5. Run `cargo xtask verify --skip-hub-build` (Rust-only change in pampa, no quarto-core/pandoc-types touched, so the WASM leg is not affected).

Estimated diff size: < 20 lines in `qmd.rs`, plus 4 small fixtures and their `.snap` files.

## Open questions — resolved during triage

### 1. Empty-id auto-suppression — **NOT NEEDED**

Resolved by reading `crates/pampa/src/pandoc/treesitter_utils/pipe_table.rs:148–200`. The only path that writes to `Table.attr.0` is the `Inline::Attr` extraction at lines 191–195, which fires only when the user explicitly authored `{#id ...}` in the caption attribute block. There is no equivalent of figures' implicit-`fig-` numbering for tables. The reader does record `attr_source` (line 149), so if implicit `tbl-foo` numbering is ever introduced, the writer can adopt the same `attr_source.id.is_none()` guard headers use (`qmd.rs:557–561`). For this fix, no guard is required — emit `Table.attr.0` unconditionally when non-empty.

### 2. Table-attr-prefix vs. caption-suffix — **suffix is the only valid form**

Tested both shapes against pandoc 3.9.0.2 and pampa at `issue-152` HEAD.

| Fixture | Form | Pandoc native AST | Pampa native AST |
|---------|------|--------------------|------------------|
| `exp-prefix.qmd` | `{attrs}\n` line, then table, then `: caption` | **Not a table.** One `Para` containing literal `Str "{tbl-colwidths="`, `Quoted`, `Str "}"`, `SoftBreak`, then the pipe rows as `Str "|"`/`Space`/`Str "A"`/etc. The caption becomes a separate `Para [Str ":", Space, Str "ABCD"]`. | **Not a table.** Emits `Q-0-99` ("Caption found without a preceding table") and `Q-3-32` ("Standalone attributes not supported"). |
| `exp-suffix.qmd` | table, then `: caption {attrs}` | Proper `Table` with `attr = ("", [], [("tbl-colwidths", "[30,70]")])`. | Identical: `Table ( "" , [] , [("tbl-colwidths", "[30,70]")] ) ...`. |
| `exp-mixed.qmd` | suffix form with id + classes + keyvals (the docs example) | `Table ( "tbl-mytable" , ["special"] , [("tbl-colwidths", "[30,70]")] ) ...` | Byte-identical attr triple; same caption inlines. |

Both engines treat caption-suffix as the **only** way to attach attrs to a pipe table. The prefix form is a parser-level non-starter, not a valid alternative we'd ever want to emit. The writer fix is therefore unambiguous: append `{...}` to the caption line. (The list-table branch's div-attr placement is unrelated — that's the sugared form, which has its own surface syntax and is not a writer choice for pipe tables.)

Fixture files retained under `claude-notes/issue-reports/152/exp-{prefix,suffix,mixed}.qmd` for the record.

### 3. Comment in `write_table` at line 575 — **orthogonal, no action**

Confirmed: the `// FIXME` comment lives inside `write_cell_content` (the helper at line 575), not the caption-emission code we'll touch. The newline-in-pipe-cells limitation it warns about is a real bug but unrelated to this round-trip fix.

## Verification commands used during triage

```bash
# Worktree bootstrap
git worktree add -b issue-152 .worktrees/issue-152 main
echo "../../../.beads" > .worktrees/issue-152/.beads/redirect
cd .worktrees/issue-152
npm install                    # required on fresh worktrees; see bd-7giz
cargo xtask verify             # all 9 steps green at HEAD before any change

# Reproduction (from worktree root)
cargo run --bin pampa -- -t qmd < claude-notes/issue-reports/152/repro.qmd
cargo run --bin pampa --       < claude-notes/issue-reports/152/repro.qmd  # native AST
```

Both confirmed at branch `issue-152` HEAD `132c13c8`.

## Cross-references

- bd-7giz — `cargo xtask setup` for fresh-worktree bootstrap (discovered while preparing this triage).
- `docs/syntax/desugaring/table-captions.qmd` — defines the read-side desugaring contract this writer should reverse.
- `crates/pampa/tests/snapshots/json/table-caption-attr.qmd` — existing parser-side snapshot covering this fixture.
- `crates/pampa/CLAUDE.md` — TDD workflow for round-trip bug fixes (`tests/roundtrip_tests/qmd-json-qmd`).
