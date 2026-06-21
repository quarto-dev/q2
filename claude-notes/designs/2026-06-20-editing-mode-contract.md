# Editing Extensions Contract (Keystone Design)

**Date:** 2026-06-20 (rev. 2026-06-21: editing-surface axis + `EditBufferCache`)
**Branch:** `editing-mode`
**Status:** DESIGN — keystone. The implementation plans
(`2026-06-20-editing-mode-plan-{1..7}-*.md`) all depend on this document. It
defines the vocabulary, the **two extension types**, the seam contracts, the
core services, and the disposition of the existing `render-components`
mechanism. When an implementation plan and this document disagree, **this
document wins** — fix the plan.

---

## 1. Why this exists

Over ~the last month, block-editing and nesting-cursor functionality was added
directly into the vanilla q2-preview React renderer. Editing logic was threaded
into every block/inline/custom component (each reaches into `PreviewContext`,
calls `resolveSource(node)`, stamps `data-block-pool-id`/`tabIndex`) and into a
single ~77 KB `PreviewRoot` edit state machine. This is **one** way to make a
preview editable; it should be **a plug-in**, not the renderer's nature.

Editing is a **cross-cutting concern** — a single capability smeared across
every node type. That is *why* it polluted every component, and the kind of
concern AOP factors out. The goal is to make the vanilla renderer **pure** and
express editing as **extensions** plugging into a small, named set of seams.

## 2. Two extension types (the key decomposition)

Editing splits along **two orthogonal axes**:

- **Editing mode** — *control / policy*: when/where editing activates, which
  surfaces exist, navigation between them, commit policy. Bundled modes:
  **`block-editing`** (flat) and **`nesting-cursor`** (tree-aware).
- **Editing surface** — *the widget* that presents an active block's editable
  content and emits the edit. Bundled surfaces: **`textarea`** and **`tiptap`**
  (embedded per-block WYSIWYG).

They form a **matrix**: any mode × any surface. The mode renders "the active
surface"; the surface is swappable underneath it. **Two modes × two surfaces** is
the proof the system generalizes — it forces the mode↔surface boundary to be
genuinely complete (the mode never assumes textarea; the surface never assumes a
mode).

> The minimal templates emitted by `q2 create extension editing-mode foo` and
> `q2 create extension editing-surface foo` are the third, trivial proof that a
> third party can author either.

> **AOP lineage (rationale, not naming).** A mode is an *aspect* over the
> cross-cutting editing concern; the per-node seam is *around-advice*. We keep
> the mental model but use plain names. See §4.

## 3. Target architecture — three layers

```
core services (substrate, owned by core)
   SourceResolver · NodeLocator · DocumentStore(.commit) · OverlaySlot · EditBufferCache
        ▲                                   ▲
        │ fed by                            │ consumed by
   editing SURFACE (the widget)        editing MODE (control/policy)
   textarea · tiptap                   block-editing · nesting-cursor
   EditingSurface contract             NodeOverride[] + ViewController
        ▲───────────── rendered by ─────────┘
```

Data flow of one edit: **mode** activates a block → fetches
`editBufferCache.editableTextFor(node)` → renders the **active surface** with
that text → surface emits edited markdown via `onCommit(text)` → mode routes to
**`DocumentStore.commit`** (which re-wraps prefixing containers) → re-render.

With no mode active, the seams are empty and the renderer is read-only. Vanilla
components are `node → React` again.

## 4. Editing-mode contract

### 4.1 `NodeOverride` — the per-node seam

The mode's chance to change how a **specific node instance** renders, from
runtime state. Generalizes the hardcoded textarea swap at `dispatchers.tsx:526`.

```ts
type NodeOverride = {
  matches: (node: BlockNode | InlineNode, mode: ModeApi) => boolean;   // the "pointcut"
  render: (node, renderDefault: () => React.ReactNode) => React.ReactNode;
};
```

**Semantics — "override with optional `super`"** (`renderDefault` ≈ `super`):
pass-through = `return renderDefault()`; wrap = call it then modify; replace =
don't call it (active block → surface). **Composition:** a mode contributes
`nodeOverrides: NodeOverride[]`; matching overrides compose in declaration order,
**outermost-first**, each `renderDefault` rendering the remainder of the chain
down to the vanilla base. Usually shallow (active target → surface over base).

> `render-components` are the degenerate `NodeOverride` (static, type-keyed,
> ignores `renderDefault`). See §8.

### 4.2 `ViewController` — the per-session seam

> **PROVISIONAL NAME** (`ModeRoot`/`SessionController` are alternates).

A single component the active mode mounts at the render root, wrapping the
document. It exercises three capabilities:

```ts
type ViewController = (props: ViewControllerProps) => {
  handleInput?: RootInputHandlers;      // root events; uses NodeLocator (activation/keys)
  renderOverlay?: () => React.ReactNode; // floating UI into OverlaySlot (e.g. breadcrumb)
  exposeHook: () => ModeApi;            // the ONE hook this mode publishes (read via useMode())
};
type ViewControllerProps = {
  children: React.ReactNode; hostRef: React.RefObject<HTMLElement>;
  sourceResolver: SourceResolver; documentStore: DocumentStore;
  nodeLocator: NodeLocator; overlaySlot: OverlaySlot; editBufferCache: EditBufferCache;
  surface: EditingSurfaceComponent;     // the SELECTED surface the mode renders for active blocks
  settings: Record<string, unknown>;
};
```

**Active-mode binding (CONSUMED BY PLAN 2/4/5):**
`ActiveMode = { viewController, nodeOverrides, settings }`; the preview root takes
an optional `activeMode?` prop. Provided → mount it; absent → in-tree bundled
fallback (Plan 1). Plan 2's host shim resolves the selected extension(s) →
`ActiveMode` + selected `surface` and sets the props.

## 5. Editing-surface contract

The widget the mode renders for an active block. **Markdown string in, markdown
string out** — uniform across textarea and tiptap.

```ts
type EditingSurfaceComponent = React.ForwardRefExoticComponent<
  EditingSurfaceProps & React.RefAttributes<EditingSurfaceHandle>>;

interface EditingSurfaceProps {
  value: string;                 // clean per-block markdown (mode got it from EditBufferCache)
  box: MeasuredBox;              // measure-and-set geometry to size into
  initialCaret?: CaretHint;
  onChange?(text: string): void; // dirty tracking
  onCommit(text: string): void;  // edited markdown → mode routes to DocumentStore.commit
  onCancel(): void;
  onEdgeReached(dir: 'up'|'down'|'left'|'right'): void; // arrow-out → mode navigates surfaces
}
interface EditingSurfaceHandle {
  focus(caret?: CaretHint): void;
  // edge/caret queries the mode needs for cross-surface landing (provided by the surface)
}
```

**The hard boundary:** geometry/navigation (`onEdgeReached`, caret placement,
first/last-visual-line) is the **surface's** responsibility, not the mode's.
`caretGeometry` becomes the **textarea surface's internal implementation**; tiptap
implements the same handle methods via ProseMirror selections. The mode delegates
to the surface and never assumes textarea.

**`tiptap` is an *embedded per-block surface*, not a host.** It parses the
block's markdown → ProseMirror → WYSIWYG edit in a bounded element → serializes
back to markdown on commit. The whole-doc fidelity problem (see §13) collapses to
per-block; the boundary is just a markdown string from `EditBufferCache`.

## 6. The stable accessor and baseline `ModeApi`

The framework owns one accessor; the mode supplies its value via `exposeHook`.

```ts
const ModeContext = React.createContext<ModeApi | null>(null);
export function useMode(): ModeApi { return useContext(ModeContext) ?? NO_OP_MODE; }

type ModeApi = {
  resolveSource: (node: BlockNode) => ResolvedSource | null;  // from SourceResolver (core)
  commit: CommitFn;                                           // from DocumentStore (core)
  // …mode-specific extras (editTarget, nesting state); only the mode's own
  //   NodeOverrides may rely on them — shared components use only the baseline.
};
```

> **PROVISIONAL NAME** (`useMode()` vs `useEditing()`).

**The baseline is core-backed, always live** (even with no mode); `NO_OP_MODE`
zeroes only the mode-specific extras. This keeps render-components editable with
no editing mode mounted.

## 7. Core services (owned by core; the substrate)

1. **`SourceResolver`** — `node → ResolvedSource | null` (source range +
   reachability/prefixing class). From `sourceIndex.ts`.
2. **`DocumentStore`** — the live AST + the one mutation entry `commit` (which
   re-wraps prefixing containers on commit). From the AST + `setAst` +
   `commitTextEdit`/`commitSubtreeEdit`, extracted from `PreviewRoot`. See §9.
3. **`NodeLocator`** — DOM ↔ node. Core stamps identity at the dispatcher
   (replacing per-component pool-ids). Includes the **byte-offset identity +
   self-heal / re-anchor** logic (the concurrency-correctness code) — shared by
   both modes.
4. **`OverlaySlot`** — positioned layer above content that `renderOverlay` fills.
5. **`EditBufferCache`** — **the node → clean-editable-buffer service.** Given a
   node, returns its editable text: a raw source slice for flush-left blocks
   (source fidelity), or the **generated AST serialization** for prefixing
   containers (blockquote/list/def-list), where a raw slice is not a faithful
   editable buffer. **Both modes consume it** (nesting just calls it for more,
   inner surfaces). See §7.1.

### 7.1 `EditBufferCache` — a swappable iframe-side port

The iframe has **no WASM**; only the **parent** (hub-client / q2-preview-spa) can
serialize nodes (`write_single_block` via WASM). So the parent generates and
**pushes** buffers down (today: `nestedEditBuffers`). Eager population is forced
by that boundary — fixing it (WASM in the iframe) is **out of scope**. We hide all
of it behind a port so modes never know:

```ts
interface EditBufferCache {            // the stable interface both modes depend on
  editableTextFor(node: BlockNode): string;  // sync; generated-or-raw; never leaks "not ready"
}
```

- **Today — `PushedEditBufferCache`** (Plan 1): holds the parent-pushed map;
  `editableTextFor` is a keyed lookup with raw-slice fallback. Has a population
  port **off the public interface**: `acceptPushedBuffers(Record<key,string>)`,
  fed by the parent's existing generate-and-push plumbing.
- **Later — `WasmEditBufferCache`** (out of scope): serializes lazily via
  in-iframe WASM; same interface; no population port. **Swapping = construct a
  different impl at the root; modes untouched.**

Invariants that make the swap free: modes depend only on `editableTextFor`;
population is not on the interface; the identity **key** (today's siKey
`"0:<r0>-<r1>:0"`) is a shared derivation all impls + the parent generator agree
on; `editableTextFor` is **synchronous** (lookup or in-thread WASM both are). The
**generated-vs-raw decision is a single shared predicate** the cache and
`DocumentStore.commit` both consult, so the seed and the re-wrap can't diverge.

**The detect-router (modes never decide).** `editableTextFor(node)` routes on
`isPrefixed(node)` (the shared predicate): **prefixed** → the *generated* branch
(the WASM-pushed buffer for deep-nested blocks; for the **outermost** prefixing
container the cache produces the de-prefixed buffer **in TS** — **option B**, the
mechanism in production since mid-June, which needs no iframe WASM); **flush-left**
→ the **raw source slice** (the user's exact, un-reformatted markdown — kept
*because* it is unreformatted). The cache holds both sources (raw content + pushed
map); `DocumentStore.commit`'s re-wrap is the exact inverse of the generated
branch. (We do **not** extend `pampa` to emit the container buffer — option A is
out of scope.)

## 8. Disposition of `render-components`

Not removed. (1) **Delivery rail — kept, generalized:** Plan 2 adds extension
sources into the same transpile→`LOAD_CUSTOM_COMPONENTS`→merge pipeline; one
rail, multiple front doors (document attribute, mode/surface extensions). (2)
**Component model — folds into `NodeOverride`** (static type-keyed override). (3)
**Editing API — decomposed into core services:** `usePreviewEdit()` → the
baseline `{ resolveSource (SourceResolver), commit (DocumentStore) }` via
`useMode()`, available with no mode active. **Coexistence:** a document may run
render-components *and* a mode; both register `NodeOverride`s and compose on the
one chain. "At most one mode / one surface" (§10) is about `ViewController`/
`exposeHook` and the active surface, not about overrides.

## 9. Commit API + boundary-splice coordination

**Build on current API, migrate later** (do not block on
`2026-06-19-boundary-splice-implementation.md`). `DocumentStore.commit` v1 wraps
today's `commitTextEdit`/`commitSubtreeEdit`; type `CommitFn` as a small union so
the later collapse to `commit(splice)` is a body change behind a stable seam.
The commit path owns the **re-wrap** of a generated buffer back into its
container (the inverse of `EditBufferCache`'s generation).

## 10. Selection and settings (config, not seams)

- **Selection — two axes.** At most **one mode** and **one surface** active.
  Config-driven (project/document option and/or hub toggle). Plan 2 builds both.
- **Settings.** A mode/surface **declares** its settings in `_extension.yml`
  (e.g. nesting-cursor's `unlockNestingCursor`); the host renders the control and
  feeds values via `ViewControllerProps.settings` / surface props.

## 11. Composition algebra (why single-active is the v1 boundary)

Multi-mode/multi-surface is **out of scope**; reference only.

| Seam | Stackable? | Model |
|---|---|---|
| `NodeOverride` | ✅ | ordered super-chain |
| `renderOverlay` | ✅ (freest) | additive z-stack in `OverlaySlot` |
| `handleInput` | ⚠️ partial | chain-of-responsibility w/ gesture arbitration |
| `exposeHook` | ❌ | singular/keyed — one `useMode()` value |
| active surface | ❌ | one widget renders an active block at a time |
| core services | n/a | singletons; substrate everything composes against |

`exposeHook`'s singularity is **physics** (one document / one `DocumentStore`),
not design. Relaxing later: overrides + overlays are free; input needs
arbitration; `exposeHook` needs keying.

## 12. The bundled extensions + the matrix

| Bundled | Type | NodeOverride(s) | ViewController | Surface usage |
|---|---|---|---|---|
| `block-editing` | mode | one (active→surface) | handleInput=flat activation+arrows; exposeHook; **no overlay** | renders active surface |
| `nesting-cursor` | mode | one (active→surface) | + nesting keys; renderOverlay=breadcrumb; + clean-buffer for inner surfaces | renders active surface |
| `textarea` | surface | — | — | reference `EditingSurface` (caretGeometry inside) |
| `tiptap` | surface | — | — | embedded per-block WYSIWYG `EditingSurface` |

`block-editing` ⊂ `nesting-cursor` in features; **independent siblings** in code.
Their shared feature set (textarea/caret primitives, **byte-offset identity +
self-heal/concurrency**, activation, cross-surface arrows, delete-by-emptying,
expand-on-edit, **`EditBufferCache`**) is Plan 1 substrate — *"two consumers ⇒
it's a primitive."* Only tree-awareness is nesting-specific.

## 13. TipTap (corrected)

Earlier we rejected "tiptap as host/tenant" — correct: a ProseMirror
`EditorView` owns the doc model + contenteditable, the mirror image of our
contract. **But "tiptap as an embedded per-block editing surface" is different and
viable:** a bounded widget editing one block's markdown behind the
`EditingSurface` contract (§5), fed by `EditBufferCache`. The fidelity concern
(TipTap's marked-based markdown ≠ qmd) is bounded per-block and is the
**`tiptap`-surface extension's** problem (Plan 7). Prior research to lean on:
`MarkdownManager.parse/serialize`, per-extension `parseMarkdown`/`renderMarkdown`.

## 14. Vocabulary (use these exact names)

- **Extension types:** `editing-mode` · `editing-surface`
- **Mode seams:** `NodeOverride` · `ViewController` *(prov.)* (`handleInput`/
  `renderOverlay`/`exposeHook`)
- **Surface contract:** `EditingSurface` (`EditingSurfaceProps`/`EditingSurfaceHandle`)
- **Core services:** `SourceResolver` · `NodeLocator` · `DocumentStore`(`.commit`)
  · `OverlaySlot` · `EditBufferCache` (`editableTextFor`; `PushedEditBufferCache`)
- **Accessor:** `useMode()` *(prov.)* · `ModeContext` · `NO_OP_MODE` ·
  `ModeApi = { resolveSource, commit }`
- **Binding/config:** `ActiveMode` + `activeMode?` prop · single-active mode +
  surface selection · declarative settings

## 15. Open names to settle (cosmetic, find-replace later)

1. `ViewController` vs `ModeRoot`/`SessionController`.
2. `useMode()` vs `useEditing()`.
3. The `_extension.yml` keys for `editing-mode:` / `editing-surface:`
   contributions.
4. `EditBufferCache` vs `EditBufferService`/`EditBufferProvider` (lean: keep
   `EditBufferCache`).
