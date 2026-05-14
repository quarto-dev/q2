# Issue #195 / bd-u50w — fix qmd writer for truly-empty bullet-list items

- **GitHub issue**: https://github.com/quarto-dev/q2/issues/195
- **Beads**: bd-u50w
- **Triage**: claude-notes/issue-reports/195/triage.md
- **Branch**: `issue-195` (worktree `.worktrees/issue-195`)
- **Start date**: 2026-05-14

## Overview

The qmd writer fails to round-trip two distinct AST shapes involving
"truly empty" list items (where the item's `Vec<Block>` has length 0,
written `[]` in pandoc-native).

1. **Plain `BulletList`**: the empty item is silently dropped on write
   (no marker line emitted). Round-trip: `[..., []]` → `[...]`.
2. **`:::{.list-table}` cells**: the empty cell is written as the literal
   text `- []`, which the reader re-parses as an inline `[]` inside a
   `Plain` block. Round-trip: cell `[]` → `[Plain []]`.

Triage identified `write_orderedlist` as having the same root cause
class (no `is_empty_item` check at all), bundled into the same fix.

### Reader contract (confirmed during triage)

- Bare marker line `-` or `*` (with optional trailing space) → item with
  zero blocks (`Vec<Block>` of length 0). This is the syntactic shape
  the reporter's input uses, and what every existing in-the-wild
  instance produces.
- Literal `- []` → item containing one `Plain` block with an empty
  inline list (`[Plain []]`). The existing test fixture
  `empty_list_item.qmd` covers this shape and must keep round-tripping.

These are *distinct* AST shapes and must serialize differently. The
existing writer conflates them by emitting `* []` for one case and
nothing for the other.

### Fix shape

Emit a bare marker line (`*\n` / `-\n` / `1.\n`) for truly-empty
items / cells in:

- `write_bulletlist` — currently `is_empty_item` only matches
  `item.len() == 1` (the `[Plain []]` shape). Add a branch for
  `item.is_empty()` (the `[]` shape).
- `write_orderedlist` — currently no empty-item handling at all.
  Add `is_empty()` and `is_empty_item` branches mirroring bullet list.
- `write_list_table` — currently writes literal `[]` after the `- `
  marker for cells without attrs. Change to write nothing (terminating
  the line, so the cell appears as a bare `-` marker).

Trailing whitespace on the `- ` line (in the list-table case) is
relied on already by other code paths in this writer (the `- ` token
is emitted unconditionally at L1095 before the cell content). The
round-trip test will pin whether the reader tolerates it; if not, the
fix grows to restructure the marker emission to be content-aware.

## Phases

### Phase 1 — Failing tests (TDD step 1-2)

Round-trip qmd-json-qmd fixtures are picked up automatically by
`test_qmd_roundtrip_consistency` in `crates/pampa/tests/test.rs`
(globs `tests/roundtrip_tests/qmd-json-qmd/*.qmd`). Adding fixtures is
the way to add coverage.

- [x] Add fixture `empty_bullet_item_trailing.qmd` — plain bullet list,
      trailing empty item.
- [x] Add fixture `empty_bullet_item_only.qmd` — single-item bullet
      list whose only item is empty (`-\n`).
- [x] Add fixture `empty_bullet_item_nested.qmd` — the exact shape
      from the reporter's first repro (outer list of inner lists,
      trailing empty inner item).
- [x] Add fixture `empty_ordered_item_trailing.qmd` — ordered list
      analog of `empty_bullet_item_trailing.qmd`.
- [x] Add fixture `list_table_empty_cell.qmd` — list-table with one
      empty cell (the reporter's second repro shape).
- [x] Add fixture `list_table_empty_cell_with_attrs.qmd` — list-table
      with an empty cell that has attributes (`colspan`/`rowspan`),
      to pin behavior of the `needs_attrs` branch which is intended
      to keep writing `[]{...attrs}`.
- [x] Run `cargo nextest run -p pampa test_qmd_roundtrip_consistency`,
      capture failure output for each new fixture so the symptom is
      documented before the fix lands.

### Phase 2 — Implementation

- [x] `write_bulletlist`: add `if item.is_empty()` branch that emits
      bare marker. Also revised `is_tight` to skip empty items rather
      than force the list loose.
- [x] `write_orderedlist`: add `if item.is_empty()` branch that emits
      bare marker (computed inline from `delimiter`/`number_style`).
      Same `is_tight` revision. Did NOT add an `is_empty_item` /
      `[Plain []]` branch: that AST shape doesn't arise from the reader
      for ordered lists (a bare `1. []` parses as `[Plain [Span [] []]]`
      via the span-shorthand path, not `[Plain []]`).
- [x] `write_list_table`: in the empty-cell branch, drop the literal
      `[]` when `!needs_attrs`. The `- ` token already emitted at L1125
      is sufficient (trailing space is accepted by the reader, verified
      by round-trip). Kept `[]{...attrs}` form when attrs are present —
      that path produces `[Plain []]` cell content, which round-trips
      faithfully (pinned by `list_table_empty_cell_with_attrs.qmd`).
- [x] Decision: did NOT change `* []` output for `[Plain []]` shape.
      Reader parses `[]` as inline text inside `Plain`, so the existing
      writer behavior is the only way to preserve that AST. The two
      shapes (`[]` and `[Plain []]`) now serialize distinctly; comment
      block at the top of the empty-cell branch documents why.

### Phase 3 — Verify

- [x] Run new round-trip fixtures, confirm pass. `cargo nextest run -p
      pampa --test test test_qmd_roundtrip_consistency` → 1 passed, 15
      skipped.
- [x] Run full pampa test suite. `cargo nextest run -p pampa` → 3687
      passed, 2 skipped.
- [x] Run full workspace tests. `cargo nextest run --workspace` → 8864
      passed, 195 skipped.
- [x] Run `cargo xtask verify --skip-hub-build --skip-hub-tests` →
      "All verification steps passed!".
- [x] End-to-end CLI verification with the reporter's exact commands
      (see § End-to-end evidence below).
- [x] Record E2E evidence in this plan (per CLAUDE.md "End-to-end
      verification before declaring success" section).

### Phase 4 — Wrap

- [ ] Update bd-u50w with the commit SHA and close it.
- [ ] `br sync --flush-only` + commit JSONL on main.
- [ ] Prepare PR description (don't push without permission).

## Implementation notes / open questions

- **Trailing whitespace tolerance.** If the reader rejects `- \n`
  (with trailing space) but accepts `-\n`, the `write_list_table` fix
  needs to restructure the marker emission at L1095 to be content-aware.
  Will know after writing Phase 1 tests.
- **`write_orderedlist` and `OrderedListContext` prefix.** The
  ordered-list context emits the number marker only on first content
  write. For length-0 items I'll need to write `writeln!(buf)?` after
  the marker, or write `writeln!(buf, "{}.", n)` directly bypassing
  the context.
- **Nested context safety.** When `write_bulletlist` is called inside
  another list (outer item content), `buf` is itself a
  `BulletListContext`. Emitting `writeln!(buf, "*")?` for a length-0
  inner item passes through the outer's prefix machinery correctly:
  outer prepends `* ` (first line) or `  ` (continuation), then the
  literal `*\n` follows. Confirmed by reading the context's `write`
  impl.

## End-to-end evidence (observed 2026-05-14)

All transcripts below are the actual output of `target/debug/pampa`
built from the fix commit. Output was inspected by hand and compared
against the pre-fix transcripts captured during triage (see
`claude-notes/issue-reports/195/triage.md` § Reproduction).

### Reporter's first repro (plain bullet list)

```
$ printf -- '- - H1\n  - H2\n- - X\n  -\n' | target/debug/pampa
[ BulletList [[BulletList [[Plain [Str "H1"]], [Plain [Str "H2"]]]], [BulletList [[Plain [Str "X"]], []]]] ]

$ printf -- '- - H1\n  - H2\n- - X\n  -\n' | target/debug/pampa -t qmd
* * H1
  * H2

* * X
  *

$ printf -- '- - H1\n  - H2\n- - X\n  -\n' | target/debug/pampa -t qmd 2>/dev/null | target/debug/pampa
[ BulletList [[BulletList [[Plain [Str "H1"]], [Plain [Str "H2"]]]], [BulletList [[Plain [Str "X"]], []]]] ]
```

AST identical after one round trip. The trailing empty inner item
(`Vec<Block> = []`) is now preserved. The writer emits the bare `*`
marker for the empty item; the outer bullet-list context prepends its
continuation prefix (`"  "`), giving the literal line `"  *"`.

A second round trip (`qmd → qmd → qmd`) is byte-identical to the first,
so the writer is idempotent on this input.

### Reporter's second repro (list-table empty cell)

```
$ printf -- ':::{.list-table}\n- - H1\n  - H2\n- - X\n  -\n:::\n' | target/debug/pampa
[ Table … Cell …  AlignDefault (RowSpan 1) (ColSpan 1) [Plain [Str "X"]] ,
                Cell …  AlignDefault (RowSpan 1) (ColSpan 1) [] ] …

$ printf -- ':::{.list-table}\n- - H1\n  - H2\n- - X\n  -\n:::\n' | target/debug/pampa -t qmd
::: {.list-table}

* - H1
  - H2
* - X
  -
:::

$ printf -- ':::{.list-table}\n- - H1\n  - H2\n- - X\n  -\n:::\n' | target/debug/pampa -t qmd 2>/dev/null | target/debug/pampa
[ Table … Cell …  AlignDefault (RowSpan 1) (ColSpan 1) [Plain [Str "X"]] ,
                Cell …  AlignDefault (RowSpan 1) (ColSpan 1) [] ] …
```

Empty cell content stays `[]`. Before the fix this round-trip produced
`[Plain []]` (`[]` was reparsed as inline text). The writer now emits
the bare `- ` token (trailing space from the unconditional `- ` write
at L1125 of the writer) and no `[]` placeholder. Second round trip is
byte-identical.

### Existing `empty_list_item.qmd` fixture still round-trips

The `[Plain []]` AST shape (input `* []`) must keep working — it's a
different shape from the `[]` case fixed here. Confirmed:

```
$ cat crates/pampa/tests/roundtrip_tests/qmd-json-qmd/empty_list_item.qmd
* []

$ … | target/debug/pampa
[ BulletList [[Plain []]] ]

$ … | target/debug/pampa -t qmd
* []

$ … | target/debug/pampa -t qmd | target/debug/pampa
[ BulletList [[Plain []]] ]
```

The two AST shapes (`[]` vs `[Plain []]`) now serialize distinctly and
round-trip independently.
