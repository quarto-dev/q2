# Investigation notes — bd-9fwn1504

Proptest counterexample: `property_tests::reconciliation_preserves_structure_full_ast`
in `crates/quarto-ast-reconcile` fails on the persisted seed
`cc 2c379a4ae900cb3d235f771b204e0cb0e307ee6808562d588dc6ecd81088e38e`
(file `crates/quarto-ast-reconcile/proptest-regressions/lib.txt`, currently
**untracked on purpose** — committing it makes the suite deterministically red
until the fix lands; the fix session commits it as the TDD regression pin).

## Root cause (confirmed by minimal repro)

The `needs_plan` optimization in `compute.rs` drops a nested
`ReconciliationPlan` when **every executed-side alignment is `KeepBefore`**
and there are no nested container plans, on the theory that "everything was
kept, so the contents are identical." That inference is wrong: alignments are
one-per-*executed* item, so "all executed items were found in original" says
nothing about **extra original items** (deletions), nor about **order**
(`KeepBefore` matches by hash at *any* position).

When the plan is dropped, the apply-side fallback uses the **original**
content wholesale "to preserve source_info" — resurrecting deleted blocks
(and, in the reorder case, restoring the old order).

In the CI counterexample: original table `caption.long` =
`[HorizontalRule, DefinitionList{…}, CodeBlock, Paragraph]`, executed
`caption.long` = `[HorizontalRule]`. The lone executed block hash-matches
original index 0 → alignments `[KeepBefore(0)]` → `needs_plan = false` →
`caption_plan = None` → apply keeps all four original blocks. Result
caption.long ≠ after caption.long → property violated.

Minimal repro (validated, fails identically): original caption.long
`[para "a", para "b"]`, executed `[para "a"]` → result has len 2, expected 1.
Code preserved in `minimal-repro-test.rs.txt` (drop into the
`property_tests` module of `lib.rs` to rerun).

## Affected sites (all share the same fallacy)

Compute side (plan dropped when all-`KeepBefore`):

1. **Table caption** — `compute.rs:507-526` (`compute_table_plan`), apply
   fallback at `apply.rs:713-719` (`long: Some(orig_long)`). ← the CI case
2. **Table cells** — `compute.rs:539-556` (`reconcile_rows` closure), apply
   fallback at `apply.rs:600-612` ("No plan means content matched exactly —
   use orig content").
3. **Custom-node `Slot::Blocks`** — `compute.rs:435-451`, apply fallback at
   `apply.rs:456-464` ("Same type, content must match (otherwise we'd have a
   plan)" → uses orig slot wholesale).
4. **Custom-node `Slot::Inlines`** — `compute.rs:465-479`, same apply
   fallback.

Sites 2–4 are *invisible to the property test today* (see next section) but
are real content-corruption bugs in the preview/write-back path: a cell or
slot that lost blocks between original and executed silently gets them back.

Second manifestation (same code path, not separately run but guaranteed by
inspection): pure **reordering** with identical items — original `[a, b]`,
executed `[b, a]` → alignments `[KeepBefore(1), KeepBefore(0)]`, all
`KeepBefore` → plan dropped → original order restored.

## Why the property test only caught the caption

`structural_eq_block` for `Block::Table` is a **simplified comparison**
(`hash.rs:860-865`): only `attr`, `colspec`, and `caption.long`. Cell
contents, head/foot, and bodies are not compared, so cell-level divergence
cannot fail the property test. Note the asymmetry: the *hash*
(`hash.rs:194-224`) covers the full table, eq does not. Filed as discovered
work (see plan).

## How the failure was localized (recipe, reusable)

proptest replays every `cc` seed in `proptest-regressions/lib.txt` against
**every** proptest in `lib.rs`. So a "debug twin" test with the *identical
generator signature* (`before in gen_full_pandoc(), after in
gen_full_pandoc()`) reproduces the exact failing inputs, but can print
diagnostics instead of asserting. See `minimal-repro-test.rs.txt` (second
test in the file) — it prints the plan's block alignments plus the first
diverging block triple (before/result/after) at ~5k lines each.

Debug dumps contain `source_info:`/`attr_source:` subtrees that legitimately
differ (that's the whole point of reconciliation), so a naive diff is noise.
`strip_source_info.py` strips those subtrees; diffing the cleaned dumps
(`result0-clean.txt` vs `after0-clean.txt`, kept here as evidence) shows the
three resurrected caption blocks and nothing else. (The line offsets at the
top of the script are specific to that captured run; re-grep the markers if
you regenerate.)

## Reproduction status

- `cargo xtask verify --skip-hub-build` at `main` @ 78d55deb: everything
  green **except** exactly this test (7744 passed, 1 failed, fail-fast) —
  confirming the strand's "pre-existing latent bug on main" claim.
- Deterministic: seed file + `cargo nextest run -p quarto-ast-reconcile -E
  'test(reconciliation_preserves_structure_full_ast)'`.
