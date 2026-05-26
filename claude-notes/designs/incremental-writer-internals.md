# Incremental writer internals — `CoarsenedEntry` and the self-contained contract

**Status:** Active (contract pinned 2026-05-25 by the
`CoarsenedEntry::Rewrite` refactor).
**Types:** `pampa::pandoc::Block`, `quarto_source_map::SourceInfo`,
`quarto_ast_reconcile::ReconciliationPlan`.
**Reference impl:**
[`crates/pampa/src/writers/incremental.rs`](../../crates/pampa/src/writers/incremental.rs)
(`CoarsenedEntry`, `coarsen`, `coarsen_blocks`, `coarsen_keep_before_block`,
`assemble`, `emit_entries`).
**Plans:**
[Plan 7](../plans/2026-05-04-q2-preview-plan-7-incremental-writer.md)
(writer design) ·
[Plan 7c](../plans/2026-05-25-q2-preview-plan-7c-closure-gaps.md)
(Phase 8 — Transparent recursion in `RecurseIntoContainer`) ·
[CoarsenedEntry self-contained refactor](../plans/2026-05-25-coarsened-entry-self-contained.md).
**Sibling docs:**
[Transparent wrappers](./transparent-wrappers.md) (the *traversal*
primitive — what the writer skips through) ·
[Provenance contract](./provenance-contract.md) §7 (how atomic-kind
decisions flow into the writer's branches).

## Purpose

The incremental writer answers a single question: *given an
`(original_qmd, original_ast, new_ast, plan)` tuple, what qmd text
should we hand back to the user?* Its output round-trips through the
read pipeline to produce an AST that the next reconciliation matches
against — so the writer's bytes are the canonical persistence form
of an edit.

It does this in two phases. **Coarsen** walks the reconciler's
hierarchical alignment plan and reduces it to a flat list of
`CoarsenedEntry` values — one per emitted block sequence. **Assemble**
walks that list, concatenates the bytes, and inserts separators. The
split lets the coarsen step be tested in isolation and lets a future
"minimal Monaco edit" consumer reuse the entry list without
re-running the diff.

This document pins the contract that holds the two phases together.

## The `CoarsenedEntry` contract

> Every variant of `CoarsenedEntry` must carry enough information
> to produce its emit bytes **without further context**. No
> index-into-an-ambient-slice deferral. No "look this up at emit
> time" handoffs. Each entry is self-describing.

The five variants today:

| Variant | Bytes come from | Self-contained because |
|---|---|---|
| `Verbatim` | `original_qmd[byte_range]` | `byte_range` is absolute. |
| `InlineSplice` | `block_text` field | Pre-computed at coarsen time. |
| `Rewrite` | `block_text` field | Pre-computed at coarsen time. |
| `Transparent` | concatenation of children | Children are themselves self-contained. |
| `Omit` | (nothing) | Emits nothing. |

The two indices that *do* appear on entries — `Verbatim::orig_idx`,
`InlineSplice::orig_idx` — are not used for *byte content*. They're
hints to `compute_separator` for its "consecutive-in-original gap"
optimization, and they're always `Option`: `None` for children
inside a `Transparent` wrapper, where any index would be ambiguous
(top-level? child-level?). The bytes themselves never look up against
an ambient slice.

`emit_entries` walks entries in order and concatenates. Its
`new_ast: &Pandoc` parameter is currently unused for byte production
in any variant — that's the post-condition of the contract. (We
leave the parameter in the signature for now; removing it is a
tidying follow-up flagged in the refactor plan.)

## Why this matters

The contract isn't decorative. Three reasons it exists:

### 1. `Transparent` recursion composes only if children are self-contained

A `Transparent` entry represents a synthesized wrapper whose own
bytes are empty (sectionize Div, footnotes container, appendix
container) but whose children carry real source preimage. The
writer "looks through" the wrapper by inlining the children into the
emit stream.

This composition requires that each child knows how to produce its
own bytes *without* depending on its position in some ambient slice.
A child carries `orig_idx: None` to opt out of the original-gap
optimization (its index is child-relative, not top-level). If the
same child also tried to defer its *bytes* to a "look up index N in
new_ast.blocks" handoff, the lookup would silently target the wrong
slice — `new_ast.blocks` is the top-level array, and child indices
don't index into it.

That is exactly the bug that motivated this contract. Before
2026-05-25 the `Rewrite` variant carried `new_idx: usize`, which
worked at the top level (every entry corresponded one-to-one with
a top-level block; indices were unambiguous) but broke the moment
`Rewrite` could be produced inside a `Transparent` recursion. The
panic shape: *"index out of bounds: the len is 1 but the index is N"*
— top-level slice has one entry (the wrapper), child index N is
out of bounds.

### 2. Minimal-edit diffing wants a self-contained intermediate form

Today `incremental_write` returns a single full-document edit. A
future "produce minimal Monaco edits" consumer (Plan 7's deferred
follow-up) wants to walk the coarsened plan and emit *per-entry*
deltas — `Verbatim` entries are no-ops if the original gap matches;
`InlineSplice` and `Rewrite` are localized text replacements at
known source ranges.

That walker needs every entry to expose its *intended text* (the
bytes that would land in the result) directly. If `Rewrite` deferred
to an emit-time lookup, the walker would have to re-thread `new_ast`
into a context it doesn't otherwise need. The self-contained shape
gives the walker exactly what it asks for — one record per emitted
block, fully self-describing.

### 3. Behaviour is the *same*, the *timing* changes

Pre-refactor, `write_block_to_string(&new_ast.blocks[new_idx])` ran
inside `emit_entries`. Post-refactor, the equivalent call runs at
the corresponding producer site in `coarsen_blocks` /
`coarsen_keep_before_block`. `write_block_to_string` is referentially
transparent — it depends only on its `Block` argument, has no global
state, no I/O, no clock reads. Moving the call earlier produces
byte-identical output and runs exactly once either way (Rewrite is
the catch-all path; we always emit it when produced).

That matters because the change is "free of behaviour" — it's a
shape change, not a semantics change. A reader reviewing the diff
shouldn't need to worry that some downstream test will break in a
subtle way.

## Anti-patterns

Don't add a `CoarsenedEntry` variant that:

- **Defers to a named slice.** "Index N into `new_ast.blocks`,"
  "child M of original block at index K," etc. The moment a future
  refactor calls the producer in a different *context* (recursion,
  reuse from a sibling crate, a test fixture), the index points at
  the wrong slice and the failure is silent until the panic.
- **Depends on context not encoded in the variant itself.** If you
  need "the prev sibling's bytes," "the wrapper's original
  position," or similar context to make sense of an entry, pre-fold
  the context into the entry's payload or restructure so it doesn't
  need the context.
- **Requires specific timing of side effects.** `write_block_to_string`
  is pure — calling it at coarsen vs emit time is observably
  identical. If your variant only works when its bytes are computed
  at one specific moment, that's a sign the entry shape is wrong.

When in doubt, look at `InlineSplice`. It was the first variant to
carry pre-computed `block_text` (introduced when partial inline
rewrites made deferral impossible — the splice text doesn't
reconstruct from any single block) and is the structural blueprint
the rest of the variants should match.

## History

`CoarsenedEntry` started life with two variants in commit
`eb81cbc5` (the original incremental-writer landing): `Verbatim`
carrying a `byte_range`, and `Rewrite` carrying a `new_idx: usize`.
The writer was top-level only — each entry corresponded one-to-one
with a top-level block, indices were unambiguous, and deferring
`write_block_to_string` to emit time saved a call when the entry
was never emitted (defensive, but the entry was always emitted in
practice).

The asymmetry was introduced silently in `ab10f37b`, which added
`InlineSplice { block_text, orig_idx }` to support partial block
rewrites. Splice text mixes original bytes with newly-serialized
inlines and doesn't reconstruct from any single `Block` — so the
text was necessarily pre-computed at coarsen time. No one
refactored `Rewrite` to match; the two patterns coexisted.

`9a473fe9` (Plan 7 phase 2+3a) added `Transparent` and `Omit`.
`Verbatim::orig_idx` and `InlineSplice::orig_idx` became `Option`
so children inside `Transparent` could opt out of the original-gap
optimization. The commit **explicitly flagged** the latent `Rewrite`
issue with a comment: *"result_idx is unused for child Rewrites
(a child Rewrite would need a different lookup mechanism; not
exercised by today's synthesizers)."* Accurate at the time — no
producer of child entries was emitting `Rewrite`.

`bdcfdc53` (Plan 7c phase 8) added a Transparent-recursion path in
`coarsen_blocks` for the *changed-wrapper* case
(`RecurseIntoContainer` with a `block_container_plans` entry). For
the first time, `coarsen_blocks` ran on child slices, and a
`Rewrite` produced there carried a child-relative index. The "not
exercised" caveat from `9a473fe9` no longer held — the panic the
contract addresses became reachable.

The 2026-05-25 refactor that motivated this doc lifted `Rewrite`
to `{ block_text: String }`, matching `InlineSplice`. All four
producer sites now pre-compute. The implementation cost is a moved
`write_block_to_string` call; the gain is the contract this doc
pins.

The same session also closed a latent soft-drop gap that the panic
had been masking. The `BlockAlignment::UseAfter` arm now detects
*atomic-Generated with preimage* (the user edited inside a
shortcode-resolved block, the reconciler split the edit into a
deleted-original + new-block, but the new block still carries the
token's `Invocation` anchor) and emits `Verbatim` of the preimage
plus a `Q-3-43` warning, instead of the previous let-user-win
`Rewrite` (which would have written the resolved bytes — the edit
applied to *generated* content — back into the source qmd, poisoning
the user's source). The pattern: when an entry's *new* block looks
like an attempt to edit content the user can't actually edit, refuse
the edit at the writer regardless of what the reconciler's alignment
said.

## Promotion path

`CoarsenedEntry` is private to
`crates/pampa/src/writers/incremental.rs` today, with two internal
consumers: `assemble`'s `emit_entries` and the
`compute_edits_from_coarsened` helper (which currently calls
`assemble` internally).

Promote the type (and its emission helpers) to a shared module the
moment a second crate wants to consume the coarsened plan. The
expected first non-pampa consumer is the minimal-edit-diffing
walker described above. Until then, the type stays here — premature
generalisation has its own debt, and the contract above is what
matters, not the import path.

## Adding a new variant

If you find yourself wanting a new `CoarsenedEntry` variant:

1. Ask whether one of the existing five already serves. Most "I need
   a new shape" instincts collapse into `Transparent` (for
   wrappers) or `Rewrite` (for "anything else, re-serialize").
2. If you genuinely need a new variant, design it self-contained
   from the start. The variant's payload should be everything
   `emit_entries` needs to produce its bytes; nothing more, nothing
   deferred.
3. Update this doc's table and the variant's doc comment in the
   `CoarsenedEntry` enum to describe the self-containment story.
4. Add at least one test that exercises the variant inside a
   `Transparent` recursion. That's the canary that catches
   composition bugs early.
