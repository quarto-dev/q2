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

- [x] Re-add the minimal unit test from the investigation dir
  (`minimal-repro-test.rs`) to `lib.rs`'s test module; verify it fails.
  (`test_reconcile_cite_citations_changed_not_paired`, plus companion
  `test_reconcile_cite_same_citations_recurses` which passes by design.)
- [x] Add both saved seeds (`cc 8f798bbf…` from the description, `cc c8f78493…`
  from the comment) to `proptest-regressions/lib.txt`; verify they fail.

### Phase 1 — Fix

- [x] Add a `Cite` identity guard to compute Step 2, mirroring bd-3zp3z4jx:
  new `structural_eq_citations` helper in `hash.rs`, shared with
  `structural_eq_inline`'s Cite arm; guard falls through to `UseAfter`.
- [x] Unit test + both seeds green; crate tests green (228/228).

### Phase 2 — Verify + commit

- [x] Full `cargo nextest run --workspace` — 12909/12909 passed.
- [x] Committed as `35f5d95a` (fix + both regression seeds + unit tests).

### Phase 3 — Sibling audit (same branch, after the fix)

- [x] Audit findings (see table below):
  - **Quoted** — real, user-reachable source-level bug, confirmed end-to-end:
    `anchor "hello"` edited to `anchor 'hello'` came back **byte-identical to
    the original** from `incremental_write` (edit silently dropped). The quote
    chars are the container delimiters, spliced verbatim from original source.
  - **Insert/Delete/Highlight/EditComment** — same structural hole (attr is
    serialized as `]{attr}` inside the closing delimiter), but *not reachable
    from parsed qmd*: `postprocess.rs` unconditionally desugars all four to
    `Span` (class `quarto-insert` etc.), and Span is already guarded. Guarded
    anyway as defense-in-depth — the reconcile crate should not depend on a
    pampa desugaring invariant.
  - **Block-level containers (Div/BlockQuote/Header attr)** — safe by
    construction: `coarsen()` in the incremental writer always Rewrites
    recursed block containers from the new AST, and inline-content blocks
    require `block_attrs_eq` before splicing. No strand needed.
- [x] Failing tests first: `quoted_quote_type_change_survives_incremental_write`
  (pampa integration, end-to-end through the JSON round-trip path) and
  `test_reconcile_identity_changed_containers_not_paired` (plan-shape test
  covering all five containers). Both confirmed failing before the guard.
- [x] Fix: Step-2 guards for `Quoted.quote_type` and the four marks' `attr`.
- [x] Findings recorded here; nothing scoped out, no follow-up strands needed.

### Audit table (final state)

| Container | Step-2 identity guard | Notes |
|---|---|---|
| Link / Image | target+attr (bd-3zp3z4jx) | |
| Span | attr (bd-3zp3z4jx) | |
| Custom | type_name (bd-3zp3z4jx) | |
| Cite | citations, structural (bd-205v6) | the original bug |
| Quoted | quote_type (bd-205v6 audit) | confirmed e2e incremental-write bug |
| Insert/Delete/Highlight/EditComment | attr (bd-205v6 audit) | defense-in-depth; desugared to Span at parse |
| Emph/Strong/Underline/Strikeout/Super/Subscript | none needed | no non-child identity |
| Note | none needed | block plan; no identity |

### Phase 4 — Close out

- [x] `cargo xtask verify` (full — pampa is in the WASM dependency chain).
  All steps passed 2026-08-19.
- [ ] Close bd-205v6 (ask user first).

## Risks / tradeoffs (draft)

- Tightening Step 2 turns "recurse + keep stale citations" into `UseAfter`, which
  loses original source-info mapping for a Cite whose citations changed — same
  accepted tradeoff as bd-3zp3z4jx made for Link/Image/Span.
- Committing the regression seeds before the fix would hard-red the workspace;
  they must land in the same commit as (or after) the fix.
- `quarto-ast-reconcile` feeds pampa's incremental writer; any behavior change
  here should get a full-workspace test run (monorepo rule).
