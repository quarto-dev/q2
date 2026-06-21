# Editing-Mode Plan 5 — Block-Editing as the Recommended-First Bundled Editing-Mode Extension (flat)

**Date:** 2026-06-20 (rewritten 2026-06-21 — the previous draft was written on a WRONG premise: it instructed dropping the clean-buffer fallback for the flat mode. That is wrong. Flat block-editing edits a blockquote/list as its *outermost* surface and MUST seed from the generated AST serialization, or it re-introduces the G19 prefix-pollution bug. This rewrite consumes the keystone's `EditBufferCache` exactly like nesting-cursor.)
**Branch:** `editing-mode` (worktree `.worktrees/editing-mode`)
**Layer:** TypeScript (`ts-packages/preview-renderer`, a new bundled extension dir, `hub-client`, `q2-preview-spa`)
**Status:** PLAN — ready for TDD execution.
**Depends on:** Plan 1 (core services incl. **`EditBufferCache`** + depollute renderer + seams + `EditingSurface` + in-tree `TextareaSurface` + the mode-agnostic self-heal primitive) and Plan 2 (two-axis extension type + discovery + delivery + selection). **Keystone:** `claude-notes/designs/2026-06-20-editing-mode-contract.md` (rev. 2026-06-21, esp. §5 `EditingSurface`, §7 + §7.1 `EditBufferCache`, §12 the matrix) — **the keystone wins on any conflict**. Epic index: `claude-notes/plans/2026-06-20-editing-mode-epic.md` ("Plan 5"). **Sibling:** Plan 4 (`2026-06-20-editing-mode-plan-4-nesting-cursor-extension.md`) — packaging, test-split, harness, and vitest-discovery conventions are deliberately kept consistent with it.

---

## Overview

This is the **proof-of-contract** plan and the **recommended first** of the two bundled modes (epic: "build Plan 5 first"). It re-expresses the **pre-nesting-cursor block-editing feature set** — flat, in-place block editing — as a bundled **editing-mode** extension `block-editing` that:

- plugs into Plan 1's mode seams (one `NodeOverride` + one `ViewController`),
- renders the mode's **selected `EditingSurface`** (NOT a hardcoded textarea — keystone §5),
- consumes Plan 1's core services + the mode-agnostic **self-heal/concurrency** primitive,
- **AND consumes Plan 1's `EditBufferCache.editableTextFor(node)`** to seed the editable buffer — a *raw source slice* for flush-left blocks, the **generated AST serialization** for prefixing containers (blockquote/list/def-list). This is the **buffer fix the previous draft omitted**, and it is the whole reason this rewrite exists.

`block-editing` is the **simpler sibling** of `nesting-cursor` (Plan 4): same seam shape, same shared primitives, **same `EditBufferCache` consumption** — but **explicitly no tree-awareness**. It edits exactly **one** surface (the outermost block) and never descends. It exists to validate the contract — and especially the **shared-primitive rule** (keystone §1, §12: "two consumers ⇒ it's a primitive") — with the full self-heal/concurrency feature set AND the generated-buffer seeding, *before* Plan 4 layers nesting (descent into inner surfaces) on top of the same primitives.

> **CRITICAL FRAMING — this is NOT a code revert.** The feature *set* is the
> *pre-nesting* one (commits `015c5d98c` layout-stable wrapper/a11y/touch,
> `439ea2383` byte-offset identity + self-heal, `bc892248f` cross-surface
> arrows, plus `s6`/`s7`), but it is re-expressed on the *new* seam +
> `EditingSurface` + `EditBufferCache` architecture and **keeps**:
> - the **self-heal / concurrency** fixes (`439ea2383`'s byte-offset identity +
>   re-anchor, the P2.3b commit-suppression guard),
> - **the indented-block clean-buffer seeding** (`seedForRange`'s
>   `nestedEditBuffers[siKey] ?? anchorSlice` fallback + `editBaseline`'s
>   single-source-of-baseline rule — G19/Layer-2), now reached through the
>   `EditBufferCache.editableTextFor` port.
>
> We drop ONLY the **nesting-specific** additions (commits from `3347147ba`/
> `33a638131` onward): nesting nav keys (Cmd/Alt+Arrow in/out), the breadcrumb
> chip, **descent into inner surfaces**, the `unlockNestingCursor` *setting*, and
> the unlock branches inside the shared activation/roving code.
>
> **What we do NOT drop (the rewrite's correction):** the previous draft told
> the implementer to "Remove the buffer-aware seeding's nesting fallback:
> `seedForRange(..., ctx?.nestedEditBuffers)` becomes plain `anchorSlice`
> seeding." **DO NOT do that.** A blockquote/list edited as the outermost flat
> surface carries the `> `/indent prefix in its raw source slice; seeding the
> editor with the raw slice and baselining against it reads an untouched editor
> as dirty and round-trips the prefix back into the document. The generated
> serialization (clean buffer) is required for *any* prefixing block, flat or
> nested. The `EditBufferCache` is the shared service that makes the right
> choice (generated-vs-raw) for both modes.

Per the keystone (§3, §4, §5, §12), block-editing repackages as **exactly one** state-predicated `NodeOverride` plus **one** `ViewController` with **no** `renderOverlay`:

| Keystone capability | Source (flat subset) today | What it becomes in `block-editing` |
|---|---|---|
| `NodeOverride` (active target → **selected surface**) | `dispatchers.tsx` `isBlockEditTarget` gate (`:109-121`) + `renderMeasuredEdit` (`:60-83`) + `EditTextarea` (`:137-484`) + swap in `Block`/`CustomBlock` | one `NodeOverride { matches, render }`; `matches` = "this node is the active flat edit target"; `render` = the **selected `EditingSurface`** (from `ViewControllerProps.surface`) with `value = editBufferCache.editableTextFor(node)`, `box`/`initialCaret` from the shared geometry primitives, `onCommit → DocumentStore.commit`, `onEdgeReached → mode.requestMove`. **NOT a hardcoded textarea.** |
| `ViewController.handleInput` | `useBlockEditHover.tsx` activation (hover/click/touch/keyboard roving) **with the `unlockNestingCursor` branches removed** + the cross-surface arrow machine's `requestMove` entry (`PreviewRoot.tsx`) | root input handlers via Plan 1 `NodeLocator`; flat roving (`enumerateOuterBlocks`, **not** `enumerateNestingSurfaces`) + cross-surface arrows. **NO nesting keys.** |
| `ViewController.renderOverlay` | — (no breadcrumb in the flat set) | **OMITTED** — `block-editing` contributes no `renderOverlay`. |
| `ViewController.exposeHook` | the **flat** edit + self-heal + cross-surface-nav state machine in `PreviewRoot.tsx` + the flat commit paths | the `ModeApi` from `exposeHook()`: baseline `{ resolveSource, commit }` (core) + flat extras (`editTarget`, the refs the surface reads, `requestMove`, `requestFocusRestore`, `requestClickSwitch`, `handleClickSwitchBlur`, `cancelPendingLand`, expand state). **NO nesting state** (`requestNestingMove`/`requestNestingSelect`/`leafAnchorR0`/`unlockNestingCursor`). |
| editable-buffer source | `seedForRange(range, content, ctx?.nestedEditBuffers)` (`outerBlocks.ts:684`) + `editBaseline` (`:706`) | **`editBufferCache.editableTextFor(node)`** (Plan 1 `EditBufferCache`) — generated-or-raw, the SAME service nesting-cursor calls; block-editing just calls it for the ONE outermost surface, not for inner surfaces. |
| settings | — (pre-nesting block editing was simply *on* when the host allowed edits) | **NONE declared.** See "Decision: settings" below. |

The vanilla renderer left behind must be **pure** (`node → React`). Plan 1 owns making that true; **Plan 4 owns deleting the in-tree implementation** as the *nesting* mode lands. **Plan 5 does NOT re-delete the in-tree machine** — by the time Plan 5 runs (after Plan 1, in parallel with / after Plan 4), the shared primitives are already lifted by Plan 1 and the in-tree controller's removal is Plan 4's job. Plan 5's job is to **author the flat extension from the shared primitives + `EditBufferCache`, render the selected surface, and re-point the shared/flat pinning tests at it.** (See "Boundary with Plan 1 and Plan 4".)

**The Rust/WASM backend is unchanged.** `crates/pampa/src/{apply_node_edit.rs,node_lookup.rs}`, `crates/pampa/src/writers/qmd.rs`, and `crates/wasm-quarto-hub-client/src/lib.rs` stay as-is; the flat extension calls them through the same `commit` path `PreviewRoot`/`PreviewApp` use today. `regenerate_nested_buffers.rs` (`crates/pampa/src/regenerate_nested_buffers.rs`) is the **generator** that produces the clean buffers `EditBufferCache` serves — it stays as-is (Plan 1 owns wrapping it behind the cache's population port). The boundary-splice migration (keystone §9) is **noted but not performed** here.

---

## ✅ Resolved — outermost-container clean buffer = option B (READ FIRST)

**Decision (user, 2026-06-21): option B.** `EditBufferCache` owns the
outermost-container de-prefixing **in TS** — the mechanism already in production
since mid-June, confirmed working. **Option A (a `pampa` change) is NOT taken;
Plan 5 stays pure-TS.** The analysis below is retained as rationale; Phase 0.2a
just *verifies* the cache returns the de-prefixed buffer (option C collapses to
"consume it" if Plan 1 already wired it).

**What we read.** `regenerate_nested_buffers_ast` (`crates/pampa/src/regenerate_nested_buffers.rs:42-203`) emits a clean buffer ONLY for a block that **has a prefixing ancestor** — it sets `has_prefixing_ancestor = true` for the *children* of a BlockQuote/BulletList/OrderedList/DefinitionList, and `maybe_emit` fires only when that flag is already true (`:85-87`). So today the map is keyed by the **content block inside** the container (e.g. the paragraph inside the blockquote), keyed `0:<r0>-<r1>:0` on that inner block's range. **It does NOT emit a buffer for the outermost container itself** (the `BlockQuote`'s own `(r0,r1)` range carrying the `> ` prefix).

**Why that matters for flat block-editing.** Nesting-cursor descends, so its *active* surface is usually that inner prefixed block — the map already has its key. Flat block-editing does **not** descend: its active surface for `> quote\n> text` is the **`BlockQuote` block itself**, whose raw slice is `> quote\n> text`. `EditBufferCache.editableTextFor(blockQuoteNode)` must return the de-prefixed `quote\ntext` — but the current generator has **no entry** for the container's own range, so a naive `nestedEditBuffers[siKey] ?? anchorSlice` lookup falls through to the polluted raw slice. That is exactly the G19 bug.

**Resolution options (CHOSEN: B; A rejected — kept as rationale):**
- **(A) Backend extension — NOT TAKEN (rejected 2026-06-21).** Extend `regenerate_nested_buffers_ast` so it ALSO emits a clean buffer for the **outermost prefixing container itself** (emit on entering a prefixing container, keyed by the container's `(r0,r1)`), not only its prefixed descendants. This makes `EditBufferCache.editableTextFor` correct for the flat outermost surface by the same keyed-lookup path both modes use. **This is a Rust change to a `pampa` source file**, so it pulls the full WASM/`cargo xtask verify` gate in and needs the TDD backstop (`regenerate_nested_buffers_tests`). If chosen, it is a small, well-scoped backend task — but it means Plan 5 is NOT pure-TS. **Flag it to the user before starting** (CLAUDE.md: a Rust change here means the scope widened).
- **(B) `EditBufferCache`-side generation.** Have `EditBufferCache.editableTextFor` compute the de-prefixed buffer for an outermost prefixing container itself (the cache's "generated" branch) rather than relying solely on the pushed map — i.e. the cache, not the backend walk, owns the outermost-container case. This keeps Plan 5 pure-TS but moves the generated-vs-raw predicate's outermost case into Plan 1's `EditBufferCache` (where the keystone §7.1 already says "the generated-vs-raw decision is a single shared predicate the cache and `DocumentStore.commit` both consult"). **This is the cleanest fit with the keystone** and is the default unless Plan 1 says otherwise.
- **(C) Confirm it is already covered.** If Plan 1's `EditBufferCache` work (or a backend change Plan 4 already landed for nesting) already emits/handles the outermost container, this collapses to "consume it." **Phase 0.2a verifies this empirically** before any code.

**Whichever option:** the `DocumentStore.commit` re-wrap (keystone §9: "re-wraps prefixing containers on commit") must be the exact inverse of whatever `editableTextFor` returns for the outermost container — the seed and the re-wrap **cannot diverge** (keystone §7.1). The commit re-wrap is Plan 1's `DocumentStore`; Plan 5 consumes it and the round-trip test (Phase 2a) is the proof they agree.

---

## Decision: settings — `block-editing` declares NO settings (justified)

The keystone (§10) says a mode *declares* its settings in `_extension.yml`; nesting-cursor declares `unlockNestingCursor`. **`block-editing` declares none**, and the manifest's `settings:` block is empty/absent. Justification, grounded in what we read:

1. **Pre-nesting block editing had no per-mode toggle.** It was simply *on*. The only gate is `editingDisabled` (`PreviewContext.tsx:154`, threaded from `q2-preview-spa/src/PreviewApp.tsx:1249` `editingDisabled={!state.allowEdit}` and the hub host). That is a **host-level read-only flag**, **not** a mode setting — it controls whether *any* editing mode is interactive at all, and lives in the host/selection layer. The mode being *selected* (keystone §10) is what turns it on; a redundant "enable block editing" setting would duplicate the host gate and the selection mechanism.

2. **`unlockNestingCursor` is the only existing setting, and it is nesting-specific.** It gates a *nesting* behaviour fork (descend-to-inner-surface vs edit-outer-block) AND the `regenerateNestedBuffers` WASM pass (`hub-client/src/services/preferences/schema.ts:22`, `ReactPreview.tsx:421,444-453` `computeNestedEditBuffers`). The flat mode has no descent fork. **NOTE the decoupling this rewrite forces:** today buffer regen is gated on `unlockNestingCursor`; under the `EditBufferCache` contract the generated buffer for a prefixing container must be produced **regardless of nesting mode**, because flat block-editing needs it too. The gating moves from "the nesting setting" to "the cache's generated-vs-raw predicate" — that decoupling is Plan 1's (`EditBufferCache`) responsibility; Phase 0.2a confirms the cache produces buffers with no nesting setting active. There is still nothing for a *flat-mode setting* to switch.

3. **Keeping the manifest setting-free is the cleaner proof.** The contract must support a tenant with zero settings (the minimal `q2 create extension editing-mode foo` template — keystone §2 — also has none). `block-editing` is that proof: a real, full-featured mode that declares no settings, validating that the `settings:` block is genuinely optional in Plan 2's schema.

**Consistency with Plan 4:** Plan 4 declares `unlockNestingCursor`; Plan 5 declares nothing. This asymmetry is intentional — two sibling tenants, one with a setting and one without, both on the same manifest shape (the contract-generalization point).

---

## Decision: extension packaging & location (consistent with Plan 4)

**Decision: ship `block-editing` as a bundled editing-mode extension at `resources/extensions/quarto/editing-mode-block-editing/`** — the **sibling** of Plan 4's `editing-mode-nesting-cursor/` — authored in TSX, delivered through Plan 2's discovery + the existing `customComponentsCode` / `LOAD_CUSTOM_COMPONENTS` rail.

This mirrors Plan 4's packaging verbatim, for the same grounded reasons:

1. **One rail, two front doors (keystone §8).** The delivery rail is `read paths → look up source → Babel-transpile → LOAD_CUSTOM_COMPONENTS → blob-import → merge`. Plan 2 adds extension-discovered `.tsx` as a second source feeding the same `customComponentsCode`. Author-style TSX discovered from an extension dir is the only artifact that flows through that rail — exactly the form the bundled `render-components` demos take. A `ts-package` would NOT be discovered, would NOT transpile through Babel-standalone, and would force a new path.

2. **`resources/extensions/quarto/` is the established home for bundled, version-controlled extensions** (existing: `kbd/`, `lipsum/`, `placeholder/`, `version/`, `video/`, each with `_extension.yml`). The External Sources Policy (CLAUDE.md) forbids `external-sources/`; bundling under `resources/` is the sanctioned pattern Plan 2's discovery already handles.

3. **`_extension.yml` is where the contribution is declared** (keystone §10). Co-locating TSX + manifest in one dir satisfies the contract even though `block-editing`'s `settings:` block is empty.

**Why a separate dir from `nesting-cursor`.** The two modes are **independent sibling tenants** (keystone §1, §12). At most one mode is selected at a time (keystone §10). Two dirs keep the selection mechanism a clean one-of-N pick and the "two independent tenants on one contract" proof literal. **The shared code does not live in either extension** — it lives in Plan 1's primitives / core services / `EditBufferCache` (the shared-primitive rule). So the two extension dirs are genuinely small and non-overlapping; nothing is duplicated.

**Boundary with Plan 2/3:** the *manifest schema* (the `editing-mode:` contribution key, the `settings:` block, discovery of the dir's `.tsx`) is **Plan 2's** to define (`crates/quarto-core/src/extension/{types.rs,discover.rs,read.rs}`). This plan **authors the manifest and TSX that conform to it** and marks the integration point (`PLAN-2-KEY`). If Plan 2's key name differs, a find-replace settles it (keystone §15.3).

Directory shape this plan creates (sibling of Plan 4's):

```
resources/extensions/quarto/editing-mode-block-editing/
  _extension.yml                 # editing-mode contribution; NO settings (Plan 2 schema)  [PLAN-2-KEY]
  src/
    index.tsx                    # extension entry: exports { nodeOverrides, viewController }
    blockEditingController.tsx   # the ViewController (handleInput + exposeHook; NO renderOverlay)
    activeTargetOverride.tsx     # the one NodeOverride (active flat target → SELECTED surface, seeded via EditBufferCache)
    useActivation.ts             # flat activation (from useBlockEditHover.tsx, unlock branches removed)
    flatEditStateMachine.ts      # flat edit + self-heal + cross-surface-nav machine (exposeHook body)
    __tests__/                   # relocated vitest pinning tests (see Phase 6) incl. the new indented-block round-trip
```

> **NOTE — shared primitives import boundary (identical to Plan 4).** A discovered/transpiled extension cannot reach into `preview-renderer`'s source tree. So `block-editing` imports the shared primitives off the renderer API surface (`window.__Q2_PREVIEW_RENDERER__`) and consumes the core services / `EditBufferCache` / selected `surface` via `ViewControllerProps`: the measure-and-set geometry, `caretGeometry`, `byteLineMap`, `sliceSource`, **the shared self-heal / re-anchor + delete-by-emptying + expand-on-edit helpers**, **`EditBufferCache.editableTextFor`**, and the **selected `EditingSurface` component** (`ViewControllerProps.surface`). **INTEGRATION POINT (Plan 1):** the exact accessor names + `ViewControllerProps` field names are Plan 1's to publish. Until known, import by the keystone names and mark `// PLAN1:`. **Do NOT reimplement self-heal, the buffer cache, or a surface — that is the entire point of the shared-primitive rule (keystone §1). Importing/duplicating any is a plan failure.**

---

## Boundary with Plan 1 and Plan 4 (read before starting)

The three plans touch overlapping code; the division is exact:

- **Plan 1** lifts the core services (`SourceResolver`/`NodeLocator`/`DocumentStore`/`OverlaySlot`/**`EditBufferCache`**), the `EditingSurface` contract + the in-tree `TextareaSurface`, wires the `NodeOverride` super-chain + single root `ViewController` + `useMode()`, and **factors the shared, mode-agnostic helpers** (activation/hover/cross-surface arrows, byte-offset identity + **self-heal/re-anchor**, delete-by-emptying, expand-on-edit, **`editableTextFor`**) out of the in-tree bundled controller and onto the renderer API surface / core services (Plan 1 Task 5.3 "two-mode factoring requirement"; epic "Plan 1 produces": `EditBufferCache` interface + `PushedEditBufferCache` + `acceptPushedBuffers`). Plan 5 **consumes** these; it does not derive them.

- **Plan 4** moves the *nesting* mode into its extension and **deletes the in-tree implementation** (`PreviewRoot` edit machine, `useBlockEditHover`, `BreadcrumbChip`, `nestingNav`, the `PreviewContext` edit fields, the `unlockNestingCursor` hub wiring). Plan 5 does **not** repeat those deletions. Plan 4 also consumes `EditBufferCache` — for **inner** surfaces (descent); Plan 5 for the **one outermost** surface only.

- **Plan 5** (this plan) authors the *flat* extension purely by **composing Plan 1's shared primitives + `EditBufferCache` + the selected surface** into one `NodeOverride` + one (overlay-free) `ViewController`, and **re-points the shared/flat pinning tests** (plus the NEW indented-block round-trip) at the flat extension path. Plan 5 owns no in-tree deletion.

**Both-orderings contingency (recommended 5-first).** If Plan 5 runs **before** Plan 4:
- Plan 1's shared helpers + `EditBufferCache` + `TextareaSurface` exist on the renderer API surface / `ViewControllerProps` (Plan 1 is a hard dep — Phase 0 confirms or STOPs). The in-tree bundled controller still exists (Plan 1's temporary scaffolding); fine — Plan 5 selects the `block-editing` extension *instead of* the in-tree fallback via Plan 2's selection, and the e2e fixtures choose `block-editing`. The in-tree controller is Plan 4's to delete.
- The shared/flat pinning tests are **moved** from `q2-preview/` into the `block-editing` dir and re-pointed. When Plan 4 later runs it must **not** re-move these. Phase 6 records exactly which files Plan 5 claimed so Plan 4 reconciles rather than double-moves.

If Plan 5 runs **after** Plan 4: the in-tree machine is already deleted; Plan 5 still authors its own flat extension from the shared primitives (it does NOT depend on Plan 4's extension), and re-points the shared/flat tests (which by then live in Plan 4's dir or `q2-preview/` — Phase 0 records, Phase 6 moves into `block-editing/`). **In either order, the shared/flat tests end up owned by `block-editing`.**

> **`prevalidating-test-seams` / `fail-on-revert` are mandatory** (epic: heaviest compose-and-keep-green plan after Plan 4). Invoke `prevalidating-test-seams` when prepping each relocation/authoring step's test phase; invoke `fail-on-revert` after each step to prove the relocated test **binds to the `block-editing` extension code** (and, for self-heal and the buffer round-trip, to Plan 1's *primitive*), not a leftover in-tree path or a stale `nesting-cursor` copy.

---

## Global Constraints (bake into every step)

- **TDD, test-first.** Every step writes/relocates the failing test BEFORE authoring/moving code, watches it fail for the *intended* reason, then writes the minimal code to green, runs the relevant suite, commits. One bite-sized step at a time.
- **No behaviour change to the flat feature set.** This is a re-expression + relocation, not a redesign. Every relocated test keeps its current assertions; if an assertion's expected *value* must change because the seam moved (an import path, `ctx.X` → `mode.X`, or `seedForRange(...)` → `editBufferCache.editableTextFor(node)`), pre-validate with `prevalidating-test-seams` and record the revert hunk it binds to. **Keep self-heal AND buffer-seeding behaviour byte-identical** — only their *invocation sites* move (both are Plan 1 primitives the extension consumes).
- **Drop only the nesting subset; KEEP the buffer fix.** When porting `useBlockEditHover`, remove the `unlockNestingCursor`/`unlockNestingCursorRef` branches and the `enumerateNestingSurfaces` roving fork — the flat mode always uses `resolveOuterBlock` + `enumerateOuterBlocks`. When porting the cross-surface-nav machine, drop the `nest`/`crumb` `ResolverSpec` kinds; keep `open`/cross-surface landing. **But the buffer seeding is NOT a nesting branch — it is shared.** Replace `seedForRange(range, content, ctx?.nestedEditBuffers)` with `editBufferCache.editableTextFor(node)` (the port over the same `nestedEditBuffers[siKey] ?? anchorSlice` logic), and baseline via the shared `editBaseline` rule (now inside the cache/`DocumentStore`). **If you find yourself wanting to drop the clean buffer "because it looks like nesting," STOP — re-read the framing block; the prefix-stripping is required for the flat outermost surface too.**
- **Consume Plan 1/Plan 2 by keystone name.** `NodeOverride`, `ViewController`, `ViewControllerProps` (incl. `.surface`, `.editBufferCache`, `.overlaySlot`), `EditingSurfaceComponent`/`EditingSurfaceProps`/`EditingSurfaceHandle`, `ModeApi`, `useMode()`, `ModeContext`, `NO_OP_MODE`, `SourceResolver`, `NodeLocator`, `DocumentStore` (`.commit`), `OverlaySlot` (provided, unused), `EditBufferCache` (`editableTextFor`), and the textarea/`caretGeometry`/`byteLineMap`/`sliceSource` + self-heal/delete/expand shared helpers. Where Plan 1's exact export path is not yet known, reference by name and mark `// PLAN1: <name>` / `// PLAN2: <name>` / `PLAN-2-KEY`. **Do not duplicate any — importing a duplicate (especially self-heal or the buffer cache) is a plan failure.**
- **`commit` shape + re-wrap.** Use `DocumentStore.commit` (keystone §9 v1: wraps today's `commitTextEdit(destJson, text)` / `commitSubtreeEdit(destJson, block)`). The commit path owns the **re-wrap** of a generated buffer back into its prefixing container (the inverse of `EditBufferCache`'s generation — keystone §7.1, §9). Do NOT migrate to boundary-splice here. Note the future swap with a `// keystone §9` comment at the call site.
- **No `renderOverlay`.** The `block-editing` `ViewController` returns `{ handleInput, exposeHook }` and **omits** `renderOverlay` (keystone §3.2 types it optional). `OverlaySlot` is still provided via `ViewControllerProps`; the controller never paints into it.
- **Selected surface, not hardcoded textarea.** The override renders `props.surface` (the selected `EditingSurface`) with `value`/`box`/`onCommit`/`onEdgeReached`. Cross-surface arrows go through the surface's `onEdgeReached(dir)` → `mode.requestMove(dir)` (keystone §5: geometry/edge detection is the surface's job, not the mode's). The mode never assumes textarea. In the both-orderings reality where Plan 6 hasn't extracted the surface yet, the **in-tree `TextareaSurface`** (Plan 1) is the default selected surface — Phase 0.2 confirms it is reachable via `ViewControllerProps.surface`.
- **VFS path convention.** `/project/` prefix (CLAUDE.md), honored if any path is constructed.
- **Verification gates (CLAUDE.md).** This plan touches TS render paths and (via the extension dir + Plan 2 manifest) `quarto-core`-reachable resources; if the flagged integration point resolves to **option (A)** it also touches `pampa` Rust. The **full** gate applies before declaring done:
  - `cargo build --workspace` and `cargo nextest run --workspace` (the Rust backstop tests live in `pampa`).
  - `cargo xtask verify` (full — WASM leg in scope because `wasm-quarto-hub-client` is the render engine; mandatory if option (A) changes `pampa`).
  - `cd hub-client && npm run build:all` — the production `tsc -b && vite build` is stricter than `tsc --noEmit`/`vitest`.
  - vitest: `cd ts-packages/preview-renderer && npx vitest run` (never pipe through `tail` — CLAUDE.md). Run the extension's `__tests__/` under the renderer's vitest project (Phase 6.2).
  - Playwright e2e (both hosts): hub-client `e2e/q2-preview-*.spec.ts` and `q2-preview-spa/e2e/`. Geometry-dependent behaviour is browser-only.
  - **Stale-WASM trap (CLAUDE.md "Verifying Rust changes in `q2 preview`"):** for any `q2 preview` end-to-end check, run `cd hub-client && npm run build:wasm` → `cargo xtask build-q2-preview-spa` → `cargo build --bin q2` first. If option (A) changed `pampa`, the WASM rebuild is **mandatory** (the cache's generated buffers come from the rebuilt WASM); otherwise it is still needed if SPA bundling of the extension changed. Honor the `q2 mcp` bundle trap if any TS the MCP bundle consumes is touched (it is not, here).
  - **hub-client changelog (two-commit workflow).** Any commit touching `hub-client/` MUST add an entry to `hub-client/changelog.md` in a *second* commit carrying the first commit's short hash. Plan 5 touches `hub-client/` only in the e2e-fixture selection step (Phase 6.4) and possibly a host selection toggle (Phase 5).
  - **Integration-test layout rule.** Rust tests stay in `crates/pampa/tests/integration/` + `main.rs` (`.claude/rules/integration-tests.md`). We add a backstop test only under option (A) (extend `regenerate_nested_buffers_tests`); we do not move existing Rust test files.
- **Do not gold-plate.** Re-express what exists in the flat set; do not add features. If you want a hack or a TODO that undoes work, STOP and ask (CLAUDE.md).

---

## Consumes / Produces (inter-plan interface)

**Consumes (from Plan 1):**
- Seam types/host: `NodeOverride`, `ViewController`, `ViewControllerProps`, `ModeApi`, `ModeContext`, `useMode()`, `NO_OP_MODE` — `// PLAN1:` module path (epic: `ts-packages/preview-renderer/src/framework/mode/`); the `ActiveMode = { viewController, nodeOverrides, settings }` binding + `activeMode?` root prop (the seam Plan 2's selection + this plan plug into).
- **`EditingSurface` contract** (`EditingSurfaceComponent`/`EditingSurfaceProps`/`EditingSurfaceHandle`) + the in-tree **`TextareaSurface`** reference impl (`caretGeometry` internal to it), reachable via `ViewControllerProps.surface`. The override renders the **selected** surface, never a hardcoded textarea.
- **`EditBufferCache`** (`editableTextFor(node): string`) via `ViewControllerProps.editBufferCache` — the generated-or-raw editable-buffer port (keystone §7.1). Population (`acceptPushedBuffers` ← parent `regenerateNestedBuffers`) is **off** the interface and is Plan 1/host plumbing; Plan 5 only calls `editableTextFor`.
- Core services: `SourceResolver`, `NodeLocator` (incl. self-heal/re-anchor), `DocumentStore.commit` (incl. the prefixing-container re-wrap), `OverlaySlot` (provided, unused).
- Primitives + shared helpers on the renderer API surface: measure-and-set geometry, `caretGeometry`, `byteLineMap`, `sliceSource`, **and the mode-agnostic self-heal / re-anchor, delete-by-emptying, expand-on-edit, and activation/cross-surface-arrow helpers** lifted by Plan 1 Task 5.3.
- The dispatcher consulting a `NodeOverride` super-chain; the root mounting a single `ViewController`.

**Consumes (from Plan 2):**
- `_extension.yml` editing-mode contribution schema (`PLAN-2-KEY`) + discovery of the dir's `.tsx`.
- The Rust→iframe delivery channel merging the extension's components into `customComponentsCode`.
- The **two-axis selection** (active mode + active surface) that resolves the selected mode to an `ActiveMode` and the selected surface to `ViewControllerProps.surface`, feeding Plan 1's `activeMode?`/`surface` props. (For `block-editing` there are no declared settings to surface.)

**Produces (consumed by no later plan — leaf, sibling of Plan 4):**
- The bundled `editing-mode-block-editing` extension (manifest + TSX) = one `NodeOverride` (active → selected surface, seeded via `EditBufferCache`) + one (overlay-free) `ViewController` + **no** settings.
- Relocated **shared/flat** pinning tests (vitest in the extension dir) + the **NEW indented-block generated-buffer round-trip** test, all green and bound to the extension path + Plan 1's `EditBufferCache`.
- ~~(option A — rejected)~~ **No `pampa` change.** The outermost-container buffer is option B (cache-owned, pure-TS).

---

## Phases

> Ordering rationale (mirrors Plan 4): build the extension *additively* first (Phases 1–4) so the flat path exists and is tested before relocating the bulk of pinning tests (Phase 6). Phase 2a inserts the buffer-fix proof immediately after the override exists. Phase 5 wires selection; Phase 7 is end-to-end.

### Phase 0 — Pre-flight: confirm Plan 1 & Plan 2 landed; resolve the buffer integration point; baseline the flat suite

- [ ] **0.1** Confirm Plan 1's seam exports exist and are importable: grep `ts-packages/preview-renderer/src/framework/` for `NodeOverride`, `ViewController`, `ViewControllerProps`, `useMode`, `NO_OP_MODE`, `ModeContext`, `ActiveMode`, `EditingSurfaceComponent`/`EditingSurfaceProps`/`EditingSurfaceHandle`, and the core services `SourceResolver`/`NodeLocator`/`DocumentStore`/`OverlaySlot`/**`EditBufferCache`**. Record actual module paths into a `// PLAN1:` reference block at the top of `index.tsx`. **If absent, STOP** — blocked on Plan 1.
- [ ] **0.2** Confirm Plan 1's primitives + shared helpers are on `window.__Q2_PREVIEW_RENDERER__`: the measure-and-set geometry, `caretGeometry`, `byteLineMap`, `sliceSource`, AND the mode-agnostic **self-heal / re-anchor**, delete-by-emptying, expand-on-edit, and activation/cross-surface-arrow helpers (Plan 1 Task 5.3). Confirm the **in-tree `TextareaSurface`** is reachable as the selected surface via `ViewControllerProps.surface`. Record accessor names. **If the self-heal helper is NOT lifted (still buried in the in-tree controller), STOP and coordinate with Plan 1** — re-deriving it violates the shared-primitive rule.
- [ ] **0.2a (the buffer integration point — RESOLVE before any Phase-2 code).** Confirm `ViewControllerProps.editBufferCache.editableTextFor(node)` exists and decide the **outermost-container** case (see the flagged section above). Empirically, with a fixture `> quote line one\n> quote line two`:
  - Call `editBufferCache.editableTextFor(blockQuoteNode)` and assert it returns the **de-prefixed** `quote line one\nquote line two`, NOT the raw `> ...` slice.
  - If it already does (Plan 1 / a prior Plan-4 backend change covered the container case) → **option (C)**, consume it; record where it is handled.
  - If it returns the polluted raw slice → coordinate with Plan 1 to wire **option (B)** (the cache owns the outermost-container de-prefixing in TS, per keystone §7.1's "single shared predicate"). **Option (A) is rejected — do NOT add a `pampa` change; Plan 5 stays pure-TS.** Record confirmation in Notes. **Do NOT proceed to Phase 2 until the de-prefixed buffer for an outermost blockquote/list is verifiably available from `editableTextFor`.**
- [ ] **0.3** Confirm Plan 2's `_extension.yml` editing-mode contribution schema (`PLAN-2-KEY`) + the **two-axis** selection are in place: grep `crates/quarto-core/src/extension/types.rs` for the editing-mode contribution variant; grep the host for where a selected mode's `ActiveMode` + selected `surface` are installed into Plan 1's `activeMode`/`surface` props. Record the manifest key + selection install point + confirm the `settings:` block is OPTIONAL (so a setting-free manifest parses). **If absent, STOP** — blocked on Plan 2.
- [ ] **0.4** Sanity-baseline. Run `cd ts-packages/preview-renderer && npx vitest run` and `cargo nextest run -p pampa`. Record the current green set of the **shared/flat** pinning tests this plan owns (enumerated in **Test surfaces**) and **their current location** (some may already be in Plan 4's dir if Plan 4 ran first). Do not modify any test yet.

### Phase 1 — Scaffold the extension dir + entry

- [ ] **1.1 (test, then code)** Create `src/index.tsx` exporting the contract shape Plan 2 installs (`export const nodeOverrides: NodeOverride[]`, `export const viewController: ViewController` — exact field names per Plan 2's installer; mark `// PLAN2:`). Add `src/__tests__/index.smoke.test.ts` asserting both exports present, `nodeOverrides.length === 1`, and the `viewController` factory return value has `handleInput` and `exposeHook` but **no `renderOverlay`** (the flat distinction from Plan 4). Stub override/controller so the smoke test compiles; flesh out in Phases 2–4. Run → fail (module absent) → green.
- [ ] **1.2** Author `_extension.yml` per Plan 2's schema (`PLAN-2-KEY`): declare the editing-mode contribution (controller + overrides entry) with **no `settings:` block** (justified above). Mirror Plan 4's `editing-mode-nesting-cursor/_extension.yml` minus `settings:`. **INTEGRATION POINT (Plan 2):** key names. If Plan 2 globs `resources/extensions/quarto/*`, confirm this dir is picked up; if the discovery/parse test belongs to Plan 2, leave a `// PLAN2:` note + checklist item. Add a focused assertion that a **setting-free** manifest parses (block-editing's contract-proof contribution) — co-locate with Plan 2's discovery tests or note it for Plan 2.

### Phase 2 — The one `NodeOverride` (active flat target → selected surface, seeded via `EditBufferCache`)

- [ ] **2.1 (test)** Relocate the dispatcher swap-behaviour tests that pin "active edit target renders the editing surface, others render normally" into `src/__tests__/activeTargetOverride.integration.test.tsx`, expressed against the `NodeOverride` (`matches` + `render`) instead of `dispatchers.Block`. Source assertions: the narrow swap-predicate assertions in `useEditableBlock.integration.test.tsx` and `p2-3a.integration.test.tsx`, plus the box-reproduction assertions exercised via `s7-expand-on-edit.integration.test.tsx` (narrow swap subset only). They must FAIL first. `prevalidating-test-seams`: pre-validate that the only expected assertion changes are (a) import path, (b) `dispatchers.Block` → the override surface, (c) the rendered widget is "the selected surface" not "a textarea literal".
- [ ] **2.2** Author `src/activeTargetOverride.tsx`:
  - `matches(node, mode)` = the **flat** `isBlockEditTarget` gate (`dispatchers.tsx:109-121`) re-expressed against `mode` (the `ModeApi`): non-Opaque reachability AND `mode.editTarget?.anchorR0 === mode.resolveSource(node)?.sourceEntry.r[0]`. Use the **baseline** `mode.resolveSource` (core) for the lookup and the **mode extra** `mode.editTarget` for the active-target match (keystone §4: only this mode's own override may rely on extras). **No nesting/leaf branch** — flat identity only.
  - `render(node, renderDefault)` = render the **selected `EditingSurface`** (`props.surface` threaded through the controller, `// PLAN1:`), NOT a hardcoded textarea. It does **not** call `renderDefault()` (it *replaces* — keystone §3.1). Wire its props:
    - `value = editBufferCache.editableTextFor(node)` — **the buffer fix.** Generated serialization for a prefixing container, raw slice for a flush-left block; the cache decides. **Do NOT pass a raw `anchorSlice`.** (`// PLAN1: EditBufferCache`)
    - `box` from the measure-and-set geometry primitive (the no-reflow box), `id="q2-active-edit-region"` and `LEFT_INSET_STRIPPED_TYPES` handling (`dispatchers.tsx:40-42,73-77`) preserved so the no-reflow contract (`#q2-active-edit-region`, `ts-packages/preview-e2e-helpers`) holds. **This `id` is the single most load-bearing DOM contract carried across the move.**
    - `onCommit(text) → mode.commit` routed to **Plan 1's `DocumentStore.commit`** (which re-wraps the prefixing container — keystone §9). Add the `// keystone §9` boundary-splice note.
    - `onEdgeReached(dir) → mode.requestMove(dir)` (cross-surface arrows; the surface owns the geometry/edge detection — keystone §5).
    - `onCancel`/`onChange` to the controller's flat handlers.
  - The textarea-specific keystroke/caret logic that was inline in `EditTextarea` (`dispatchers.tsx:137-484`, **flat subset**) now lives **inside the surface** (keystone §5), not in the override. The override is thin: select surface, feed `value` from the cache, wire the four callbacks. Where the old `EditTextarea` reached a **nesting** handler (`requestNestingMove`/`requestNestingSelect`/`commitNestingEdit`), **it has no equivalent** — those are not in the flat `ModeApi`.
- [ ] **2.3** Run 2.1 to green. `fail-on-revert`: revert `activeTargetOverride.tsx`'s `matches` body and confirm the relocated test fails — proving it binds to the extension override, not a leftover dispatcher path or the in-tree fallback.

### Phase 2a — NEW: the indented-block generated-buffer round-trip (the fix this rewrite mandates)

This is the regression test the previous draft lacked. It proves block-editing seeds a blockquote/list from the **generated serialization** and **round-trips cleanly** on commit (no `> `/indent prefix doubling, no false-dirty).

- [ ] **2a.1 (test, FAIL first against a naive raw-slice override)** Add `src/__tests__/indented-block-buffer-roundtrip.integration.test.tsx`. Mount the vanilla root + the `block-editing` mode (use the Phase-4 harness `mountWithBlockEditingMode`; if Phase 4 hasn't built it yet, build a minimal harness here and have Phase 4 reuse it). Fixtures (one per prefixing type): a multi-line **blockquote** (`> line one\n> line two`), a multi-line **bullet-list item**, and a multi-line **definition-list** definition. Assert, for each:
  - **(seed)** Activating the outermost block opens the surface seeded with the **de-prefixed** generated text (`line one\nline two`), NOT the raw `> line one\n> line two`. Read the surface's `value` (or the `#q2-active-edit-region` content). This binds to `editBufferCache.editableTextFor`.
  - **(not false-dirty)** Without typing, the editor is **not dirty** — `editBaseline`/the cache baseline equals the seeded value, so a blur/commit is a no-op (no spurious write). (Pins the G19/Layer-2 invariant for the flat outermost surface.)
  - **(round-trip)** Type a small edit, commit, and assert the committed document re-wraps the prefix correctly (the `DocumentStore.commit` re-wrap is the inverse of the generation — keystone §9): the blockquote is still a blockquote, the prefix is present exactly once, and the edited text is inside it. Assert against the resulting AST/QMD payload `commit` produced (mirror the assertion style of `commit-destination-equivalence` / `node_edit_tests`).
  - Run → it must **FAIL** if the override seeds from the raw slice (proving the test actually constrains the fix), then **PASS** once `editableTextFor` + the re-wrap are wired (Phase 2.2 + the resolved 0.2a option). `prevalidating-test-seams`: confirm each assertion is bound to a concrete value (seeded text, dirty flag, committed payload), not a tautology.
- [ ] **2a.2 (`fail-on-revert`)** Revert the override's `value = editBufferCache.editableTextFor(node)` to `value = anchorSlice` (the previous-draft mistake) and confirm 2a.1's **seed** and **not-false-dirty** assertions go RED. This is the proof the extension genuinely consumes the buffer cache and that the fix is load-bearing. Restore.
- [ ] **2a.3 (`fail-on-revert`, re-wrap side)** Revert `DocumentStore.commit`'s prefixing-container re-wrap (or, if Plan 1 owns it untouchably, stub the override's `onCommit` to write the de-prefixed text raw) and confirm 2a.1's **round-trip** assertion goes RED (the prefix is lost / the blockquote degrades to a paragraph). This proves seed and re-wrap agree (keystone §7.1 "seed and re-wrap can't diverge"). Restore.

### Phase 3 — `ViewController.handleInput` (flat activation + cross-surface arrows; NO overlay, NO nesting keys)

- [ ] **3.1 (test)** Relocate `useBlockEditHover.integration.test.tsx` → `src/__tests__/useActivation.integration.test.tsx`, re-pointed at the flat activation module and driven through the `ViewController`'s `handleInput` rather than `useBlockEditHover()`'s `hostProps`. **Drop the unlock-mode assertions** (those move with Plan 4): keep mouse hover+click, touch hold, keyboard roving over `enumerateOuterBlocks`, the `editingDisabled` inert path (now: no mode active / read-only ⇒ no handlers), `HOLD_MS`, `MOVE_THRESHOLD_PX`, the latest-ref guards, the active-region guard. FAIL first. `prevalidating-test-seams`: pre-validate that the removed unlock assertions are genuinely nesting-specific (cross-check Plan 4's claimed set).
- [ ] **3.2** Author `src/useActivation.ts` from `useBlockEditHover.tsx`, re-expressed as `handleInput: RootInputHandlers` (keystone §3.2):
  - Replace every `ctx?.X` read with `mode.X` / props.
  - Replace `el.closest('[data-block-pool-id]')` / `resolveOuterBlock(el)` hit-testing with **Plan 1's `NodeLocator`**.
  - **Remove the `unlockNestingCursor` / `unlockNestingCursorRef` branches** in `activate` (`:67-69`), `onPointerMove` (`:169-173`), `onPointerDown` (`:197-199,224`), and `onKeyDown` roving (`:282-284` — always `enumerateOuterBlocks`, never `enumerateNestingSurfaces`).
  - **Buffer seeding (KEEP, via the cache):** the current `seedForRange({r0,r1}, content, ctx?.nestedEditBuffers)` (`useBlockEditHover.tsx:98`) becomes **`editBufferCache.editableTextFor(node)`** for the activated outermost block. **Do NOT collapse it to a raw `anchorSlice`.** Keep `captureEditTarget` identity extraction and `measureBlockBox` geometry (shared primitives). (`// PLAN1: EditBufferCache`)
  - Keep the flat expand-on-edit capture (`captureGeometry`, `s7`); drop only nesting-subtree concerns (if `captureGeometry` has a nesting-subtree branch, gate it out; if uncertain, treat as a spike pinned by `s7`).
  - Preserve the three activation paths (mouse hover+click, touch hold, keyboard roving), the cross-surface click-switch (`requestClickSwitch`), and the `editingDisabled` inert return.
  - FAIL→green.
- [ ] **3.3 (test)** Relocate the **cross-surface arrow** pinning tests (flat pre-nesting arrow behaviour, `bc892248f`): `p2-4-real.integration.test.tsx`, `p2-4d.integration.test.tsx` (+ the flat subset of `p2-4b` — `open`/cross-surface landing; the `nest`/`crumb` cases are Plan 4's). Move into `src/__tests__/`. Re-point at the flat extension path (mount the vanilla root + `block-editing` `ViewController` via the Phase-4 harness). FAIL first. `prevalidating-test-seams`: split `p2-4b` flat vs nesting at the case level.
- [ ] **3.4** Wire cross-surface arrows: the surface's `onEdgeReached(dir)` → `mode.requestMove(dir)` (Phase 4 holds the machine; this wires keystroke/edge → `mode.requestMove`). Drop the `nest`/`crumb` keystrokes (Cmd/Alt+Arrow in/out) entirely. FAIL→green.

### Phase 4 — `ViewController.exposeHook` (flat edit + self-heal + cross-surface-nav state machine + commit)

The **flat subset** of the editing machine in `PreviewRoot.tsx`. Non-edit parts (note-numbering, link handlers, scroll, AST parse) stay in the vanilla root; nesting parts (breadcrumb, nesting nav, **descent**) are NOT moved here (Plan 4's).

- [ ] **4.1 (test)** Build the harness `src/__tests__/mountWithBlockEditingMode.tsx` (sibling of Plan 4's `mountWithNestingMode.tsx`): mount the vanilla root + install the `block-editing` extension into Plan 1's `activeMode` seam with the in-tree `TextareaSurface` selected + a `PushedEditBufferCache` seeded for the fixtures (so `editableTextFor` returns generated buffers in jsdom — `acceptPushedBuffers` with a precomputed map, since the iframe has no WASM; keystone §7.1). Then relocate the **shared/flat** state-machine pinning tests to mount through it: `self-heal-on-write.integration.test.tsx`, `p2-3b-real.integration.test.tsx`, `s6-delete-by-emptying.integration.test.tsx`, `s7-expand-on-edit.integration.test.tsx`, `s4-dirty-caret-col.integration.test.tsx` (if flat), and flat glitch/identity rounds not nesting-tied (`g19-spurious-dirty` if flat — **note `g19` is the false-dirty-from-prefix test; it is squarely the buffer-fix territory, so confirm it stays flat-owned and binds to the cache baseline**). FAIL first. `prevalidating-test-seams` per file: confirm each is genuinely flat (no breadcrumb/nesting-nav/unlock/descent assertions).
- [ ] **4.2** Move the **flat** edit + self-heal + cross-surface-nav machine into `src/flatEditStateMachine.ts` + `src/blockEditingController.tsx`. Map each flat `PreviewRoot` member to its new home (exposeHook returns `ModeApi`):
  - **Self-heal / re-anchor is a Plan 1 PRIMITIVE — import it, do not re-derive it** (`// PLAN1:`). The controller wires the imported self-heal helper to its `editTargetRef`/`editDraftRef` + `NodeLocator.findReanchorCandidate`; it does NOT contain a second copy of the self-heal layout effect.
  - **Buffer baseline is the cache/`DocumentStore`'s, not a local copy.** The dirty guard baselines against the **seeded generated buffer** (the cache's `editableTextFor` value), via the shared `editBaseline` rule (keystone §7.1 single predicate). Do NOT re-derive a raw-slice baseline. (`// PLAN1:`)
  - State held via `useState`/`useRef` (keystone §3.2): `editTarget`, `editDraftRef`, `editExpandedRef`, `editTargetRef`, `activeEditRegionRef`, `editGeometryRef`, `pendingLandingRef`, `pendingCaretRef`, fade/settle-gate refs, `clickSwitchRef`. **NOT** `sourceIndexRef` (→ Plan 1 `SourceResolver`), **NOT** `nestedEditBuffersRef`/`unlockNestingCursorRef`/`leafAnchorR0` (nesting/Plan-4 mirrors; the buffer access is now `editBufferCache.editableTextFor`, not a local `nestedEditBuffersRef`).
  - Pure machine functions moved verbatim (behaviour-preserving), **flat subset only**: `setEditTarget`, `openEditTarget`, `resolveLanding` (with `nest`/`crumb` spec kinds removed — keep `open`/cross-surface), `openFromResolved`, `executeLanding`, `armRelandBackstop`, `requestMove`, `cancelPendingLand`, `requestFocusRestore`, `requestClickSwitch`, `handleClickSwitchBlur`, `readLiveCaret`, `commitAndArmReland`, `cleanCaretHint`, `captureGeometry` (flat expand-on-edit subset), the reland layout effect, the fade layout effect. **Drop** `commitNestingEdit`, `applyNestingRetarget`, `requestNestingMove`, `requestNestingSelect`, and the nesting branches of `resolveLanding`/`buildNestingCommitDestination`.
  - Commit entry points (`commitTextEdit`/`commitSubtreeEdit` + `setAst` payloads) route through **Plan 1's `DocumentStore.commit`** (keystone §9 v1 shape; owns the prefixing-container re-wrap). Add the `// keystone §9` note. **Delete-by-emptying** (`s6`): keep its three-way `commitIfDirty` guard byte-identical, sourcing the baseline from the shared `editBaseline` (cache) so it agrees with the generated seed; it is a shared primitive Plan 1 may have lifted (`// PLAN1:` if so, else port verbatim and flag for Plan 1 to lift so Plan 4 shares it).
  - `exposeHook()` returns the **flat** `ModeApi`: baseline `{ resolveSource: SourceResolver, commit: DocumentStore.commit }` PLUS flat extras (`editTarget`, the refs the surface reads, `requestMove`, `requestFocusRestore`, `requestClickSwitch`, `handleClickSwitchBlur`, `cancelPendingLand`, `captureGeometry`, expand state). **No nesting extras.** Only this mode's own NodeOverride/handlers may read extras (keystone §4).
  - The `ViewController` returns `{ handleInput, exposeHook }` — **no `renderOverlay`**.
- [ ] **4.3** Run 4.1 + 3.3 + 2a.1 to green, iterating. `fail-on-revert` on a representative subset (self-heal, delete-by-emptying, expand-on-edit, one cross-surface arrow, **the buffer round-trip**) to prove the relocated tests bind to the controller + Plan 1 primitives, not a residual `PreviewRoot` path. **Critically: revert the imported self-heal helper's wiring → `self-heal-on-write` REDs; revert the `editBufferCache` wiring → `indented-block-buffer-roundtrip` REDs.** Both prove the extension composes Plan 1's primitives rather than leftover in-tree effects.

### Phase 5 — Selection wiring (host picks `block-editing` + the default surface)

Plan 5 owns no in-tree deletion (Plan 4 does). It owns making the host able to *select* `block-editing`.

- [ ] **5.1** Confirm Plan 2's two-axis selection resolves `editing-mode-block-editing` → `ActiveMode` and a selected `surface` → `ViewControllerProps.surface`, setting Plan 1's `activeMode`/`surface` props. **INTEGRATION POINT (Plan 2):** the selection UI/config. If Plan 2 surfaces a one-of-N mode picker + a surface picker, confirm `block-editing` appears and selecting it installs the override + controller with the (default) `textarea` surface. If not yet wired, leave a `// PLAN2:` note + a thin host shim selecting `block-editing` + `textarea` for the e2e fixtures, rather than orphaning the path.
- [ ] **5.2** No `unlockNestingCursor`-style setting to wire (block-editing has none). Confirm the host does **not** require a settings control for `block-editing` (empty `settings:`) and that Plan 2's settings host tolerates a setting-free mode (keystone §10). If host code assumes every mode has ≥1 setting, file a `// PLAN2:` note (a Plan 2 bug). Apply the **two-commit changelog** workflow if any `hub-client/` file changes here.

### Phase 6 — Relocate the shared/flat pinning tests; keep Rust + e2e green

- [ ] **6.1** Confirm every vitest file enumerated in Phases 2–4 + Phase 2a lives under `…/editing-mode-block-editing/src/__tests__/` and imports the extension, not `q2-preview/`. The **shared/flat** set this plan owns: `self-heal-on-write`, `p2-3b-real`, `p2-4-real`, `p2-4d`, flat subset of `p2-4b`, `s6-delete-by-emptying`, `s7-expand-on-edit`, `useBlockEditHover.integration` (→ `useActivation.integration`), `useEditableBlock.integration`, `p2-3a`, `s4-dirty-caret-col` (if flat), `g19-spurious-dirty` (if flat — buffer-fix-adjacent), **plus the NEW `indented-block-buffer-roundtrip`**. Pure-renderer / Plan-1-surface tests **stay** in `q2-preview/` (`PreviewDocument`, `q2-preview.integration`, `entry.integration`, `registry`, `RevealDeck`, `assetWalker`, `sourceIndex`, `stripSourceInfoFields`, `custom-components`, `entry-slide-theme`, and `caretGeometry` if Plan 1/Plan 6 own the primitive). The **nesting-specific** tests are Plan 4's (`nestingNav`, `BreadcrumbChip.geometry`, `p3-*`, `s0`/`s1`/`s2`, `nest-caret`, `g5`/`g6-g7`/`g9`, `p2-3b-real`'s nesting cases, `commit-destination-equivalence` — pins `buildNestingCommitDestination`). **Reconcile with Plan 4:** if a shared/flat file already moved into Plan 4's dir, move it here and leave a `// PLAN5-OWNS:` breadcrumb; record final ownership in Notes.
- [ ] **6.2 (vitest)** Resolve vitest discovery so the extension's `__tests__/` run (the dir is under `resources/`, outside `ts-packages`). **Be consistent with Plan 4's choice** — both extension dirs must run under the *same* discovery mechanism. Reuse whatever Plan 4 established; if Plan 5 runs first, establish it and document it. Run the full vitest suite from `q2-preview/` and the extension dir → all green. **INTEGRATION POINT:** document the discovery choice in Notes.
- [ ] **6.3 (Rust)** Run `cargo nextest run -p pampa` and confirm the flat-relevant backend tests are green: `node_edit_tests`, `tiling_phase3_tests`, `inline_splice_property_tests` (+ `inline_splice_integration_tests`, `inline_splice_safety_tests`), and `regenerate_nested_buffers_tests` (the generator behind `EditBufferCache`; **if the integration point resolved to option (A), this is where the new outermost-container emit test lives** — write it test-first). The nesting-only `nesting_cursor_roundtrip_tests` is Plan 4's backstop but must stay green workspace-wide. We changed no Rust unless option (A) was chosen.
- [ ] **6.4 (Playwright, hub-client)** Re-point and run the **flat** specs against the `block-editing` extension path: `q2-preview-inline-edit`, `q2-preview-self-heal-on-write`, `q2-preview-expand-on-edit`, `q2-preview-delete-by-emptying`, `q2-preview-block-nav-p2-5b` (cross-surface arrows), `q2-preview-item-edit-size`, `q2-preview-locked-hover`, `q2-preview-scrolljack`, `q2-preview-columns-layout`, and `q2-preview-render-components-{kanban,drag,comment}` (render-components stay editable via Plan 1's baseline `useMode()` with NO mode active — keystone §4/§8; re-confirm under `block-editing` selected too). **If an existing flat e2e exercises blockquote/list editing, confirm the generated-buffer seed in a real browser; if none does, add a minimal flat blockquote-edit spec** (the browser is the only true geometry/round-trip check). The **nesting** specs are Plan 4's. Confirm the e2e fixtures **select `block-editing`** (Phase 5.1). Use `assertNoReflowOnActivation` / `ACTIVE_REGION` (`#q2-active-edit-region`) from `ts-packages/preview-e2e-helpers/src/index.ts`. Apply the **two-commit changelog** workflow for the hub-client fixture change.
- [ ] **6.5 (Playwright, SPA)** Run the flat SPA e2e: `q2-preview-spa/e2e/edit-cell-sizing.spec.ts` against `block-editing` selected. (`nesting-cursor.spec.ts` is Plan 4's.) Confirm the boot path selects `block-editing` (no `?nestingCursor=1`). Update boot plumbing only if the selection contract changed.
- [ ] **6.6** `fail-on-revert` sweep: pick high-value relocated tests — one geometry e2e (`edit-cell-sizing` or `inline-edit`), `self-heal-on-write`, one cross-surface arrow (`p2-4-real`), one delete (`s6`), **and the `indented-block-buffer-roundtrip`** — and confirm each fails when its bound code is reverted. This proves the relocation didn't create vacuous passes and that self-heal AND the buffer fix really flow through Plan 1's primitives.

### Phase 7 — End-to-end verification + cleanup

- [ ] **7.1** Full gate: `cargo xtask verify` (full; mandatory if option (A) changed `pampa`), then `cd hub-client && npm run build:all` (stricter than vitest). Capture logs to a `/tmp` file once and grep for failures.
- [ ] **7.2 (E2E through the binary — required, CLAUDE.md "End-to-end verification").** Drive a real `q2 preview` (or hub) session with **`block-editing` selected** on a fixture with paragraphs/headings/lists **AND a multi-line blockquote**. Honor the stale-WASM chain first (`cd hub-client && npm run build:wasm` → `cargo xtask build-q2-preview-spa` → `cargo build --bin q2`). Inspect the actual DOM:
  - click a paragraph → `#q2-active-edit-region` surface opens; type + Cmd/Ctrl+Enter commits; arrow-down moves the cursor to the next surface (cross-surface arrow); emptying a block + committing deletes it (`s6`); a concurrent collaborator insert above re-anchors the open editor (self-heal).
  - **click the blockquote → the surface is seeded with the de-prefixed text (no `> `), and committing a small edit round-trips the blockquote with the prefix intact exactly once** (the buffer fix, end-to-end — inspect the resulting source).
  - **Confirm NO breadcrumb chip appears and Cmd/Alt+Arrow does nothing** (the flat distinction from Plan 4).
  - Record the exact invocation + an output snippet (especially the blockquote source before/after) + an explicit "inspected" note in this plan file. Also confirm a render-components fixture (kanban) is still editable with the mode OFF.
- [ ] **7.3** Confirm `resources/extensions/quarto/editing-mode-block-editing/` is self-contained and imports the shared primitives off the renderer API surface / `ViewControllerProps` (grep its `src/` for `// PLAN1:` imports; assert **no** local copy of self-heal / the buffer cache / a surface widget / `caretGeometry` — duplicating any is a plan failure). Confirm it contributes **no `renderOverlay`** and **no settings** and renders the **selected** surface (no `<textarea>` literal in `activeTargetOverride.tsx`).
- [ ] **7.4** Update `hub-client/changelog.md` (second commit, with hash) for the user-visible change (block editing now selectable as a bundled editing-mode extension; indented blocks edit cleanly via the shared buffer service). Only if a `hub-client/` file changed (Phases 5/6.4).
- [ ] **7.5** Update the epic index ("Plan 5 produces") checkboxes and note residual `// PLAN1:`/`// PLAN2:`/`PLAN-2-KEY` integration points needing follow-up strands (braid, `discovered-from` this work — side-issues only). Record the final shared/flat test-ownership list (incl. the new round-trip test) so Plan 4 reconciles cleanly. **Record the resolved option for the outermost-container buffer integration point (A/B/C) and its owner.**

---

## Test surfaces this plan OWNS (shared/flat — both modes inherit, so they bind here)

(From the epic test-ownership split: "Plan 5 owns the shared/flat feature tests" + "indented-block buffer round-trip".)

- **Vitest → MOVE into `editing-mode-block-editing/src/__tests__/`** (shared/flat feature set): `self-heal-on-write.integration`, `p2-3b-real.integration` (flat self-heal/identity cases), `p2-4-real.integration`, `p2-4d.integration`, the **flat subset** of `p2-4b.integration` (drop `nest`/`crumb` cases → Plan 4), `s6-delete-by-emptying.integration`, `s7-expand-on-edit.integration`, `useBlockEditHover.integration` (→ `useActivation.integration`, unlock cases dropped → Plan 4), `useEditableBlock.integration`, `p2-3a.integration`, `s4-dirty-caret-col.integration` (if flat), `g19-spurious-dirty.integration` (if flat). Pre-validate the flat/nesting split per file with `prevalidating-test-seams`.
- **Vitest → NEW, OWNED here** (the buffer fix): `indented-block-buffer-roundtrip.integration` (blockquote/list/def-list → seeded from generated serialization via `EditBufferCache.editableTextFor`, not false-dirty, round-trips cleanly through `DocumentStore.commit`'s re-wrap). This is the keystone-mandated regression the previous draft omitted.
- **Vitest → STAY in `q2-preview/`** (pure renderer / Plan 1 surface): `PreviewDocument.integration`, `q2-preview.integration`, `entry.integration`, `entry-slide-theme.integration`, `registry`, `RevealDeck.integration`, `assetWalker`, `sourceIndex`, `stripSourceInfoFields`, `custom-components.integration`, `caretGeometry` (if Plan 1/Plan 6 own the primitive).
- **Vitest → Plan 4's (nesting-specific, do NOT move here):** `nestingNav.test`, `BreadcrumbChip.geometry`, `commit-destination-equivalence` (pins `buildNestingCommitDestination`), `p3-2-nesting-cursor-context`, `p3-3-*`, `p3-4-breadcrumb`, `nest-caret`, `s0-list-item-surfaces`, `s1-unlock-line-nav`, `s2-mode-aware-roving`, `g5-carry-expansion`, `g6-g7-settle-gate`, `g9-reland-fade`.
- **Rust (`crates/pampa/tests/integration/`) → KEEP GREEN as backstop, do not move:** `node_edit_tests`, `tiling_phase3_tests`, `inline_splice_property_tests`, `inline_splice_integration_tests`, `inline_splice_safety_tests` (flat commit/edit backend), and `regenerate_nested_buffers_tests` (the generator behind `EditBufferCache` — **if integration option (A) is chosen, ADD the outermost-container emit test here, test-first**). (`nesting_cursor_roundtrip_tests` is Plan 4's backstop but must stay green workspace-wide.)
- **Playwright (both hosts) → re-point at `block-editing` path, keep green (flat specs):** `q2-preview-inline-edit`, `q2-preview-self-heal-on-write`, `q2-preview-expand-on-edit`, `q2-preview-delete-by-emptying`, `q2-preview-block-nav-p2-5b`, `q2-preview-item-edit-size`, `q2-preview-locked-hover`, `q2-preview-scrolljack`, `q2-preview-columns-layout`, `q2-preview-render-components-{kanban,drag,comment}` (mode-off editability) + `q2-preview-spa/e2e/edit-cell-sizing.spec.ts` + (if absent) a minimal **flat blockquote-edit** spec proving the generated-buffer seed + clean round-trip in a real browser. The `q2-preview-nesting-*` / `q2-preview-breadcrumb-*` / `q2-preview-crumb-*` specs + `q2-preview-spa/e2e/nesting-cursor.spec.ts` are Plan 4's. Shared helper: `ts-packages/preview-e2e-helpers/src/index.ts` (`assertNoReflowOnActivation`, `ACTIVE_REGION = '#q2-active-edit-region'`).
- **Surface-geometry tests (`caretGeometry`, visual-line/edge detection, measure-and-set sizing) are Plan 6's, NOT here.** Edge detection now lives inside the surface (keystone §5); the mode delegates to it.

---

## Risks & notes

- **The buffer fix is the reason this rewrite exists — do NOT drop the clean buffer.** The single biggest failure mode is re-committing the previous draft's mistake (`editableTextFor` → raw `anchorSlice`). Phase 0.2a resolves the outermost-container source; Phase 2a + 2a.2 + 4.3 + 6.6 `fail-on-revert` prove the extension genuinely seeds from the generated serialization and round-trips cleanly. A blockquote/list edited as the outermost flat surface MUST be de-prefixed.
- **Outermost-container buffer = option B (resolved 2026-06-21).** The current `regenerate_nested_buffers_ast` emits buffers for *prefixed descendants*, not the *container itself* (grounded read: `crates/pampa/src/regenerate_nested_buffers.rs:42-203`), so `EditBufferCache` owns the outermost-container de-prefixing **in TS** (the in-production mechanism; no `pampa` change; Plan 5 stays pure-TS). Phase 0.2a *verifies* it; option (A) is rejected. **Do not start Phase 2 until the de-prefixed outermost buffer is verifiably available from `editableTextFor`.**
- **Buffer-regen was historically gated on `unlockNestingCursor`** (`ReactPreview.tsx:421,444-453`, `PreviewContext.tsx:173`). Under the `EditBufferCache` contract the generated buffer must exist for flat editing too, independent of any nesting setting — that decoupling is Plan 1's `EditBufferCache` (population via `acceptPushedBuffers`), and Phase 0.2a confirms it works with no nesting setting active.
- **Self-heal must be IMPORTED, never re-derived** (keystone §1). Phase 0.2 STOPs if the primitive isn't published; 4.3/6.6 `fail-on-revert` proves the extension flows through it.
- **Selected surface, not textarea.** The override renders `props.surface` (keystone §5). 7.3 asserts no `<textarea>` literal in the override. The mode never assumes textarea; edge/caret geometry is the surface's job (Plan 6 owns those tests).
- **Flat/nesting test split is per-file and per-case** (`p2-4b`, `p2-3b-real`, possibly `s4`/`g19`). Use `prevalidating-test-seams` to split at the case level; flat cases bind here, nesting cases to Plan 4. `g19` (false-dirty) is buffer-fix-adjacent — confirm it stays flat-owned and binds to the cache baseline.
- **Geometry/round-trip regressions are browser-only.** jsdom returns zero rects and has no WASM; seed the `PushedEditBufferCache` with a precomputed map in jsdom (Phase 4.1 harness), but gate the true round-trip on Playwright/7.2.
- **The override must keep emitting `#q2-active-edit-region`** or every `assertNoReflowOnActivation` caller breaks (Phase 2.2).
- **No `renderOverlay`, no nesting keys — verify by absence.** 1.1 asserts no `renderOverlay`; 7.2 confirms no breadcrumb and Cmd/Alt+Arrow inert.
- **Render-components coexistence (keystone §4, §8).** Render-components stay editable with NO mode active (baseline `useMode()` → core `resolveSource`/`commit`). A break is routing, not a missing `PreviewContext`.
- **Boundary-splice (keystone §9).** Do NOT migrate `commit` here. Leave `// keystone §9` at the `DocumentStore.commit` call site.
- **Provisional names (keystone §15).** `ViewController`, `useMode`, `EditBufferCache`, the manifest key (`PLAN-2-KEY`) may be renamed by global find-replace; author against the provisional names and keep them greppable.
- **Reconcile with Plan 4 on shared-test ownership and vitest discovery.** Whichever sibling runs second must not double-move the shared/flat tests or re-establish discovery. Phase 6.1/6.2 record the choices; the epic index is the reconciliation surface.

---

## References

- Keystone: `claude-notes/designs/2026-06-20-editing-mode-contract.md` (rev. 2026-06-21) — §1 shared-primitive rule, §3 mode seams, §4 `ModeApi`/`useMode`, §5 `EditingSurface`, §7 + §7.1 `EditBufferCache` (the buffer fix), §8 render-components, §9 commit/re-wrap/boundary-splice, §10 selection/settings, §12 matrix.
- Epic: `claude-notes/plans/2026-06-20-editing-mode-epic.md` ("Plan 5 produces"; test-ownership split incl. indented-block buffer round-trip; recommended-first ordering).
- Plan 1: `claude-notes/plans/2026-06-20-editing-mode-plan-1-core-services-and-seams.md` (Produces surface; Task 5.3 "two-mode factoring requirement"; `EditBufferCache`/`PushedEditBufferCache`/`acceptPushedBuffers`; `EditingSurface`/`TextareaSurface`; self-heal primitive).
- Plan 4 (sibling): `…-plan-4-nesting-cursor-extension.md` (packaging, test-split, vitest-discovery, harness conventions; inner-surface `EditBufferCache` use).
- Source today (flat subset + buffer machinery): `ts-packages/preview-renderer/src/q2-preview/{useBlockEditHover.tsx,dispatchers.tsx,PreviewRoot.tsx,PreviewContext.tsx,outerBlocks.ts,usePreviewEdit.ts,entry.tsx}` — esp. `outerBlocks.ts:684` (`seedForRange`), `:706` (`editBaseline`, the G19/Layer-2 single-source-of-baseline rule), `:717` (`isDirty`).
- Buffer machinery you CONSUME: `ts-packages/preview-runtime/src/wasmRenderer.ts:808` (`regenerateNestedBuffers`); `hub-client/src/components/render/ReactPreview.tsx:49,421,444-453` (`computeNestedEditBuffers`, today gated on `unlockNestingCursor`); `crates/pampa/src/regenerate_nested_buffers.rs:42-203` (the generator; `maybe_emit` only fires for prefixed descendants — see the flagged integration point).
- Pre-nesting vs nesting commit boundary: pre-nesting = `015c5d98c` (layout-stable wrapper/a11y/touch), `439ea2383` (byte-offset identity + self-heal), `bc892248f` (cross-surface arrows + `PreviewRoot.tsx`/`caretGeometry`/`p2-4-real`), `s6`/`s7`; nesting begins at `3347147ba` (depthNav→nestingNav, `BreadcrumbChip`, nested-buffer plumbing) / `33a638131` (`unlockNestingCursor` setting). Confirm with `git -C /Users/gordon/src/q2 show --stat <hash>`.
- Read-only gate (NOT a setting): `q2-preview-spa/src/channelRouting.integration.test.tsx:257-269`, `q2-preview-spa/src/PreviewApp.tsx:1249`, `PreviewContext.tsx:154`.
- Backend (unchanged unless option A): `crates/pampa/src/{apply_node_edit.rs,node_lookup.rs}`, `crates/pampa/src/writers/qmd.rs` (`write_single_block:2590`), `crates/wasm-quarto-hub-client/src/lib.rs`.
- Bundled-extension precedent: `resources/extensions/quarto/{kbd,video,lipsum,version,placeholder}/`.
- E2E helper: `ts-packages/preview-e2e-helpers/src/index.ts`.
- Skills: `prevalidating-test-seams`, `fail-on-revert` (mandatory for this plan per epic).
- Rules: `.claude/rules/integration-tests.md`, `.claude/rules/cross-platform.md`, `.claude/rules/wasm.md`.

---

## Notes / discovery log

- Fill in Phase 0 baseline pass-counts + the current location of each shared/flat pinning test (some may already be in Plan 4's dir if Plan 4 ran first).
- **Record the Phase 0.2a resolution of the outermost-container buffer integration point (option A/B/C) + its owner.** This gates Phase 2.
- Record the per-file/per-case flat-vs-nesting split decisions (`p2-4b`, `p2-3b-real`, `s4`, `g19`) as they are pre-validated.
- Record the vitest-discovery mechanism for `resources/extensions/quarto/*/src/__tests__/` (shared with Plan 4).
- Record the Phase 7.2 e2e invocation + observation (CLAUDE.md end-to-end rule), including the blockquote round-trip source before/after and the negative checks (no breadcrumb, Cmd/Alt+Arrow inert).
- Record the final shared/flat test-ownership list (incl. `indented-block-buffer-roundtrip`) handed to Plan 4 for reconciliation.
