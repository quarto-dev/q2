# Plan 7c — Plan 7 closure gaps (Q-3-41, TS editability gate, per-kind tests)

**Date:** 2026-05-25
**Branch:** `feature/provenance` (or fresh worktree branched from it).
The contract docs the plan references — `provenance-contract.md` and
`incremental-writer-contract.md` — currently live on
`review/provenance-plan-7` and merge into `feature/provenance` as
part of the review-pass merge that is the same prerequisite for this
plan.
**Status:** Implementation plan — **partially superseded by Plan 7d (2026-06-03).**
**Milestone:** none directly — closes correctness/coverage gaps in
the writer surface Plan 7 already shipped.

> **Impact of Plan 7d (ships before 7c).** Plan 7d replaces the
> denylist cascade this plan tightens with an allowlist algebra whose
> inline `UseAfter` dispatch keys on the **new** node's own
> `source_info` (mirroring the block-level `e584428d` fix). As a
> result:
>
> - **Phase 7 (`displaced_before_idx`) is OBSOLETE** — the algebra
>   makes no original-side lookup, so there is nothing for
>   `displaced_before_idx` to make precise. See the tombstone in the
>   Phase 7 section.
> - **Phase 7b (inline new-side atomicity check) is OBSOLETE** — it is
>   exactly what 7d's rule R1' does by construction. See the Phase 7b
>   tombstone.
> - **Phases 4 and 5 (per-kind soft-drop tests; `#[should_panic]`
>   debug-assert test)** remain valuable but target code 7d
>   restructures (`coarsen_keep_before_block` disappears; the inline
>   two-phase soft-drop collapses). When written, target them at 7d's
>   **dispatch rows**, not the old cascade arms.
> - **Phases 1, 2, 3, 6, 8, 9** (Q-3-41 catalog + first-edit gates,
>   TS-side `hasPreimageIn`/`isEditableInside`, `Q343Reason` enum,
>   `target_file_id` derivation, verification) are **orthogonal** to
>   7d and unaffected.

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
post-review contract, and close two correctness/UX issues that the
post-implementation code review surfaced:

- The user always sees *some* signal when an edit is dropped —
  Q-3-42 for atomic-content edits, Q-3-43 for no-preimage edits,
  Q-3-41 for first-edit-before-render. No silent drops.
- The React framework's read-only gate matches the writer's
  editability predicate, so edits that the writer would soft-drop
  are gated at the DOM rather than reverting after a round-trip.
- The writer's debug-assert + each atomic kind's soft-drop path
  has explicit regression coverage.
- **Q-3-43's diagnostic body actually names what was dropped** —
  include path, metadata key, or container kind — instead of three
  emission sites sharing one generic message. (Code-review item,
  not part of the original closure audit; see Phase 6.)
- **Inline-level soft-drop looks up the original by the
  reconciler's index**, not by the result-side positional proxy
  that today's code uses. Today's proxy is exact for in-place
  retypings (the shortcode case the tests cover) but misfires on
  any inline insert/delete before the soft-drop site. (Code-review
  item; see Phase 7.)
- **`target_file_id` derivation walks past synthesized first
  blocks** instead of falling back to `FileId(0)` on a title-block-
  first document. Dormant bug today (single-file fixtures
  happen to land on `FileId(0)`); pre-empts Plan 8's multi-file
  story. (Code-review item; see Phase 8.)

Behaviour outside these items is unchanged. The code-review phases
tighten the writer's existing contract — they don't add new
contract surface, new diagnostic semantics, or a new pipeline tier.

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

**Cross-language parity test — keeping TS in sync with Rust.**

Hand-mirrored unit tests catch most desync the day the desync
happens, but they rely on a contributor noticing that "I changed
the Rust walker; I should update the TS one too." That discipline
fails the first time someone forgets. We need a structural check.

The mechanism: a corpus of `(SourceInfoPool, node_s, target_file_id,
expected_preimage_or_null)` cases that's **generated from Rust** and
**consumed from TS**. Rust is the source of truth; if the Rust
`preimage_in` semantics change, the corpus regenerates; the TS test
runs against the new corpus and fails until the TS walker is
updated to match.

Corpus shape (single JSON file, committed):

```json
{
  "schema_version": 1,
  "generated_from": "crates/quarto-source-map/src/source_info.rs",
  "cases": [
    {
      "name": "original_same_file",
      "pool": [ /* SourceInfoEntry wire-format entries, code 0/1/2/4 */ ],
      "node_s": 0,
      "target_file_id": 0,
      "expected": [10, 25]
    },
    {
      "name": "generated_with_value_source_only_no_invocation",
      "pool": [ ... ],
      "node_s": 2,
      "target_file_id": 0,
      "expected": null
    }
  ]
}
```

Location: `crates/quarto-source-map/test-fixtures/preimage-parity/cases.json`.
Lives with the producer of truth (the Rust walker), consumed by
the verifier (the TS walker). The TS test reads the file via
Vite's `import.meta.glob` or a path-relative fetch in test
config.

**Rust side — generator + freshness gate.**

Rust generates the fixture from a hand-written enumeration of
cases that mirror the existing `preimage_in` unit tests at
`crates/quarto-source-map/src/source_info.rs:1614-1750`. The
generator runs as a Rust integration test:

```rust
// crates/quarto-source-map/tests/preimage_parity_fixture.rs
//
// Generates the cross-language parity corpus consumed by
// ts-packages/preview-renderer/src/utils/sourceInfo.parity.test.ts.
// Run with `cargo nextest run -p quarto-source-map preimage_parity`.
// Fails if `cases.json` is stale relative to the in-code corpus —
// re-run with `QUARTO_BLESS_PREIMAGE_PARITY=1` to regenerate.

#[test]
fn preimage_parity_fixture_is_up_to_date() {
    let cases = build_corpus();           // hand-written enumeration
    let expected = serialize_corpus(&cases);
    let path = "test-fixtures/preimage-parity/cases.json";
    if std::env::var("QUARTO_BLESS_PREIMAGE_PARITY").is_ok() {
        std::fs::write(path, &expected).unwrap();
        return;
    }
    let actual = std::fs::read_to_string(path).unwrap_or_default();
    assert_eq!(
        actual.trim(),
        expected.trim(),
        "preimage parity fixture is stale; rerun with \
         QUARTO_BLESS_PREIMAGE_PARITY=1 to regenerate"
    );
}
```

The corpus enumeration covers, at minimum:

- `Original` in target file (positive)
- `Original` in non-target file (None)
- `Substring` composing offsets through a parent in target
- `Substring` rooted outside target (None)
- `Concat` of contiguous pieces in target (positive)
- `Concat` with a gap (None)
- `Concat` empty (None)
- `Generated` with `Invocation` anchor resolving in target (positive)
- `Generated` with `Invocation` anchor in non-target (None)
- `Generated` with only `ValueSource` anchor (None — role-asymmetry)
- `Generated` with only `Other("…")` anchor (None — forward-compat)
- `Generated` with empty `from[]` (None)
- Nested cases: `Substring` of a `Generated`'s Invocation;
  `Generated` whose Invocation is itself a `Substring`.

Every shape `preimage_in` matches on must appear at least once;
every "None" reason must appear at least once. The
`role-asymmetry` cases are load-bearing — they're the contract
that Plans 9/10 inherit.

**TS side — consumer test.**

```ts
// ts-packages/preview-renderer/src/utils/sourceInfo.parity.test.ts
import cases from
  '../../../../crates/quarto-source-map/test-fixtures/preimage-parity/cases.json';
import { hasPreimageIn } from './sourceInfo';

describe('preimage parity with Rust', () => {
    for (const c of cases.cases) {
        test(c.name, () => {
            const node = { s: c.node_s };
            const actual = hasPreimageIn(node, c.pool, c.target_file_id);
            expect(actual ?? null).toEqual(c.expected);
        });
    }
});
```

The test relies on the TS wire-format types
(`ts-packages/preview-renderer/src/types/sourceInfo.ts`) deserializing
the corpus `pool` entries directly — that is the same wire format
the runtime consumes, so if the corpus deserializes, the runtime
contract holds.

**Atomic-kinds parity (belt-and-suspenders).**

Separately from the walker corpus, a small text-level check
keeps the atomicity sets in sync. Add a Rust integration test
that generates a JSON file listing the `is_atomic_kind` kinds:

```rust
// crates/quarto-source-map/tests/atomic_kinds_fixture.rs
#[test]
fn atomic_kinds_fixture_is_up_to_date() {
    let kinds = ["filter", "shortcode", "title-block",
                 "tree-sitter-postprocess"];
    // (in-code enumeration is the source of truth; assert
    // every kind here is_atomic_kind-true and no other kind
    // we synthesize is true)
    for k in kinds { assert!(By::raw(k, json!(null)).is_atomic_kind()); }
    // ... write to test-fixtures/atomic-kinds.json with bless flag ...
}
```

And a TS test that asserts `ATOMIC_KINDS` equals the fixture's
set. Same bless-flag freshness gate, same desync-loud failure.

**Implementation steps for the parity work.**

- [ ] Create `crates/quarto-source-map/tests/preimage_parity_fixture.rs`
      with the corpus builder per the sketch above. Enumerate the
      cases listed in §"corpus enumeration."
- [ ] Run with `QUARTO_BLESS_PREIMAGE_PARITY=1` to generate
      `crates/quarto-source-map/test-fixtures/preimage-parity/cases.json`.
      Commit the fixture.
- [ ] Create `ts-packages/preview-renderer/src/utils/sourceInfo.parity.test.ts`
      per the sketch. Configure the test runner to find the
      `cases.json` path (relative import works under Vitest's
      default config; confirm `npm run test:ci` picks it up).
- [ ] Create the atomic-kinds parity fixture + Rust generator +
      TS consumer test. The TS consumer test imports
      `ATOMIC_KINDS` from `utils/sourceInfo.ts` and asserts
      set-equality with the fixture.
- [ ] Document the bless flag in `crates/quarto-source-map/README.md`
      (create if missing): a single paragraph on when to bless
      the fixtures (any Rust-side change that affects
      `preimage_in`'s behaviour or the atomic-kinds enumeration).
- [ ] CI: `cargo nextest run` already runs the freshness gate;
      no CI changes needed. The TS parity test runs under
      `npm run test:ci`, which is already in `cargo xtask verify`.

**Why the freshness gate matters.**

Without the gate, a Rust-side change (say, adding `By::callout()`
to `is_atomic_kind`'s matches arm) would silently leave the TS
fixture stale, and the TS parity test would pass against the
stale fixture. The gate makes that change a Rust test failure —
loud, immediate, easy to fix by re-running with the bless flag.
The TS side then trips when the contributor regenerates the
fixture without updating `ATOMIC_KINDS` to match. Two-step
diagnosis, but both steps fail loudly.

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

**The block-Omit path is `is_atomic_kind`-driven, not kind-specific.**

The block-level Omit branch (`coarsen_keep_before_block`) and the
inline soft-drop branch (`assemble_inline_content`) both consult
`by.is_atomic_kind()` — they don't pattern-match on kind. A
hand-written per-kind test exercises the same `matches!` arm
through a different constructor; the only regression it catches
is "someone dropped a kind from the `matches!` arm at
`source_info.rs:647`." That's a real but narrow failure mode.

A single enumeration property test catches the same failure with
less scaffolding and stays correct as the atomic-kind set grows.
The hand-written inline-soft-drop pair is more justified — the
inline path has subtle wiring (diagnostic-location selection in
`diagnostic_q3_42_inline`, dedupe interaction with `Invocation`
equality) that isn't a function of kind alone.

**Block-level: one property test, not three hand-written tests.**

```rust
#[test]
fn every_atomic_kind_emits_omit_under_keep_before_with_empty_from() {
    // Drives every kind in the documented atomic-kind set through
    // coarsen and asserts the Omit verdict. New kinds added to
    // `By::is_atomic_kind()` must be added here too; if a kind
    // ever leaves the set without leaving this test, the test
    // either fails (kind no longer atomic) or false-passes
    // (regression) — the latter is caught by the corresponding
    // freshness gate in Plan 7c Phase 2's atomic-kinds parity
    // fixture.
    let atomic_kinds: Vec<By> = vec![
        By::filter("upper.lua", 14),
        // shortcode is excluded — its empty-`from` case trips
        // the debug-assert (see Phase 5); the property below
        // only enumerates kinds whose empty-`from` is "normal."
        By::title_block(),
        By::tree_sitter_postprocess(),
    ];
    for by in atomic_kinds {
        assert!(by.is_atomic_kind(), "kind {:?} no longer atomic", by);
        let block = para(vec![], SourceInfo::generated(by.clone()));
        let ast = quarto_pandoc_types::Pandoc {
            blocks: vec![block],
            meta: ConfigValue::default(),
        };
        let plan = ReconciliationPlan {
            block_alignments: vec![BlockAlignment::KeepBefore(0)],
            ..Default::default()
        };
        let mut warnings = Vec::new();
        let entries = coarsen("", &ast, &ast, &plan, &mut warnings).unwrap();
        assert!(
            matches!(entries[0], CoarsenedEntry::Omit),
            "expected Omit for kind {:?}, got {:?}", by, entries[0],
        );
        assert!(warnings.is_empty(), "KeepBefore branch should not warn");
    }
}
```

**Inline-level: keep the hand-written pair.** The inline path is
worth exercising once per kind because the diagnostic builder
and the soft-drop substitution have distinct behaviour beyond
the `is_atomic_kind()` gate.

- [ ] Add `every_atomic_kind_emits_omit_under_keep_before_with_empty_from`
      per the sketch above. ~30 LOC, replaces the three
      block-Omit per-kind tests.
- [ ] Add `inline_use_after_on_filter_constructed_inline_soft_drops`:
      mirror the shortcode test at line ~2028, build
      `By::filter("emoji.lua", 9)` on the original inline, assert
      Q-3-42 + KeepBefore. ~25 LOC. (Complements Plan 7b's Phase-1
      *block-level* filter UseAfter test by exercising the inline
      path.)
- [ ] Add `inline_use_after_on_title_block_inline_soft_drops`:
      same shape, `By::title_block()`. ~25 LOC.
- [ ] Add `multi_inline_dedupe_filter_case`: shape-equivalent to
      `multi_inline_dedupe_emits_token_once_when_invocation_shared`
      but using `By::filter("decoration.lua", 12)`. Filter
      constructions rarely produce multi-inline output in practice,
      but the dedupe rule consults `Invocation` regardless of
      kind, so the test pins the regression shape. ~30 LOC.
- [ ] `cargo nextest run -p pampa -E 'test(/coarsen|inline_use_after|multi_inline|every_atomic_kind/)'`
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

#### Phase 6 — Differentiated `Q-3-43` builder via `Q343Reason` enum

**Repo facts the implementer needs:**

- Current builder:
  `crates/pampa/src/writers/incremental.rs:552-563`
  (`diagnostic_q3_43_block`). Returns a single generic message —
  `"An edit to pipeline-generated content was reverted."` — and a
  single generic hint that lists three possible upstreams
  ("an include, a metadata key, or other source").
- Three call sites, each currently calls
  `diagnostic_q3_43_block(block)` with no case discriminator:
  - `incremental.rs:320` — block `UseAfter` on a no-preimage
    Generated container (user wholesale-replaced a synthesized
    container via React).
  - `incremental.rs:344` — block `RecurseIntoContainer` on an
    atomic CustomNode whose wrapper has preimage in target
    (typically `IncludeExpansion` / `CrossrefResolvedRef`; soft-drop
    substitutes Verbatim).
  - `incremental.rs:350` — block `RecurseIntoContainer` on a
    no-preimage Generated container (synthesized
    footnotes / appendix / etc.; soft-drop substitutes Omit).
- The post-review contract doc
  (`claude-notes/designs/incremental-writer-contract.md`,
  §"User-facing diagnostic surface") promises body text that names
  the upstream: `"To edit this content, open `<path>` directly."`
  for includes; `"This content is generated from metadata; edit
  `_quarto.yml` to change it."` for metadata-derived containers.
  Today's code delivers neither.
- For the include-recurse case, the include path lives in the
  atomic CustomNode's `plain_data["source_path"]`. Look at
  `crates/quarto-pandoc-types/src/custom.rs` for the `plain_data`
  shape; use `.as_str()` on the `Value` to extract.
- For metadata-derived containers, the synthesizer's `By::kind`
  string (`"footnotes"`, `"appendix"`, etc.) is the only stable
  identifier today — there is no metadata-key anchor in v1. Plan 9
  (`ValueSource`) will give us the actual metadata range; until
  then, naming the kind is the best the diagnostic can do.

**Design — `Q343Reason` enum at the call boundary.**

The three emission sites collapse to one builder that takes a
typed reason. The enum forces every new emission site to pick a
case (compile-time exhaustiveness) and centralises the body-text
choices for the message catalog.

```rust
/// Why a Q-3-43 was emitted. One variant per emission path in
/// `coarsen`; new soft-drop sites must extend this enum so the
/// match in `diagnostic_q3_43_block` covers them at compile time.
enum Q343Reason<'a> {
    /// User edited inside an atomic CustomNode whose wrapper has
    /// preimage in target — typically an `IncludeExpansion` or a
    /// `CrossrefResolvedRef`. `include_path` is the wrapper's
    /// `plain_data["source_path"]` if present (Plan 8); `None` for
    /// CustomNodes without a source-path field.
    IncludeRecurse { include_path: Option<&'a str> },
    /// User edited inside a no-preimage Generated container
    /// (footnotes / appendix / sectionize / etc.). `kind` is the
    /// `by.kind` string of the container.
    MetadataContainerRecurse { kind: &'a str },
    /// User wholesale-replaced a no-preimage Generated container
    /// via React. `kind` is the new-side block's `by.kind`.
    NoPreimageReplacement { kind: &'a str },
}

fn diagnostic_q3_43_block(
    block: &Block,
    reason: Q343Reason,
) -> quarto_error_reporting::DiagnosticMessage {
    let (title, problem, hint): (&str, String, String) = match reason {
        Q343Reason::IncludeRecurse { include_path: Some(path) } => (
            "Include content edit dropped",
            format!("An edit inside `{{{{< include {} >}}}}` was reverted.", path),
            format!("To edit this content, open `{}` directly.", path),
        ),
        Q343Reason::IncludeRecurse { include_path: None } => (
            "Generated content edit dropped",
            "An edit inside an atomic block was reverted.".into(),
            "This block is read-only; edit its upstream source instead.".into(),
        ),
        Q343Reason::MetadataContainerRecurse { kind } => (
            "Generated content edit dropped",
            format!("An edit inside the synthesized `{}` container was reverted.", kind),
            "This content is generated from metadata; edit `_quarto.yml` to change it.".into(),
        ),
        Q343Reason::NoPreimageReplacement { kind } => (
            "Generated content edit dropped",
            format!("A replacement of the synthesized `{}` container was reverted.", kind),
            "Generated containers must be changed by editing their metadata source.".into(),
        ),
    };
    quarto_error_reporting::DiagnosticMessageBuilder::warning(title)
        .with_code("Q-3-43")
        .with_location(block.source_info().clone())
        .problem(problem)
        .add_hint(hint)
        .build()
}
```

The `Block` parameter stays so `with_location` can anchor the
warning at the original wrapper's source range (atomic CN paths)
or fall through to a no-range diagnostic (no-preimage container
paths — `with_location` accepts a `SourceInfo::Generated` whose
`preimage_in` returns `None`; the resulting warning lands without
a Monaco squiggle and surfaces in the diagnostics banner only).

**Catalog reconciliation.** The catalog entry
(`crates/quarto-error-reporting/error_catalog.json`) currently
carries one Q-3-43 with a generic `message_template`. The
builder is now responsible for the per-case body text (matching
Plan 7's already-established "builder picks body text, catalog
holds metadata" convention for Q-3-43), so no catalog change is
needed. Confirm by grepping the catalog entry's `since_version`
is still `"99.9.9"`; if a later catalog reformat tries to pin
body text, that's the point to push back on.

**Implementation steps.**

- [ ] Add the `Q343Reason` enum next to `diagnostic_q3_43_block`
      in `incremental.rs`. Keep it `pub(super)` or module-private;
      it's a call-boundary type, not part of the writer's external
      API.
- [ ] Replace the body of `diagnostic_q3_43_block` per the sketch
      above. Title, problem, hint per variant.
- [ ] Update the three call sites in `coarsen`:
      - `incremental.rs:320`: pass `Q343Reason::NoPreimageReplacement
        { kind: kind_of(new_block) }` where `kind_of` reads
        `Generated.by.kind` (use the existing `.is_kind(...)` helper
        family or write a small `by_kind_of_block(&Block) -> Option<&str>`).
      - `incremental.rs:344`: pass `Q343Reason::IncludeRecurse
        { include_path: include_path_of(orig_block) }` — write a
        small helper that downcasts `Block::Custom(cn)` and reads
        `cn.plain_data.get("source_path").and_then(|v| v.as_str())`.
      - `incremental.rs:350`: pass `Q343Reason::MetadataContainerRecurse
        { kind: by_kind_of_block(orig_block).unwrap_or("generated") }`.
- [ ] Adjust the existing soft-drop tests in
      `coarsen_plan7_tests` (`incremental.rs:1525+`) so they assert
      the *new* per-case problem text:
      - `recurse_into_atomic_custom_node_soft_drops_to_verbatim`
        (line ~1807): wrap the original `CrossrefResolvedRef`
        CustomNode with `plain_data` containing
        `{"source_path": "foo.qmd"}`; assert the warning's problem
        contains `"foo.qmd"`. Add a `_no_source_path` variant that
        omits `plain_data` and asserts the fallback wording.
      - `recurse_into_no_preimage_generated_soft_drops_to_omit`
        (line ~1851): assert the problem contains `"appendix"`
        (the `By::appendix()` kind used by the fixture).
      - `use_after_on_no_preimage_generated_soft_drops_to_omit`
        (line ~1769): assert the problem contains the new-side
        block's kind.
- [ ] Add a Phase-6-specific test that exercises all three
      `Q343Reason` variants through `diagnostic_q3_43_block`
      directly (skipping `coarsen`); compact regression pin for
      the message text.

**Location anchoring — what `with_location` should resolve to.**

The current code passes `new_block` at the UseAfter→Omit site
(line 394) and `orig_block` at the two RecurseIntoContainer
sites (lines 427, 467). The new-side block in the UseAfter case
is React-constructed — its `source_info` is typically
`Generated { by: user_edit, from: [] }` or a `SourceInfo::default()`.
`preimage_in` returns `None` on either, so the Monaco squiggle
doesn't land anywhere useful. The original-side block in this
case is a no-preimage Generated container, whose `source_info`
also has no useful preimage — so the squiggle problem is intrinsic
to the case, not a fixable bug.

Two things follow from that:

- **For the two RecurseIntoContainer sites, `orig_block` is the
  right anchor and the code already does it.** The IncludeRecurse
  case has a useful range (the include token); the
  MetadataContainerRecurse case doesn't, but choosing `orig_block`
  over `new_block` is still correct because the warning is *about*
  the original wrapper, and downstream attribution layers
  (`resolve_byte_range`, etc.) prefer original-side info.
- **For the UseAfter→Omit site, switch from `new_block` to the
  original block's source_info IF available.** Today the call
  site doesn't bind any `orig_block` — `BlockAlignment::UseAfter`
  has no `displaced_before_idx`. Two options:
  - **v1 fix (cheap):** pass the new block (current behavior),
    accept that the diagnostic carries no useful location. Pin the
    behavior with a test so future contributors don't accidentally
    "fix" it without parallel work on the alignment type.
  - **v2 fix (parallel to Phase 7):** extend
    `BlockAlignment::UseAfter` the same way Phase 7 extends
    `InlineAlignment::UseAfter`, then pass `original_blocks[displaced_before_idx]`.
    Out of scope for Plan 7c — file a follow-up beads issue.

The v1 fix is what Phase 6 ships. Tests pin current behavior,
the v2 follow-up is a beads-issue note.

- [ ] Add `q3_43_location_anchors_to_original_block_on_recurse`:
      assert that for `recurse_into_atomic_custom_node_soft_drops_to_verbatim`
      and `recurse_into_no_preimage_generated_soft_drops_to_omit`,
      the emitted warning's `location` matches the *original*
      block's `source_info`, not the new block's. Cheap pin
      against accidental regression.
- [ ] Add `q3_43_location_falls_back_to_new_block_on_use_after`:
      for `use_after_on_no_preimage_generated_soft_drops_to_omit`,
      assert that the warning's `location` is the new block's
      `source_info` (current v1 behavior). Comment block explains
      the v2 follow-up.
- [ ] File a follow-up beads issue: "Block-level UseAfter soft-drop:
      extend `BlockAlignment::UseAfter` to carry
      `displaced_before_idx` (parallel to Plan 7c Phase 7's inline
      fix)." Reference Plan 7c Phase 6 location-anchoring v2.
      Priority 3 (polish — no user-visible squiggle today either
      way; affects attribution metadata downstream).

- [ ] `cargo nextest run -p pampa` green.
- [ ] `cargo xtask verify --skip-hub-build --skip-hub-tests`
      green.

**Why an enum, not three top-level helpers.**

A reasonable alternative is three named helpers
(`q3_43_include_recurse`, `q3_43_metadata_recurse`,
`q3_43_no_preimage_replace`) instead of one builder taking an
enum. The enum is preferred here because:

1. The failure mode we're fixing — "someone added a new soft-drop
   site and reused the generic message" — is exactly what landed
   in Plan 7. The enum's exhaustiveness check makes the regression
   structural: a new `Q343Reason::Foo` is a compile error until
   the builder handles it.
2. The catalog has one Q-3-43 entry; modelling the call sites as
   one builder mirrors that shape and avoids future drift between
   the catalog and the emission code.
3. Adding a fourth Q-3-43 emission site (likely from Plan 8's
   IncludeExpansion work) means one new enum variant and one new
   match arm — no scaffolding to copy-paste.

If a future case grows wildly different message structure (e.g.
a multi-paragraph body), peel it off into its own helper at that
point.

#### Phase 7 — Inline soft-drop carries the displaced original index — ❌ OBSOLETE (superseded by Plan 7d)

> **DO NOT IMPLEMENT.** Plan 7d ships before 7c and replaces the inline
> cascade with an allowlist dispatch that keys on the **new** node's own
> `source_info` (like the block-level `e584428d` fix). It makes **no**
> original-side lookup, so the `result_idx` positional proxy is *removed*,
> not made precise — there is nothing for `displaced_before_idx` to fix.
> The checklist below is retained as historical analysis only; the
> `InlineAlignment::UseAfter` struct-variant migration it proposes is no
> longer needed. (`InlineAlignment` payload stays a tuple variant per 7d's
> "What 7d does not change.")

**Repo facts the implementer needs (historical):**

- Soft-drop site:
  `crates/pampa/src/writers/incremental.rs:1069-1080`
  (`assemble_inline_content`, the `UseAfter(_)` arm of the
  effective-alignment-rewriting loop). The current code reaches
  for `orig_inlines.get(result_idx)` to find the original inline
  whose editability gates the soft-drop.
- The comment in the code is honest about the proxy:
  > "exact for in-place retypings (the common shortcode-edit
  > case), approximate for arbitrary insertions/deletions."
- Reconciler type:
  `crates/quarto-ast-reconcile/src/types.rs:112-124`
  (`InlineAlignment`). The relevant variant is
  `UseAfter(usize)` — tuple variant carrying only `after_idx`.
- The same shape exists for blocks:
  `BlockAlignment::UseAfter(usize)` at line 100. The block
  soft-drop path does **not** consult an original-side index
  (it checks the new-side block's editability via
  `new_block.source_info().preimage_in(...)`), so this phase is
  inline-only. Block soft-drop is correct as-is.
- Today's test suite for inline soft-drop:
  `inline_use_after_on_atomic_generated_soft_drops_to_keep_before_with_q3_42`
  at `incremental.rs:2027`. All inline-soft-drop fixtures align
  `orig_inlines[i]` with `new_inlines[i]` 1:1, so the proxy
  bug is invisible to CI.

**The fix — extend `InlineAlignment::UseAfter` to a struct variant.**

The reconciler is the only place that knows which original inline
(if any) the `UseAfter` is replacing. Today's tuple variant
throws that information away; the fix is to keep it. Change to:

```rust
// crates/quarto-ast-reconcile/src/types.rs
pub enum InlineAlignment {
    KeepBefore(usize),

    /// Use the after-side inline. `displaced_before_idx` is
    /// `Some(i)` when the reconciler treated this as a replacement
    /// of `orig_inlines[i]` (the common positional-edit case);
    /// `None` for genuine inserts where no original aligns with
    /// this slot. Consumers that gate on the original inline's
    /// editability (e.g. the writer's soft-drop) MUST use this
    /// field rather than deriving it from the alignment index.
    #[serde(rename = "use_after")]
    UseAfter {
        after_idx: usize,
        #[serde(default)]
        displaced_before_idx: Option<usize>,
    },

    RecurseIntoContainer { before_idx: usize, after_idx: usize },
}
```

`Option<usize>` rather than `usize` because inserts (no
displaced original) and replacements (displaced original known)
both need to be expressible. The `#[serde(default)]` makes the
new field absent-friendly on the wire — pre-existing JSON
serializations of `UseAfter` deserialize cleanly with
`displaced_before_idx = None`, which is the "be conservative,
don't soft-drop" answer.

**Why a struct variant, not a new enum variant.**

A less-invasive alternative is to add `UseAfterReplacing
{ after_idx, before_idx }` alongside `UseAfter(usize)` and leave
the existing variant for genuine inserts. Rejected because:

- Every consumer of `InlineAlignment` then has to handle two
  variants that mean almost the same thing. The writer's match
  arms double.
- The reconciler still has to decide which variant to emit on
  every alignment, and that decision *is* the
  `displaced_before_idx` Option — just expressed in two enum
  variants instead of one struct variant with an `Option`.

Struct-variant migration is mechanical: `cargo build` will list
every pattern match that needs updating.

**Reconciler-side: populate `displaced_before_idx`.**

The reconciler at
`crates/quarto-ast-reconcile/src/inline.rs` (or wherever
inline alignment is decided — locate via `git grep
'InlineAlignment::UseAfter'`) produces `UseAfter` from its
positional alignment loop. In practice:

- LCS-style alignment: when `UseAfter(j)` is emitted at result
  position `r`, the reconciler has just consumed `orig_inlines[i]`
  on the original side (or hasn't, in which case this is an
  insert). The `displaced_before_idx` is `Some(i)` in the
  consumed case, `None` in the insert case.
- Positional alignment: `displaced_before_idx = Some(r)` when
  `r < orig_inlines.len()`, `None` otherwise.

The exact derivation depends on the reconciler's algorithm.
Locate the alignment loop and add the index alongside the
existing `after_idx` emission.

**Writer-side: consume `displaced_before_idx`.**

```rust
// crates/pampa/src/writers/incremental.rs (assemble_inline_content)
InlineAlignment::UseAfter { after_idx, displaced_before_idx } => {
    if let Some(orig_idx) = displaced_before_idx
        && let Some(orig) = orig_inlines.get(*orig_idx)
        && !is_editable_inside_inline(orig, target_file_id)
    {
        warnings.push(diagnostic_q3_42_inline(orig));
        effective.push(InlineAlignment::KeepBefore(*orig_idx));
        continue;
    }
    effective.push(alignment.clone());
}
```

When `displaced_before_idx` is `None` (a genuine insert), there
is no original to gate against, and the alignment passes through
unchanged. That is the correct behaviour — inserts can't soft-drop
because there's nothing they're displacing.

**Implementation steps.**

- [ ] In `crates/quarto-ast-reconcile/src/types.rs`: change
      `InlineAlignment::UseAfter` from tuple variant `(usize)` to
      struct variant `{ after_idx, displaced_before_idx }` per
      the sketch. Update the serde rename and add
      `#[serde(default)]` on the new field.
- [ ] `cargo build --workspace` and walk every compile error;
      update each pattern match. Reconciler tests in the same
      crate will surface most of them. Writer call sites in
      `pampa::writers::incremental` will surface the rest.
- [ ] Reconciler: populate `displaced_before_idx` in the inline
      alignment loop. Add a test in
      `quarto-ast-reconcile` asserting the field is populated for
      a fixture where `UseAfter` replaces an original inline,
      and is `None` for a fixture that inserts a fresh inline.
- [ ] Writer: replace `orig_inlines.get(result_idx)` at
      `incremental.rs:1074` with the `displaced_before_idx`-aware
      logic. Remove the `result_idx` positional proxy and its
      explanatory comment.
- [ ] Add a regression test:
      `inline_use_after_with_insert_before_shortcode_does_not_misfire`.
      Construct an inline plan with `[Insert("X"), UseAfter
      (over-shortcode)]` so the result-side index `1` and the
      original-side index `0` differ. Assert the soft-drop fires
      against the original shortcode inline (the
      `displaced_before_idx`), not against
      `orig_inlines.get(result_idx=1)` (which would be out of
      bounds, or wrong).
- [ ] Add a complementary test:
      `inline_use_after_pure_insert_does_not_soft_drop`. A new
      inline with `displaced_before_idx = None` must not consult
      `orig_inlines` at all. Assert no Q-3-42 is emitted.
- [ ] `cargo xtask verify --skip-hub-build --skip-hub-tests`
      green.
- [ ] `cargo xtask verify` (full) — the WASM bridge passes
      `ReconciliationPlan` JSON over the wire; the
      `#[serde(default)]` makes the change wire-compatible, but
      a full verify confirms nothing else broke.

**Wire-format compatibility.**

The TS side at
`ts-packages/quarto-sync-client/src/types.ts` does not currently
deserialize `ReconciliationPlan` itself — the plan is computed
inside WASM and never crosses the boundary as JSON. Confirm with
`git grep -l 'InlineAlignment'` in `ts-packages/` and
`hub-client/`; if any TS consumer turns up, the same
`#[serde(default)]` semantics apply on the parsing side (new
field absent ⇒ `null`/`undefined` ⇒ "don't soft-drop").

#### Phase 7b — Inline `UseAfter` soft-drop checks the new-side inline's atomicity — ❌ OBSOLETE (superseded by Plan 7d)

> **DO NOT IMPLEMENT.** The new-side atomic-Generated-with-preimage check
> this phase adds to the inline cascade *is* Plan 7d's rule **R1'** by
> construction — 7d's inline `UseAfter` dispatches on the new node's own
> `source_info` and emits `Verbatim` of its preimage + Q-3-42/Q-3-43. The
> current-code soundness gap this phase documents (resolved bytes leaking
> through the inline cascade) is real and unaddressed *today*; 7d closes
> it, and since 7d ships first, this phase never needs to. Retained below
> as historical analysis. (Note the original-side framing here predates
> the 2026-06-03 decision; it does not change the conclusion.)

**Discovered 2026-05-26** during the algebraic-soundness research
that produced today's block-level UseAfter fix (commit
`e584428d`). The block-level cascade in `coarsen_blocks` had a gap
for `BlockAlignment::UseAfter(j)` where `new_blocks[j]` was
atomic-Generated *with preimage* — the let-user-win Rewrite
fell through and emitted the resolved bytes back into source. The
fix added a branch that detects atomic-Generated-with-preimage on
the *new* block and substitutes `Verbatim` of preimage + Q-3-43.

The inline cascade in `assemble_inline_content`
(`crates/pampa/src/writers/incremental.rs:1325-1362`) has the
exact analogue gap. Today's Phase 1 of `assemble_inline_content`
only checks the **original-side** inline's editability (via the
positional proxy that Phase 7 above fixes). It does not check
whether the *new* inline at `after_idx` is atomic-Generated with
preimage. If a reconciler emits `InlineAlignment::UseAfter(j)`
where `new_inlines[j]` carries `Generated{by:shortcode, from:
[Invocation -> token_si in target]}`, the cascade lets it through
to splice/rewrite and the resolved bytes leak.

**The fix mirrors the block-level fix shipped today.** Add a new
check at the head of the `InlineAlignment::UseAfter` arm:

```rust
InlineAlignment::UseAfter { after_idx, displaced_before_idx } => {
    let new_inline = &new_inlines[*after_idx];
    let new_si = new_inline.source_info();
    let atomic_generated_preimage = match new_si {
        SourceInfo::Generated { by, .. } if by.is_atomic_kind() =>
            new_si.preimage_in(target_file_id),
        _ => None,
    };
    if let Some(_range) = atomic_generated_preimage {
        // User edited inside an atomic-kind Generated inline
        // (typically a shortcode-resolved Str). The new inline
        // still carries the token's Invocation anchor; emit the
        // token bytes verbatim by substituting KeepBefore of the
        // displaced original (if known) or the positional proxy.
        let orig_idx = displaced_before_idx
            .or_else(|| Some(*after_idx).filter(|i| *i < orig_inlines.len()))?;
        warnings.push(diagnostic_q3_42_inline(&orig_inlines[orig_idx]));
        effective.push(InlineAlignment::KeepBefore(orig_idx));
        continue;
    }
    // ... existing original-side check follows.
}
```

This fix and Phase 7's `displaced_before_idx` enrichment compose
naturally: Phase 7 gives us the precise original-side index;
Phase 7b uses it (or falls back to the positional proxy) when
emitting `KeepBefore`. The two phases can land in either order;
Phase 7 lands first if it's already scoped as drafted, then
Phase 7b layers on top.

**Why this is a separate phase, not folded into Phase 7.** Phase 7
fixes an *accuracy* bug in the existing original-side check (the
positional proxy misfires on inserts/deletes). Phase 7b adds a
*new branch* (new-side atomicity) that doesn't exist in any form
today. Both are denylist tightenings; both become moot once Plan
7d's algebraic refactor lands.

- [ ] Add the atomic-Generated-with-preimage check at the head of
      `InlineAlignment::UseAfter` in `assemble_inline_content`.
- [ ] Regression test:
      `inline_use_after_on_atomic_generated_shortcode_with_preimage_soft_drops`.
      Construct an inline plan with `UseAfter` targeting a Span
      whose `source_info` is `Generated{by:shortcode, from:
      [Invocation -> token_si]}` and whose content differs from the
      original. Assert the qmd output preserves the token bytes
      verbatim and one Q-3-42 warning fires. Mirrors today's
      block-level `sectionize_wrapper_shortcode_child_edit_soft_drops`.
- [ ] `cargo xtask verify --skip-hub-build --skip-hub-tests` green.

#### Phase 8 — `target_file_id` derivation skips no-`root_file_id` first blocks

**Repo facts the implementer needs:**

- Current derivation site:
  `crates/pampa/src/writers/incremental.rs:289-293`. The current
  shape:
  ```rust
  let target_file_id = original_ast
      .blocks
      .first()
      .and_then(|b| b.source_info().root_file_id())
      .unwrap_or(quarto_source_map::FileId(0));
  ```
- `root_file_id()` lives at
  `crates/quarto-source-map/src/source_info.rs:487-498`. For
  `Generated`, it walks the `Invocation` anchor; for an empty
  `from[]` it returns `None`. So a document whose first block is
  a synthesized title-block (no Invocation) gets `None` →
  fallback to `FileId(0)`.
- `FileId(0)` is the wire-format default — the same FileId the
  parser stamps on a fresh single-file parse. So on a one-file
  document, `FileId(0)` happens to be correct by coincidence,
  and the bug only surfaces when there's a real cross-file
  story (Plan 8's IncludeExpansion, the q2-preview-spa's project
  mode addressing multiple files).
- Today the bug is dormant. We don't ship multi-file editing
  in this writer pass yet; Plan 8 will. But the test is cheap
  and the fix is cheap, and shipping them now means Plan 8 doesn't
  have to rediscover the issue.

**The fix — `iter().find_map(...)` over `first().and_then(...)`.**

```rust
let target_file_id = original_ast
    .blocks
    .iter()
    .find_map(|b| b.source_info().root_file_id())
    .unwrap_or(quarto_source_map::FileId(0));
```

`find_map` walks blocks in order, returning the first block whose
`root_file_id()` resolves to `Some`. Synthesized title-blocks,
sectionize wrappers, footnotes containers — anything Generated
with empty `from[]` — get skipped. The fallback to `FileId(0)`
remains for the genuinely-empty-document case (no blocks at all,
or every block is no-`root_file_id` Generated).

**Implementation steps.**

- [x] Write the failing test first:
      `target_file_id_skips_synthesized_first_block`. Build a
      Pandoc whose `blocks[0]` is a synthesized title-block (e.g.
      `Block::Header` with
      `SourceInfo::generated(By::title_block())` and empty `from[]`)
      and whose `blocks[1]` is a real `Original` paragraph with
      `FileId(7)`. Drive `coarsen` and assert that the editability
      check on `blocks[1]` returns `true` (i.e. `target_file_id`
      resolved to `FileId(7)`, not `FileId(0)`). The pre-fix
      coarsen sees `target_file_id == FileId(0)`,
      `preimage_in(FileId(0))` on a `FileId(7)`-Original returns
      `None`, and the block is gated as non-editable — the test
      fails.
- [x] Apply the `find_map` fix at `incremental.rs:289-293`.
      Implemented as a recursive `derive_target_file_id` helper
      that descends through `block_block_children` as well, so a
      sole-top-level sectionize wrapper (with the user's real
      blocks inside) also yields the right file id rather than
      `FileId(0)` by accident. The implementation note in §"Why
      this isn't already broken in CI" below remains accurate:
      single-file fixtures with `Original`-first blocks hit the
      fast path; the wrapper-first variant required descent.
- [x] Re-run the test; assert it passes.
- [x] Add a fully-empty-document test:
      `target_file_id_defaults_to_zero_for_empty_document`. The
      `FileId(0)` fallback only kicks in when every block returns
      `None` from `root_file_id()` — or there are no blocks.
- [x] `cargo nextest run -p pampa target_file_id` green.
- [x] `cargo xtask verify --skip-hub-build --skip-hub-tests`
      green.

**Why this isn't already broken in CI.**

The existing test suite uses fixtures with `Original`-first
blocks: `keep_before_with_original_in_target_emits_verbatim`
at `incremental.rs:1565` builds a `Paragraph` with
`SourceInfo::original(TARGET, 10, 25)` at `blocks[0]`, so
`root_file_id()` returns `Some(TARGET)` immediately and the
fallback path is never hit. A title-block-first fixture
exposes it. The Plan 8 single-file include story would hit
it too — pre-empting that discovery is the value here.

#### Phase 9 — Verification

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
- `claude-notes/designs/transparent-wrappers.md` — sibling
  contract introduced 2026-05-25 alongside Phase 8's fix. Names
  the descent pattern that `derive_target_file_id` implements
  and lifts it into a reusable primitive (`first_in_user_tree`)
  that future plans (8/9/10/replay) can cite without
  rediscovering.
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
| 2 — TS predicates + context + gate + unit tests | ~250 |
| 2 — Cross-language parity fixture + tests (Rust gen + TS consumer + atomic-kinds belt-suspenders) | ~200 |
| 3 — First-edit Q-3-41 emission + helper + tests | ~120 |
| 4 — Rust per-kind tests | ~120 |
| 5 — `cfg(debug_assertions)` `#[should_panic]` test | ~25 |
| 6 — Differentiated `Q-3-43` builder + call-site updates + test adjustments + location-anchoring tests | ~180 |
| 7 — Inline soft-drop: extend `InlineAlignment::UseAfter` to struct variant + reconciler population + writer consumption + regression tests | ~180 |
| 8 — `target_file_id` derivation: `find_map` over `first()` + regression tests | ~40 |
| 9 — Verification | (no code) |
| **Total** | **~1130** |

Roughly the size of Plan 7 itself. Phase 6 and Phase 7 add real
correctness fixes (Phase 6 closes a doc-vs-code drift on Q-3-43
body text; Phase 7 fixes a positional-proxy hole in inline
soft-drop). The new parity-test work in Phase 2 adds a structural
sync check so the TS↔Rust walker pair can't drift silently.

No new diagnostic codes. No new pipeline tier. The
`InlineAlignment::UseAfter` shape change in Phase 7 is the only
type-surface change; `#[serde(default)]` keeps it wire-compatible.

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
  agree on which kinds are atomic, which roles are walked, and
  how `preimage_in` chains resolve. The parity fixture work
  inside Phase 2 makes this structural: Rust generates
  `test-fixtures/preimage-parity/cases.json` from its in-code
  corpus, a Rust test fails when that fixture is stale, and the
  TS test fails when its walker disagrees with the regenerated
  fixture. Future-walker changes either re-bless both sides
  (matching) or trip one of the two gates (loud). The atomic-
  kinds belt-and-suspenders fixture catches the simpler "added a
  kind on one side only" drift in one Rust + one TS assertion.
- **Reconciler change in Phase 7 ripples through pattern
  matches.** Changing `InlineAlignment::UseAfter` from
  `(usize)` to `{ after_idx, displaced_before_idx }` is a
  breaking change for every consumer of the type. The mechanical
  fix is `cargo build --workspace` until clean; the risk is a
  consumer that silently ignores the new field (e.g. wildcards
  the variant). After Phase 7, audit for `InlineAlignment::UseAfter
  { .. }` matches that don't bind `displaced_before_idx`; any such
  match outside test code should be reviewed.
- **`Q343Reason::IncludeRecurse { include_path: None }` fallback.**
  Atomic CustomNodes without a `source_path` field in `plain_data`
  (e.g. `CrossrefResolvedRef` today) fall back to a generic
  message. That's worse than the catalog promise but better than
  Plan 7's all-cases-identical text. Plan 8's IncludeExpansion
  will give us the include path universally for include cases;
  CrossrefResolvedRef would need its own `Q343Reason` variant
  later (e.g. `Q343Reason::CrossrefRecurse { ref_id: &str }`) if
  the message text needs to differ.

## Notes

This is the third Plan-7 follow-up alongside Plan 7a (runtime
filter idempotence, `bd-bk3y` / Q-3-44/45) and Plan 7b
(test-o-rama). Each addresses a different gap left by the
2026-05-24 implementation session; together with this plan, the
post-Plan-7 surface is closed.

Phases 1–5 close gaps where the implementation drifted from the
post-review intent — no contract change. Phases 6 and 7 close
correctness/UX issues that the post-implementation code review
surfaced:

- **Phase 6** brings Q-3-43's body text up to the contract the
  doc already promises. Mechanical fix; the contract itself is
  unchanged.
- **Phase 7** narrows the inline soft-drop's positional proxy by
  threading the displaced original index through
  `InlineAlignment::UseAfter`. This is a small reconciler-type
  contract change (struct variant + `Option<usize>` field) — and
  the only contract change in the plan. The semantics it adds
  (the reconciler tells consumers which original was displaced)
  is what the writer already needed and approximated; the type
  now expresses it honestly.

If a reviewer reads this and thinks "this needs a design
discussion," the only candidate is Phase 7's reconciler-type
change, which is the kind of small structural sharpening that
fits inside this plan rather than a separate design doc. The
other six phases are wiring + test work + a single-file
diagnostic refactor.

Update the contract docs alongside the implementation:

- `claude-notes/designs/incremental-writer-contract.md` —
  §"User-facing diagnostic surface" should note that Q-3-43
  body text differentiates by reason (include / metadata /
  replacement), with the wording the builder produces.
- `claude-notes/designs/incremental-writer-contract.md` —
  §"Soft-drop semantics" should note that the inline-level
  case consults `InlineAlignment::UseAfter`'s
  `displaced_before_idx` (the reconciler's truth) rather than
  the alignment's result-side index.
