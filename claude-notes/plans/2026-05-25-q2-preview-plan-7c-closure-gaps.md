# Plan 7c — Plan 7 closure gaps (Q-3-41, TS editability gate, per-kind tests)

**Date:** 2026-05-25
**Branch:** `feature/provenance` (or fresh worktree branched from it).
The contract docs the plan references — `provenance-contract.md` and
`incremental-writer-contract.md` — currently live on
`review/provenance-plan-7` and merge into `feature/provenance` as
part of the review-pass merge that is the same prerequisite for this
plan.
**Status:** Implementation plan
**Milestone:** none directly — closes correctness/coverage gaps in
the writer surface Plan 7 already shipped.

## Epic context

Part of the **provenance epic** (Plans 3–8 + 7a + 7b + this). When
the Plan-7 implementation agent ran on 2026-05-24, the post-review
Plan-7 doc had not yet been merged into `feature/provenance`. Three
correctness/coverage gaps survived as a result. Plan 7c closes them:

1. **Q-3-41 "Edit dropped — render not ready yet"** — the
   first-edit-before-render diagnostic the review pass introduced.
   Neither the catalog entry nor the React/SPA emission landed.
2. **TS-side `hasPreimageIn` + `isEditableInside`** — the predicate
   pair that closes Plan 2A's React framework gate. The Rust side
   has the canonical version (`pampa::writers::incremental::is_editable_inside_*`);
   the TS side at `ts-packages/preview-renderer/src/utils/sourceInfo.ts`
   only exports the atomicity half.
3. **`cfg(debug_assertions)` `#[should_panic]` test** for the
   shortcode-Generated-with-empty-`from` debug-assert at
   `crates/pampa/src/writers/incremental.rs:448`.
4. **Per-kind soft-drop test symmetry** — explicit tests for each
   atomic kind (filter / title-block / tree-sitter-postprocess) on
   the Omit and inline UseAfter paths; the multi-inline dedupe
   filter case.

Plan 7b (`claude-notes/plans/2026-05-24-q2-preview-plan-7b-test-orama.md`)
already covers two adjacent test gaps — the writer-lossless baseline
test and the filter-construction-UseAfter test. Plan 7c is the
*disjoint* gap; do not duplicate Plan 7b's items here.

## Hand-off start point

1. Worktree: `feature/provenance` at
   `/Users/gordon/src/q2/.worktrees/provenance/` (the integration
   branch). `cargo xtask verify` is green there at the current tip;
   confirm before starting.
2. The review-pass commits that introduced the missing design — `00222099`,
   `bfb40962`, `561eefa0`, plus the cross-link commit `7c03be64` —
   live on `review/provenance-plan-7`. Either merge that branch
   into `feature/provenance` before starting (preferred — gives the
   contract docs to consult) or work from the audit summary
   below.
3. The audit that produced this plan: see the conversation transcript
   on 2026-05-25 (Claude session resolving the rebase of
   `review/provenance-plan-7` onto `feature/provenance`).
4. **Phase order matters.** Do Phase 1 (catalog) first so Phases 2
   and 3 can reference `Q-3-41`. Phase 4 (TS gate) is independent
   of Phase 1 in code but conceptually pairs with Phase 3 (Q-3-41
   is the visible signal for the gate's "no baseline yet" branch).
5. Don't push without explicit user permission.

## Goal

Bring Plan 7's user-visible surface back into alignment with the
post-review contract:

- The user always sees *some* signal when an edit is dropped —
  Q-3-42 for atomic-content edits, Q-3-43 for no-preimage edits,
  Q-3-41 for first-edit-before-render. No silent drops.
- The React framework's read-only gate matches the writer's
  editability predicate, so edits that the writer would soft-drop
  are gated at the DOM rather than reverting after a round-trip.
- The writer's debug-assert + each atomic kind's soft-drop path
  has explicit regression coverage.

Behaviour outside these four items is unchanged. No new design
surface; no new diagnostic semantics; no new pipeline tier.

## Scope

### In scope

#### Phase 1 — `Q-3-41` catalog entry (`quarto-error-reporting`)

**Repo facts the implementer needs:**

- Catalog file: `crates/quarto-error-reporting/error_catalog.json`.
  Q-3-42 / Q-3-43 entries at lines 527–541 are the shape to mirror
  (`subsystem`, `title`, `message_template`, `docs_url`,
  `since_version`).
- Subsystem for writer-side codes is `"writer"`. `since_version`
  is `"99.9.9"` for unreleased entries.
- Q-3-41 is unallocated today (Q-3-40 is taken; Q-3-42/Q-3-43 are
  the Plan-7 codes). Slot Q-3-41 between them.
- Q-3-41 is **TS-emitted** — there is no Rust caller (the writer
  isn't invoked when the baseline is missing). No diagnostic
  builder needed on the Rust side. The catalog entry exists so the
  docs URL and version metadata are consistent.

- [ ] Add Q-3-41 entry to `error_catalog.json` between Q-3-40 and
      Q-3-42. Title: `"Edit dropped — render not ready yet"`.
      `message_template`: `"Your edit was dropped because the
      document hasn't finished rendering. Try again in a moment."`
      `docs_url`: `"https://quarto.org/docs/errors/Q-3-42"`-style
      shape; `since_version`: `"99.9.9"`.
- [ ] Build: `cargo xtask verify --skip-hub-build --skip-hub-tests`
      green (the catalog has a unit test that asserts every entry
      parses).

#### Phase 2 — TS-side `hasPreimageIn` + `isEditableInside`

**Repo facts the implementer needs:**

- Target file: `ts-packages/preview-renderer/src/utils/sourceInfo.ts`
  (59 lines today; will roughly double).
- Wire-format types: `ts-packages/preview-renderer/src/types/sourceInfo.ts`
  documents codes 0/1/2/3/4. Walk pattern: `entryFor(node, pool)`
  for the entry; `entry.t` discriminates.
- Rust reference: `crates/pampa/src/writers/incremental.rs:113-162`
  (`is_editable_inside_block` / `_inline` / `_source_info`) +
  `crates/quarto-source-map/src/source_info.rs:406-442`
  (`preimage_in`).
- Anchor roles on the wire: `Generated` entries (code 4) carry
  `from?: AnchorRef[]` where `role: "invocation" | "value-source"
  | "other:<…>"`. Walk only `role === "invocation"`.
- `targetFileId` derivation, Rust-side: `original_ast.blocks.first()
  .and_then(|b| b.source_info().root_file_id()).unwrap_or(FileId(0))`
  (`incremental.rs:289-293`). On the TS side, look up the first
  block's `s`-index in the pool, walk to its root Original, take
  its `d` (file id). Default to `0` if absent.
- React context to extend: `ts-packages/preview-renderer/src/framework/RegistryContext.tsx`.
  Add an optional `targetFileId?: number`. Default `0` when absent
  (mirrors the Rust default, and covers callers that don't pass
  the field yet).
- React dispatcher gate to update:
  `ts-packages/preview-renderer/src/framework/dispatch.tsx:404-411`.
  Replace the `isAtomic` check with `!isEditableInside(...)`.
- The Ast provider that builds the context value:
  `ts-packages/preview-renderer/src/framework/Ast.tsx:121`.
  Compute `targetFileId` once and pass it alongside `sourceInfoPool`.

**Implementation sketch:**

```ts
// In ts-packages/preview-renderer/src/utils/sourceInfo.ts

/** Walk an entry's preimage chain in the pool; return [start, end]
 *  if the chain resolves to bytes in `targetFileId`, else undefined.
 *  Mirrors Rust `SourceInfo::preimage_in`. */
export function hasPreimageIn(
    node: { s?: number },
    pool: SourceInfoPool | undefined,
    targetFileId: number,
): [number, number] | undefined {
    const entry = entryFor(node, pool);
    if (!entry) return undefined;
    return preimageInEntry(entry, pool, targetFileId);
}

function preimageInEntry(
    entry: SourceInfoEntry,
    pool: SourceInfoPool | undefined,
    targetFileId: number,
): [number, number] | undefined {
    if (entry.t === 0) {
        return entry.d === targetFileId ? entry.r : undefined;
    }
    if (entry.t === 1) {
        const parent = pool?.[entry.d];
        if (!parent) return undefined;
        const parentRange = preimageInEntry(parent, pool, targetFileId);
        if (!parentRange) return undefined;
        return [parentRange[0] + entry.r[0], parentRange[0] + entry.r[1]];
    }
    if (entry.t === 2) {
        // Concat: every piece must resolve in target AND be byte-contiguous.
        const ranges: Array<[number, number]> = [];
        for (const [si_id, _offset, _len] of entry.d) {
            const piece = pool?.[si_id];
            if (!piece) return undefined;
            const r = preimageInEntry(piece, pool, targetFileId);
            if (!r) return undefined;
            ranges.push(r);
        }
        if (ranges.length === 0) return undefined;
        for (let i = 1; i < ranges.length; i++) {
            if (ranges[i - 1][1] !== ranges[i][0]) return undefined;
        }
        return [ranges[0][0], ranges[ranges.length - 1][1]];
    }
    if (entry.t === 4) {
        // Generated: walk the Invocation anchor only.
        const inv = entry.d.from?.find((a) => a.role === 'invocation');
        if (!inv) return undefined;
        const anchored = pool?.[inv.si_id];
        if (!anchored) return undefined;
        return preimageInEntry(anchored, pool, targetFileId);
    }
    // t === 3 (legacy) and any future codes — not consulted.
    return undefined;
}

/** Combined editability gate. Mirrors Rust
 *  `pampa::writers::incremental::is_editable_inside_*`. */
export function isEditableInside(
    node: { s?: number; t?: string; type_name?: string },
    pool: SourceInfoPool | undefined,
    targetFileId: number,
    atomicKinds: ReadonlySet<string>,
): boolean {
    // Atomic CustomNodes — never editable inside.
    const isCustom = node.t === 'CustomBlock' || node.t === 'CustomInline';
    if (isCustom && isAtomicCustomNode(node.type_name ?? '')) return false;
    // Atomic-kind Generated — never editable inside.
    if (isAtomicSourceInfo(node, pool, atomicKinds)) return false;
    // No preimage in target — never editable inside.
    return hasPreimageIn(node, pool, targetFileId) !== undefined;
}
```

- [ ] Implement `hasPreimageIn` per the sketch above. Export from
      `sourceInfo.ts`.
- [ ] Implement `isEditableInside`. Place the
      `isAtomicCustomNode` import alongside the existing
      `entryFor` / `isAtomicSourceInfo` imports
      (`ts-packages/preview-renderer/src/utils/atomicCustomNodes.ts`).
- [ ] Add unit tests for `hasPreimageIn` mirroring the Rust ones
      at `crates/quarto-source-map/src/source_info.rs:1614-1750`:
      Original same / different file; Substring composes offsets;
      Concat contiguous / gappy / empty; Generated with Invocation
      / with ValueSource only / no anchors. New test file:
      `ts-packages/preview-renderer/src/utils/sourceInfo.test.ts`.
- [ ] Add unit tests for `isEditableInside` covering the three
      uneditable reasons (atomic CustomNode, atomic-kind Generated,
      no-preimage Generated) plus positive cases.
- [ ] Extend `RegistryContext` to carry optional `targetFileId?: number`
      with default `0` in the empty-registry initial value.
- [ ] In `Ast.tsx`, compute `targetFileId` from the pool's first
      block (walk to root Original; default `0`) and pass it
      through the provider value.
- [ ] Update `framework/dispatch.tsx:404-411`'s `Node` gate:
      replace the `isAtomic` check with
      `!isEditableInside(node, sourceInfoPool, targetFileId, ATOMIC_KINDS)`.
      Keep `NOOP_SET_LOCAL_AST` as the substituted callback.
- [ ] `cd hub-client && npm run build:all` green (hits the
      preview-renderer build via project references).
- [ ] `cd hub-client && npm run test:ci` green.

#### Phase 3 — First-edit gates emit `Q-3-41`

**Repo facts the implementer needs:**

- ReactPreview no-baseline branch:
  `hub-client/src/components/render/ReactPreview.tsx:444-446`.
  Currently `console.warn` + bare `return`.
- SPA no-baseline branch:
  `q2-preview-spa/src/PreviewApp.tsx:437-440`. Currently
  `console.warn` + bare `return`.
- SPA already has a Q-3-42/Q-3-43 surface — `DiagnosticStrip` at
  `q2-preview-spa/src/components/DiagnosticStrip.tsx` and the
  `setWriteWarnings` state in `PreviewApp.tsx:392`. Push Q-3-41
  through the same channel.
- ReactPreview already drains write-back warnings into
  `pendingWriteWarningsRef` (line 320) and flushes via
  `onDiagnosticsChange` on the next render (line 361-366). Push
  Q-3-41 into `pendingWriteWarningsRef.current` so it surfaces in
  the existing diagnostics panel. Per the autosave-context
  suppress-after-3 policy, the merging already de-dupes by source
  range; Q-3-41 has no range so it'll just repeat — acceptable
  for v1 because the user will keep retrying until the render
  catches up.
- TS `Diagnostic` shape:
  `ts-packages/preview-renderer/src/types/diagnostic.ts:28-49`.
  Required fields: `kind: 'warning'`, `title`, `hints: string[]`,
  `details: DiagnosticDetail[]` (can be empty). Optional: `code`,
  `problem`, `start_line` / `start_column` / `end_line` /
  `end_column` (omit — no source range), `rendered`.

**Helper sketch** — shared between both call sites. Live in
`ts-packages/preview-runtime/src/firstEditDiagnostic.ts` (new file;
both ReactPreview and the SPA already import from this package):

```ts
import type { Diagnostic } from '@quarto/preview-renderer/types/diagnostic';

/** Construct a Q-3-41 warning for the "edit before first render
 *  produced a baseline AST" case. Body text mirrors the catalog
 *  entry; the helper is the TS counterpart to a Rust
 *  `diagnostic_q3_41()` builder that doesn't exist (the writer is
 *  never called in this branch). */
export function diagnosticQ3_41(): Diagnostic {
    return {
        kind: 'warning',
        code: 'Q-3-41',
        title: 'Edit dropped — render not ready yet',
        problem:
            "Your edit was dropped because the document hasn't " +
            "finished rendering. Try again in a moment.",
        hints: [],
        details: [],
    };
}
```

- [ ] Create `ts-packages/preview-runtime/src/firstEditDiagnostic.ts`
      with `diagnosticQ3_41()` per the sketch. Export from
      `ts-packages/preview-runtime/src/index.ts`.
- [ ] Co-located unit test
      `ts-packages/preview-runtime/src/firstEditDiagnostic.test.ts`:
      assert `diagnosticQ3_41()` returns the expected shape (kind,
      code, title, problem present).
- [ ] In `ReactPreview.tsx`'s `handleSetAst`, replace the
      `console.warn` + return in the no-baseline branch
      (`!baseline`) with:
      `pendingWriteWarningsRef.current = [...pendingWriteWarningsRef.current, diagnosticQ3_41()];`
      followed by the early return. Trigger a re-render so the
      pending warnings flush — pass through `onDiagnosticsChange`
      directly with the merged set rather than waiting for the
      next render, since no qmd content change happens here.
      (Implementation detail: store `pendingWriteWarningsRef` flush
      logic in a small helper if duplicated from the post-render
      drain.)
- [ ] In `PreviewApp.tsx`'s `handleSetAst`, replace the
      `console.warn` + return in the `!path || !baselineJson` branch
      with `setWriteWarnings((prev) => [...prev, diagnosticQ3_41()]);`
      followed by the early return.
- [ ] In ReactPreview: assert the diagnostic still surfaces if the
      user fixes the underlying issue (render eventually completes,
      baseline becomes available, next edit succeeds — the Q-3-41
      from the dropped edit remains in the diagnostics panel until
      the next successful render's drain clears it). Document this
      in the call-site comment.
- [ ] Hub-client integration test (Vitest): mount ReactPreview
      with `ast=''` (no baseline), call `handleSetAst({})`, assert
      `onDiagnosticsChange` is called with a list containing
      `code: 'Q-3-41'`. Place alongside the existing ReactPreview
      tests; if there's no test file for ReactPreview yet, model
      on `hub-client/src/services/incrementalWrite.wasm.test.ts`'s
      structure.
- [ ] SPA integration test
      (`q2-preview-spa/src/PreviewApp.integration.test.tsx`):
      drive `handleSetAst` before the first successful render
      completes; assert `DiagnosticStrip` renders a row with the
      Q-3-41 title.
- [ ] `cd hub-client && npm run build:all && npm run test:ci` green.

#### Phase 4 — Per-kind soft-drop test symmetry (Rust)

**Repo facts the implementer needs:**

- Existing test module at the bottom of
  `crates/pampa/src/writers/incremental.rs` (search `#[cfg(test)]`).
  Models to mirror:
  - Omit on atomic-kind: `keep_before_with_atomic_kind_generated_no_anchor_emits_omit`
    (line ~1590; uses `By::filter("upper.lua", 14)`).
  - Inline UseAfter soft-drop:
    `inline_use_after_on_atomic_generated_soft_drops_to_keep_before_with_q3_42`
    (line ~2028; uses `By::shortcode("meta")`).
  - Multi-inline dedupe positive:
    `multi_inline_dedupe_emits_token_once_when_invocation_shared`
    (line ~1909; shortcode case).
- The code paths in question (`coarsen_keep_before_block` for
  Omit, `assemble_inline_content` for inline UseAfter) do not
  branch on `by.kind` — they branch on `by.is_atomic_kind()`. New
  per-kind tests exercise the same code, but a regression in
  `is_atomic_kind`'s enumeration (e.g. dropping `"title-block"`
  from the match) would be caught here whereas the generic test
  alone wouldn't.

- [ ] `keep_before_with_atomic_kind_generated_title_block_emits_omit`:
      mirror the filter test, build `By::title_block()`, assert
      `CoarsenedEntry::Omit`. ~20 LOC.
- [ ] `keep_before_with_atomic_kind_generated_tree_sitter_postprocess_emits_omit`:
      same shape, `By::tree_sitter_postprocess()`. ~20 LOC.
- [ ] `inline_use_after_on_filter_constructed_inline_soft_drops`:
      mirror the shortcode test at line ~2028, build
      `By::filter("emoji.lua", 9)` on the original inline, assert
      Q-3-42 + KeepBefore. ~25 LOC. (This complements Plan 7b's
      Phase-1 *block-level* filter UseAfter test by exercising
      the inline path.)
- [ ] `inline_use_after_on_title_block_inline_soft_drops`:
      same shape, `By::title_block()`. ~25 LOC.
- [ ] `multi_inline_dedupe_filter_case`: shape-equivalent to
      `multi_inline_dedupe_emits_token_once_when_invocation_shared`
      but using `By::filter("decoration.lua", 12)`. Filter
      constructions rarely produce multi-inline output in practice,
      but the dedupe rule consults `Invocation` regardless of
      kind, so the test is meaningful as a regression-shape pin.
      ~30 LOC.
- [ ] `cargo nextest run -p pampa -E 'test(/coarsen|inline_use_after|multi_inline/)'`
      green.

#### Phase 5 — `cfg(debug_assertions)` `#[should_panic]` test

**Repo facts the implementer needs:**

- The debug-assert site:
  `crates/pampa/src/writers/incremental.rs:448-455`. Panic message
  starts with `"Generated { by: shortcode, from: [] } reached the
  writer — Plan 6's stamper must always attach an Invocation
  anchor for shortcode resolutions."`
- `#[should_panic(expected = "…")]` matches on a substring. Use
  the unique prefix `"Generated { by: shortcode, from: [] } reached"`
  to avoid false positives.
- Release builds compile `debug_assert!` out. The test must be
  cfg-gated to `debug_assertions` so release-profile test runs
  don't trip the `should_panic` reverse-failure.

**Sketch:**

```rust
#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "Generated { by: shortcode, from: [] } reached")]
fn shortcode_with_empty_from_trips_debug_assert() {
    // The Plan-6 stamper invariant: every Generated{by:shortcode}
    // carries an Invocation anchor. A hand-constructed shape that
    // skips the anchor must trip the writer's debug_assert.
    let gen_info = SourceInfo::generated(By::shortcode("meta"));
    let block = para(vec![], gen_info);
    let ast = quarto_pandoc_types::Pandoc {
        blocks: vec![block],
        meta: ConfigValue::default(),
    };
    let plan = ReconciliationPlan {
        block_alignments: vec![BlockAlignment::KeepBefore(0)],
        ..Default::default()
    };
    let mut warnings = Vec::new();
    // Coarsen panics inside `coarsen_keep_before_block` via the
    // debug-assert at incremental.rs:448.
    let _ = coarsen("", &ast, &ast, &plan, &mut warnings);
}
```

- [ ] Add the test per the sketch above to the same test module.
      Document the `cfg(debug_assertions)` gating with a one-line
      comment so release-profile runners aren't confused.
- [ ] `cargo nextest run -p pampa shortcode_with_empty_from` green
      (default profile = `debug_assertions` on).
- [ ] `cargo nextest run --release -p pampa shortcode_with_empty_from`
      green (test is compiled out, suite still passes).

#### Phase 6 — Verification

- [ ] `cargo xtask verify` (full) green.
- [ ] End-to-end exercise: open `q2-preview` against a small
      fixture, type into a `{{< meta foo >}}`-resolved region
      *before* the first render completes (or use the dev server
      with artificial render delay), confirm Q-3-41 appears in the
      `DiagnosticStrip`. Record the invocation + observed
      diagnostic in the plan body under §"Verification" per
      `CLAUDE.md`'s end-to-end rule.
- [ ] End-to-end exercise for the framework gate: open a fixture
      with a no-preimage Generated container (e.g. the synthesized
      footnotes Div from Plan 6 + a single inline edit), confirm
      the React dispatcher's gate now intercepts the typing before
      the writer's soft-drop fires (no `Q-3-43` flashes through).
- [ ] Plan-7 doc gets a "Closed via Plan 7c" footnote on the four
      open items (do not flip the checkboxes — they describe
      Plan-7 scope; Plan 7c is a follow-up).

### Out of scope

- Anything in Plan 7b (writer-lossless baseline test;
  filter-construction *block-level* UseAfter test; e2e Playwright
  matrix).
- `is_editable_inside` migration to `quarto_core::editability`.
  The Rust module lives in `pampa::writers::incremental` for
  documented dependency-cycle reasons (see Plan 7 Phase 1
  implementation note). The TS-side predicate goes into
  `preview-renderer`, mirroring the consumer placement; no
  attempt is made to unify the module names.
- Plan 9 (`ValueSource`) / Plan 10 (`Dispatch`) work. The role-
  asymmetry rule (`preimage_in` walks `Invocation` only) is
  already in place on both sides; future anchor roles inherit
  the gate behaviour for free.
- New diagnostic codes beyond Q-3-41. The codes for the gate
  surfaces (Q-3-42, Q-3-43) are already implemented.
- Suppressing Q-3-41 spam in autosave contexts. The current
  `suppressAfterThree` helper in `DiagnosticStrip` keys by source
  range; Q-3-41 has no range so will repeat per keystroke. If
  this proves noisy in practice, file a follow-up to extend the
  helper to also key by code.

## Design decisions (settled in conversation)

- **Q-3-41 is TS-constructed, not Rust-constructed.** The writer
  is never invoked in the no-baseline branch — the gate intercepts
  before the bridge. A Rust `diagnostic_q3_41()` builder would be
  dead code; the catalog entry exists for docs URL / version
  consistency only. (Plan 7 §"Catalog mechanics" already
  established that the writer's Q-3-43 emission picks its body
  text via the builder, not the catalog template; Q-3-41 takes
  the same path with the builder on the TS side.)
- **`targetFileId` defaults to `0`.** Both sides default the
  target FileId to 0 when the AST lacks a first-block root
  FileId — see `incremental.rs:289-293` for the Rust precedent.
  The default is safe for empty documents (won't match any real
  source bytes; `hasPreimageIn` returns `undefined`; gate
  conservatively denies editing).
- **TS predicate placement.** `hasPreimageIn` /
  `isEditableInside` go into the existing `utils/sourceInfo.ts`
  rather than a new module — they're a natural extension of the
  atomicity helpers already there, and the `ATOMIC_KINDS` set is
  next to them.
- **No new context fields.** `targetFileId` joins the existing
  `RegistryContext`; no new context type is introduced. The
  default-`0` semantics matter: dispatchers that don't pass it
  fall through to the same "no preimage anywhere" behaviour they
  had before (since the wire-format default `d` is FileId 0,
  which matches the gate). The only practical regression is if a
  caller relies on editing happening inside a non-zero-FileId
  AST without setting `targetFileId` — that's a Plan 8 / include
  story and not regressed today.
- **Phase ordering inside Phase 2.** The implementation order
  inside Phase 2 is: predicate + tests → context plumbing →
  dispatcher gate. The predicate is independently testable; the
  context plumbing only matters when the gate consumes it; the
  gate is the integration point.

## References

- Audit transcript (2026-05-25 Claude session): the four items
  numbered 1–4 in §Goal map to that audit's items 1, 2, 3, and 4.
- `claude-notes/designs/incremental-writer-contract.md` —
  consumer-side contract; §"Role-asymmetry" and §"Unified
  editability predicate" pin the rules this plan implements.
- `claude-notes/designs/provenance-contract.md` — producer-side
  contract; §4 "Role-asymmetry" and §7 "Atomic-kind set"
  cross-reference the editability work.
- `claude-notes/plans/2026-05-04-q2-preview-plan-7-incremental-writer.md`
  — Phase 1 implementation note documents the
  `pampa::writers::incremental` placement (the deliberate
  deviation from the post-review `quarto_core::editability`
  pin).
- `claude-notes/plans/2026-05-24-q2-preview-plan-7b-test-orama.md`
  — the *other* Plan-7-followup test pass. Plan 7c is disjoint;
  scan Plan 7b before adding any test to make sure it's not
  already covered there.
- `crates/pampa/src/writers/incremental.rs:113-162` — Rust
  reference for the editability predicate.
- `crates/quarto-source-map/src/source_info.rs:406-442` — Rust
  reference for `preimage_in` (Original / Substring / Concat /
  Generated walk).
- `ts-packages/preview-renderer/src/utils/sourceInfo.ts` — TS
  target file for the new predicates.
- `ts-packages/preview-renderer/src/framework/dispatch.tsx:404-411`
  — the gate to update.

## Estimated scope

| Phase | Lines (rough) |
|-------|---------------|
| 1 — Q-3-41 catalog entry | ~15 |
| 2 — TS predicates + context + gate + tests | ~250 |
| 3 — First-edit Q-3-41 emission + helper + tests | ~120 |
| 4 — Rust per-kind tests | ~120 |
| 5 — `cfg(debug_assertions)` `#[should_panic]` test | ~25 |
| 6 — Verification | (no code) |
| **Total** | **~530** |

Roughly half the size of Plan 7 itself. No new types, no new
diagnostic semantics; the work is wiring + symmetric test
coverage.

## Risk areas

- **Q-3-41 spam in autosave.** Without a code-keyed suppression
  rule, every keystroke before first-render emits a fresh
  warning. The DiagnosticStrip's `suppressAfterThree` keys on
  source range and Q-3-41 has none. Acceptable for v1 — the
  pre-render window is short — but document the limitation in
  the strip's comment so a future contributor can extend the
  helper.
- **`targetFileId` derivation under include.** Plan 8's
  IncludeExpansion wrapper introduces source content from a
  non-zero FileId. The default-`0` derivation in Phase 2 is
  conservative: nodes whose root FileId is the included file
  fail `hasPreimageIn(target=0)`, so the gate denies editing.
  This is the *correct* behavior for v1 (editing inside an
  included child should require the user to open the child),
  but worth confirming with a fixture once Plan 8 lands.
- **Gate desync between Rust and TS.** The two predicates must
  agree on which kinds are atomic and which roles are walked.
  `ATOMIC_KINDS` in `sourceInfo.ts` already mirrors the Rust
  `is_atomic_kind`; the new TS predicates inherit that contract.
  If a future kind is added on one side without the other, the
  desync is silent. Add a CI test in Plan 7c's verification step
  that exercises a representative fixture through both sides and
  asserts the gate verdicts match.

## Notes

This is the third Plan-7 follow-up alongside Plan 7a (runtime
filter idempotence, `bd-bk3y` / Q-3-44/45) and Plan 7b
(test-o-rama). Each addresses a different gap left by the
2026-05-24 implementation session; together with this plan, the
post-Plan-7 surface is closed.

The plan does not propose any change to the writer's behaviour
or contract — it only closes gaps where the implementation drifted
from the post-review intent. If a reviewer reads this and thinks
"this needs a design discussion," check the prior contract docs
(provenance + writer) first; if a discussion is still warranted,
the conclusion goes into one of those docs, not into Plan 7c.
