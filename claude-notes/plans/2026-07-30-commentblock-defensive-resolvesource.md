# Fix bd-ddaqjb91 — s0-list-item-surfaces crash: defensive `CommentBlock` + honest test stub

**Strand:** bd-ddaqjb91 (bug, P1)
**Discovered:** 2026-07-30, while landing the #442 sidecar-stripping fix (`c33c40bd`).
**Broken since:** `a6fc44b8` (Comments v1, PR #441) on `main`.

## Overview

`ts-packages/preview-renderer`'s integration suite fails on main:
`s0-list-item-surfaces.integration.test.tsx` has 18/23 tests failing. All 18
share a single crash:

```
TypeError: Cannot read properties of undefined (reading 't')
  at sameCommentableKind (src/q2-preview/custom/CommentBlock.tsx:120)
  at CommentBlock (src/q2-preview/custom/CommentBlock.tsx:211)
```

### Root-cause chain (diagnosed, experiment-validated)

1. **PR #441 put `CommentBlock` on every block render path.** The registry now
   maps `Block: CommentBlock` (`registry.ts:56`), and the framework `Node`
   dispatcher routes *every* block — top-level and nested — through
   `registry['Block']` (`framework/dispatch.tsx:402`). For any commentable-
   looking block with no comments, `CommentBlock` calls
   `edit.resolveSource(block)` and passes `resolved.sourceNode` to
   `sameCommentableKind()` **without checking that `sourceNode` exists**
   (`CommentBlock.tsx:205-216`).

2. **The s0 test harness's `resolveSource` stub predates `ResolvedSource`'s
   current shape** (`s0-list-item-surfaces.integration.test.tsx:80-89`). It
   returns `{ reachabilityClass: 'Reachable', sourceEntry, sourceIndex: null }`:
   - missing `sourceNode` (required by the interface, crashes CommentBlock);
   - `'Reachable'` is not a member of `ReachabilityClass`
     (`'TopLevel' | 'Descendable' | 'Opaque'`, `sourceIndex.ts:17`);
   - `sourceIndex` is not a property of `ResolvedSource` at all.
   Before #441 nothing on this render path consumed the resolved value's
   `sourceNode`, so the stale stub passed.

3. **No gate type-checks test files**, so the drift was invisible:
   `tsconfig.json` excludes `*.test.ts(x)` from `tsc`, and vitest transforms
   with esbuild (no type checking). `npm run build` passes with a stub that
   violates the interface three ways.

4. **Process note:** `cargo xtask verify` step 11 *does* run these suites
   (`verify.rs:391-403`); the merge evidently went in without a full verify.
   No tooling change needed for this — the gate exists.

### What is NOT broken (verified)

Production is likely unaffected: the real `resolveSource`
(`PreviewRoot.tsx:1473-1489`) always includes `sourceNode` when it returns
non-null. The crash needs a malformed `ResolvedSource`, which only the test
stub produces today. (The defensive guard is still worth having:
`PreviewContextValue.resolveSource` is a pluggable, optional context member.)

Also verified: the comment chrome's wrapper `<div>` inside `<li>` does **not**
violate s0's assertions — they check `textContent` and *pool-id-carrying*
inner elements, not arbitrary wrappers, and the geometry tests mock rects on
the `<li>`/`<ul>` directly. Comments on list items are an intended #441
feature; no policy change needed.

**Experiment (2026-07-30, reverted):** patching the stub to return
`sourceNode: node` flips the suite from 18-failed to **23/23 passing**. No
assertion changes needed. This bounds the fix to the two defects below.

## Work Items

### Phase 1 — defensive guard in CommentBlock (TDD)

- [x] Write a unit test (new file,
      `src/q2-preview/custom/CommentBlock.defensive.integration.test.tsx`):
      mount a Para through `previewRegistry` with a `PreviewContext` whose
      `resolveSource` returns a malformed entry (no `sourceNode`); assert the
      block renders as passthrough (text present, no crash). Plus a guard-not-
      over-broad case: well-formed entry still gets comment chrome.
- [x] Run it; verified it fails with the exact `TypeError` above.
- [x] Fix: added `!resolved.sourceNode` to the render gate and to
      `resolveCommittable` (which `addComment`/`resolveCommentAtIndex` both
      funnel through — no other `sourceNode` consumers in the file).
- [x] Run the test; passes (2/2).

### Phase 2 — honest s0 stub

- [x] Fixed `makeResolveSource`: `sourceNode: node`,
      `reachabilityClass: 'Descendable'`, typed `sourceEntry`; dropped the
      bogus `sourceIndex` property and the `as any`.
- [x] s0 suite: 23/23 pass.

### Phase 3 — close the type-check gap (the systemic fix)

- [x] Chose the separate-tsconfig route: `tsconfig.tests.json` in
      preview-renderer (extends base; `noEmit`; `noUnusedLocals`/
      `noUnusedParameters` off — lint-grade noise, not drift detection),
      exposed as `npm run typecheck:tests`, and wired into `verify.rs`
      step 11 ahead of the test runs. Fixed the pre-existing type errors
      it surfaced in 6 preview-renderer test files (typed `vi.fn`
      generics, a `Mock` import, missing `setLocalAst` props, a
      `useContext` typo'd type, tuple-typed pool fixtures, two narrow
      casts documented in place).
- [x] Proved it catches the original defect: reintroducing the stale stub
      fails with `TS2322: '"Reachable"' is not assignable to
      'ReachabilityClass'` (plus the pool-tuple errors); reverted.
- [x] Extended to `preview-runtime` (same tsconfig + script + verify leg).
      Surfaced real drift there too: `MockSyncClient`'s interface was
      missing `applyEditorOperations` (impl had it), and the handler mocks
      were untyped. Fixed; 74/74 tests still pass.

### Phase 4 — verification + landing

- [x] `npm run test` (542 passed / 36 skipped) and `npm run test:integration`
      (572 passed / 1 skipped) in `ts-packages/preview-renderer`.
- [x] `cargo xtask verify --skip-rust-tests` — includes the hub-client
      `build:all` leg and the new step-11 typecheck legs.
- [ ] Commit; comment on bd-ddaqjb91 with the commit hash; close the strand
      with `--reason`. Open PR.

## Open Questions

1. **Should `CommentBlock` also tolerate a *throwing* `resolveSource`?**
   Recommendation: no try/catch for now — swallowing exceptions hides real
   bugs; the guard should only normalize *shape*, not *behavior*. Revisit if
   user render-components can inject `resolveSource`.
2. **Typecheck mechanism** (Phase 3): a separate `tsc -p tsconfig.tests.json`
   is predictable and CI-friendly; vitest `typecheck` runs per-test-run and
   may be slower in watch mode. Default to the separate tsconfig unless
   something surfaces.
3. **Tighten `ResolvedSource` consumers?** `sourceNode: BlockNode` could stay
   required in the type while consumers guard at runtime (types don't survive
   pluggable-context boundaries). Do not weaken the type to
   `sourceNode?: BlockNode` — that would force needless guards on the many
   sound call sites in `PreviewRoot.tsx`.

## Details / references

- Crash gate: `CommentBlock.tsx:205-216` (`comments.length === 0` branch).
- Kind check: `sameCommentableKind`, `CommentBlock.tsx:118-131`.
- Registry wiring: `registry.ts:51-56` (`Block: CommentBlock`).
- Dispatcher: `framework/dispatch.tsx:402+` (all blocks route via
  `registry['Block']`).
- Real resolver: `PreviewRoot.tsx:1473-1489`; index builder + types:
  `sourceIndex.ts`.
- Stale stub: `s0-list-item-surfaces.integration.test.tsx:80-89`.
- Related work landed just before: sidecar stripping for subtree commits
  (`c33c40bd`, GH #442, follow-up strand bd-d01m11aw).
