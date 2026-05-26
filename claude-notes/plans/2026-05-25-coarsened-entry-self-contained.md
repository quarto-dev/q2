# Plan — Make `CoarsenedEntry::Rewrite` self-contained

**Status:** Drafted 2026-05-25.
**Branch:** `feature/provenance`.
**Trigger:** Panic discovered 2026-05-25 during the q2-preview gate-bypass
UX experiment (see §History below). Index-out-of-bounds in
`emit_entries` when a `Rewrite` entry produced inside the Transparent
recursion (added in commit `bdcfdc53`) carried a child-relative
`new_idx` but was looked up against the top-level `new_ast.blocks`.

## Goal

Lift the existing implicit invariant — *every `CoarsenedEntry` variant
must be self-contained (carry its own emit-time bytes)* — to be an
explicit, enforced architectural rule. Today four of the five variants
already satisfy this:

| Variant | Self-contained? | How |
|---|---|---|
| `Verbatim` | ✓ | `byte_range` into `original_qmd` |
| `InlineSplice` | ✓ | pre-computed `block_text: String` |
| `Transparent` | ✓ | list of self-contained child entries |
| `Omit` | ✓ | emits nothing |
| `Rewrite` | ✗ | `new_idx: usize` — a deferred index into `new_ast.blocks` |

`Rewrite` is the outlier. Make it match its siblings by carrying
pre-computed `block_text: String`. The qmd writer call moves from
emit time (`assemble`) to coarsen time (`coarsen`); the work is the
same, the timing changes; the entry becomes self-describing.

Behaviour does not change. Tests stay green. The Transparent-recursion
panic disappears.

## History — why was `Rewrite` written context-dependently?

`git log -S CoarsenedEntry -- crates/pampa/src/writers/incremental.rs`
(top of file, latest 4 entries):

1. **`eb81cbc5`** ("Add incremental QMD writer with idempotence and
   round-trip tests") — original commit. `CoarsenedEntry` had **two**
   variants:
   ```rust
   enum CoarsenedEntry {
       Verbatim { byte_range: Range<usize>, orig_idx: usize },
       Rewrite { new_idx: usize },
   }
   ```
   The writer was top-level only. Every entry corresponded directly
   to one top-level block. `Verbatim` carried its own bytes; `Rewrite`
   deferred to `assemble`-time via an index into `new_ast.blocks` —
   correct because indices were unambiguous and the deferral saved
   a `write_block_to_string` call when the entry was never emitted
   (defensive). Behaviour invariant: `new_idx` is always a *top-level*
   index. Honoured by construction at this point.

2. **`ab10f37b`** ("Implement inline splicing for incremental writer
   (bd-1hwd)") — added `InlineSplice` variant for partial block
   rewrites:
   ```rust
   InlineSplice { block_text: String, orig_idx: usize }
   ```
   Inline splicing builds *bespoke* block text by mixing original
   bytes with newly-serialized inlines. There's no `new_idx` that
   would reconstruct it — the text is necessarily pre-computed at
   coarsen time. This was the first variant to break the "defer to
   emit time" pattern, **out of necessity**, but no one refactored
   `Rewrite` to match. The asymmetry was introduced silently.

3. **`9a473fe9`** ("plan-7 phase 2+3a: writer internals — soft-drop,
   Transparent/Omit, multi-inline dedupe") — Plan 7 added the
   `Transparent` and `Omit` variants. `Transparent { child_entries }`
   allows recursive emission for non-atomic Generated wrappers
   (sectionize, footnotes, appendix). `orig_idx` became `Option`
   so children inside `Transparent` could opt out of the
   `compute_separator` original-gap optimization. The commit
   **explicitly flagged** the latent Rewrite issue:

   > // result_idx is unused for child Rewrites (a child Rewrite
   > // would need a different lookup mechanism; not exercised by
   > // today's synthesizers).

   Accurate at the time — coarsen_keep_before_block was the only
   producer of child entries (under static Transparent recursion
   for unchanged wrappers), and its catch-all hit Rewrite only on
   cross-file Original / gappy Concat / Generated-without-source-bearing-children
   shapes that the pipeline didn't produce in practice.

4. **`bdcfdc53`** ("recurse into non-atomic Generated wrappers in
   RecurseIntoContainer") — *this PR's* fix from earlier today.
   Added a Transparent-recursion path in `coarsen_blocks` for the
   *changed-wrapper* case (RecurseIntoContainer with a
   `block_container_plans` entry). For the first time, **`coarsen_blocks`
   runs on child slices**, and any `Rewrite` it produces carries a
   child-relative index. The "not exercised by today's synthesizers"
   caveat from `9a473fe9` no longer holds.

The takeaway: `Rewrite`'s context-dependent design was a vestige of
the original Phase-1 top-level-only writer. It survived because every
expansion since (`InlineSplice`, then `Transparent`) sidestepped it
rather than refactoring. Today's panic is the bill coming due.

## Behavioural equivalence — coarsen-time vs emit-time

**Question:** does pre-computing `block_text` at coarsen time produce
byte-identical output to deferred emit-time computation?

**Answer:** yes. `write_block_to_string`
(`crates/pampa/src/writers/incremental.rs:1089`) is a pure function of
its `Block` argument:

```rust
fn write_block_to_string(block: &Block) -> Result<String, …> {
    let mut buf = Vec::new();
    qmd::write_single_block(block, &mut buf)?;
    String::from_utf8(buf).map_err(…)
}
```

`qmd::write_single_block` (`writers/qmd.rs:2392`) constructs a fresh
`QmdWriterContext::new()` per call. The context's mutable fields
(`emphasis_stack`, `prev_emitted_alnum`) accumulate state only
**within** a single `write_single_block` invocation — they're created,
used, and dropped per call. No state leaks across calls.

There is no global state in `crates/pampa/src/writers/qmd.rs` (verified
by `git grep 'static\|thread_local' crates/pampa/src/writers/qmd.rs`
returning empty). No file I/O, no environment reads, no system clock.
The function depends only on the input `Block`.

Therefore: `write_block_to_string(b)` is referentially transparent.
Calling it at coarsen time vs emit time produces identical output.

Performance: `Rewrite` is the catch-all path — when we get an entry we
*always* emit. No coarsened plan keeps Rewrite entries it doesn't use
(emit_entries walks every non-Omit entry). The qmd-write work is
performed exactly once either way; only its timing changes. No extra
allocations.

## Consumers — confirming the scope

`CoarsenedEntry` is private to `crates/pampa/src/writers/incremental.rs`
(lowercase `enum`, no `pub`). Two internal consumers:

1. `assemble`'s `emit_entries` — concatenates bytes per entry.
2. `compute_edits_from_coarsened` — currently calls `assemble`
   internally and returns a single full-document edit.

No external consumers. The refactor is fully local to one file.

Future consumers (Phase 3 minimal-edit diffing, Plan-X-WIP) will benefit
from the self-contained invariant: every entry carries its own *intended
text* and (where applicable) its *intended source range*, which is the
right shape to derive minimal Monaco edits without re-deriving a
post-assemble diff. Mentioned in §Out-of-scope but worth noting as
direction-of-travel.

## Work items

### Phase 1 — Pin the panic with a failing test

- [x] Add `sectionize_wrapper_with_shortcode_child_edit_does_not_panic`
      to `crates/pampa/tests/incremental_writer_tests.rs`. The current
      draft (commit `5f2bbab0`'s working tree) reaches the panic via
      a cross-file Original child shape; alternative is a synthesized
      empty section Div or a Lua-filter-emitted Generated wrapper
      with no source-bearing children. Either reproduces the
      `Rewrite { new_idx: child_idx }` → `new_ast.blocks[child_idx]`
      out-of-bounds.
- [x] Run; confirm the test panics with "index out of bounds" on
      `incremental.rs:890` (the `Rewrite` arm of `emit_entries`).
- [x] Added `sectionize_wrapper_shortcode_child_edit_soft_drops` —
      goes further than no-panic by asserting on output bytes +
      Q-3-43 warning. This caught a *second* bug Phase 1's no-panic
      test would have hidden: the `UseAfter` arm fell through to
      let-user-win Rewrite for atomic-Generated with preimage,
      writing the resolved bytes (the edit applied to generated
      content) back into the source qmd. The architectural Rewrite
      fix made this newly visible by replacing the panic with silent
      wrong-bytes; see Phase 2 below for the additional soft-drop
      branch that closes the gap.

### Phase 2 — Lift `Rewrite` to self-contained

- [x] Change the variant to carry pre-computed `block_text: String`.
      Drop the `new_idx: usize` field.
- [x] Update every `Rewrite` producer to pre-compute (four sites:
      `coarsen_blocks` UseAfter, two RecurseIntoContainer sub-branches,
      and `coarsen_keep_before_block`'s catch-all).
- [x] Convert `coarsen_keep_before_block` to
      `→ Result<CoarsenedEntry, Vec<DiagnosticMessage>>`. Both call
      sites updated to `?`.
- [x] Update `emit_entries` to `block_text.clone()`. `new_ast` is now
      unused for byte production in any variant (kept in signature for
      now; removal is a tidying follow-up).
- [x] Delete the "result_idx is unused for child Rewrites" comment.

#### Phase 2b — Soft-drop for atomic-Generated in UseAfter (scope expansion)

Discovered during Phase 3 verification: the user reported that with
the dispatch.tsx bypass in place, clicking +react on a paragraph
inside `{{< lipsum 3 >}}` produced wrong qmd output — the resolved
lipsum bytes + reactji were being written back into source. The
architectural Rewrite refactor made this newly observable by
replacing the panic with silent wrong-bytes.

Root cause: when the user edits inside an atomic-Generated block
with realistic content delta, the reconciler can emit
`KeepBefore` (Header) + `UseAfter` (new lipsum) at the
sectionize-child level — implicit deletion of the original lipsum
Para. The `UseAfter` arm filtered atomic-CustomNode and
no-preimage-Generated but had no branch for atomic-Generated-with-
preimage, so it fell through to let-user-win Rewrite (write the
new bytes).

- [x] Add an `atomic_generated_preimage` check at the head of the
      `UseAfter` arm in `coarsen_blocks`. If the new block is
      `Generated` with `is_atomic_kind() == true` AND has preimage
      in target → emit `Verbatim` of the preimage range + a
      Q-3-43 soft-drop warning. The pattern: when an entry's *new*
      block looks like an attempt to edit content the user can't
      actually edit, refuse the edit at the writer regardless of
      what the reconciler's alignment said.
- [x] Test: `sectionize_wrapper_shortcode_child_edit_soft_drops` —
      asserts on output bytes (token preserved, reactji NOT
      emitted) and the Q-3-43 warning.

### Phase 3 — Tests + verification

- [x] Re-run the Phase 1 test; passes (Ok, no panic).
- [x] `cargo nextest run -p pampa` — 3902 / 3902 passing
      (one new soft-drop test added).
- [x] `cargo xtask verify --skip-hub-build --skip-hub-tests` — Rust
      workspace 9655 / 9655 passing. (The
      `ts-packages/preview-renderer` integration tests fail under the
      bypass; expected — they assert the atomic-aware NOOP gate
      fires, which the bypass disables. They pass once the bypass
      is reverted.)
- [x] Rebuild WASM (`hub-client && npm run build:wasm`) — exit 0.
- [ ] Playwright e2e `q2-preview-render-components-write` — *blocked
      by a dev server holding port 5173 in this worktree; deferred,
      see "scaffolding cleanup" task*.
- [x] Manual: user confirmed the no-panic + soft-drop behavior in
      their local browser session after rebuilding. Initial report
      flagged wrong-bytes (resolved lipsum text in qmd), which led
      to discovering Phase 2b. After Phase 2b lands, the
      regression test `sectionize_wrapper_shortcode_child_edit_soft_drops`
      asserts: token bytes preserved, reactji NOT emitted, Q-3-43
      warning fires.
- [ ] Restore the dispatch.tsx gate before this plan's commits ship
      (it was a one-shot UX experiment; the proper TS-side intercept
      signal is separate work — see §Out of scope).

### Phase 4 — Design doc

- [x] Write `claude-notes/designs/incremental-writer-internals.md`
      (new file). Sections:
      - *Purpose*. The incremental writer takes `(original_qmd,
        original_ast, new_ast, plan)` and produces `(new_qmd,
        warnings)`. It does so by *coarsening* the hierarchical
        reconciliation plan into a flat list of self-contained
        emit instructions, then *assembling* the result by walking
        the instructions in order.
      - *The `CoarsenedEntry` contract* — the rule this plan
        enforces. Every variant carries enough information to
        produce its emit bytes *without further context*. No
        index-into-an-ambient-slice deferral. Each variant
        documented with its payload and self-containment property.
      - *Why this matters* — the panic story, the Transparent
        recursion composition story, the minimal-edit-diffing
        future story.
      - *Anti-patterns* — "don't add a variant that defers to a
        named slice"; "don't add a variant that depends on context
        not encoded in the variant itself"; "if you need timing of
        side effects, that's a sign the entry shape is wrong."
      - *History* — pointer to this plan; pointer to the historical
        commits (`eb81cbc5`, `ab10f37b`, `9a473fe9`, `bdcfdc53`).
      - *Promotion path* — same shape as
        `transparent-wrappers.md`'s "where the code lives + when
        to promote it" — `CoarsenedEntry` is private today; if a
        second crate ever wants to consume the coarsened plan
        (e.g. minimal-edit-diffing in a separate crate), promote
        the type and its emission helpers to `quarto-pandoc-types`
        or a new module.
- [x] Cross-link from `transparent-wrappers.md` §"Reference
      primitive" — added a "Sibling primitive on the emission side"
      preamble that points to the new doc.
- [x] Cross-link from `provenance-contract.md` §7 "Atomic-kind set
      and consumer impact" — added a closing paragraph pointing to
      the new doc as the place where the writer's internal shape is
      pinned.

### Phase 5 — Plan annotations

Plans whose work would build on the self-contained invariant:

- [x] `claude-notes/plans/2026-05-04-q2-preview-plan-7-incremental-writer.md`
      — added a "Follow-ups closed" section pointing here.
- [x] `claude-notes/plans/2026-05-24-q2-preview-plan-7b-test-orama.md`
      — its Phase 1 writer-lossless fixtures should include at least
      one shape where the writer's catch-all Rewrite path fires
      (cross-file Original child, or empty Generated wrapper). Already
      flagged from the sectionize-wrapper audit; this plan supplies
      the structural reason such fixtures matter.

## Out of scope

- The TS-side gate's silent NOOP (lipsum-paragraph clicks produce no
  user feedback today). Separate plan; the temporary
  `dispatch.tsx` bypass exists only to surface the writer-side
  diagnostic UX once and must be reverted as part of Phase 3.
- The proper TS-side "edit rejected at the gate" signal — needs
  its own design (synthetic diagnostic shape, framework emit
  callback, location resolution via the source pool). Tracked
  separately.
- Removing `new_ast: &Pandoc` from the `emit_entries` signature.
  Once Rewrite no longer reads it, the parameter might be fully
  removable (audit the other arms). Defer to a tidying commit
  unrelated to this plan's correctness work.
- Eventual minimal-edit diffing from `CoarsenedEntry` directly
  (rather than `assemble` + post-diff). The self-contained
  invariant is a precondition; the actual diff-emitting work is
  its own plan.

## Risk assessment

**Low risk overall.** Three reasons:

1. **No behaviour change.** `write_block_to_string` is referentially
   transparent (§Behavioural equivalence). The refactor moves a
   pure-function call earlier in the pipeline; emit bytes are
   byte-identical.
2. **Fully local.** `CoarsenedEntry` is private to one file; two
   internal consumers; no FFI; no wire format.
3. **Mirrors an existing precedent.** `InlineSplice` already carries
   pre-computed `block_text`. The new `Rewrite` is structurally
   identical.

Risks worth naming:

- **Tests pass but production hits a path we missed.** Mitigation:
  the Plan-7b §"writer-lossless baseline" call-out for adding a
  catch-all Rewrite fixture; verify with the e2e + manual browser
  repro before committing.
- **Coarsen-time errors surface differently.** Before: `write_block_to_string`
  errored at emit time, propagated up through `assemble` to
  `incremental_write`. After: errors propagate from `coarsen_blocks`
  (via the `?` in producer sites) — same overall return path
  (`Result<_, Vec<DiagnosticMessage>>`), but the *order* of error
  vs. soft-drop-warning emission could shift. Verify the existing
  error tests still produce the same diagnostic ordering.
- **Increased coarsen-time allocations.** Each Rewrite producer now
  allocates a `String` immediately. Negligible at typical document
  sizes; flagged for awareness rather than as a real concern.

## Estimated scope

| Phase | Lines (rough) |
|-------|---------------|
| 1 — pin panic with failing test | ~80 |
| 2 — Rewrite self-contained refactor | ~60 net change (delete + add) |
| 3 — verification (test runs, e2e) | 0 LOC (verification only) |
| 4 — design doc | ~200 |
| 5 — plan annotations | ~30 |
| **Total** | **~370** |

## References

- This plan's panic: `2026-05-25` session transcript; stack trace shows
  `Rewrite { new_idx: 8 }` against `new_ast.blocks.len() == 1`.
- Plan 7's original `CoarsenedEntry` design:
  `claude-notes/plans/2026-05-04-q2-preview-plan-7-incremental-writer.md`
  §"Coarsen step".
- Plan 7c's transparent-wrapper fix:
  `claude-notes/plans/2026-05-25-q2-preview-plan-7c-closure-gaps.md`
  §Phase 8.
- The "not exercised by today's synthesizers" landmine comment:
  `crates/pampa/src/writers/incremental.rs` around line ~640 (after
  the `coarsen_keep_before_block` Transparent recursion).
- Existing precedent for pre-computed text:
  `CoarsenedEntry::InlineSplice` (introduced in commit `ab10f37b`).
