# Issue #175 — qmd writer drops empty header row from pipe tables, promotes first body row to header

- **GitHub**: https://github.com/quarto-dev/q2/issues/175
- **Reporter**: @rundel (Colin Rundel), 2026-05-11
- **Triage date**: 2026-05-11
- **Worktree**: `.worktrees/issue-175` (branch `issue-175`, based on `main` @ `53394156`)
- **Beads issue**: bd-7mpv
- **Scope**: the single round-trip bug described in the issue body. No other reports.

## Summary

A pipe table whose header row is all empty cells is parsed as `Table` with
`TableHead [] []` (zero header rows). The qmd writer's `write_table` path
unconditionally emits the first collected row as the header line, so when
there are no header rows it promotes the first body row instead, and the
re-parser then reads that body row as the header. Round-trip loses one
body row and gains one (wrong) header row. Real bug, root cause is one
function in `crates/pampa/src/writers/qmd.rs`, fix scope is small. Pandoc's
`gfm` writer demonstrates the expected output (emit an empty header line
`|     |     |`).

## Reproduction

Fixture: `claude-notes/issue-reports/175/repro.qmd`

```
|     |     |
|-----|-----|
| a   | b   |
| c   | d   |
```

```
$ cargo run --quiet --bin pampa -- claude-notes/issue-reports/175/repro.qmd
[ Table … (TableHead ("",[],[]) [])
  [TableBody ("",[],[]) (RowHeadColumns 0) []
    [Row … [Cell … [Plain [Str "a"]], Cell … [Plain [Str "b"]]]
    ,Row … [Cell … [Plain [Str "c"]], Cell … [Plain [Str "d"]]]]]
  …]
# ✓ parser correctly produces zero header rows
```

```
$ cargo run --quiet --bin pampa -- -t qmd claude-notes/issue-reports/175/repro.qmd
| a   | b   |
| --- | --- |
| c   | d   |
# ✗ writer emits 'a | b' as the header line; the empty original header is gone
```

```
$ cargo run --quiet --bin pampa -- -t qmd claude-notes/issue-reports/175/repro.qmd \
    | cargo run --quiet --bin pampa --
[ Table … (TableHead ("",[],[]) [Row … [Cell … [Plain [Str "a"]], Cell … [Plain [Str "b"]]]])
  [TableBody … [Row … [Cell … [Plain [Str "c"]], Cell … [Plain [Str "d"]]]]]
  …]
# ✗ on re-parse, the first body row has become the header, body has 1 row instead of 2
```

Asymmetry check (`claude-notes/issue-reports/175/exp-empty-body-row.qmd`):
an all-empty *body* row round-trips correctly (`Plain []` cells are
preserved); the bug is specifically in the header path.

Pandoc reference behavior on the same input:

```
$ pandoc -f markdown -t native claude-notes/issue-reports/175/repro.qmd
… (TableHead ("",[],[]) []) …    # same AST as ours
$ pandoc -f markdown -t gfm claude-notes/issue-reports/175/repro.qmd
|     |     |
|-----|-----|
| a   | b   |
| c   | d   |                   # round-trips faithfully
```

## Localization

`crates/pampa/src/writers/qmd.rs:1120-1214` `write_table`:

- Line 1130-1143: builds a flat `all_rows: Vec<&Row>` from
  `table.head.rows` followed by every body row in `table.bodies`.
- Line 1145-1147: bails on empty.
- Line 1184-1190: writes `row_contents[0]` as the header line **without
  checking whether it came from `table.head.rows`**.
- Line 1206-1213: emits the remaining rows as body.

The fix needs to branch on `table.head.rows.is_empty()`:

- If zero header rows, emit one synthetic empty header line (cells of
  the configured width filled with spaces) and the separator, then emit
  *all* `row_contents` as body. This matches Pandoc `gfm` and matches
  what the parser will read back.
- If one header row (current happy path), keep current behavior.
- If more than one header row, the pipe-table format cannot represent
  it; `table_can_use_pipe_format` should be extended to reject
  multi-header tables and fall through to `write_list_table`.

The `table_can_use_pipe_format` predicate at lines 870-910 currently
inspects only cell shape and content, not header-row count, so the
multi-header check would be a small addition there.

## Open questions — resolved during triage

**Q1. Is this a parser bug or a writer bug?**
Experiment: parsed `repro.qmd` with both `pampa` and `pandoc 3.9.0.2`.
Both produce `TableHead [...] []` (zero header rows). The Pandoc data
model permits zero-header tables, so the parser is correct.
**Conclusion**: writer-only bug.

**Q2. Does the writer also corrupt all-empty *body* rows?**
Experiment: `exp-empty-body-row.qmd` (header `A|B`, empty body row,
then `c|d`). Round-trips faithfully — the empty body row stays as
`|     |     |`. The bug is scoped to the header path.
**Conclusion**: header line only; body rows are fine.

**Q3. What output should the fix produce?**
Experiment: `pandoc -t gfm` on the same input produces
`|     |     |` then the separator then the body rows verbatim.
**Conclusion**: emit a synthetic empty header line. This matches the
reporter's stated expectation and is consistent with at least one
established markdown writer.

**Q4. Are there impacted real-world docs?**
The issue links two quarto-web files (`docs/authoring/callouts.qmd`,
`docs/output-formats/all-formats.qmd`) that use this construct. Anyone
running these through `qmd-syntax-helper` or any other round-tripping
tool would silently lose a row.
**Conclusion**: not a hypothetical; user-visible impact on existing
content.

## Outcome / recommended next step

File a beads bug with the fix scope captured in Localization. No
follow-on GH response needed — the reporter already documented the
expected behavior. Recommend P1 (silent data loss in round-trip, real
content affected, small fix surface).

## Verification commands used

```bash
# Pre-flight (from main repo root)
cargo xtask verify --skip-hub-build

# Worktree setup
git worktree add -b issue-175 .worktrees/issue-175 main
echo "../../../.beads" > .worktrees/issue-175/.beads/redirect
cd .worktrees/issue-175 && npm install

# Reproduce
cargo run --quiet --bin pampa -- claude-notes/issue-reports/175/repro.qmd
cargo run --quiet --bin pampa -- -t qmd claude-notes/issue-reports/175/repro.qmd
cargo run --quiet --bin pampa -- -t qmd claude-notes/issue-reports/175/repro.qmd \
  | cargo run --quiet --bin pampa --

# Pandoc reference
pandoc -f markdown -t native claude-notes/issue-reports/175/repro.qmd
pandoc -f markdown -t gfm     claude-notes/issue-reports/175/repro.qmd

# Asymmetry check
cargo run --quiet --bin pampa -- -t qmd claude-notes/issue-reports/175/exp-empty-body-row.qmd
```

## Cross-references

- `crates/pampa/src/writers/qmd.rs:1120-1214` — `write_table` (root cause)
- `crates/pampa/src/writers/qmd.rs:870-910` — `table_can_use_pipe_format`
  (predicate that should grow a multi-header guard)
- `tests/roundtrip_tests/qmd-json-qmd` — per `crates/pampa/CLAUDE.md`,
  this is the directory for round-trip regression tests; the fix should
  add a fixture there.
- quarto-web files exercised by the bug (from the GH issue):
  - `docs/authoring/callouts.qmd:87`
  - `docs/output-formats/all-formats.qmd:33`
