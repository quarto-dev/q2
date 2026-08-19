# Flaky proptest: reconciliation_preserves_structure_full_ast (bd-205v6)

**Date:** 2026-08-19
**Braid:** bd-205v6
**Branch:** `braid/bd-205v6-cite-reconcile-identity` (off `main` @ `4b4a63ce`)
**Status:** Approved 2026-08-19 — in execution.

## Design decisions (user-aligned 2026-08-19)

1. **Fix shape: compute-side guard only (Option A).** Pair two Cites in Step 2 only
   when their `citations` are structurally equal; otherwise `UseAfter`. No
   apply-side copy of `e.citations`: unlike Link's `attr`/`target` (plain data),
   `citations` carry source-info-bearing inline vecs — copying the exec side would
   clobber original source info even when structurally equal, and would disagree
   with the incremental writer's verbatim source splice when not equal.
2. **Sibling audit in this same branch**, as a phase *after* the Cite fix lands
   (so the fix shape is proven first).
3. **Topic branch** off `main` in the main checkout (this branch).

## Real-world manifestation (mechanism)

Two production surfaces consume the reconciliation plan:

- **`q2 render` with engines** (`quarto-core/src/stage/stages/engine_execution.rs:676`):
  after jupyter/knitr execution, the executed AST is reconciled against the
  pre-execution AST and the *reconciled* AST flows downstream (`ast = reconciled_ast`)
  into crossref/citeproc/writers. If execution changes a citation in a paragraph
  that keeps at least one hash-identical sibling inline (the `has_kept_inlines`
  recursion gate), the rendered output shows the **stale** citation — output
  corruption, not just wrong source mapping.
- **Incremental qmd writing** (hub-client `incremental_write_qmd` in
  `wasm-quarto-hub-client/src/lib.rs`, and `pampa/src/apply_node_edit.rs`): an
  editor-side AST edit that changes a citation gets paired as
  `RecurseIntoContainer`; `assemble_recursed_container` then splices the citation
  delimiters verbatim from the *original* source, silently dropping the edit from
  the written qmd.

## Triage verdict

**Ready to design.** The "flaky" proptest failure is a real, deterministic `apply_reconciliation` bug — a recurrence of the bd-3zp3z4jx container-identity bug class for `Cite` — with a confirmed 2-inline minimal repro and a precedent-backed fix shape.

## Issue context

Filed 2026-05-26 by gordon (bug, P2, open): the proptest
`property_tests::reconciliation_preserves_structure_full_ast` in
`quarto-ast-reconcile` failed under `cargo xtask verify` with a persisted seed, but
did not reproduce with fresh seeds — hence "flaky". The strand asked to (1)
regenerate/inspect the seed, (2) decide real-bug vs `structural_eq_blocks`
false-negative, (3) commit the regression seed once resolved.

A 2026-08-19 comment (Carlos) captured a second, deterministic seed
(`cc c8f78493…`, deliberately not committed) noting the shrunk case involves a
`Cite` with nested prefix/suffix inlines.

## Dependency graph

**Empty** — no edges in either direction. No incoming pressure; the context lives
entirely in the description + comment. (The relevant prior art, bd-3zp3z4jx, is
referenced in a code comment rather than a braid edge.)

## What the code looks like today

Reproducible at HEAD (`4b4a63ce`): adding the comment's `cc` line to
`crates/quarto-ast-reconcile/proptest-regressions/lib.txt` fails the proptest
deterministically. Full diagnosis + minimal repro in
`claude-notes/plans/cite-reconciliation-proptest-investigation/` (notes.md,
minimal-repro-test.rs).

**Root cause (confirmed, not a structural_eq false-negative):**

- `compute_inline_alignments` Step 2 (type-based container matching,
  `crates/quarto-ast-reconcile/src/compute.rs` ~654–689) pairs two `Cite`s by
  discriminant alone — the bd-3zp3z4jx identity guard covers
  `Custom`/`Link`/`Image`/`Span` but falls through to `_ => true` for `Cite`.
- `apply_inline_container_reconciliation`'s `Cite` arm
  (`crates/quarto-ast-reconcile/src/apply.rs:346`) reconciles only `content` and
  keeps the original's `citations` wholesale.

Result: when before/after Cites differ in `citations` (id, mode, prefix, suffix)
and sit in a block that recurses (needs one hash-identical sibling inline), the
reconciled AST keeps the stale before-citations. The hash and `structural_eq`
already cover citation fields correctly; only Step 2's guard is missing. The
"flakiness" was just generator rarity — every saved seed fails deterministically.

## Work items

### Phase 0 — Test plan (TDD)

- [ ] Re-add the minimal unit test from the investigation dir
  (`minimal-repro-test.rs`) to `lib.rs`'s test module; verify it fails.
- [ ] Add both saved seeds (`cc 8f798bbf…` from the description, `cc c8f78493…`
  from the comment) to `proptest-regressions/lib.txt`; verify they fail.

### Phase 1 — Fix

- [ ] Add a `Cite` identity guard to compute Step 2, mirroring bd-3zp3z4jx:
  pair two Cites only when their `citations` are structurally equal (same
  length; per-citation `id`/`mode` equal, `prefix`/`suffix` via
  `structural_eq_inlines`); otherwise fall through to `UseAfter`. Factor the
  citation comparison shared with `structural_eq_inline`'s Cite arm.
- [ ] Unit test + both seeds green; crate tests green.

### Phase 2 — Verify + commit

- [ ] Full `cargo nextest run --workspace`.
- [ ] Commit fix + regression seeds (strand item 3).

### Phase 3 — Sibling audit (same branch, after the fix)

- [ ] For each container with non-child identity and no Step-2 guard
  (`Quoted.quote_type`; `Insert`/`Delete`/`Highlight`/`EditComment` `attr`):
  determine what `assemble_recursed_container` in
  `pampa/src/writers/incremental.rs` actually splices — does a recursed
  container with changed identity emit stale delimiters/attr in the written qmd?
- [ ] Write failing tests for any confirmed source-level bug; fix with the same
  Step-2 guard shape (or document why the container is safe).
- [ ] Record findings in this plan; file strands for anything deliberately
  scoped out.

### Phase 4 — Close out

- [ ] `cargo xtask verify` (full — pampa is in the WASM dependency chain).
- [ ] Close bd-205v6.

## Risks / tradeoffs (draft)

- Tightening Step 2 turns "recurse + keep stale citations" into `UseAfter`, which
  loses original source-info mapping for a Cite whose citations changed — same
  accepted tradeoff as bd-3zp3z4jx made for Link/Image/Span.
- Committing the regression seeds before the fix would hard-red the workspace;
  they must land in the same commit as (or after) the fix.
- `quarto-ast-reconcile` feeds pampa's incremental writer; any behavior change
  here should get a full-workspace test run (monorepo rule).
