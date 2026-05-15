# Issue #195 — Empty bullet-list items do not round-trip: dropped in plain `BulletList`, mutated to `[Plain []]` in list-table cells

- **GitHub**: https://github.com/quarto-dev/q2/issues/195
- **Reporter**: @rundel (Colin Rundel), 2026-05-14
- **Triage date**: 2026-05-14
- **Worktree**: `.worktrees/issue-195` (branch `issue-195`, based on `main` @ `59e8003f`)
- **Beads issue**: bd-u50w
- **Scope**: both reported facets — (a) plain `BulletList` dropping a trailing empty item on round-trip, and (b) `:::{.list-table}` cells mutating empty cells from `[]` to `[Plain []]`. Both have the same root cause class (writer emits the wrong text for a *truly* empty AST item / cell, i.e. `Vec<Block>` of length 0).

## Summary

Two qmd-writer bugs reported as one issue, both reproduced exactly as
described. Reader behavior is correct: input `-` (bare bullet marker, no
content) parses to a `Vec<Block>` of length 0 (`[]`). The writer, however,
has no codepath that emits a bare `-` marker line. For plain `BulletList`
the truly-empty item falls through to a loop that writes nothing, so the
item disappears. For list-table cells the writer emits literal `- []`,
which the reader reparses as `[Plain []]` (`[]` becomes inline text inside
a `Plain` block).

The fix is small and localized to two functions in
`crates/pampa/src/writers/qmd.rs`. The reporter's suggested approach
(emit a bare `-` marker for empty items) matches existing reader behavior
and is the right shape.

## Reproduction

Fixtures committed alongside this doc:

- `repro-plain.qmd` — plain `BulletList` with a trailing empty item
- `repro-list-table.qmd` — `:::{.list-table}` with an empty cell

### Plain bullet list (item dropped)

```
$ cat claude-notes/issue-reports/195/repro-plain.qmd
- - H1
  - H2
- - X
  -

$ cargo run --quiet --bin pampa -- < claude-notes/issue-reports/195/repro-plain.qmd
[ BulletList [[BulletList [[Plain [Str "H1"]], [Plain [Str "H2"]]]],
              [BulletList [[Plain [Str "X"]], []]]] ]

$ cargo run --quiet --bin pampa -- -t qmd < claude-notes/issue-reports/195/repro-plain.qmd
* * H1
  * H2

* * X
                                       # ← no marker line for the empty item

$ <same input> | qmd -> qmd -> AST:
[ BulletList [[BulletList [[Plain [Str "H1"]], [Plain [Str "H2"]]]],
              [BulletList [[Plain [Str "X"]]]]] ]
                                       # ← the empty [] child is gone
```

### List-table cell (`[]` mutated to `[Plain []]`)

```
$ cat claude-notes/issue-reports/195/repro-list-table.qmd
:::{.list-table}
- - H1
  - H2
- - X
  -
:::

$ cargo run --quiet --bin pampa -- -t qmd < claude-notes/issue-reports/195/repro-list-table.qmd
::: {.list-table}

* - H1
  - H2
* - X
  - []                                  # ← writer emits literal "- []"
:::

# After round-trip, the second cell on the last row goes from
#   Cell ... []            (empty content)
# to
#   Cell ... [Plain []]    (Plain block containing zero inlines)
```

## Localization

Both bugs live in `crates/pampa/src/writers/qmd.rs`.

**Bug A — plain `BulletList` drops truly-empty items.** `write_bulletlist`
at L461–502. The `is_empty_item` predicate at L480–485 only matches
items of `item.len() == 1` whose single block is an empty `Plain` /
`Paragraph`:

```rust
let is_empty_item = item.len() == 1
    && match &item[0] {
        Block::Plain(plain) => plain.content.is_empty(),
        Block::Paragraph(para) => para.content.is_empty(),
        _ => false,
    };
```

A truly empty item (`item.is_empty()`, i.e. `Vec<Block>` of length 0)
falls through to the `else` branch at L490–499, where the inner
`for (j, block) in item.iter().enumerate()` loop runs zero times and
writes nothing. The outer `BulletListContext`'s prefix machinery is
all that's left, producing the `  ` (two-space) blank line we saw —
not a `*` marker. There is no codepath in this function that ever
emits a bare `*` (or `*\n`) marker.

**Bug B — list-table empty cells written as `- []`.** `write_list_table`
at L986–1163. The empty-cell branch at L1129–1133:

```rust
if cell.content.is_empty() {
    if !needs_attrs {
        write!(buf, "[]")?;
    }
    writeln!(buf)?;
}
```

The marker `- ` has already been written at L1095. So an empty cell with
no special attrs becomes the literal line `- []`. The reader then parses
`[]` as inline text inside the cell's content, producing `[Plain []]`.

**Model for the fix.** The reader already accepts a bare `-` line as an
empty item — the input `repro-plain.qmd` parses to `[Plain "X"], []`
exactly because of that. The fix in both functions is to emit a bare
marker (e.g. `*` or `-` followed by `\n`) instead of nothing (Bug A) or
`- []` (Bug B). The existing fixture
`crates/pampa/tests/roundtrip_tests/qmd-json-qmd/empty_list_item.qmd`
covers a *different* shape — `[Plain []]` writing back to `* []` — and
should keep round-tripping; only the truly-empty (`[]`) case needs new
behavior.

## Open questions — resolved during triage

- **Q: Is the same bug present in `write_orderedlist` (L504+) for
  ordered lists?**
  Inspected the function. `write_orderedlist` has no `is_empty_item`
  check at all, so an empty item iterates zero blocks and writes
  nothing (analogous to Bug A). Not reported in #195, but the same
  fix-shape applies. Flagging in the beads issue's "in scope?" section
  rather than triaging separately — bundled work is small.
- **Q: Does `- ` with trailing whitespace round-trip the same as bare
  `-`?**
  Not tested in this triage. The fix should prefer no trailing
  whitespace to keep snapshot diffs clean. The implementer should
  verify by reading the existing reader behavior or adding a fixture.

## Outcome / recommended next step

Filed bd-u50w with the fix scope: emit bare marker lines for truly-empty
items in (1) `write_bulletlist`, (2) `write_orderedlist` (covered by the
same root cause; flagged as a discovered defect), and (3) the empty-cell
branch of `write_list_table`. Add round-trip fixtures for each of the
two reported AST shapes under `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/`.

## Triage-time side quest: beads JSONL corruption (resolved)

During this triage, `br` refused to operate due to a pre-existing JSONL
inconsistency (`dependencies row count mismatch — expected 900, found
882`). Root cause: two parallel branches both touched
`.beads/issues.jsonl`; a merge landed a state that dropped five records
(`bd-kw93`, `bd-mrx1`, `bd-hfjj`, `bd-pf63`, `bd-z529`) while keeping
later commits that referenced them as parents/deps, leaving 18 dangling
edges. All five records were restored verbatim from git history in a
separate commit on `main` (see commit message
"sync beads: restore 5 records dropped by branch merge"). After
restore: `br doctor` reports HEALTH OK. Unrelated to issue #195 itself.

## Verification commands used

```bash
# Pre-flight
cargo xtask verify --skip-hub-build

# Issue retrieval
gh issue view 195 --repo quarto-dev/q2 \
  --json title,body,author,createdAt,labels,comments

# Reproduce both bugs
cargo build --bin pampa
cargo run --quiet --bin pampa -- \
  < claude-notes/issue-reports/195/repro-plain.qmd
cargo run --quiet --bin pampa -- -t qmd \
  < claude-notes/issue-reports/195/repro-plain.qmd
cargo run --quiet --bin pampa -- -t qmd \
  < claude-notes/issue-reports/195/repro-plain.qmd \
  | cargo run --quiet --bin pampa --
# (and likewise for repro-list-table.qmd)

# Localize
grep -n "BulletList\|list_table\|list-table" crates/pampa/src/writers/qmd.rs
```

## Cross-references

- `crates/pampa/src/writers/qmd.rs` — `write_bulletlist` (L461),
  `write_orderedlist` (L504), `write_list_table` (L986).
- `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/empty_list_item.qmd`
  — existing coverage for `[Plain []]`, **not** for `[]`.
- `crates/pampa/CLAUDE.md` — round-trip test workflow (test first, fail
  first, then fix).
- In-the-wild instance cited by the reporter:
  https://github.com/quarto-dev/quarto-web/blob/baeab38627fcc3f3a9ea3ca3ea689ece413df65d/docs/extensions/lua-api.qmd#L322
