# Editing-Mode Plan 4 — Nesting-Cursor as a Bundled Editing-Mode Extension

**Date:** 2026-06-20 (rev. 2026-06-21: surface-rendering + shared `EditBufferCache` + narrowed test ownership)
**Branch:** `editing-mode` (worktree `.worktrees/editing-mode`)
**Layer:** TypeScript (`ts-packages/preview-renderer`, a new bundled extension dir, `hub-client`, `q2-preview-spa`)
**Status:** PLAN — ready for TDD execution.
**Depends on:** Plan 1 (core services incl. **`EditBufferCache`** + depollute renderer + mode seams + **`EditingSurface` contract** + **in-tree `TextareaSurface`** + mode↔surface decouple) and Plan 2 (two extension types + discovery + delivery + two-axis selection). Keystone: `claude-notes/designs/2026-06-20-editing-mode-contract.md` (rev. 2026-06-21) — **the keystone wins on any conflict**. Epic index: `claude-notes/plans/2026-06-20-editing-mode-epic.md`.

---

## Overview

This is a **payoff** plan: take today's nesting-cursor implementation — smeared across the *vanilla* q2-preview renderer (`PreviewRoot.tsx`, `PreviewContext.tsx`, `dispatchers.tsx`, `useBlockEditHover.tsx`, `BreadcrumbChip.tsx`, `nestingNav.ts`) — and **move only the tree-aware part OUT of in-tree core and INTO a bundled editing-mode extension**, plugging into the Plan 1 seams, consuming the Plan 1 core services + the **selected editing surface**, delivered through Plan 2's rail.

**The crucial reframing in this revision (keystone rev. 2026-06-21 §2/§5/§7):**

1. **The mode renders the SELECTED SURFACE, not a hardcoded textarea.** The active-target `NodeOverride` renders the mode's `EditingSurface` (the one Plan 2 selected) via `ViewControllerProps.surface`, seeding it with `value = editBufferCache.editableTextFor(node)` and wiring `onCommit`/`onEdgeReached`/`onCancel`. Cross-surface + nesting navigation consume the **surface handle** (edge/caret callbacks), **not** `caretGeometry` directly — `caretGeometry` is now the textarea surface's internal implementation (Plan 6).
2. **`EditBufferCache` is a SHARED Plan 1 service that nesting CONSUMES, not owns.** The clean-buffer / generated-serialization machinery (today: `seedForRange` + `nestedEditBuffers` + the clean-vs-raw predicate in `outerBlocks.ts`, fed by the Rust generator) is reclassified as Plan 1's `EditBufferCache.editableTextFor`. The **only** nesting-specific buffer behavior is *calling `editableTextFor` for inner surfaces as the cursor descends the tree*. The Rust backend (`regenerate_nested_buffers` / `write_single_block`) stays as the shared generator feeding Plan 1's `PushedEditBufferCache` via the parent push — it is **not** nesting-exclusive.
3. **Nesting owns only tree-awareness.** Everything flat — self-heal/concurrency, activation, cross-surface arrows, delete-by-emptying, expand-on-edit — is Plan 1 substrate consumed by Plan 5 (block-editing) *and* this mode. This plan pins **nesting-specific tests only**.

Per the keystone (§4.1, §4.2, §11), nesting-cursor repackages as **exactly one** state-predicated `NodeOverride` plus **one** `ViewController`:

| Keystone capability | Source today | What it becomes in the extension |
|---|---|---|
| `NodeOverride` (active-target → **selected surface**) | `dispatchers.tsx` `isBlockEditTarget` gate + `renderMeasuredEdit` + `EditTextarea` + the swap predicate in `Block`/`CustomBlock` | one `NodeOverride { matches, render }`; `matches` = "this node is the active edit target"; `render` = renders **`props.surface`** (the selected `EditingSurface`, NOT a hardcoded textarea), seeded `value = editBufferCache.editableTextFor(node)`, wired to the mode's `onCommit`/`onEdgeReached`/`onCancel` |
| `ViewController.handleInput` | `useBlockEditHover.tsx` (hover/click/touch/keyboard activation) | root input handlers using Plan 1 `NodeLocator`. **Activation is shared (Plan 5 owns its tests)**; nesting only adds the nesting-chord keys (`requestNestingMove`) |
| `ViewController.renderOverlay` | `BreadcrumbChip.tsx` | overlay painted into Plan 1 `OverlaySlot` — **nesting-exclusive** (block-editing has no overlay, keystone §12) |
| `ViewController.exposeHook` | the edit + **nesting** state machine in `PreviewRoot.tsx` + `nestingNav.ts` + commit paths | the `ModeApi`: baseline `{ resolveSource, commit }` (core) + mode extras (`editTarget`, **nesting** state, `requestNestingMove`, `requestNestingSelect`, …). The **edge/caret** that drives cross-surface landing comes from the **surface handle**, not the mode. |
| `unlockNestingCursor` setting | `hub-client/src/services/preferences/schema.ts` | **declared** in the extension's `_extension.yml`; fed via `ViewControllerProps.settings`. **Nesting-exclusive** (gates the nesting chords). |
| inner-surface buffers | `nestedEditBuffersRef` lookups during nesting descent | **consume** `editBufferCache.editableTextFor(innerNode)` (Plan 1 service) when the cursor descends to an inner surface. The descent itself is the only nesting-specific buffer concern. |

The vanilla renderer left behind must be **pure** (`node → React`): no `PreviewContext`, no `data-block-pool-id` stamping in components, no edit state machine in the root, no hardcoded textarea swap. Plan 1 owns making that true; **this plan owns deleting the nesting-specific in-tree implementation as it lands in the extension** and **re-pointing every nesting-specific pinning test at the extension path so it stays green**.

**The Rust/WASM backend is unchanged.** `crates/pampa/src/{regenerate_nested_buffers.rs,apply_node_edit.rs,node_lookup.rs}`, `crates/pampa/src/writers/qmd.rs`, and `crates/wasm-quarto-hub-client/src/lib.rs` stay as-is. The extension reaches the buffer generator through Plan 1's `EditBufferCache` (which the parent feeds via the existing `regenerateNestedBuffers` → `acceptPushedBuffers` push) and reaches commit through `DocumentStore.commit`. The boundary-splice migration (keystone §9) is **noted but not performed** here.

---

## Decision: extension packaging & location

**Decision: ship nesting-cursor as a bundled editing-mode extension at `resources/extensions/quarto/editing-mode-nesting-cursor/`** (UNCHANGED), authored in TSX, delivered through Plan 2's discovery + the existing `customComponentsCode` / `LOAD_CUSTOM_COMPONENTS` rail.

Justification, grounded in what we read (unchanged from rev. 1):

1. **It must travel the same rail Plan 2 generalizes.** The keystone (§8) locks the delivery rail as `read paths → look up source → Babel-transpile → LOAD_CUSTOM_COMPONENTS → blob-import → merge`. The only artifact that flows through it is **author-style TSX discovered from an extension directory** — exactly the form the bundled `render-components` demos already take (`resources/extensions/quarto/...`). A `ts-package` would NOT be discovered by `extension/discover.rs`, would NOT transpile through Babel-standalone, and would force a *new* delivery path — contradicting "one rail, two front doors."
2. **`resources/extensions/quarto/` is the established home for bundled, version-controlled extensions.** The External Sources Policy (CLAUDE.md) forbids referencing `external-sources/`; bundling under `resources/` is the sanctioned pattern.
3. **`_extension.yml` is where settings are declared** (keystone §10). Co-locating TSX + manifest in one extension dir is the only layout that satisfies that contract.

**Boundary with Plan 2/3:** the *manifest schema* (the `editing-mode:` contribution key, the `settings:` block, discovery of the dir's `.tsx`) is **Plan 2's** to define (`crates/quarto-core/src/extension/{types.rs,discover.rs,read.rs}`). This plan **authors the manifest + TSX that conform to it** and marks the exact integration point. If Plan 2's key name differs, a find-replace settles it (keystone §15.3).

Directory shape this plan creates:

```
resources/extensions/quarto/editing-mode-nesting-cursor/
  _extension.yml                 # editing-mode contribution + unlockNestingCursor setting (Plan 2 schema)
  src/
    index.tsx                    # extension entry: exports { nodeOverrides, viewController }
    nestingCursorController.tsx  # the ViewController (handleInput nesting-chords/renderOverlay/exposeHook)
    activeTargetOverride.tsx     # the one NodeOverride (active target → props.surface)
    BreadcrumbChip.tsx           # MOVED from q2-preview/ (renderOverlay body — nesting-exclusive)
    useActivation.ts             # nesting-chord additions over Plan 5's shared activation (handleInput)
    nestingNav.ts                # MOVED from q2-preview/ (pure nav utilities — nesting-exclusive)
    nestingStateMachine.ts       # MOVED nesting machine from PreviewRoot.tsx (exposeHook body)
    __tests__/                   # relocated NESTING-SPECIFIC vitest pinning tests (see Phase 6)
```

> NOTE — pure-utility import boundary. `nestingNav.ts` imports `ByteLineMap`/`sliceUtf8` from `../utils/...` today. Inside the extension those become imports of Plan 1's published primitives (`byteLineMap`, `sliceSource`) off the renderer API surface (`window.__Q2_PREVIEW_RENDERER__`), since a discovered/transpiled extension cannot reach into `preview-renderer`'s source tree. **INTEGRATION POINT (Plan 1):** exact accessor name is Plan 1's to publish. Until known, import by keystone names and mark with `// PLAN1:` comments.

> NOTE — surface, not textarea. The override **never** imports `caretGeometry`, `EditTextarea`, or `renderMeasuredEdit`. It renders `props.surface` (Plan 1 `EditingSurfaceComponent`). The textarea's geometry is internal to the textarea surface (Plan 6). If this plan finds itself importing `caretGeometry`, that is a plan failure — the wiring goes through `EditingSurfaceHandle.onEdgeReached` / `focus(caret)` instead.

---

## Global Constraints (bake into every step)

- **TDD, test-first.** Every step writes/relocates the failing test BEFORE moving code, watches it fail for the *intended* reason, then moves code to green. Invoke `prevalidating-test-seams` when prepping each relocation and `fail-on-revert` after each relocation to prove the relocated test **binds to the extension code** (not a leftover in-tree copy). The epic assigns these skills to Plan 4.
- **No behavior change.** This is a *relocation*. Every relocated test keeps its current assertions; if an assertion's expected *value* must change because a seam moved (e.g. an import path, or "reads `props.surface`" instead of "renders a `<textarea>`"), pre-validate that change with `prevalidating-test-seams` and record the revert hunk it binds to.
- **Consume Plan 1/2 by keystone name; do not duplicate.** `NodeOverride`, `ViewController`, `ViewControllerProps` (incl. `.surface`, `.editBufferCache`), `ModeApi`, `useMode()`, `ModeContext`, `NO_OP_MODE`, `SourceResolver`, `NodeLocator`, `DocumentStore` (`.commit`), `OverlaySlot`, `EditBufferCache` (`.editableTextFor`), the `EditingSurface` contract (`EditingSurfaceProps`/`EditingSurfaceHandle`/`EditingSurfaceComponent`), and Plan 1's self-heal/re-anchor helpers (carried by `NodeLocator`). Where Plan 1's export path is not yet known, reference by name and mark `// PLAN1:` / `// PLAN2:`. **Importing a duplicate of any of these is a plan failure.**
- **Render the selected surface (keystone §4.1/§5).** The override's `render` mounts `props.surface` with `value = editBufferCache.editableTextFor(node)`, `box`, `initialCaret`, and the mode's `onCommit`/`onCancel`/`onEdgeReached`. It does NOT reimplement a textarea and does NOT read `caretGeometry`. Until Plan 6 extracts the bundled `textarea` surface, the selected surface is Plan 1's **in-tree `TextareaSurface`** (consume by keystone name).
- **Consume `EditBufferCache` for inner surfaces (keystone §7.1).** When the cursor descends into an inner surface, seed it with `editBufferCache.editableTextFor(innerNode)`. Do NOT reach into `nestedEditBuffersRef` or re-derive the clean-vs-raw predicate — that predicate is Plan 1's single shared decision. The parent's `regenerateNestedBuffers` push stays existing host plumbing feeding `acceptPushedBuffers` (off the public interface).
- **`commit` shape.** Use `DocumentStore.commit` (keystone §9 v1: wraps today's `commitTextEdit(destJson, text)` / `commitSubtreeEdit(destJson, block)`; the commit path owns the re-wrap of a generated buffer back into its container — the inverse of `editableTextFor`'s generation). Do NOT migrate to boundary-splice here. Mark the future swap with a `// keystone §9` comment.
- **VFS path convention.** `/project/` prefix (CLAUDE.md). Honored if any path is constructed.
- **Verification gates (CLAUDE.md).** This plan touches TS render paths and (via the extension dir + Plan 2 manifest) `quarto-core`-reachable resources, so the **full** gate applies before declaring done:
  - `cargo build --workspace` and `cargo nextest run --workspace` (Rust roundtrip tests live in `pampa`).
  - `cargo xtask verify` (full — WASM leg in scope because `wasm-quarto-hub-client` is the render engine).
  - `cd hub-client && npm run build:all` — the production `tsc -b && vite build` is stricter than `tsc --noEmit`/`vitest`.
  - vitest: `cd ts-packages/preview-renderer && npm test` (or the workspace runner).
  - Playwright e2e (both hosts): hub-client `e2e/q2-preview-*.spec.ts` and `q2-preview-spa/e2e/`. Geometry-dependent behavior is browser-only.
  - **Stale-WASM trap:** for any `q2 preview` end-to-end check, run `cd hub-client && npm run build:wasm` → `cargo xtask build-q2-preview-spa` → `cargo build --bin q2` first (CLAUDE.md). We change no Rust here, but the SPA-embed step is still needed if SPA bundling of the extension changed.
- **hub-client changelog (two-commit workflow).** Any commit touching `hub-client/` MUST add an entry to `hub-client/changelog.md` in a *second* commit carrying the first commit's short hash (CLAUDE.md).
- **Integration-test layout rule.** Rust tests stay in `crates/pampa/tests/integration/` + `main.rs` (`.claude/rules/integration-tests.md`). We are NOT moving the Rust test files — only confirming the **nesting** ones still pass against the unchanged backend.
- **Do not gold-plate.** Move only what is **nesting-specific**; the flat/shared pieces are Plan 1 substrate and Plan 5's test-ownership. If you find yourself wanting a hack or a TODO that undoes work, STOP and ask (CLAUDE.md).

---

## Consumes / Produces (inter-plan interface)

**Consumes (from Plan 1):**
- Seam types/host: `NodeOverride`, `ViewController`, `ViewControllerProps` (incl. `.surface`, `.editBufferCache`, `.settings`), `ModeApi`, `ModeContext`, `useMode()`, `NO_OP_MODE` — `// PLAN1:` module path TBD (`ts-packages/preview-renderer/src/framework/`).
- The **`EditingSurface` contract** (`EditingSurfaceProps`/`EditingSurfaceHandle`/`EditingSurfaceComponent`) + the in-tree **`TextareaSurface`** reference impl (with `caretGeometry` internal to it). The mode renders the **selected surface** (`props.surface`), never a hardcoded textarea.
- Core services: `SourceResolver` (was `resolveSource`/`sourceIndex`/`reachabilityClass`), `NodeLocator` (was `data-block-pool-id` stamping + `outerBlocks.ts` hit-testing + **self-heal/re-anchor**), `DocumentStore.commit` (was `commitTextEdit`/`commitSubtreeEdit` + `setAst`), `OverlaySlot`.
- **`EditBufferCache.editableTextFor`** (was `seedForRange` + the clean-vs-raw predicate in `outerBlocks.ts` + `nestedEditBuffers` lookup) + `PushedEditBufferCache` + the `acceptPushedBuffers` population port (the parent generate-and-push stays existing host plumbing feeding the port).
- Pure primitives on the renderer API surface: `byteLineMap`, `sliceSource`, `editableTextFor` wrapper.
- The dispatcher consulting a `NodeOverride` super-chain; the root mounting a single `ViewController`.

**Consumes (from Plan 2):**
- `_extension.yml` editing-mode contribution schema + `settings:` declaration + discovery of the dir's `.tsx`.
- The Rust→iframe delivery channel merging the extension's components into `customComponentsCode`.
- The **two-axis selection** mechanism that installs this mode's `ViewController`/`NodeOverride`s into Plan 1's seams, sets `ViewControllerProps.surface` to the **selected surface**, and surfaces the declared `unlockNestingCursor` **setting** value via `ViewControllerProps.settings`.

**Produces (leaf — consumed by no later plan):**
- The bundled `editing-mode-nesting-cursor` extension = one `NodeOverride` (active → selected surface) + one `ViewController` (nesting-chord `handleInput` + breadcrumb `renderOverlay` + nesting `exposeHook`) + the `unlockNestingCursor` setting declaration + **inner-surface** `editableTextFor` consumption.
- A **pure** vanilla renderer, with the nesting-specific in-tree machine / `BreadcrumbChip` / `nestingNav` removed (the flat/shared edit machine is Plan 1/Plan 5's to remove; reconcile, don't double-delete).
- Relocated **nesting-specific** pinning tests (vitest in the extension dir; Rust roundtrip + nesting Playwright re-pointed) all green.

---

## Phases

> Ordering rationale: build the extension *additively* first (Phases 1–4) so the new nesting path exists and is tested before the old one is deleted (Phase 5). Keeps the suite green at every step and lets `fail-on-revert` prove each relocated test binds to the new code. Phase 6 relocates the nesting pinning tests; Phase 7 is end-to-end verification.

### Phase 0 — Pre-flight: confirm Plan 1 & Plan 2 landed

- [ ] **0.1** Confirm Plan 1's seam exports exist and are importable: grep the renderer API surface for `NodeOverride`, `ViewController`, `ViewControllerProps` (with `.surface` + `.editBufferCache`), `useMode`, `NO_OP_MODE`, `ModeContext`, the **`EditingSurface` contract** + in-tree **`TextareaSurface`**, and the core services `SourceResolver`/`NodeLocator`/`DocumentStore`/`OverlaySlot`/`EditBufferCache`. Record actual module paths into a `// PLAN1:` reference block at the top of `index.tsx`. **If absent, STOP** — Plan 4 is blocked on Plan 1.
- [ ] **0.2** Confirm Plan 1's primitives (`byteLineMap`, `sliceSource`, `editableTextFor`) and the **`TextareaSurface`** (so the mode has a selected surface to render before Plan 6) are on `window.__Q2_PREVIEW_RENDERER__`. Record accessor names. Confirm `caretGeometry` is **internal to the textarea surface** and NOT something the mode imports.
- [ ] **0.3** Confirm Plan 2's `_extension.yml` editing-mode contribution schema + the **two-axis selection** are in place (grep `crates/quarto-core/src/extension/types.rs` for the editing-mode contribution variant; grep the host for where a selected mode's `ViewController`/`NodeOverride`s are installed and where `ViewControllerProps.surface` is set to the selected surface). Record the manifest key + the install point. **If absent, STOP** — blocked on Plan 2.
- [ ] **0.4** Sanity-baseline the suite: run vitest + the e2e specs + `cargo nextest run -p pampa` and record the current green set. Capture the list of **nesting-specific** pinning tests enumerated in **Test surfaces** below (the flat/shared and surface-geometry ones are Plan 5/6's to relocate — do not relocate them here).

### Phase 1 — Scaffold the extension dir (pure utilities first)

- [ ] **1.1 (test)** Create `…/editing-mode-nesting-cursor/src/__tests__/nestingNav.test.ts` by **moving** `ts-packages/preview-renderer/src/q2-preview/nestingNav.test.ts` and re-pointing its import to `../nestingNav`. Run it; it must FAIL (module not yet present). `prevalidating-test-seams`: only the import path changes.
- [ ] **1.2** Move `ts-packages/preview-renderer/src/q2-preview/nestingNav.ts` → `…/src/nestingNav.ts`. Repoint its `BlockNode`/`SourceIndexEntry`/`ByteLineMap`/`sliceUtf8` imports to the Plan 1 primitives on the renderer API surface (`// PLAN1:`). Keep every exported function byte-identical (`parseSiKey`, `buildNestingSurfaces`, `parentSurface`, `topBlockR0`, `depthOfSurface`, `relocateSurface`, `surfaceAtLine`, `childSurfaceToward`, `surfaceLineSpan`, `childSurfaceTowardLine`, `classifyNestingKey`, `detectPlatform`, `labelForSourceNode`, `abbrevForSourceNode`, `categoryForSourceNode`, `buildAncestorPath`, `buildNestingCommitDestination`). Test 1.1 → green.
- [ ] **1.3 (test, then code)** Create `src/index.tsx` exporting the contract shape Plan 2 installs (`export const nodeOverrides: NodeOverride[]`, `export const viewController: ViewController` — exact field names per Plan 2's installer; `// PLAN2:`). Add `__tests__/index.smoke.test.ts` asserting both exports present and `nodeOverrides.length === 1`. Stub override/controller so the smoke test compiles; flesh out in Phases 2–4.
- [ ] **1.4** Author `_extension.yml` per Plan 2's schema: declare the editing-mode contribution (controller + overrides entry + `settings:` with `unlockNestingCursor` type `boolean`, default `true` — mirroring `hub-client/src/services/preferences/schema.ts`). **INTEGRATION POINT (Plan 2):** key names. If Plan 2 already globs `resources/extensions/quarto/*`, confirm pickup; otherwise leave a `// PLAN2:` note + a checklist item rather than duplicating Plan 2's parser test.

### Phase 2 — The one `NodeOverride` (active target → selected surface)

- [ ] **2.1 (test)** Relocate the **nesting-specific** swap-behavior assertions that pin "active edit target renders the selected surface seeded from `editableTextFor`, others render normally" — the nesting subset of `p3-2-nesting-cursor-context.integration.test.tsx` and the seeding assertions in `p3-3-seeding.integration.test.tsx`. Express them against the `NodeOverride` (`matches` + `render(props.surface)`) and against a **mock `EditingSurface`** that records its `value` prop. They must FAIL first. (The *flat* active-target swap test — "any active block renders a surface" — is Plan 5's; do not relocate it here.) `prevalidating-test-seams`: the expected-value change is "renders `props.surface` seeded with `editableTextFor(node)`" replacing "renders a `<textarea defaultValue={draft}>`".
- [ ] **2.2** Author `src/activeTargetOverride.tsx`:
  - `matches(node, mode)` = the old `isBlockEditTarget` gate re-expressed against `mode` (the `ModeApi`): non-Opaque reachability AND `mode.editTarget?.anchorR0 === mode.resolveSource(node)?.sourceEntry.r[0]`. Use the **baseline** `mode.resolveSource` (core) for the lookup and the **mode extra** `mode.editTarget` for the active-target match (keystone §4: only the mode's own overrides may rely on extras).
  - `render(node, renderDefault)` = mount **`props.surface`** (the selected `EditingSurfaceComponent`), NOT a textarea, with:
    - `value = props.editBufferCache.editableTextFor(node)` (Plan 1 service; was `seedForRange`/`anchorSlice`),
    - `box` = the measured box geometry (the no-reflow box from `dispatchers.tsx` left-inset handling so `#q2-active-edit-region` and the `assertNoReflowOnActivation` contract hold unchanged),
    - `initialCaret` from the mode's pending-caret state,
    - `onCommit(text)` → `mode.commit` (via `DocumentStore.commit`; `// keystone §9`),
    - `onCancel()` → `mode.cancelPendingLand` / close,
    - `onEdgeReached(dir)` → `mode.requestNestingMove(dir)` when `unlockNestingCursor` and the chord context apply, else `mode.requestMove(dir)` (the **shared** cross-surface move — Plan 5 owns its tests). **The override never computes first/last-visual-line itself** — that is the surface's job, delivered through `onEdgeReached` (keystone §5).
  - Preserve `id="q2-active-edit-region"` and the `LEFT_INSET_STRIPPED_TYPES` box handling so the no-reflow contract holds.
- [ ] **2.3** Run 2.1 → green. `fail-on-revert`: revert `activeTargetOverride.tsx`'s `matches` body (and separately its `value = editableTextFor(...)` line) and confirm each relocated test fails — proving it binds to the extension override + the surface seeding, not a leftover dispatcher path.

### Phase 3 — `ViewController.handleInput` (nesting chords) + `renderOverlay` (breadcrumb)

> Activation itself (hover/click/touch/keyboard roving) is **shared** and Plan 5 owns its tests. This phase adds **only** the nesting-chord layer over Plan 1's activation `handleInput`, plus the nesting-exclusive breadcrumb overlay.

- [ ] **3.1 (test)** Relocate the **nesting-chord** activation assertions: the nesting-chord subset of `s1-unlock-line-nav.integration.test.tsx` and `s2-mode-aware-roving.integration.test.tsx` (the `requestNestingMove`/`unlockNestingCursor`-gated paths). Move into `…/src/__tests__/`, driven through the `ViewController`'s `handleInput`. FAIL first. (The flat roving/activation assertions stay with Plan 5.)
- [ ] **3.2** Author `src/useActivation.ts` as the **nesting-chord additions** to `handleInput: RootInputHandlers` (keystone §4.2): consume Plan 1's shared activation, and add the `unlockNestingCursor`-gated nesting-chord keys (`classifyNestingKey` → `mode.requestNestingMove(dir)`). Replace `el.closest('[data-block-pool-id]')` hit-testing with **Plan 1's `NodeLocator`**. Do NOT re-implement hover/touch/keyboard roving — those are Plan 1/Plan 5. FAIL→green.
- [ ] **3.3 (test)** Relocate the breadcrumb tests (nesting-exclusive): `BreadcrumbChip.geometry.test.ts` (pure `computeChipGeometry`) and the chip-render assertions in `p3-4-breadcrumb.integration.test.tsx`. Move into `…/src/__tests__/`. FAIL first.
- [ ] **3.4** Move `BreadcrumbChip.tsx` → `src/BreadcrumbChip.tsx`, re-expressed as the body of `renderOverlay()` painting into **Plan 1's `OverlaySlot`**. Replace `useContext(PreviewContext)` with `useMode()`; replace `ctx?.activeEditRegionRef`/`ctx?.sourceIndex` reads with the `ModeApi` extras + `mode.resolveSource`. Keep `MIN_GLYPH_W`, `CRUMB_W`, `computeChipGeometry`, the `#quarto-content` offset-parent positioning, and the `requestNestingMove`/`requestNestingSelect` wiring. **INTEGRATION POINT (Plan 1):** how `OverlaySlot` exposes the host element for geometry (the chip reads `document.getElementById('quarto-content')`); confirm `OverlaySlot` provides or permits that. FAIL→green.

### Phase 4 — `ViewController.exposeHook` (nesting state machine + commit + inner-surface buffers)

This is the largest move: the **nesting-specific** machine in `PreviewRoot.tsx`. The flat edit machine (self-heal, plain activation, cross-surface arrows, delete-by-emptying, expand-on-edit) is Plan 1/Plan 5's; **reconcile, do not relocate it here**.

- [ ] **4.1 (test)** Relocate the **nesting** state-machine pinning tests that exercise the machine through the public surface: `p3-3-nesting.integration.test.tsx`, `p3-3-unlocked-subclauses.integration.test.tsx`, `nest-caret.integration.test.tsx`, `s4-dirty-caret-col.integration.test.tsx` (nesting-caret-column), `g5-carry-expansion.integration.test.tsx`, `g6-g7-settle-gate.integration.test.tsx`, `g9-reland-fade.integration.test.tsx`, `g19-spurious-dirty.integration.test.tsx`, `s0-list-item-surfaces.integration.test.tsx` (inner-surface seeding), and the nesting subsets of `p3-2`/`p3-3-seeding` not already moved in Phase 2. Re-point them to mount the **vanilla root + the extension's `ViewController`** via a once-built harness `…/src/__tests__/mountWithNestingMode.tsx` that installs the mode into Plan 1's seams and sets `props.surface` to a test surface + `props.editBufferCache` to a stub returning known clean buffers. FAIL first.
  > **Not here (Plan 5's):** `self-heal-on-write`, `s6-delete-by-emptying`, `s7-expand-on-edit`, `p2-3b-real`, `p2-4-real`, `p2-4b`, `p2-4d`, `useBlockEditHover.integration`, `useEditableBlock.integration`, `p2-3a`. **Not here (Plan 6's):** `caretGeometry.test.ts`, `p2-4-real`/`p2-4b` *visual-line* assertions, `edit-cell-sizing`.
- [ ] **4.2** Move the **nesting** machine into `src/nestingStateMachine.ts` + `src/nestingCursorController.tsx`. Map each *nesting-specific* `PreviewRoot` member to its new home, reading the **exposeHook** contract (returns `ModeApi`):
  - Nesting state held in the controller (`useState`/`useRef`): the nesting-cursor target/depth, `pendingLandingRef`, `pendingCaretRef`, the fade/settle-gate refs, `clickSwitchRef`, and the `unlockNestingCursorRef` mirror. (`sourceIndexRef`/the flat `editTarget`/`editDraftRef`/`editExpandedRef` are **Plan 1/Plan 5 substrate** — read them from the `ModeApi`, don't re-own them. Reconcile with Plan 5.)
  - Nesting machine functions moved verbatim (behavior-preserving): `resolveLanding`, `openFromResolved`, `executeLanding`, `armRelandBackstop`, `commitNestingEdit`, `applyNestingRetarget`, `commitAndArmReland`, `requestNestingMove`, `requestNestingSelect`, the reland layout effect, the fade layout effect, and the nesting parts of `requestMove`/`cancelPendingLand`/`requestFocusRestore`/`requestClickSwitch`/`handleClickSwitchBlur`/`captureGeometry`. (Where these are shared with flat editing, consume Plan 1's version; only the tree-descent branches are nesting's.)
  - **Inner-surface buffers (the nesting-specific buffer concern):** when the cursor descends to an inner surface, seed it with `props.editBufferCache.editableTextFor(innerNode)` — NOT a direct `nestedEditBuffersRef` lookup. The clean-vs-raw decision is Plan 1's single shared predicate. Add a `// PLAN1: editableTextFor` note at each descent seeding site.
  - The commit entry points route through **Plan 1's `DocumentStore.commit`** (keystone §9 v1). Add the `// keystone §9` boundary-splice migration note.
  - `exposeHook()` returns the `ModeApi`: baseline `{ resolveSource, commit }` (core, pass-through) PLUS the nesting extras the override/overlay/handlers rely on (`editTarget` mirror, `requestNestingMove`, `requestNestingSelect`, the reland/fade/settle refs, `unlockNestingCursor` value). **Only this mode's own NodeOverride/overlay/handlers may read the extras** (keystone §4).
  - `unlockNestingCursor` arrives via `ViewControllerProps.settings` (keystone §10). The buffers reach the controller via `props.editBufferCache` (fed by the host's `regenerateNestedBuffers` → `acceptPushedBuffers` push — unchanged backend, `// PLAN1:`/`// PLAN2:`).
- [ ] **4.3** Run 4.1 → green, iterating. `fail-on-revert` on a representative subset (`nest-caret`, `p3-3-nesting`, settle-gate, inner-surface seeding) to prove the relocated tests bind to the controller + `editBufferCache`, not a residual `PreviewRoot` path.

### Phase 5 — Depollute: delete the nesting-specific in-tree implementation

Only after Phases 1–4 are green. **Plan 1/Plan 5 own deleting the flat/shared machine** (self-heal, plain activation, cross-surface arrows, the hardcoded textarea swap, `caretGeometry` callers). This plan deletes only what is **nesting-exclusive**; reconcile rather than double-delete.

- [ ] **5.1** Remove the **nesting** members from `PreviewRoot.tsx` (the 4.2 list), the nesting types (`ResolverSpec`/`PendingLanding`/`ClickSwitchRecord` if nesting-only), the nesting imports (`nestingNav`, the nesting branches of `outerBlocks` edit helpers), and the nesting fields from the `PreviewContext.Provider value`. **Confirm with Plan 1/5 which of these are shared** before deleting; leave shared ones for their owner. Keep note-numbering, link handlers, scroll, AST parse/`pool`/`sourceIndex` (now likely Plan 1's `SourceResolver`/`EditBufferCache`), `RevealDeck`, registry merge.
- [ ] **5.2** Remove the **nesting** edit-swap branches from `dispatchers.tsx` if any are nesting-only. (The textarea swap deletion — `renderMeasuredEdit`, `EditTextarea`, `renderBlockTextarea`, `isBlockEditTarget`, `LEFT_INSET_STRIPPED_TYPES`, the `caretGeometry` imports, the `Block`/`CustomBlock` swap branches — is **Plan 1/Plan 5's** as part of the surface extraction + flat-mode move; the dispatcher should already consult Plan 1's `NodeOverride` super-chain. Confirm `Block`/`CustomBlock` render the vanilla component otherwise.) Do not re-add anything; if a nesting-only branch remains, delete it.
- [ ] **5.3** Delete `BreadcrumbChip.tsx`, `nestingNav.ts`, and the now-unused **nesting** `PreviewContext` edit fields. (`useBlockEditHover.tsx` and the flat `PreviewContext` fields are Plan 1/Plan 5's — confirm ownership; leave the shared activation for its owner.) Delete the old **nesting** test files left behind by Phases 1–4 relocations.
- [ ] **5.4** Grep for dangling nesting references: `grep -rn "BreadcrumbChip\|nestingNav\|requestNestingMove\|requestNestingSelect\|unlockNestingCursor" ts-packages/preview-renderer/src` — every hit must be either a Plan 1 core service or removed. Also confirm **the extension never imports `caretGeometry`**: `grep -rn "caretGeometry" resources/extensions/quarto/editing-mode-nesting-cursor` must be empty.
- [ ] **5.5** Remove the `unlockNestingCursor` field from `hub-client/src/services/preferences/schema.ts` and its `usePreference('unlockNestingCursor')` wiring in `hub-client/src/components/tabs/SettingsTab.tsx`. The setting is now **declared in the extension manifest** and surfaced by Plan 2's settings host. **INTEGRATION POINT (Plan 2):** confirm Plan 2 renders the declared setting's control (keystone §10); if Plan 2's settings UI is not yet wired, leave the hub-client toggle as a thin shim that writes the Plan 2 settings store and file a `// PLAN2:` note rather than orphaning the UI. Mirror the `?nestingCursor=1` boot-query handling in `q2-preview-spa/src/PreviewApp.tsx` onto the Plan 2 settings path. Apply the **two-commit changelog** workflow for the hub-client change.

### Phase 6 — Relocate remaining nesting pinning tests; keep Rust + e2e green

- [ ] **6.1** Confirm every **nesting** vitest file enumerated in Phases 1–4 now lives under `…/editing-mode-nesting-cursor/src/__tests__/` and imports the extension, not `q2-preview/`. `commit-destination-equivalence.test.ts` moves with the controller (pins `buildNestingCommitDestination` ≡ closure form — nesting-specific). The flat/shared tests (`self-heal-on-write`, `s6`, `s7`, `p2-3b-real`, `p2-4-real`, `p2-4b`, `p2-4d`, `useBlockEditHover.integration`, `useEditableBlock.integration`, `p2-3a`) stay for **Plan 5** to relocate; the surface-geometry tests (`caretGeometry.test.ts`, visual-line assertions, `edit-cell-sizing`) stay for **Plan 6**. Pure-renderer/Plan-1 tests (`PreviewDocument`, `q2-preview.integration`, `entry.integration`, `registry`, `RevealDeck`, `assetWalker`, `sourceIndex`, `stripSourceInfoFields`, `custom-components`, `entry-slide-theme`) stay in `q2-preview/`.
- [ ] **6.2 (vitest)** Run the full vitest suite from both `q2-preview/` and the extension dir. Resolve the vitest config so the extension's `__tests__/` are discovered (the extension dir is under `resources/`, outside `ts-packages`; add it to the renderer's vitest `include`/`projects`, or run under the `preview-renderer` package via a path alias). **INTEGRATION POINT:** decide whether the extension's tests run under `preview-renderer`'s vitest project or a new one; document the choice. All green.
- [ ] **6.3 (Rust)** Run `cargo nextest run -p pampa` and confirm the **nesting** backend roundtrip tests are unchanged and green: `nesting_cursor_roundtrip_tests`, `regenerate_nested_buffers_tests`. (The flat backend tests — `node_edit_tests`, `tiling_phase3_tests`, `inline_splice_*` — are Plan 5's backstop; run them as a regression sweep but they are not this plan's to own.) We changed no Rust source. If Plan 2 added an `_extension.yml` discovery test, run it too.
- [ ] **6.4 (Playwright, hub-client)** Re-point and run the **nesting** hub-client e2e specs against the extension path (selection on in the fixtures): `q2-preview-nesting-caret-in`, `q2-preview-nesting-size-in`, `q2-preview-breadcrumb-geometry`, `q2-preview-breadcrumb-isolation`, `q2-preview-crumb-no-carry-expansion`. The flat specs (`inline-edit`, `self-heal-on-write`, `expand-on-edit`, `delete-by-emptying`, `block-nav-p2-5b`, `item-edit-size`, `locked-hover`, `scrolljack`, `columns-layout`) are Plan 5's; the `render-components-{kanban,drag,comment}` specs stay green via Plan 1's baseline `useMode()` (keystone §4/§8). Use the shared `assertNoReflowOnActivation` / `ACTIVE_REGION` (`#q2-active-edit-region`) helper from `ts-packages/preview-e2e-helpers/src/index.ts` — the override must keep emitting that id (Phase 2.2).
- [ ] **6.5 (Playwright, SPA)** Run `q2-preview-spa/e2e/nesting-cursor.spec.ts`. Confirm the `?nestingCursor=1` boot path now drives Plan 2's settings/selection (Phase 5.5). (`edit-cell-sizing.spec.ts` is Plan 6's surface-geometry backstop.) Update the boot-query plumbing only if the activation mechanism's URL contract changed.
- [ ] **6.6** `fail-on-revert` sweep: pick 3–4 high-value relocated **nesting** tests (one geometry e2e, one `nest-caret` vitest, one breadcrumb, one Rust roundtrip) and confirm each fails when its bound code is reverted. Proof the relocation didn't create vacuous passes.

### Phase 7 — End-to-end verification + cleanup

- [ ] **7.1** Full gate: `cargo xtask verify` (full), then `cd hub-client && npm run build:all` (stricter than vitest). Capture logs to `/tmp` and grep for failures.
- [ ] **7.2 (E2E through the binary — required, CLAUDE.md).** Drive a real `q2 preview` (or hub) session with the nesting mode selected on a fixture containing nested lists/blockquotes. Honor the stale-WASM chain first (`build:wasm` → `build-q2-preview-spa` → `cargo build --bin q2`). Inspect the actual DOM: activate a nested block → confirm the **selected surface** renders inside `#q2-active-edit-region` seeded from the clean buffer, the breadcrumb chip paints, a nesting chord (⌘⌃→ / Alt+Shift+→) descends into an inner surface (seeded via `editableTextFor`), a dirty nest commits + relands. Record the exact invocation + an output snippet + an explicit "inspected" note in this plan file. Confirm a render-components fixture (kanban) is still editable with the mode OFF.
- [ ] **7.3** Confirm the vanilla renderer carries no **nesting** references: re-run the grep from 5.4. Confirm `resources/extensions/quarto/editing-mode-nesting-cursor/` is the only home of the nesting code and that it imports **no** `caretGeometry`.
- [ ] **7.4** Update `hub-client/changelog.md` (second commit, with hash) for the user-visible change.
- [ ] **7.5** Update the epic index ("Plan 4 produces") checkboxes and note any residual `// PLAN1:`/`// PLAN2:` integration points needing follow-up strands (braid, discovered-from — side-issues only).

---

## Test surfaces — NESTING-SPECIFIC ownership only

(Narrowed from the epic "Test surfaces". The **flat/shared** tests move to **Plan 5**; the **surface-geometry** tests move to **Plan 6**. This plan owns relocating the **nesting** subset and keeping the **nesting** Rust + e2e backstops green.)

- **Vitest → MOVE into the extension dir (nesting-specific):** `nestingNav.test.ts`, `BreadcrumbChip.geometry.test.ts`, `commit-destination-equivalence.test.ts`, `p3-2-nesting-cursor-context` (nesting subset), `p3-3-nesting`, `p3-3-seeding`, `p3-3-unlocked-subclauses`, `p3-4-breadcrumb`, `nest-caret`, `s0-list-item-surfaces`, `s1-unlock-line-nav` (nesting-chord subset), `s2-mode-aware-roving` (nesting-chord subset), `s4-dirty-caret-col` (nesting-caret-column), `g5-carry-expansion`, `g6-g7-settle-gate`, `g9-reland-fade`, `g19-spurious-dirty`.
- **Vitest → NOT this plan's (Plan 5, flat/shared):** `self-heal-on-write`, `s6-delete-by-emptying`, `s7-expand-on-edit`, `p2-3b-real`, `p2-4-real`, `p2-4b`, `p2-4d`, `useBlockEditHover.integration`, `useEditableBlock.integration`, `p2-3a`, `outerBlocks*` (likely follow `NodeLocator`/`EditBufferCache` into Plan 1).
- **Vitest → NOT this plan's (Plan 6, surface geometry):** `caretGeometry.test.ts`, the visual-line/edge assertions inside `p2-4-real`/`p2-4b`, `edit-cell-sizing`.
- **Vitest → STAY in `q2-preview/`** (pure renderer / Plan 1 surface): `PreviewDocument.integration`, `q2-preview.integration`, `entry.integration`, `entry-slide-theme.integration`, `registry`, `RevealDeck.integration`, `assetWalker`, `sourceIndex`, `stripSourceInfoFields`, `custom-components.integration`.
- **Rust (`crates/pampa/tests/integration/`) → KEEP GREEN, do not move (nesting backstop):** `nesting_cursor_roundtrip_tests`, `regenerate_nested_buffers_tests`. (Flat backstops — `node_edit_tests`, `tiling_phase3_tests`, `inline_splice_*` — are Plan 5's; run as a regression sweep.) Backend unchanged.
- **Playwright → re-point at extension path, keep green (nesting):** `q2-preview-nesting-caret-in`, `q2-preview-nesting-size-in`, `q2-preview-breadcrumb-geometry`, `q2-preview-breadcrumb-isolation`, `q2-preview-crumb-no-carry-expansion` + `q2-preview-spa/e2e/nesting-cursor.spec.ts`. (Flat specs are Plan 5's; `edit-cell-sizing` is Plan 6's; `render-components-{kanban,drag,comment}` stay green via Plan 1.) Shared helper: `ts-packages/preview-e2e-helpers/src/index.ts` (`assertNoReflowOnActivation`, `ACTIVE_REGION = '#q2-active-edit-region'`).

---

## Risks & notes

- **Geometry/no-reflow regressions are browser-only.** jsdom returns zero rects; the box-reproduction + breadcrumb-geometry contracts are only truly verified by Playwright. Do not declare Phase 2/3 done on vitest alone — gate on 6.4/6.5/7.2.
- **The override must keep emitting `#q2-active-edit-region`** or every `assertNoReflowOnActivation` caller breaks. The single most load-bearing DOM contract carried across the move — preserved by passing the measured `box` into `props.surface`.
- **Surface, not textarea (keystone §5).** The override renders `props.surface`; edge/caret navigation flows through `onEdgeReached`/`focus(caret)` on the `EditingSurfaceHandle`, never through `caretGeometry`. If you reach for `caretGeometry`, stop — that geometry is Plan 6's, internal to the textarea surface.
- **`EditBufferCache` is shared (keystone §7.1).** Seed every surface (outer and inner) with `editBufferCache.editableTextFor(node)`; never re-derive the clean-vs-raw predicate or read `nestedEditBuffersRef` directly. The descent-to-inner-surface is the *only* nesting-specific buffer behavior. The Rust generator (`regenerate_nested_buffers`/`write_single_block`) feeds `PushedEditBufferCache` via the parent push — unchanged backend.
- **Render-components coexistence (keystone §4, §8).** The demos must stay editable with NO mode active, because `useMode()` returns `NO_OP_MODE` whose baseline `resolveSource`/`commit` are core services. If a demo breaks, the bug is that something still routes editing through the (now-deleted) `PreviewContext`; fix the routing, do not re-add `PreviewContext`.
- **Boundary-splice (keystone §9).** Do NOT migrate `commit` here. Leave the `// keystone §9` note at the `DocumentStore.commit` call site.
- **Reconcile, don't double-delete.** Plan 1/Plan 5 remove the flat/shared machine and the textarea swap; this plan removes only the nesting-exclusive code. Confirm ownership before each Phase-5 deletion.
- **Provisional names (keystone §14–15).** `ViewController`, `useMode`, the manifest key may be renamed by find-replace; author against provisional names and keep them greppable.
- **vitest discovery of `resources/extensions/…`.** The extension dir sits outside `ts-packages`; 6.2 must make the renderer's vitest project discover it. The one piece of infra novelty in an otherwise pure relocation — pin it down early.

---

## References

- Keystone: `claude-notes/designs/2026-06-20-editing-mode-contract.md` (rev. 2026-06-21; §4 seams, §5 `EditingSurface`, §6 `ModeApi`/`useMode`, §7 core services incl. `EditBufferCache`, §8 render-components, §9 commit/boundary-splice, §10 selection/settings).
- Epic: `claude-notes/plans/2026-06-20-editing-mode-epic.md` (inter-plan interface; test-surface ownership split; verification gates).
- Source today: `ts-packages/preview-renderer/src/q2-preview/{nestingNav.ts,BreadcrumbChip.tsx,useBlockEditHover.tsx,dispatchers.tsx,PreviewRoot.tsx,PreviewContext.tsx,outerBlocks.ts,caretGeometry.ts}`.
- Settings wiring: `hub-client/src/services/preferences/schema.ts`, `hub-client/src/components/tabs/SettingsTab.tsx`, `q2-preview-spa/src/PreviewApp.tsx`.
- Backend (unchanged): `crates/pampa/src/{regenerate_nested_buffers.rs,apply_node_edit.rs,node_lookup.rs}`, `crates/pampa/src/writers/qmd.rs` (`write_single_block`), `crates/wasm-quarto-hub-client/src/lib.rs`.
- Bundled-extension precedent: `resources/extensions/quarto/{kbd,video,lipsum,version,placeholder}/`.
- E2E helper: `ts-packages/preview-e2e-helpers/src/index.ts`.
- Skills: `prevalidating-test-seams`, `fail-on-revert` (mandatory for this plan).
- Rules: `.claude/rules/integration-tests.md`, `.claude/rules/cross-platform.md`.
