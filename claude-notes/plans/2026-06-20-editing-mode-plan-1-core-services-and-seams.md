# Plan 1 — Core services, depollute the renderer, seams, surface contract, and primitives

> **For agentic workers:** This is **Plan 1** of the editing-mode epic. The
> binding design is the keystone:
> `claude-notes/designs/2026-06-20-editing-mode-contract.md` (rev. 2026-06-21) —
> read it first; when it and this plan disagree, **the keystone wins** (fix the
> plan). The epic index + inter-plan interface contract is
> `claude-notes/plans/2026-06-20-editing-mode-epic.md`. Use the keystone's exact
> vocabulary verbatim: `NodeOverride`, `ViewController`, `ViewControllerProps`,
> `SourceResolver`, `NodeLocator`, `DocumentStore`, `OverlaySlot`,
> **`EditBufferCache`**, `useMode()`, `ModeContext`, `NO_OP_MODE`, `ModeApi`,
> `CommitFn`, **`EditingSurface`** (`EditingSurfaceProps`/`EditingSurfaceHandle`/
> `EditingSurfaceComponent`), **`TextareaSurface`**. Provisional names
> (`ViewController`, `useMode`, `EditBufferCache`) are settled later by global
> find-replace, not by re-architecture — do not rename them now.
>
> **Workflow.** Strict TDD: write a failing test → run it → confirm it fails for
> the stated reason → write the minimal implementation → run the test → confirm
> green → run the relevant existing suite → commit. One bite-sized step at a
> time. This is a **refactor-heavy** plan: invoke the
> **`prevalidating-test-seams`** skill while prepping each task's test phase and
> the **`fail-on-revert`** skill as you build, so every relocated/repointed test
> is proven to actually bind to the new seam (a green suite after a pure
> file-move can be vacuous). Never delete an existing test to make the suite
> green — repoint it.

---

## Goal

Lift the **five** core services out of the editing tangle, make the vanilla
q2-preview renderer **pure** (`node → React`, no `PreviewContext`, no
`data-block-pool-id` stamping inside components), introduce the seam plumbing
(`NodeOverride` super-chain at the dispatcher + a single root `ViewController` +
`useMode()`/`ModeContext`/`NO_OP_MODE`), define the **`EditingSurface` contract**
and extract the in-tree **`TextareaSurface`** reference impl (with `caretGeometry`
internal to it), **decouple the mode from the surface** (the in-tree controller
renders the *selected surface* via the contract — never a hardcoded textarea —
and delegates caret/edge to the surface handle), and extract the reusable editing
primitives onto the renderer API surface — **all while keeping the current
block-editing / nesting-cursor behaviour working in-tree** (still bundled, but now
expressed *through* the seams) and **all existing tests green**.

The five core services (keystone §7):

1. `SourceResolver` — `node → ResolvedSource | null`.
2. `NodeLocator` — DOM ↔ node identity + self-heal/re-anchor.
3. `DocumentStore` — live AST + the one mutation entry `commit` (re-wraps
   prefixing containers).
4. `OverlaySlot` — positioned paint layer.
5. **`EditBufferCache`** — `node → clean-editable-buffer` (the swappable
   iframe-side port; today eager-pushed because the iframe has no WASM —
   keystone §7, §7.1). **Both modes consume it; it is shared substrate.**

Explicitly out of scope for Plan 1:
- **No extension type** (`_extension.yml` editing-mode / editing-surface
  contribution, discovery, delivery, two-axis selection) — that is Plan 2.
- **No `q2 create extension` scaffolder / minimal templates** — that is Plan 3.
- **No moving nesting-cursor / block-editing out of the renderer** into their own
  extensions — those are Plans 4 / 5. In Plan 1, both modes' logic stays in-tree,
  but it is **re-wired to ride the new seams** (the bundled `ViewController` + a
  state-predicated `NodeOverride` rendering the selected surface) rather than
  `PreviewContext` + per-component pool-ids.
- **No `WasmEditBufferCache`** (the lazy in-iframe-WASM impl) — out of scope;
  Plan 1 ships only `PushedEditBufferCache`. The interface must be drawn so that
  swap is free (population off the public interface).
- **No bundled `tiptap` surface** — that is Plan 7. Plan 1 ships `TextareaSurface`
  only, but its job is to prove the surface contract is genuinely complete.

This is almost certainly a **TypeScript-only** plan (`ts-packages/preview-renderer`
+ small re-points in `ts-packages/preview-runtime` consumers and `hub-client`/
`q2-preview-spa` if any import moved symbols). **No Rust is expected.** The
`regenerate_nested_buffers` WASM entry point already exists (`wasmRenderer.ts`'s
`regenerateNestedBuffers` wraps it); Plan 1 only re-homes its *consumer* behind
the `EditBufferCache` port — it does **not** touch the Rust generator. If a task
turns out to touch Rust (it should not), the async-trait `?Send` rule
(`.claude/rules/wasm.md`) and the integration-test layout rule
(`.claude/rules/integration-tests.md`) apply — but flag it and stop, because Rust
here means the scope was misread.

---

## Architecture (what changes, end-to-end)

**Today** (grounded in the read of the code, 2026-06-21):
- Every vanilla block component (`blocks/Para.tsx` etc.) and several custom
  components (`custom/Callout.tsx` etc.) `useContext(PreviewContext)`, call
  `ctx.resolveSource(node)`, compute an `isEditable` predicate, and stamp
  `data-block-pool-id={poolId}` + `tabIndex={-1}` onto their root DOM element.
- The block dispatcher `Block`/`CustomBlock` (`dispatchers.tsx:526-606`)
  `useContext(PreviewContext)`, computes `isBlockEditTarget`
  (`dispatchers.tsx:109-121`), and **hardcodes** the textarea swap
  (`renderMeasuredEdit` at `dispatchers.tsx:60-83` + `EditTextarea` at
  `dispatchers.tsx:137-484`, invoked via `renderBlockTextarea` at `:487-492`).
- `EditTextarea` reaches **directly** into `caretGeometry`
  (`placeCaretAtColumn`, `isOnFirstVisualLine`, `isOnLastVisualLine`,
  `getLogicalColumn` — imported at `dispatchers.tsx:13`) for caret placement and
  arrow-out / edge detection. **This is the mode↔surface coupling Plan 1 breaks:**
  caret geometry must move *inside* the surface and be reached only through the
  surface handle.
- `PreviewRoot.tsx` (~1521 lines) owns the AST (`useMemo` parse +
  `props.setAst`), the `resolveSource`/`commitTextEdit`/`commitSubtreeEdit`
  callbacks, the full edit/nav/self-heal state machine, and provides everything
  via one giant `PreviewContext.Provider` value.
- `outerBlocks.ts` (788 lines) holds DOM hit-testing (`resolveOuterBlock`,
  `captureEditTarget`, `measureBlockBox`, `enumerateOuterBlocks`,
  `rectsCoincide`, …) plus the **buffer-seed pure helpers** `seedForRange`
  (`:684-693`), `editBaseline` (`:706-708`), `isDirty` (`:717-720`), all keying
  off `[data-block-pool-id]`.
- **The node→buffer port today (the thing `EditBufferCache` abstracts):**
  `seedForRange(range, content, nestedEditBuffers)` returns `seededDraft =
  nestedEditBuffers?.[siKey] ?? anchorSlice`, where `siKey =
  serializeSourceEntry({t:0, r:[r0,r1], d:0})` =
  ``${t}:${r0}-${r1}:${d}`` = `"0:<r0>-<r1>:0"` (`sourceIndex.ts:30-36`) and
  `anchorSlice = normalizeLineEndings(sliceBytes(content,r0,r1)).trimEnd()` (the
  raw-slice fallback). The pushed map `nestedEditBuffers` flows host →
  `Q2PreviewIframe` (`:88-93,129,251,269`) → `PreviewRoot` (`:162-165,296`) →
  `PreviewContext.nestedEditBuffers` (`PreviewContext.tsx:172-177`) →
  `useBlockEditHover` (`:96-98` calls `seedForRange`). `editBaseline` / `isDirty`
  consume the resulting `seededDraft`.
- **The parent generator (stays existing host plumbing):**
  `regenerateNestedBuffers(content, untransformedAstJson)` in
  `ts-packages/preview-runtime/src/wasmRenderer.ts:808-822` calls
  `wasmModule.regenerate_nested_buffers` and returns a `Record<siKey,string>` for
  every block with a **prefixing ancestor** (BlockQuote / BulletList /
  OrderedList / DefinitionList) that is multi-line in source — this is the
  generated-vs-raw boundary, made explicit. The same prefixing-container set is
  encoded a second time in the renderer as `LEFT_INSET_STRIPPED_TYPES`
  (`dispatchers.tsx:40-42`: BulletList/OrderedList/DefinitionList). **Plan 1
  unifies these into one shared predicate** (see Phase 5b).
- `usePreviewEdit()` (`usePreviewEdit.ts`) reaches into `PreviewContext` for
  `resolveSource` / `commitSubtreeEdit` / `commitTextEdit`.
- The renderer API surface `window.__Q2_PREVIEW_RENDERER__` is set at the top of
  `entry.tsx:107-126`.

**After Plan 1:**
- **Five core services** live in `framework/coreServices/`, each a named factory
  with a typed interface, consumed via React context:
  - `SourceResolver` wraps `buildSourceIndex`/`resolveSource`/`ReachabilityClass`.
  - `NodeLocator` owns DOM↔node identity (core stamps `data-block-pool-id`/
    `tabIndex` at the **dispatcher**, not the components) and absorbs the
    hit-testing + the **self-heal/re-anchor** logic.
  - `DocumentStore` holds the live AST and exposes the one mutation entry
    `commit` (typed as a small union; **owns the re-wrap** of a generated buffer
    back into its prefixing container — the inverse of `EditBufferCache`'s
    generation, via the **one shared generated-vs-raw predicate**).
  - `OverlaySlot` is a positioned paint region above the content.
  - **`EditBufferCache`** is the `node → clean-editable-buffer` port:
    `editableTextFor(node)` returns the pushed generated buffer for prefixing
    containers, else the raw source slice. The `PushedEditBufferCache` impl holds
    the parent-pushed map (today's `nestedEditBuffers`), fed by a population port
    **off the public interface** (`acceptPushedBuffers`). The
    `regenerateNestedBuffers` parent generate-and-push plumbing is unchanged — it
    just feeds the port instead of `PreviewContext.nestedEditBuffers` directly.
- **Seams**:
  - The dispatcher composes a `NodeOverride[]` super-chain over the vanilla base
    (replacing the hardcoded textarea swap).
  - The root mounts exactly **one** `ViewController` (default = the in-tree
    bundled controller). `ViewControllerProps` now carries **`editBufferCache`**
    and **`surface`** (the selected `EditingSurfaceComponent`) per keystone §4.2.
  - `useMode()` returns the `ModeApi` from `ModeContext`, falling back to
    `NO_OP_MODE`. The **baseline** `{ resolveSource, commit }` is always live
    from the core services, even with no mode active.
- **`EditingSurface` contract + in-tree `TextareaSurface`** (keystone §5):
  - `EditingSurfaceProps`/`EditingSurfaceHandle`/`EditingSurfaceComponent` are
    defined in `framework/surface/`.
  - `TextareaSurface` is the reference impl, extracted from today's `EditTextarea`
    + `renderMeasuredEdit` + `caretGeometry`. **`caretGeometry` becomes internal
    to `TextareaSurface`**, exposed only via the handle (`focus`, edge/caret
    queries surfaced through `onEdgeReached`). Markdown string in (`value`),
    markdown string out (`onCommit`).
- **mode↔surface decoupling**: the in-tree default `ViewController`'s
  `NodeOverride` renders **`props.surface`** (default = `TextareaSurface`) via the
  contract, passing `value = editBufferCache.editableTextFor(node)`, wiring
  `onCommit`→`documentStore.commit` and `onEdgeReached`→cross-surface navigation.
  The mode **delegates** caret/edge to the surface handle and **never imports
  `caretGeometry` directly**.
- **Vanilla components are pure** (`node → React`): no `PreviewContext`, no
  `resolveSource` call, no `data-block-pool-id`/`tabIndex` stamping.
- **Reusable primitives** are exported on `window.__Q2_PREVIEW_RENDERER__`: the
  measure-and-set wrapper, `caretGeometry`, `byteLineMap`, `sliceSource`,
  `editableTextFor`.

The single biggest risk is **behaviour preservation** of the
PreviewRoot/`outerBlocks` state machine through the re-wiring. The mitigation is
the dense existing test corpus (~30 vitest files + 17 Playwright specs, see the
epic), the `prevalidating-test-seams`/`fail-on-revert` skills, and a
**characterization checkpoint** (Phase 0) that pins current behaviour before any
move.

---

## Correction to the prior plan revision (DELTA D)

The previous Plan 1 draft noted that "clean-buffer regen is mode-specific." **That
is WRONG and is corrected here.** `EditBufferCache` is **shared core substrate** —
*both* modes consume `editableTextFor(node)` to seed a surface. The buffer
machinery (the pushed map, the siKey derivation, the raw-slice fallback, the
generated-vs-raw predicate, the re-wrap on commit) is shared; **only navigating to
*more* surfaces (the inner-surface walk) is nesting-specific.** Plan 5
(block-editing) uses `EditBufferCache` for indented top-level blocks; Plan 4
(nesting-cursor) additionally calls it for inner surfaces. The cache itself is a
Plan 1 primitive: "two consumers ⇒ it's a primitive" (keystone §1, §12).

---

## Global constraints (bake these in)

- **Test runners.** TS: `vitest` (jsdom) for unit/integration, Playwright for
  e2e. Per CLAUDE.md, never pipe runners through `tail`. Run from the worktree
  root or `ts-packages/preview-renderer` / `hub-client` as appropriate.
- **The strict gate is the production build.** After any task that changes the
  renderer, run `cd hub-client && npm run build:all` — `tsc -b && vite build`
  (project-references mode) catches errors `tsc --noEmit` + `vitest` miss
  (CLAUDE.md). A task is not "done" until `build:all` is green.
- **WASM-reachable?** `ts-packages/preview-renderer` + `ts-packages/preview-runtime`
  are bundled *from source* by hub-client, so a Rust/WASM rebuild is **not**
  required for these TS-only changes. But the preview SPA embeds a prebuilt
  bundle (`q2 preview` stale-WASM/stale-SPA trap, CLAUDE.md): for an end-to-end
  `q2 preview` smoke at the very end, rebuild the SPA
  (`cargo xtask build-q2-preview-spa && cargo build --bin q2`). For the normal
  per-task loop, `vitest` + `npm run build:all` is the gate.
- **Final verification before any push:** full `cargo xtask verify` (the renderer
  is hub-reachable) + `cd hub-client && npm run build:all`. Per CLAUDE.md, never
  push without explicit user permission.
- **Provisional names are load-bearing for downstream plans.** Export the exact
  symbols listed in "Produces" at the exact module paths — Plans 2–7 import them.
- **No behaviour change.** Every move is a *refactor*. If you find yourself
  wanting a "TODO" that undoes existing work or a hacky shim, STOP and ask the
  user (CLAUDE.md) — it means the seam shape is wrong.
- **Snapshot discipline.** If any `.snap`/jsdom-serialized snapshot changes,
  report counts + summarize per CLAUDE.md.

---

## Produces (the public surface Plans 2–7 consume)

These are the **exact** exported symbols and module paths Plan 1 commits to.
Plans 2–7 import from here; do not deviate without updating the epic interface
contract.

### Seam + accessor types — `ts-packages/preview-renderer/src/framework/mode/`
New barrel `framework/mode/index.ts`, re-exported from `framework/index.ts`:

- `framework/mode/types.ts`
  - `export type NodeOverride` — `{ matches: (node, mode: ModeApi) => boolean;
    render: (node, renderDefault: () => React.ReactNode) => React.ReactNode }`
    (keystone §4.1).
  - `export type ViewController` — the per-session component factory returning
    `{ handleInput?, renderOverlay?, exposeHook }` (keystone §4.2).
  - `export type ViewControllerProps` — **(UPDATED per DELTA C / keystone §4.2)**
    `{ children, hostRef, sourceResolver, documentStore, nodeLocator,
    overlaySlot, editBufferCache, surface, settings }`. Note the two new fields:
    **`editBufferCache: EditBufferCache`** and **`surface: EditingSurfaceComponent`**
    (the selected surface the mode renders for active blocks).
  - `export type RootInputHandlers` — the handler bag attached to the render root
    (the subset of `React.HTMLAttributes` `handleInput` returns: pointer + key).
  - `export type ModeApi` — baseline `{ resolveSource: (node: BlockNode) =>
    ResolvedSource | null; commit: CommitFn }` plus open mode-specific extras
    (keystone §6).
  - `export type CommitFn` — the small union (see "CommitFn type" below).
- `framework/mode/ModeContext.ts`
  - `export const ModeContext: React.Context<ModeApi | null>`
  - `export function useMode(): ModeApi` (returns `useContext(ModeContext) ??
    NO_OP_MODE`).
  - `export const NO_OP_MODE: ModeApi` — **constructed from core services at
    provider time** for the baseline; the exported constant is the zero-extras
    fallback whose `resolveSource`/`commit` are wired by the provider (keystone
    §6: baseline is core-backed and always live).
- `framework/mode/composeOverrides.tsx`
  - `export function composeNodeOverrides(node, overrides: NodeOverride[], mode:
    ModeApi, base: () => React.ReactNode): React.ReactNode` — the
    outermost-first super-chain (keystone §4.1).
- `framework/mode/activeMode.ts` — **the active-mode binding (CONSUMED BY PLAN 2
  and PLANS 4/5).** The single seam by which an externally-selected mode + surface
  is installed; Plan 2's host shim and Plans 4/5's bundled extensions feed it.
  - `export type ActiveMode = { viewController: ViewController; nodeOverrides:
    NodeOverride[]; settings: Record<string, unknown> }`.
  - The preview root accepts **optional props** `activeMode?: ActiveMode` and
    `surface?: EditingSurfaceComponent`. When `activeMode` is provided, the
    framework mounts that `ViewController` (wrapping the document), registers its
    `nodeOverrides` into `OverridesContext`, and passes `settings` +
    `editBufferCache` + the selected `surface` (default `TextareaSurface` when the
    prop is absent) via `ViewControllerProps`. When `activeMode` is **absent**,
    Plan 1 falls back to the in-tree bundled controller (Task 5.4) so behaviour is
    unchanged before Plans 2/4/5 land.
  - Rationale for props (not a free `mountEditingMode` function): the
    `ViewController` declaratively wraps `children`; the host already threads data
    into the renderer via the `Q2PreviewIframe` → `PreviewRoot` prop chain, so
    `activeMode`/`surface` ride that existing channel. Plan 2's
    `mountEditingMode(args)` shim is just the host-side adaptor that resolves the
    selected mode+surface extensions to an `ActiveMode` + `surface` and sets these
    props.

### Editing-surface contract — `ts-packages/preview-renderer/src/framework/surface/`
New barrel `framework/surface/index.ts`, re-exported from `framework/index.ts`:

- `framework/surface/types.ts`
  - `export interface EditingSurfaceProps` — `{ value: string; box: MeasuredBox;
    initialCaret?: CaretHint; onChange?(text): void; onCommit(text): void;
    onCancel(): void; onEdgeReached(dir: 'up'|'down'|'left'|'right'): void }`
    (keystone §5). `value` is the **clean per-block markdown** the mode got from
    `editBufferCache.editableTextFor(node)`.
  - `export interface EditingSurfaceHandle` — `{ focus(caret?: CaretHint): void }`
    plus the edge/caret queries the mode needs for cross-surface landing **that the
    surface provides** (e.g. `isOnFirstVisualLine()`, `isOnLastVisualLine()`,
    `getLogicalColumn()` — surfaced on the handle, NOT imported from
    `caretGeometry` by the mode).
    > **Plan 7 note (Proposed contract notes #1) — caret queries must be
    > best-effort/optional.** `getLogicalColumn()` is a textarea-internal
    > *source-text* column. A rich surface (Plan 7's `tiptap`) lives in *rendered*
    > text space and can only report a best-effort/approximate column (or none).
    > Type the cross-surface landing column as an **advisory hint** the mode
    > tolerates approximately, and define `CaretHint`'s `column` as **advisory** (a
    > surface MAY approximate to a line edge). Prefer neutral edge-query names
    > (`isAtFirstLine()`/`isAtLastLine()`) over textarea-internal ones. Do NOT make
    > `getLogicalColumn`-style exact source columns a *required* handle method —
    > that would make the contract textarea-only and uncompilable for tiptap.
  - `export type EditingSurfaceComponent = React.ForwardRefExoticComponent<
    EditingSurfaceProps & React.RefAttributes<EditingSurfaceHandle>>`.
  - `export type MeasuredBox` — the measure-and-set geometry the surface sizes
    into (today's `editTarget.boxStyle` + `contentHeight`, named).
  - `CaretHint` is re-exported here (lifted from `caretGeometry`, since it is part
    of the surface's public shape).
- `framework/surface/TextareaSurface.tsx`
  - `export const TextareaSurface: EditingSurfaceComponent` — the reference
    implementation, extracted from `EditTextarea`
    (`dispatchers.tsx:137-484`) + `renderMeasuredEdit` (`dispatchers.tsx:60-83`)
    + `caretGeometry.ts`. **`caretGeometry` is internal to this module** (moved
    in, or imported privately and re-exported only on the renderer surface for
    Plan 6). Keeps `id="q2-active-edit-region"` and the zero-reflow box.
  - **NOTE for Plan 6:** Plan 6 *extracts* this in-tree `TextareaSurface` into a
    bundled `editing-surface` extension; it does not re-author it. Plan 1's job is
    to make the contract genuinely complete (no mode-coupling leaks), which is the
    proof Plan 7 (tiptap) can implement the same handle differently.

### Core services — `ts-packages/preview-renderer/src/framework/coreServices/`
New barrel `framework/coreServices/index.ts`, re-exported from
`framework/index.ts`:

- `coreServices/SourceResolver.ts`
  - `export interface SourceResolver { resolve(node: BlockNode): ResolvedSource |
    null }`
  - `export function createSourceResolver(args: { untransformedAstJson?: string |
    null; pool: unknown[] }): SourceResolver`
  - re-exports the lifted `buildSourceIndex`, `serializeSourceEntry`,
    `ReachabilityClass`, `ResolvedSource`, `SourceIndexEntry` (moved from
    `q2-preview/sourceIndex.ts`; that file becomes a thin re-export shim during
    migration, then is deleted in the final cleanup task).
- `coreServices/NodeLocator.ts`
  - `export interface NodeLocator` with methods
    `resolveOuterBlock(el): Element | null`,
    `enumerateOuterBlocks(host): Element[]`,
    `enumerateNestingLeaves(host): Element[]`,
    `enumerateNestingSurfaces(host): Element[]`,
    `captureEditTarget(el): { anchorR0; anchorR1; anchorSlice } | null`,
    `measureBlockBox(el): { contentHeight; boxStyle }`,
    `outerBlockForAnchorR0(host, anchorR0, opts?): Element | null`,
    `refocusTargetForAnchorR0(host, anchorR0, opts): Element | null`,
    `findReanchorCandidate(anchorR0, anchorSlice): { r0; r1 } | null`,
    `snapshotOuterBlockGeometry(openedEl, topBlockR0): Map<…>`.
  - `export function createNodeLocator(args: { pool: unknown[]; content: string
    }): NodeLocator` — closes over pool+content so callers stop threading them.
  - `export const NODE_IDENTITY_ATTR = 'data-block-pool-id'` — the single
    constant the dispatcher stamps and the locator reads (today's literal).
  - re-exports the pure helpers `rectsCoincide`, `isVisibleBlock` for tests.
- `coreServices/DocumentStore.ts`
  - `export interface DocumentStore { ast: PandocAST | null; commit: CommitFn }`
  - `export function createDocumentStore(args: { setAst: (PandocAST) => void;
    content: string; isGenerated: GeneratedPredicate }): DocumentStore` — the
    `commit` body **re-wraps** a generated buffer back into its prefixing
    container, consulting the **same shared predicate** the cache uses (see
    `EditBufferCache` + Phase 5b).
- `coreServices/OverlaySlot.tsx`
  - `export interface OverlaySlot { render(node: React.ReactNode): void }` plus
  - `export const OverlaySlot: React.FC<{ hostRef }>` — the positioned layer
    component, and a `useOverlaySlot()` accessor for the controller.
- `coreServices/EditBufferCache.ts` — **(NEW per DELTA A; keystone §7, §7.1)**
  - `export interface EditBufferCache { editableTextFor(node: BlockNode): string }`
    — the **stable interface both modes depend on**. Synchronous;
    generated-or-raw; never leaks "not ready".
  - `export function createPushedEditBufferCache(args: { content: string;
    sourceResolver: SourceResolver; isGenerated: GeneratedPredicate }):
    PushedEditBufferCache`.
  - `export interface PushedEditBufferCache extends EditBufferCache {
    acceptPushedBuffers(buffers: Record<string, string>): void }` — the
    **population port is OFF the public `EditBufferCache` interface**. Modes see
    only `editableTextFor`; the host/`PreviewRoot` calls `acceptPushedBuffers`
    with the parent-pushed `nestedEditBuffers` map. This is exactly what keeps the
    future `WasmEditBufferCache` swap free (it implements `EditBufferCache` with
    **no** population port).
  - `export const editBufferKey: (node: BlockNode) => string` — the shared key
    derivation, `= serializeSourceEntry({ t: 0, r: [r0, r1], d: 0 })` =
    `"0:<r0>-<r1>:0"`. All impls **and** the parent generator
    (`regenerateNestedBuffers`) agree on this single derivation. (Re-exported from
    `SourceResolver`/`sourceIndex` so there is one definition.)
  - `export type GeneratedPredicate = (node: BlockNode, resolved: ResolvedSource |
    null) => boolean` — **the single shared generated-vs-raw predicate** (see Phase
    5b). `editableTextFor` consults it to choose pushed-buffer vs raw slice;
    `DocumentStore.commit` consults the **same** instance to choose re-wrap vs
    flat write — so the seed and the re-wrap can never diverge.
  - `editableTextFor(node)` semantics (ports `seedForRange`'s `seededDraft`
    branch) — the **detect-router**: when `isGenerated(node, resolved)` →
    return the *generated* buffer: the pushed buffer at `editBufferKey(node)` for
    deep-nested blocks, **OR for an outermost prefixing container (which the
    push does NOT key) the cache's own TS de-prefixing** (**option B**, the
    in-production mechanism — pure-TS, no iframe WASM, no `pampa` change). Else →
    return the raw source slice
    `normalizeLineEndings(sliceBytes(content, r0, r1)).trimEnd()` (kept because it
    is un-reformatted). Modes never choose — they just call `editableTextFor`.
    **Option A (extend `pampa` to emit the container buffer) is out of scope.**

### Primitives on the renderer API surface — `window.__Q2_PREVIEW_RENDERER__`
Added in `q2-preview/entry.tsx` (the explicit object at `:107-126`):
> **Plan 7 note (Proposed contract notes #2) — surfaces may need heavy npm deps.**
> A surface is a React component delivered through the globals-only
> transpile→blob-import rail (it cannot resolve npm imports). Plan 6 already
> requires React be exposed here for transpiled surfaces; Plan 7's `tiptap`
> surface *escalates* this — it needs **additional heavy npm deps**
> (`@tiptap/core`/`@tiptap/pm`/`@tiptap/react`/`@tiptap/markdown`) that this rail
> cannot deliver. So this renderer-API surface (Plan 1) + the delivery rail
> (Plan 2) will eventually need a **dependency-provisioning seam for surfaces**,
> not just React. Plan 7 v1 stopgaps with a global-injection accessor
> (`window.__Q2_TIPTAP__`, lazy/code-split, materialized by the host only when the
> tiptap surface is selected); the durable mechanism is the in-flight
> sandbox/package-import work. **Deferred, not blocking** for Plan 1's textarea path.
- `renderMeasuredEdit` — the **reusable** measure-and-set wrapper (extracted from
  `dispatchers.tsx:60-83`); now lives inside / alongside `TextareaSurface`.
- `caretGeometry` — the module namespace (`isOnFirstVisualLine`,
  `isOnLastVisualLine`, `getLogicalColumn`, `placeCaretAtColumn`, `prefixWidth`,
  `CaretHint`). **It is internal to `TextareaSurface`; exposing it on the renderer
  surface is for Plan 6's extraction + back-compat tests, NOT for modes to import.**
- `byteLineMap` — `{ buildByteLineMap, ByteLineMap }`.
- `sliceSource` — `{ sliceBytes, sliceEncodedUtf8 }`.
- `editableTextFor` — `{ createPushedEditBufferCache, editBufferKey }` (the buffer
  port, so Plans 4/5 reach it through the surface API like the rest).

### CommitFn type (keystone §9 migration note)
```ts
// framework/mode/types.ts
export type Commit =
  | { channel: 'text'; destinationSourceInfoJson: string; newText: string }
  | { channel: 'subtree'; destinationSourceInfoJson: string; modifiedBlock: BlockNode };
export type CommitFn = (commit: Commit) => void;
```
v1 `DocumentStore.commit` dispatches the union to today's `commitTextEdit` /
`commitSubtreeEdit` wire payloads (`PreviewNodeEditPayload`), consulting the
shared `GeneratedPredicate` to re-wrap a generated buffer back into its prefixing
container. **Migration note for boundary-splice**
(`2026-06-19-boundary-splice-implementation.md`): when that lands, `Commit`
collapses to its `commit(splice)` shape (`Content = md | ast`, `Boundary`) and
only `DocumentStore.commit`'s body changes — the seam (`useMode().commit`) and
every call site stay put. Document this in the `CommitFn` doc comment so the two
efforts converge on `useMode() → { resolveSource, commit }` + `setAst`-as-prop.

---

## Phase 0 — Characterization checkpoint (pin current behaviour)

Goal: before moving anything, lock the externally-observable behaviour the
refactor must preserve, so later phases have an objective "still green" target and
`fail-on-revert` has named hunks to bind to.

- [ ] **Task 0.1 — Inventory + run the baseline suite green.**
  - **Interfaces (Consumes / Produces):** Consumes: nothing. Produces: a green
    baseline + a written inventory in this plan's "Notes" of the exact test
    files that cover (a) source resolution, (b) outer-block/identity hit-testing,
    (c) the textarea-swap edit path, (d) nesting cursor, (e) the
    **buffer-seed path** (`g19-spurious-dirty`, `s4-dirty-caret-col`,
    `nest-caret`, `g9-reland-fade`, `p3-2-nesting-cursor-context` — they inject
    `nestedEditBuffers`), (f) the `render-components-{kanban,drag,comment}` demos.
  - Run `cd ts-packages/preview-renderer && npx vitest run` and
    `cd hub-client && npm run build:all`; record pass counts.
  - From the epic's "test surfaces" list, confirm each named vitest file exists
    and passes: `sourceIndex.test.ts`, `outerBlocks.integration.test.ts`,
    `outerBlocks-p2-3b.integration.test.ts`, `caretGeometry.test.ts`,
    `commit-destination-equivalence.test.ts`, `useBlockEditHover.integration.test.tsx`,
    `nestingNav.test.ts`, `p3-2-nesting-cursor-context.integration.test.tsx`,
    `g19-spurious-dirty.integration.test.tsx`, `s4-dirty-caret-col.integration.test.tsx`,
    `s0`–`s7-*`, `p2-*`, `p3-*`, `g*`. **Do not modify them yet.**
  - No code change. Commit nothing (this is a read/measure task); record results
    in Notes.

- [ ] **Task 0.2 — Add a thin behaviour-pinning integration test for the seam
  boundary (NEW; asserts current behaviour).**
  - **Interfaces:** Consumes: current `PreviewRoot` mount + `previewRegistry`.
    Produces: `q2-preview/seam-boundary-characterization.integration.test.tsx`
    asserting four invariants that MUST survive the refactor: (1) a rendered
    editable `<p>` carries `data-block-pool-id`; (2) clicking it opens a
    `#q2-active-edit-region` textarea; (3) with `editingDisabled`, no
    `data-block-pool-id` is stamped; (4) **a nested block whose `nestedEditBuffers`
    entry is present opens its editor seeded with the clean buffer, NOT the raw
    `> `/indented slice** (the `EditBufferCache` behaviour, modelled on
    `g19-spurious-dirty`). Invariant (4) is the characterization anchor for the
    `EditBufferCache` extraction (Phase 5b).
  - Model the harness on `useBlockEditHover.integration.test.tsx` /
    `g19-spurious-dirty.integration.test.tsx` (mount `PreviewRoot` with
    `astJson`/`renderedContent`/`untransformedAstJson`/`nestedEditBuffers`).
  - Write → run → it should **pass against current code**. Use
    `prevalidating-test-seams` to confirm each assertion is bound to a concrete
    DOM/seed fact, not a tautology. Commit.

---

## Phase 1 — Lift `SourceResolver`

- [ ] **Task 1.1 — Create `coreServices/SourceResolver.ts` wrapping the existing
  source-index logic (test-first).**
  - **Interfaces (Consumes / Produces):** Consumes: `buildSourceIndex`,
    `serializeSourceEntry`, `ResolvedSource`, `ReachabilityClass` (currently in
    `q2-preview/sourceIndex.ts`). Produces: `createSourceResolver`,
    `SourceResolver` (see Produces).
  - **Test first:** new `framework/coreServices/SourceResolver.test.ts`. Reuse
    the `makeAst`/`makePara`/`makeDiv` helpers from `sourceIndex.test.ts`
    (copy them into the test or a shared `__testHelpers`). Assert
    `createSourceResolver({ untransformedAstJson, pool }).resolve(node)` returns
    the same `{ sourceNode, reachabilityClass, sourceEntry }` that today's
    `PreviewRoot.resolveSource` produces. Cover: hit, miss (`s === undefined`),
    miss (non-`t:0` entry), Opaque class. Run → fails (module absent).
  - **Implement:** `SourceResolver.ts` re-exports `buildSourceIndex` etc. from
    `../../q2-preview/sourceIndex` and adds `createSourceResolver`, which builds
    the index once and implements `resolve` by the byte-value lookup. Run → green.
  - Commit.

- [ ] **Task 1.2 — Move the source-index module into `coreServices/` and leave a
  re-export shim.**
  - **Interfaces:** Consumes: every importer of `q2-preview/sourceIndex`
    (`PreviewRoot`, `outerBlocks`, `dispatchers`, `PreviewContext`, tests).
    Produces: `framework/coreServices/sourceIndex.ts` (moved file) +
    `q2-preview/sourceIndex.ts` as a one-line `export * from
    '../framework/coreServices/sourceIndex'` shim (deleted in the final cleanup
    task once all importers are repointed).
  - `git mv` the file; fix its own relative imports. Grep-audit every relative
    import that pointed at it. Repoint `SourceResolver.ts` to the new local path.
  - Run `npx vitest run` + `npm run build:all` → green (shim preserves old
    paths). Commit.

- [ ] **Task 1.3 — Provide `SourceResolver` via context and route the dispatcher
  through it (no component change yet).**
  - **Interfaces:** Consumes: `createSourceResolver`, `PreviewRoot`'s
    `sourceIndex`/`pool`/`resolveSource`. Produces:
    `framework/coreServices/CoreServicesContext.ts` exporting
    `CoreServicesContext` (`{ sourceResolver, nodeLocator, documentStore,
    overlaySlot, editBufferCache } | null`) + `useCoreServices()`. (The
    NodeLocator/DocumentStore/OverlaySlot/EditBufferCache fields are filled in
    their phases; start with `sourceResolver`.)
  - **Test first:** `CoreServicesContext.test.tsx` mounts a probe component under
    `<CoreServicesContext.Provider>` and asserts `useCoreServices().sourceResolver
    .resolve(node)` matches the same node's resolution. Run → fails.
  - **Implement:** add the context; in `PreviewRoot`, construct
    `sourceResolver = useMemo(() => createSourceResolver({ untransformedAstJson,
    pool }), [untransformedAstJson, pool])` and provide it. Keep the existing
    `PreviewContext.resolveSource` value too (point it at `sourceResolver.resolve`
    so there is one source of truth). Run → green;
    `seam-boundary-characterization` still green. Commit.

---

## Phase 2 — Lift `NodeLocator` + move identity stamping to the dispatcher

This is the depollution phase. Order matters: introduce the locator and the
**dispatcher-side stamping** first (so identity is available from a single
place), then strip the 12 block + 4 custom components.

- [ ] **Task 2.1 — Create `coreServices/NodeLocator.ts` by lifting the
  hit-testing from `outerBlocks.ts` (test-first).**
  - **Interfaces (Consumes / Produces):** Consumes: the pure functions in
    `outerBlocks.ts` (`resolveOuterBlock`, `enumerateOuterBlocks`,
    `enumerateNestingLeaves`, `enumerateNestingSurfaces`, `captureEditTarget`,
    `measureBlockBox`, `measureLeadingBlockBox`, `outerBlockForAnchorR0`,
    `refocusTargetForAnchorR0`, `findReanchorCandidate`,
    `snapshotOuterBlockGeometry`, `rectsCoincide`, `isVisibleBlock`). Produces:
    `createNodeLocator`, `NodeLocator`, `NODE_IDENTITY_ATTR` (see Produces). The
    locator is a thin **stateful facade** binding `pool`+`content`+`host`.
  - **NOTE:** the buffer-seed helpers `seedForRange` / `editBaseline` / `isDirty`
    do **NOT** move into `NodeLocator`. `seedForRange`'s node→buffer logic is
    extracted into `EditBufferCache` (Phase 5b); `editBaseline` / `isDirty` move
    into `TextareaSurface` (Phase 5a, the dirty-tracking owner). Leave them in
    `outerBlocks.ts` for now and migrate in those phases.
  - **Test first:** `NodeLocator.test.ts` — reuse the DOM-builder helpers from
    `outerBlocks.integration.test.ts` (it mocks `getBoundingClientRect`). Assert
    `createNodeLocator({ pool, content }).resolveOuterBlock(el)` ===
    `resolveOuterBlock(el)`; `.captureEditTarget(el)` ===
    `captureEditTarget(el, pool, content)`; `.findReanchorCandidate(r0, slice)`
    === `findReanchorCandidate(pool, content, r0, slice)`. Run → fails.
  - **Implement:** the facade delegates to the free functions, supplying
    `pool`/`content` from closure. Run → green. Commit.

- [ ] **Task 2.2 — Move `outerBlocks.ts` into `coreServices/`, leave a shim, fix
  imports.**
  - **Interfaces:** Consumes: importers of `q2-preview/outerBlocks`
    (`PreviewRoot`, `dispatchers`, `useBlockEditHover`, ~6 tests). Produces:
    `framework/coreServices/domHitTest.ts` (moved `outerBlocks.ts` contents) +
    `q2-preview/outerBlocks.ts` shim re-exporting it. `NODE_IDENTITY_ATTR`
    replaces the inline `'data-block-pool-id'` literal **inside** the moved file
    (single constant). Grep-audit relative paths after the move. Run vitest +
    `build:all` → green. Commit.

- [ ] **Task 2.3 — Stamp node identity at the dispatcher (test-first).**
  - **Interfaces (Consumes / Produces):** Consumes: the `Block`/`CustomBlock`
    dispatchers (`dispatchers.tsx:526-606`), `SourceResolver`,
    `NODE_IDENTITY_ATTR`, the `editingDisabled` flag. Produces: dispatcher-side
    stamping — the `Block` dispatcher reads `sourceResolver.resolve(node)`,
    computes the same `isEditable` predicate the components use today
    (`resolved != null && reachabilityClass !== 'Opaque' && s !== undefined &&
    !editingDisabled`), and stamps identity on the component's **rendered root
    element**.
  - **Spike (see Notes):** the components today stamp the attr on **their own**
    DOM element (`<p data-block-pool-id>`), and `NodeLocator`/`domHitTest` climb
    the DOM expecting the attr on the *block's* element, not the dispatcher's
    `AttributionWrap` wrapper `<div>`. Stamping on the wrapper changes the
    coincidence-climb geometry (`rectsCoincide`/`resolveOuterBlock`). **Therefore:**
    stamp via a `ref` callback onto the rendered root element (the wrapper's
    `firstElementChild`), or pass an `identityProps` object down one level. Pin the
    exact mechanism with a failing geometry test (`s0`/`s1`/`outerBlocks` style)
    BEFORE implementing; if it cannot be settled from reading alone, leave this as
    an explicit **spike step** and ask the user.
  - **Test first:** extend `seam-boundary-characterization` (or a new
    `dispatcher-identity.integration.test.tsx`): assert the attr appears on the
    `<p>` regardless of whether the component stamps it. Run → fails (dispatcher
    doesn't stamp yet).
  - **Implement:** dispatcher stamping. Run → green; the full edit suite still
    green because the attr lands on the same element. Commit.

- [ ] **Task 2.4 — Make the 12 block components pure (test-first per component
  group).**
  - **Interfaces:** Consumes: `blocks/{Para,Header,Div,BlockQuote,BulletList,
    OrderedList,DefinitionList,Figure,CodeBlock,RawBlock,Table,LineBlock}.tsx`.
    Produces: each becomes `node → React` — remove `useContext(PreviewContext)`,
    the `resolveSource` call, the `isEditable` computation, and the
    `data-block-pool-id`/`tabIndex` stamping. **Keep** genuine rendering logic.
  - **Test first:** for each component (or group), a unit test that renders it
    **outside** any `PreviewContext` and asserts (a) it renders the expected DOM
    and (b) it does **not** read context. Run → "does not stamp pool-id" fails
    today. After the dispatcher stamps (2.3), the integration suite proves the
    attr is still present end-to-end.
  - **Implement:** strip context usage one component at a time; run the relevant
    integration tests after each. Commit per group (4 commits: simple blocks;
    lists; Figure/Table; Code/Raw/LineBlock) so a regression bisects cleanly.

- [ ] **Task 2.5 — Make the 4 editable custom components pure (test-first).**
  - **Interfaces:** Consumes: `custom/{Callout,Theorem,Proof,FloatRefTarget}.tsx`.
    Produces: each renders `node → React` with no `resolveSource`/pool-id
    stamping; the dispatcher's `CustomBlock` path now stamps identity (extend 2.3
    to `CustomBlock`).
  - **Test first:** render each custom component without `PreviewContext`; assert
    DOM + no context read. Extend the dispatcher-identity test to `CustomBlock`.
    Run → fails. Implement → green. Run callout/theorem integration coverage.
    Commit.

- [ ] **Task 2.6 — Provide `NodeLocator` via `CoreServicesContext`; repoint
  `PreviewRoot` + `useBlockEditHover` onto it.**
  - **Interfaces:** Consumes: `PreviewRoot`'s direct calls to the moved free
    functions (`outerBlockForAnchorR0`, `enumerateOuterBlocks`,
    `captureEditTarget`, `measureBlockBox`, `findReanchorCandidate`,
    `snapshotOuterBlockGeometry`, `refocusTargetForAnchorR0`);
    `useBlockEditHover`'s calls (`resolveOuterBlock`, `enumerateOuterBlocks`,
    `enumerateNestingSurfaces`, `captureEditTarget`, `measureBlockBox`). Produces:
    both read `nodeLocator` from `useCoreServices()` instead of importing free
    functions directly; `PreviewRoot` constructs `nodeLocator = useMemo(() =>
    createNodeLocator({ pool, content }), [pool, content])` and provides it.
  - **NOTE:** `useBlockEditHover`'s `seedForRange` call (`:96-98`) is **NOT**
    repointed here — it migrates to `editBufferCache.editableTextFor` in Phase 5b.
    Leave it calling `seedForRange` until then.
  - **Test first:** a `fail-on-revert`-style assertion — temporarily break the
    facade (return null) and confirm a self-heal/nav test goes red, proving the
    repointed path flows through the locator. Then restore.
  - **Implement:** mechanical repoint. The self-heal effect, reland, and nav
    resolvers keep identical behaviour. Run the full `p2-*`/`p3-*`/`s*`/`g*`
    suite. This is the highest-risk task — run it in isolation, commit only when
    fully green.

---

## Phase 3 — Lift `DocumentStore` with a typed `commit`

- [ ] **Task 3.1 — Define `Commit`/`CommitFn` + `createDocumentStore`
  (test-first).**
  - **Interfaces (Consumes / Produces):** Consumes: `PreviewRoot`'s
    `commitTextEdit`, `commitSubtreeEdit`, the `PreviewNodeEditPayload` shape,
    `props.setAst`. Produces: `Commit`, `CommitFn` (the union in Produces),
    `DocumentStore`, `createDocumentStore`.
  - **NOTE on the re-wrap:** `createDocumentStore` takes the shared
    `GeneratedPredicate` (Phase 5b). In Plan 1 the v1 body keeps today's exact
    payloads; the re-wrap of a generated buffer back into its prefixing container
    is the inverse of `EditBufferCache`'s generation. If today's commit path
    already round-trips clean buffers correctly (it does — `apply_node_edit` takes
    the destination source-info, and `regenerate_nested_buffers` produced the
    clean buffer keyed by the same range), the v1 `commit` is a thin dispatch; the
    `GeneratedPredicate` parameter is threaded now so Phase 5b can make the
    seed↔re-wrap symmetry explicit and testable without re-touching this file.
  - **Test first:** `DocumentStore.test.ts` — `createDocumentStore({ setAst,
    content, isGenerated })`; assert `commit({ channel: 'text',
    destinationSourceInfoJson, newText })` calls `setAst` with the exact
    `PreviewNodeEditPayload` today's `commitTextEdit` produces, and the `subtree`
    branch matches `commitSubtreeEdit`'s stripped-`s`/`a` + wrapped-doc payload
    (port the `JSON.parse(JSON.stringify(..., replacer))` logic verbatim). Run →
    fails.
  - **Implement:** `createDocumentStore` dispatching the union to the two
    payloads. Run → green. Commit.

- [ ] **Task 3.2 — Provide `DocumentStore` via context; repoint commit call
  sites.**
  - **Interfaces:** Consumes: every commit site — `EditTextarea.commitIfDirty`,
    `requestMove` dirty branch, `handleClickSwitchBlur`, `commitNestingEdit`,
    `commitAndArmReland`, and `usePreviewEdit` consumers. Produces: all route
    through `documentStore.commit({ channel: 'text' | 'subtree', … })`;
    `PreviewRoot` constructs `documentStore = useMemo(() => createDocumentStore({
    setAst: props.setAst, content: props.renderedContent, isGenerated }),
    [props.setAst, props.renderedContent, isGenerated])` and provides it.
  - The `buildNestingCommitDestination(editTargetRef.current)` destination
    computation stays where it is — only the final `setAstRef.current(payload)`
    becomes `documentStore.commit(...)`.
  - **Test first:** `fail-on-revert` — break `commit` and confirm a commit-path
    test (`s6-delete-by-emptying`, `commit-destination-equivalence`) goes red.
  - **Implement** → run the full edit/commit suite → commit.

---

## Phase 4 — `OverlaySlot`

- [ ] **Task 4.1 — Create `OverlaySlot` (test-first).**
  - **Interfaces (Consumes / Produces):** Consumes: the breadcrumb-chip mount
    pattern in `PreviewDocument.tsx` + `BreadcrumbChip.tsx`. Produces:
    `OverlaySlot` component + `useOverlaySlot()` accessor + `OverlaySlot`
    interface (see Produces).
  - **Test first:** `OverlaySlot.test.tsx` — render `<OverlaySlot hostRef>`; via
    `useOverlaySlot().render(<span data-testid="chip"/>)` assert the chip appears
    inside the slot and is removed when `render(null)`. Run → fails. Implement →
    green. Commit.
  - **Note:** Plan 1 does **not** move the breadcrumb into the OverlaySlot
    (that's nesting-cursor-specific → Plan 4). It only *provides* the slot and
    proves it works. The breadcrumb keeps its current mount until Plan 4.

---

## Phase 5 — Surface contract + `TextareaSurface` + `EditBufferCache` + seam plumbing

This phase has three sub-phases. **5a** defines the `EditingSurface` contract and
extracts `TextareaSurface` (moving `caretGeometry` inside it). **5b** extracts
`EditBufferCache` + the shared generated-vs-raw predicate and re-points the
seed/re-wrap. **5c** wires the `NodeOverride` super-chain + the root
`ViewController` so the mode renders the *selected surface* fed by the cache
(mode↔surface decoupling). Do 5a and 5b before 5c — 5c consumes both.

### Phase 5a — `EditingSurface` contract + in-tree `TextareaSurface` (DELTA B)

- [ ] **Task 5a.1 — Define the `EditingSurface` contract types (test-first).**
  - **Interfaces (Consumes / Produces):** Consumes: keystone §5; today's
    `editTarget.boxStyle`/`contentHeight` (the `MeasuredBox` shape), `CaretHint`
    (from `caretGeometry`). Produces: `framework/surface/types.ts`
    (`EditingSurfaceProps`, `EditingSurfaceHandle`, `EditingSurfaceComponent`,
    `MeasuredBox`, re-exported `CaretHint`).
  - **Test first:** `framework/surface/types.test.ts` — a **type-level** + tiny
    runtime test: a trivial `forwardRef` component typed as
    `EditingSurfaceComponent` compiles, exposes `focus()` on its handle, and
    accepts the full `EditingSurfaceProps` (assert by mounting a stub surface and
    calling `ref.current.focus()`). Run → fails (module absent). Implement →
    green. Commit. (This is the contract-completeness proof Plan 7 leans on.)

- [ ] **Task 5a.2 — Extract `TextareaSurface` from `EditTextarea` +
  `renderMeasuredEdit` + `caretGeometry` (test-first).**
  - **Interfaces (Consumes / Produces):** Consumes: `EditTextarea`
    (`dispatchers.tsx:137-484`), `renderMeasuredEdit` (`:60-83`),
    `caretGeometry.ts`, `editBaseline`/`isDirty` (the dirty tracking, from
    `outerBlocks.ts`). Produces: `framework/surface/TextareaSurface.tsx` exporting
    `TextareaSurface: EditingSurfaceComponent`, with:
    - `value` ← clean per-block markdown (was `seededDraft`/`editBaseline`);
    - `box` ← `MeasuredBox`; renders the measure-and-set wrapper (keeps
      `id="q2-active-edit-region"`, the zero-reflow box, and `LEFT_INSET_STRIPPED`
      left-inset drop);
    - `onCommit(text)` ← was `commitIfDirty`'s commit branch;
    - `onCancel()` ← was the cancel branch;
    - `onChange(text)` ← dirty tracking via `isDirty`/`editBaseline` (now
      internal);
    - `onEdgeReached(dir)` ← computed **inside** the surface from `caretGeometry`'s
      `isOnFirstVisualLine`/`isOnLastVisualLine`/`getLogicalColumn` on the arrow
      keydown (replacing the mode's direct `caretGeometry` use at
      `dispatchers.tsx:13`);
    - `EditingSurfaceHandle.focus(caret?)` ← wraps `placeCaretAtColumn` +
      autofocus (was the `useLayoutEffect` pending-caret apply at
      `dispatchers.tsx:171-181`);
    - handle edge/caret query methods (`isOnFirstVisualLine` etc.) surfaced for
      the mode's cross-surface landing — **the mode reads these off the handle,
      never imports `caretGeometry`.**
    - **`caretGeometry` becomes internal:** `git mv caretGeometry.ts
      framework/surface/caretGeometry.ts` (or import privately); leave a shim at
      `q2-preview/caretGeometry.ts` only until the dispatcher stops importing it
      (deleted in Phase 6 cleanup). The nesting chord/keydown logic of
      `EditTextarea` that is **mode-specific** (nesting moves) does NOT go in
      `TextareaSurface` — it is surfaced via `onEdgeReached` + the handle and
      handled by the controller (Plan 4 owns the nesting moves).
  - **Test first:** `framework/surface/TextareaSurface.test.tsx` — mount
    `<TextareaSurface value="alpha" box={…} ref onCommit onCancel onEdgeReached/>`
    and assert: (1) it renders an editable region carrying value `alpha`;
    (2) typing + Cmd-Enter fires `onCommit(newText)`; (3) escape fires
    `onCancel()`; (4) an arrow at the first visual line fires `onEdgeReached('up')`;
    (5) `ref.current.focus({column:2})` places the caret (assert via mocked
    `placeCaretAtColumn`). Use the existing `caretGeometry.test.ts` mocking
    patterns. Run → fails. Implement → green. Commit.
  - **Risk:** keep `EditTextarea`'s behaviour byte-for-byte where it is *surface*
    behaviour; only the **mode-coupling** (direct `caretGeometry` import, direct
    `ctx.commit*`) is severed via the contract. Run the surface-touching subset of
    `s*`/`p*`/`caretGeometry.test.ts` before committing.

### Phase 5b — `EditBufferCache` + the shared generated-vs-raw predicate (DELTA A + D)

- [ ] **Task 5b.1 — Define the shared `GeneratedPredicate` (the single
  generated-vs-raw decision) (test-first).**
  - **Interfaces (Consumes / Produces):** Consumes: the two places the
    prefixing-container set is encoded today — `regenerateNestedBuffers`'
    doc/behaviour (BlockQuote / BulletList / OrderedList / DefinitionList; the
    *generator* side) and `LEFT_INSET_STRIPPED_TYPES` (`dispatchers.tsx:40-42`:
    BulletList/OrderedList/DefinitionList; the *renderer* side). Produces:
    `framework/coreServices/EditBufferCache.ts`'s
    `GeneratedPredicate` + a single `defaultGeneratedPredicate(node, resolved)`
    that returns `true` when the block is reached **inside a prefixing container**
    (blockquote / list / def-list). This is the **one** predicate both
    `EditBufferCache.editableTextFor` (seed) and `DocumentStore.commit` (re-wrap)
    consult, so the seed and the re-wrap cannot diverge.
  - **Decision to pin (spike, see Notes):** the precise generated-vs-raw rule
    must match what `regenerate_nested_buffers` (the Rust generator) actually
    keyed — i.e. a buffer is pushed exactly when the generator emitted one. Cross-
    check: a node is "generated" iff `regenerate_nested_buffers` would have a key
    for it. Derive the predicate from the **reachability class chain** (a block
    whose ancestor is a prefixing container — `ReachabilityClass` 'Descendable'
    under blockquote/list/def-list) rather than re-listing types, if the source
    index carries enough. If the reachability data is insufficient to reconstruct
    the generator's exact key set, treat this as a **spike** and confirm with the
    user — do not guess a predicate that silently disagrees with the pushed map
    (that would resurrect the seed↔re-wrap divergence the keystone forbids).
  - **Test first:** `EditBufferCache.predicate.test.ts` — for a fixture pool with
    a blockquote→child-para and a flush-left para, assert
    `defaultGeneratedPredicate(childPara, resolved)` is `true` and
    `defaultGeneratedPredicate(flushPara, resolved)` is `false`. Cross-validate
    against the `nestedEditBuffers` keys the existing fixtures inject
    (`g19-spurious-dirty` injects `'0:6-14:0'` for the blockquote child; the
    predicate must say `true` for exactly that node). Run → fails. Implement →
    green. Commit.

- [ ] **Task 5b.2 — Implement `PushedEditBufferCache` + the `acceptPushedBuffers`
  population port (test-first).**
  - **Interfaces (Consumes / Produces):** Consumes: today's `seedForRange`
    (`outerBlocks.ts:684-693`) — its `seededDraft = nestedEditBuffers?.[siKey] ??
    anchorSlice` is the exact behaviour to port; `serializeSourceEntry` (the
    siKey); `sliceBytes` + `normalizeLineEndings` (the raw slice). Produces:
    `EditBufferCache` interface, `createPushedEditBufferCache`,
    `PushedEditBufferCache` (with `acceptPushedBuffers` **off** the public
    interface), `editBufferKey`, on `EditBufferCache.ts` (see Produces).
  - **Test first:** `EditBufferCache.test.ts` — three cases per DELTA A:
    1. **pushed lookup:** after `acceptPushedBuffers({ '0:6-14:0': 'oh' })`,
       `editableTextFor(childParaNode)` (whose `editBufferKey` is `'0:6-14:0'`)
       returns `'oh'`.
    2. **raw fallback:** a flush-left para (predicate `false`, or no pushed entry)
       → `editableTextFor` returns
       `normalizeLineEndings(sliceBytes(content,r0,r1)).trimEnd()` — identical to
       today's `anchorSlice`.
    3. **never-leaks-not-ready:** a generated node whose push hasn't arrived yet
       falls back to the raw slice (no `undefined`, no throw).
    Reuse the fixture shape from `g19-spurious-dirty` so the keys line up. Run →
    fails. Implement → green. Commit.
  - **`prevalidating-test-seams`:** bind case (1) to the `acceptPushedBuffers`
    call (revert the call → case 1 must fall back to raw and the assertion must
    fail), proving the population port is load-bearing, not decorative.

- [ ] **Task 5b.3 — Provide `EditBufferCache` via `CoreServicesContext`; feed it
  from the existing parent-push plumbing; repoint `seedForRange` consumers.**
  - **Interfaces (Consumes / Produces):** Consumes: the parent generate-and-push
    chain — `regenerateNestedBuffers` (`wasmRenderer.ts:808-822`, **unchanged**) →
    `Q2PreviewIframe.nestedEditBuffers` (`:88-93,251,269`) →
    `PreviewRoot.nestedEditBuffers` (`:162-165,296`) →
    `PreviewContext.nestedEditBuffers` (`:172-177`) → `useBlockEditHover`'s
    `seedForRange` (`:96-98`). Produces: `PreviewRoot` constructs
    `editBufferCache = useMemo(() => createPushedEditBufferCache({ content,
    sourceResolver, isGenerated: defaultGeneratedPredicate }), […])`, calls
    `editBufferCache.acceptPushedBuffers(props.nestedEditBuffers ?? {})` in an
    effect when the pushed map changes, and provides it on `CoreServicesContext`.
    `useBlockEditHover` stops calling `seedForRange` and instead calls
    `editBufferCache.editableTextFor(node)` for the `value`; the
    `anchorSlice`/dirty baseline it still needs comes from the surface (Phase 5a)
    / `captureEditTarget`. `seedForRange` is deleted (or shimmed) once no caller
    remains.
  - **The eager-population boundary stays.** The iframe still has no WASM; the
    parent still generates and pushes. Plan 1 only routes the push **through the
    port** (`acceptPushedBuffers`) instead of straight into context. Modes never
    see the population — they call `editableTextFor`. **Do not** attempt the
    in-iframe `WasmEditBufferCache` (out of scope).
  - **Test first:** a `fail-on-revert` integration test — the
    `g19-spurious-dirty`/`nest-caret`/`s4-dirty-caret-col` behaviour must stay
    green **through** the cache (a nested editor opens seeded with the clean
    buffer, not the raw slice). Temporarily skip `acceptPushedBuffers` → those
    tests go red → restore. This proves the port carries the pushed buffers.
  - **Implement** → run the full nested-buffer suite (`g19`, `nest-caret`, `s4`,
    `g9-reland-fade`, `p3-2`) → commit. **Note for Plan 5/Plan 4:** the epic's
    test-ownership split assigns the *flat* indented-block round-trip to Plan 5
    and the *inner-surface* regen to Plan 4; Plan 1 only proves the **port +
    predicate** here. Keep those mode-specific tests where they are (do not move
    them into Plan 1 ownership).

### Phase 5c — `NodeOverride` super-chain + root `ViewController` (mode renders the selected surface)

- [ ] **Task 5c.1 — `composeNodeOverrides` + `NodeOverride`/`ModeApi` types
  (test-first).**
  - **Interfaces (Consumes / Produces):** Consumes: keystone §4.1 semantics.
    Produces: `framework/mode/types.ts`, `framework/mode/composeOverrides.tsx`
    (`composeNodeOverrides`). Pure logic — fully unit-testable.
  - **Test first:** `composeOverrides.test.tsx` — assert: no matching override →
    `base()` is returned; a `pass-through` override returns `renderDefault()`; a
    `replace` override (ignores `renderDefault`) returns its own node; **order**
    — two matching overrides compose outermost-first. Mirror the keystone's
    super-chain diagram. Run → fails. Implement → green. Commit.

- [ ] **Task 5c.2 — `ModeContext` + `useMode()` + `NO_OP_MODE` (test-first).**
  - **Interfaces:** Consumes: `SourceResolver`, `DocumentStore` (for the
    baseline). Produces: `framework/mode/ModeContext.ts` (`ModeContext`,
    `useMode`, `NO_OP_MODE`).
  - **Test first:** `ModeContext.test.tsx` — `useMode()` with no provider returns
    `NO_OP_MODE`; with a provider returns its value; the **baseline**
    `resolveSource`/`commit` are present even in `NO_OP_MODE` when core services
    are supplied. Run → fails. Implement → green. Commit.

- [ ] **Task 5c.3 — Update `ViewControllerProps` with `editBufferCache` + `surface`
  (DELTA C) (test-first).**
  - **Interfaces (Consumes / Produces):** Consumes: keystone §4.2;
    `EditBufferCache` (Phase 5b), `EditingSurfaceComponent` (Phase 5a). Produces:
    the `ViewControllerProps` shape in `framework/mode/types.ts` extended with
    `editBufferCache: EditBufferCache` and `surface: EditingSurfaceComponent`; the
    `activeMode.ts` binding accepts the optional `surface?` root prop (default
    `TextareaSurface`).
  - **Test first:** a type+runtime test (`activeMode.test.tsx`) that a
    `ViewController` receiving the props can read `props.editBufferCache
    .editableTextFor(node)` and reference `props.surface`. Run → fails (props
    missing). Implement → green. Commit.

- [ ] **Task 5c.4 — Wire the dispatcher to the super-chain + mount one root
  `ViewController`; the in-tree controller renders the selected surface fed by the
  cache (DELTA C) (test-first).**
  - **Interfaces (Consumes / Produces):** Consumes: the `Block`/`CustomBlock`
    dispatcher (post-2.3), `composeNodeOverrides`, `ViewController`/`ModeApi`,
    `CoreServicesContext` (now incl. `editBufferCache`), `TextareaSurface`, the
    hardcoded textarea swap currently at `dispatchers.tsx:531-538,594-597`.
    Produces:
    - the dispatcher composes `nodeOverrides` (read from a new `OverridesContext`
      the `ViewController` populates) over the vanilla base, **replacing** the
      `isBlockEditTarget` → `renderBlockTextarea`/`renderMeasuredEdit` branch;
    - `PreviewRoot` mounts **one** `ViewController` wrapping the document, passing
      `ViewControllerProps` from the core services **including `editBufferCache`
      and `surface` (default `TextareaSurface`)**; the framework wraps `children`
      in `<ModeContext.Provider value={exposeHook()}>`;
    - baseline `ModeApi = { resolveSource: sourceResolver.resolve, commit:
      documentStore.commit }` is **always installed** (so render-components stay
      editable with no mode active) — `NO_OP_MODE` zeroes only mode-specific
      extras.
  - **The in-tree default `ViewController` is the repackaged controller** (the
    shared substrate of both bundled modes). In Plan 1 it lives in-tree (e.g.
    `q2-preview/bundledMode/DefaultViewController.tsx`) and contributes:
    - `handleInput` = the `useBlockEditHover` activation logic (moved behind the
      seam);
    - `exposeHook` = the current edit/nesting `ModeApi` extras + baseline;
    - a single state-predicated `NodeOverride` whose `matches` is today's
      `isBlockEditTarget` (`dispatchers.tsx:109-121`) and whose `render`
      **ignores `renderDefault` and returns the SELECTED surface** —
      `<props.surface value={props.editBufferCache.editableTextFor(node)} box={…}
      ref={handleRef} onCommit={(t) => modeRoutesTo(documentStore.commit)}
      onCancel={…} onEdgeReached={(dir) => modeNavigatesSurfaces(dir)} />` — **NOT
      a hardcoded `<EditTextarea/>`** (DELTA C). The mode **delegates** caret/edge
      to the surface handle and **does not import `caretGeometry`**.
    - `renderOverlay` stays empty in Plan 1 (breadcrumb keeps its current mount).
  - **mode↔surface decoupling is the load-bearing change here.** Concretely:
    (1) `value` comes from `editBufferCache.editableTextFor(node)`, not
    `seedForRange`; (2) the active block renders `props.surface`, not a literal
    `EditTextarea`; (3) cross-surface arrow navigation is driven by the surface's
    `onEdgeReached` + the handle's edge queries, not the mode reaching into
    `caretGeometry`; (4) commit is `onCommit` → `documentStore.commit`.
  - **Two-mode factoring requirement (for Plans 4 + 5).** This in-tree controller
    is temporary scaffolding that keeps current behaviour green; it is *replaced*
    by two independent bundled extensions — `nesting-cursor` (Plan 4) and
    `block-editing` (Plan 5). So the pieces both modes share must be lifted as
    **reusable, mode-agnostic helpers**, not buried in this controller: the
    activation/hover/cross-surface-arrow logic, the **byte-offset identity +
    self-heal / re-anchor** logic (co-located with `NodeLocator`/`DocumentStore`),
    delete-by-emptying, expand-on-edit, **and `EditBufferCache` use** (shared
    substrate — DELTA D). Only tree-awareness (nesting nav, breadcrumb,
    **navigating to *more* surfaces**) is mode-specific. Add the shared pieces to
    the renderer API surface / core services so Plans 4 and 5 both *import* them.
    "Two modes need it ⇒ it's a primitive" (keystone §1).
  - **Test first:** an integration test mounting `PreviewRoot` with the bundled
    controller asserting the SAME externally-observable behaviour as
    `seam-boundary-characterization` (click → `#q2-active-edit-region`;
    `editingDisabled` → no edit region; **nested block seeds the clean buffer via
    the cache**), now flowing through the override chain + the surface contract.
    Use `fail-on-revert`: (a) remove the override registration → click no longer
    opens an editor; (b) swap `props.surface` for a stub surface that renders
    `data-testid="stub-surface"` and confirm the active block renders the stub —
    **proving the mode renders `props.surface`, not a hardcoded textarea.** Run →
    fails. Implement → green. Commit.
  - **Risk:** this is where the textarea-swap moves from the dispatcher to a
    `NodeOverride` rendering the selected surface. The surface *behaviour* is
    `TextareaSurface` (Phase 5a, byte-for-byte where it is surface logic); only the
    *invocation* moves and is now indirected through `props.surface`. Run the
    entire `s*`/`p*`/`g*` suite + Playwright `q2-preview-block-*` specs locally
    before committing.

---

## Phase 6 — Re-point `usePreviewEdit()` + primitives + final cleanup

- [ ] **Task 6.1 — Re-point `usePreviewEdit()` onto the core services
  (test-first).**
  - **Interfaces (Consumes / Produces):** Consumes: `usePreviewEdit.ts`, its
    consumers (the `render-components-{kanban,drag,comment}` demos). Produces:
    `usePreviewEdit()` returns `{ resolveSource, commit }` sourced from
    `useMode()` baseline (core-backed), with **back-compat shims**
    `commitSubtreeEdit(dest, block)` / `commitTextEdit(dest, text)` that call
    `commit({channel:'subtree',…})` / `commit({channel:'text',…})`. Keep the no-op
    degrade when no core services are present (q2-debug/q2-slides).
  - **Test first:** `usePreviewEdit.test.tsx` — under a core-services provider
    with **no** editing mode mounted, `usePreviewEdit().resolveSource` and the
    `commit*` shims still work (the keystone §6 "baseline always live" win). Run →
    fails. Implement → green. Commit.
  - **Then:** run the demo specs in jsdom + the Playwright
    `hub-client/e2e/q2-preview-render-components-{kanban,drag,comment}.spec.ts`
    (the epic's must-stay-green invariant). Record the result in Notes.

- [ ] **Task 6.2 — Export primitives on the renderer surface (test-first).**
  - **Interfaces (Consumes / Produces):** Consumes: the lifted `renderMeasuredEdit`
    (now in `TextareaSurface`), `caretGeometry` (now internal to the surface),
    `byteLineMap.ts`, `sliceSource.ts`, `EditBufferCache` (Phase 5b), the
    `window.__Q2_PREVIEW_RENDERER__` object (`entry.tsx:107-126`). Produces:
    `caretGeometry`/`byteLineMap`/`sliceSource`/`editableTextFor` namespaces added
    to the renderer surface (so Plan 6 can extract the surface and Plans 4/5 reach
    the buffer port). **Exposing `caretGeometry` here is for Plan 6's extraction +
    back-compat, not for modes to import** — the mode reads edge/caret off the
    surface handle.
  - **Test first:** a parity test (style of the existing framework-primitive
    parity test): assert
    `window.__Q2_PREVIEW_RENDERER__.caretGeometry.getLogicalColumn`,
    `.byteLineMap.buildByteLineMap`, `.sliceSource.sliceBytes`,
    `.editableTextFor.createPushedEditBufferCache`,
    `.editableTextFor.editBufferKey` are all functions. Run → fails. Implement →
    green. Commit.

- [ ] **Task 6.3 — Collapse the shims + delete dead `PreviewContext` fields.**
  - **Interfaces (Consumes / Produces):** Consumes: the `sourceIndex.ts` /
    `outerBlocks.ts` / `caretGeometry.ts` re-export shims (Tasks 1.2, 2.2, 5a.2);
    the now-unused `PreviewContextValue` fields (`resolveSource`,
    `commitTextEdit`, `commitSubtreeEdit`, `nestedEditBuffers`, `pool`, `content`
    if fully migrated). Produces: shims deleted, all importers repointed to
    `framework/coreServices/*` / `framework/surface/*`; `PreviewContext` slimmed
    to only what the bundled controller still genuinely needs. **Be conservative:**
    keep any field the bundled controller still reads until Plans 4/5 extract it;
    the keystone says vanilla components must be pool-id/context-free, not that
    `PreviewContext` must vanish in Plan 1. In particular, `nestedEditBuffers`
    leaves `PreviewContext` only once `useBlockEditHover` reads exclusively through
    `editBufferCache` (Phase 5b) — verify no other consumer remains before
    deleting the field.
  - Grep-audit every importer (incl. `hub-client/`, `q2-preview-spa/`,
    `ts-packages/preview-runtime/`) before deleting a shim. Run vitest +
    `npm run build:all`. Commit.

- [ ] **Task 6.4 — Full-suite + build gate + e2e smoke.**
  - Run `cd ts-packages/preview-renderer && npx vitest run` (all green),
    `cd hub-client && npm run build:all` (green), and the Playwright specs the
    epic lists (17× `q2-preview-*`, the 3 render-components demos). For the
    end-to-end `q2 preview` check, rebuild the SPA
    (`cargo xtask build-q2-preview-spa && cargo build --bin q2` — no WASM rebuild,
    no Rust changed) and visually confirm a click opens an editor in a real
    browser, seeded correctly for a nested block. Record the invocation +
    observation in Notes (CLAUDE.md end-to-end rule). Do **not** push; prepare the
    commit and ask the user.

---

## Test surfaces that must stay green (from the epic; Plan 1 owns these)

- **Vitest/jsdom** in `q2-preview/`: `sourceIndex.test.ts`,
  `outerBlocks.integration.test.ts`, `outerBlocks-p2-3b.integration.test.ts`,
  `caretGeometry.test.ts`, `commit-destination-equivalence.test.ts`,
  `nestingNav.test.ts`, `useBlockEditHover.integration.test.tsx`,
  `q2-preview.integration.test.tsx`, the **nested-buffer suite**
  (`g19-spurious-dirty`, `s4-dirty-caret-col`, `nest-caret`, `g9-reland-fade`,
  `p3-2-nesting-cursor-context` — these prove the `EditBufferCache` port carries
  the pushed buffers), the `s0`–`s7-*`, `p2-*`, `p3-*`, `g*`
  glitch/identity/self-heal suites, settings/gating.
- **Playwright e2e (both hosts):** the 17× `hub-client/e2e/q2-preview-*.spec.ts`
  incl. `render-components-{kanban,drag,comment}` + 2× `q2-preview-spa/e2e/`.
  Geometry-dependent behaviour is browser-only — the jsdom suite cannot prove the
  coincidence-climb; the Playwright specs do.
- **Shared contract:** `ts-packages/preview-e2e-helpers/src/index.ts`
  (`assertNoReflowOnActivation`, `#q2-active-edit-region`) — the `TextareaSurface`
  extraction must keep the `id="q2-active-edit-region"` and zero-reflow box exactly
  (`dispatchers.tsx:80`).

**Test-ownership note (avoid double-claiming with Plans 4/5):** Plan 1 owns only
the **port + predicate** units for `EditBufferCache` (pushed lookup, raw fallback,
generated-vs-raw predicate — Tasks 5b.1/5b.2) and the characterization that the
pushed buffers reach the seed through the port (Task 5b.3). The **flat indented-
block round-trip** stays Plan 5; the **inner-surface regen** stays Plan 4 (epic
"test surfaces — ownership split").

Use `prevalidating-test-seams` to bind each relocated test to its new module and
`fail-on-revert` after each phase to confirm the green is real.

---

## Verification gates (per CLAUDE.md)

- Per-task: `npx vitest run` (scoped) → `cd hub-client && npm run build:all`.
- End of plan: full `cargo xtask verify` (renderer is hub-reachable) +
  `cd hub-client && npm run build:all` + the Playwright specs above.
- `q2 preview` end-to-end smoke needs the SPA re-embed
  (`cargo xtask build-q2-preview-spa && cargo build --bin q2`) — no WASM rebuild
  since no Rust changed.
- **Never push without explicit user permission.**

---

## Notes / discovery log

- **Spike 1 (Task 2.3) — dispatcher identity stamping.** The exact mechanism for
  moving `data-block-pool-id` stamping from each component's own root element to
  the dispatcher is **not fully pinnable from reading alone**, because the
  coincidence-climb hit-testing (`rectsCoincide`/`resolveOuterBlock` in
  `domHitTest.ts`/`outerBlocks.ts`) depends on the attr being on the *block's*
  element, not on the `AttributionWrap` wrapper `<div>` the dispatcher adds.
  Stamp via a ref-callback onto the rendered root, or a one-level `identityProps`
  pass-down; pin with a failing geometry test FIRST, and **stop and ask the user**
  if it can't be settled — do not guess.
- **Spike 2 (Task 5b.1) — the generated-vs-raw predicate must agree with the Rust
  generator.** The single `GeneratedPredicate` must say "generated" for **exactly**
  the nodes `regenerate_nested_buffers` keys (else the seed↔re-wrap symmetry the
  keystone requires breaks). Derive it from the reachability-class chain
  (prefixing ancestor → 'Descendable') and cross-validate against the
  `nestedEditBuffers` keys the existing fixtures inject (`g19` injects
  `'0:6-14:0'`). If the reachability data is insufficient to reconstruct the
  generator's exact key set, treat it as a spike and confirm with the user — do
  not ship a predicate that silently disagrees with the pushed map.
- **Spike 3 (Task 5c.4) — `handleInput`/`useBlockEditHover` move.** Moving
  `useBlockEditHover`'s activation handlers behind `ViewController.handleInput`
  may surface ordering/`useCallback`-identity subtleties (the handlers are
  `useCallback(…, [])` reading live refs). If the move can't preserve the
  latest-ref pattern cleanly, treat it as a spike and confirm before reshaping.
- **DELTA D correction recorded:** the prior note that "clean-buffer regen is
  mode-specific" is wrong. `EditBufferCache` is shared core substrate consumed by
  both modes; only the inner-surface *navigation* is nesting-specific (see the
  "Correction" section above and Task 5c.4's two-mode factoring requirement).
- Fill in Phase 0 baseline pass-counts and the Task 5b.3/6.1/6.4 e2e
  invocation+observation here as the work proceeds.
