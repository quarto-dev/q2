# Editing-Mode Plan 7 — `tiptap` as a Bundled Editing-SURFACE Extension (embedded per-block WYSIWYG)

> **For agentic workers:** REQUIRED SUB-SKILL: use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. The binding design is the keystone — `claude-notes/designs/2026-06-20-editing-mode-contract.md` (rev. 2026-06-21); **when it and this plan disagree, the keystone wins** (fix the plan). Use the keystone vocabulary verbatim: `EditingSurface` (`EditingSurfaceProps`/`EditingSurfaceHandle`/`EditingSurfaceComponent`), `MeasuredBox`, `CaretHint`, `ViewController`, `EditBufferCache`, `DocumentStore`, `NodeOverride`, `useMode()`.

**Date:** 2026-06-20 (drafted 2026-06-21)
**Branch:** `editing-mode` (worktree `.worktrees/editing-mode`)
**Layer:** TypeScript (a new bundled extension dir under `resources/extensions/quarto/`, `ts-packages/preview-renderer`, host SPA bundling in `q2-preview-spa` / `hub-client`)
**Status:** PLAN — ready for TDD execution.
**Depends on:** Plan 1 (core services + seams + **the `EditingSurface` contract + the in-tree `TextareaSurface` reference + React/primitive exposure on `window.__Q2_PREVIEW_RENDERER__`**) and Plan 2 (the **editing-surface** extension type + discovery + delivery + two-axis selection). Keystone: `claude-notes/designs/2026-06-20-editing-mode-contract.md` — **the keystone wins on any conflict**. Epic index: `claude-notes/plans/2026-06-20-editing-mode-epic.md` (this is "Plan 7", the `tiptap` editing-SURFACE).
**Siblings:** Plan 6 (`…-plan-6-textarea-surface-extension.md`) — the **reference surface** and the behavioural/packaging template this plan mirrors; Plans 4/5 — the **modes** that render whichever surface is selected. **Plan 7 is the dual of Plan 6 on the surface axis** and must work **unchanged under any mode** (the keystone §2 "any mode × any surface" matrix), and be **swappable with the textarea surface with ZERO mode changes** (keystone §11 "active surface ❌ stackable — one widget renders an active block at a time").

---

## Goal

Ship a bundled `editing-surface` extension at `resources/extensions/quarto/editing-surface-tiptap/` that implements the keystone `EditingSurface` contract (§5) by embedding **TipTap (ProseMirror)** to edit **one block's markdown** as WYSIWYG: parse the clean per-block markdown the mode hands in (`value`) → ProseMirror doc → rich-edit inside an element sized to `box` → serialize back to markdown → `onCommit(text)`.

**Architecture:** The surface is a `forwardRef` `EditingSurfaceComponent` that mounts a per-block TipTap `Editor` via `@tiptap/react`'s `useEditor`/`EditorContent`. The markdown↔doc bridge sits behind a small **`MarkdownEngine` port** (v1 implementation = TipTap's `marked`-based `MarkdownManager` over a curated **CommonMark subset**; a future `pampa.wasm` implementation — "Engine B" — is a drop-in replacement of that port). Any Quarto construct the subset can't model richly is held in a **verbatim NodeView ("code mode")** that round-trips the literal source. Fidelity is **semantic (AST-preserving through pampa's re-parse on commit), not byte-preserving** — q2 normalizes the spliced block on commit, so TipTap's marker choices never persist. TipTap itself is acquired through a single **pluggable accessor** (a global-injection stopgap today; a flagged integration point for the sandbox dependency-provisioning work) and is **lazy-loaded** only when the tiptap surface is the selected surface.

**Tech Stack:** React/TSX, `@tiptap/core` + `@tiptap/pm` + `@tiptap/react` + `@tiptap/markdown` (v3.27.x, `marked`-backed), ProseMirror, Babel-standalone transpile rail, Vite/esbuild SPA bundling, Vitest (jsdom) + Playwright e2e.

---

## Why this plan looks different from Plan 6 (read this first)

Plan 6 is a **faithful extraction** of existing textarea behaviour behind the contract — no new behaviour. Plan 7 is **net-new code** (there is no in-tree TipTap surface to extract), so it is *additive* and TDD-from-scratch, but it consumes the **same** Plan 1 contract + Plan 2 delivery as Plan 6 and must satisfy the **same** orthogonality obligation. Three things make Plan 7 genuinely different from Plan 6, and they are the spine of this plan:

1. **Heavy third-party dependency.** Plan 6's surface references only globals (React). TipTap is `@tiptap/core` + `@tiptap/pm` (11 prosemirror packages) + `@tiptap/react` + `@tiptap/markdown` (+`marked`). The transpile→blob-import delivery rail **cannot resolve npm imports** (verified: no import map, globals-only — `entry.tsx:276-306`, `tsxTranspiler.ts`). So how TipTap reaches the iframe is the central engineering problem. **DECISION (locked with the user 2026-06-21): defer the durable delivery mechanism to the in-flight sandbox/package-import work; v1 ships a global-injection stopgap** (the `window.React`/`window.katex` precedent) behind a pluggable accessor so the surface code is agnostic to *how* TipTap is obtained, and the mechanism swaps later with no surface rewrite. **Lazy-load** so textarea users never pay TipTap's weight.

2. **qmd ≠ TipTap's CommonMark.** TipTap's markdown is `marked`/CommonMark (`MarkdownManager.ts`); qmd is Pandoc-flavored. **DECISION (locked):** v1 supports a **CommonMark subset** richly and routes every other construct to a **verbatim NodeView (code mode)** that preserves the literal source. **Fidelity is semantic**: TipTap's output is re-parsed and normalized by pampa on commit (TipTap → markdown text → parent parse + splice → reconcile/normalize → CRDT), so the only contract is "the emitted markdown re-parses to the *intended* AST." Byte-normalization is a non-issue and serializer-tuning-to-pampa is explicitly **out of scope**.

3. **Rich-text caret vs source-column caret.** `CaretHint`/the handle's caret queries are *source-text columns* (textarea-shaped); TipTap's caret lives in *rendered* text space. **DECISION (locked):** map `onEdgeReached` via ProseMirror `EditorView.endOfTextblock(dir)`; **approximate caret landing to the first/last visual line** (column is advisory/best-effort). This forces a small clarification to the still-open caret/exit-column seam in Plans 1/6 (see **Proposed contract notes**).

The **sync `EditingSurface` contract survives intact** — no async lifecycle change is needed (verified: `new Editor()` is synchronous and headless-capable, `Editor.ts:123-158`; `MarkdownManager.parse`/`serialize` are synchronous, `MarkdownManager.ts:302,333`; in our plain-client SPA `useEditor` returns a live editor on first render because `immediatelyRender` defaults `true` — the Next/SSR null-first-render path does not apply; the only async is `onCreate`/focus via `setTimeout(0)`/`requestAnimationFrame`, which is internal to `focus()`).

---

## The boundary this plan makes real (keystone §5)

The mapping from each `EditingSurface` capability to its TipTap implementation (the dual of Plan 6's extraction table):

| Keystone `EditingSurface` capability (§5) | TipTap/PM implementation in `editing-surface-tiptap` |
|---|---|
| `value: string` in | parsed via the `MarkdownEngine` port → ProseMirror doc → `useEditor({ content })`. Re-seeds on `value` change (mode-driven remount). |
| measure-and-set sized to `box` | the editor's `<EditorContent>` is wrapped by the imported `renderMeasuredEdit` (`// PLAN1:` off the renderer surface) in the synthetic `<div id="q2-active-edit-region" style={box-reproduction}>` — **same id, byte-identical**. WYSIWYG content naturally fits the original block box. |
| `onCommit(text)` | `editor.on('blur')` + the commit chord (`Mod-Enter`) → `markdownEngine.serialize(editor.getJSON())` → `onCommit(text)`. The **mode** routes to `DocumentStore.commit`; the parent re-parses/normalizes. |
| `onChange(text)` | `editor.on('update')` → debounced `markdownEngine.serialize(...)` → `onChange(text)` for the mode's dirty tracking. |
| `onCancel()` | `Escape` keymap → `onCancel()`; the mode tears down (unmount → `editor.destroy()`). |
| `onEdgeReached(dir)` | a ProseMirror keymap on `ArrowUp`/`ArrowDown` (and `ArrowLeft`/`ArrowRight`) computes doc-edge via `view.endOfTextblock(dir)` + first/last-textblock test; on an edge, `preventDefault` + `onEdgeReached('up'\|'down'\|'left'\|'right')`. Non-edge arrows fall through to native PM caret movement. |
| `EditingSurfaceHandle.focus(caret?)` | `editor.commands.focus()` + `setTextSelection` mapped from the `CaretHint` (edge → `TextSelection.atStart/atEnd` of first/last textblock; column → **best-effort** approximation). |
| handle caret/edge queries (for cross-surface landing) | best-effort: `isAtFirstLine()`/`isAtLastLine()` via `endOfTextblock`; exit "column" reported as a best-effort rendered column or `0` (see **Proposed contract notes** — the column is advisory). |

**What stays mode-side (the surface does NOT own):** activation / which block is active; `DocumentStore.commit` routing and the delete-by-emptying *policy*; cross-surface *navigation* (the surface only **signals** `onEdgeReached`); `EditBufferCache` / clean-buffer generation (the mode hands a clean `value`). The surface is **markdown string in, markdown string out**.

**The Rust/WASM backend is unchanged.** Plan 7 is TypeScript + extension-resource + SPA-bundling only. No `crates/` change is expected. If a task wants Rust, the scope was misread — STOP. (The future "Engine B" `pampa.wasm`-in-iframe path *would* touch Rust/WASM build wiring — that is **explicitly out of scope for this plan**; see "Engine B" below.)

---

## Decision: extension packaging & location (sibling of Plan 6)

**Ship `tiptap` as a bundled editing-SURFACE extension at `resources/extensions/quarto/editing-surface-tiptap/`** — the surface-axis sibling of Plan 6's `editing-surface-textarea/`, authored in TSX, declared via a Plan 2 `editing-surface:` manifest, delivered through Plan 2's discovery + the `customComponentsCode`/`LOAD_CUSTOM_COMPONENTS` rail. Same grounded reasons as Plan 6 §"extension packaging" (one rail, `resources/extensions/quarto/` is the bundled-extension home, `_extension.yml` declares the contribution).

Directory shape this plan creates:

```
resources/extensions/quarto/editing-surface-tiptap/
  _extension.yml                 # editing-surface contribution (Plan 2 schema)  [PLAN-2-KEY]
  src/
    index.tsx                    # extension entry: exports the EditingSurfaceComponent (the surface)
    TiptapSurface.tsx            # the forwardRef EditingSurface impl (useEditor + measure-and-set + handle)
    tiptapRuntime.ts             # the pluggable TipTap-acquisition accessor (stopgap global injection; SANDBOX integration point)
    markdownEngine.ts            # the MarkdownEngine port + the v1 marked-based impl (CommonMark subset)
    verbatimNode.ts              # the verbatim "code mode" ProseMirror NodeView + block-kind detection
    edgeKeymap.ts                # arrow-at-edge → onEdgeReached (endOfTextblock)
    caret.ts                     # CaretHint → ProseMirror selection (best-effort)
    extensions.ts               # the curated TipTap extension set (CommonMark subset + verbatim node)
    __tests__/                   # vitest tests (see Phases)
```

> **Shared-primitive import boundary (identical rule to Plan 6).** A discovered/transpiled extension cannot reach into `preview-renderer`'s source tree. So `editing-surface-tiptap` imports React, the measure-and-set wrapper (`renderMeasuredEdit`), and the `EditingSurface` contract **types** off the renderer API surface (`window.__Q2_PREVIEW_RENDERER__`). **INTEGRATION POINT (Plan 1):** the exact accessor names + how the contract *types* are exposed to a transpiled extension are Plan 1's to publish; until known, import by keystone name and mark `// PLAN1:`. The **TipTap packages** are NOT imported through the normal module system (the rail can't resolve npm) — they come through `tiptapRuntime.ts` (see next section).

---

## The TipTap dependency boundary (the central novelty)

This is the part with no Plan 6 analogue. Three sub-decisions, all locked:

### D1 — TipTap is acquired through one pluggable accessor (`tiptapRuntime.ts`)

The surface never writes `import { Editor } from '@tiptap/core'` (that would emit a bare ESM import the blob-module scope can't resolve — verified). Instead it calls a single accessor:

```ts
// tiptapRuntime.ts — the ONE place that knows HOW TipTap is obtained.
export interface TiptapRuntime {
  Editor: typeof import('@tiptap/core').Editor;
  useEditor: typeof import('@tiptap/react').useEditor;
  EditorContent: typeof import('@tiptap/react').EditorContent;
  ReactNodeViewRenderer: typeof import('@tiptap/react').ReactNodeViewRenderer;
  NodeViewWrapper: typeof import('@tiptap/react').NodeViewWrapper;
  Node: typeof import('@tiptap/core').Node;
  MarkdownManager: typeof import('@tiptap/markdown').MarkdownManager;
  // ProseMirror primitives the surface needs directly:
  pmState: typeof import('@tiptap/pm/state');     // TextSelection, EditorState
  // the curated extension constructors (StarterKit subset), pre-built by the provider:
  baseExtensions: unknown[];
}

// v1 STOPGAP: read off a global the host materialized before loading the surface.
// SANDBOX INTEGRATION POINT: replace this body with the sandbox package-import
// mechanism when it lands (Elliot's work). The surface code does not change —
// only this accessor does.
export async function loadTiptapRuntime(): Promise<TiptapRuntime> {
  const g = (window as unknown as { __Q2_TIPTAP__?: TiptapRuntime }).__Q2_TIPTAP__;
  if (!g) throw new Error('[tiptap-surface] TipTap runtime not provisioned by host');
  return g;
}
```

- **v1 stopgap (works today):** the host SPA pre-bundles `@tiptap/*` + the curated extension set and assigns it to `window.__Q2_TIPTAP__` *before* the surface module is dynamic-imported — exactly the `window.React = React` / `window.katex = katex` pattern in `entry.tsx:280-281`. The accessor reads the global.
- **Durable fix (deferred):** when the sandbox/package-import work lands, `loadTiptapRuntime` becomes a real dynamic import; the surface is untouched. This is the flagged **SANDBOX INTEGRATION POINT**.

### D2 — TipTap is lazy-loaded (only when the tiptap surface is selected)

The ~heavy TipTap bundle must not bloat the iframe for textarea users. The host materializes `window.__Q2_TIPTAP__` **only when `editing-surface-tiptap` is the selected surface** (Plan 2's surface-axis selection), via a code-split dynamic `import()` of the tiptap bundle chunk. The surface awaits `loadTiptapRuntime()` once on mount; because the chunk is fetched at selection time it is warm by the time a block activates. **INTEGRATION POINT (Plan 2):** the selection→lazy-provision hook lives in the host (`q2-preview-spa` / `hub-client`); mark `// PLAN2:` where the surface assumes provisioning.

### D3 — The markdown engine is a port (Engine A now, Engine B later)

```ts
// markdownEngine.ts
export interface MarkdownEngine {
  parse(markdown: string, rt: TiptapRuntime): unknown;   // -> ProseMirror doc JSON (JSONContent)
  serialize(doc: unknown, rt: TiptapRuntime): string;     // ProseMirror doc JSON -> markdown
}
```

- **v1 = `MarkedMarkdownEngine`:** wraps `rt.MarkdownManager` (TipTap's `marked`-based parse/serialize), configured with the curated extension set + the verbatim node's `parseMarkdown`/`renderMarkdown` so unsupported constructs survive (see verbatim section).
- **Future = `PampaMarkdownEngine` (Engine B, OUT OF SCOPE):** when `pampa.wasm` is in the iframe, `parse` = `pampa.parse(md) → Pandoc AST → toPmDoc(ast)` and `serialize` = `fromPmDoc(doc) → Pandoc AST → pampa.write` (or PM→qmd). It replaces only this port — same TipTap editor, same `EditingSurface` contract, same NodeView infra. In-iframe `pampa.wasm` is synchronous after module load, so the sync contract is preserved. **Designing v1 behind this port is what makes Engine B a drop-in, not a rewrite.**

> **Why Engine B matters (recorded, not built):** Engine B replaces `marked` (layer 1 of TipTap) with the *real* Quarto parser, eliminating the dual-parser correctness problem and shrinking the verbatim fallback from "everything beyond CommonMark" to "only genuinely opaque constructs." It does **not** remove TipTap — ProseMirror + TipTap remain the editor shell (layers 2–3). Engine B is the robust endpoint; v1 is the pragmatic, forward-compatible now-version.

---

## The verbatim fallback ("code mode") + fidelity contract

### Verbatim NodeView

A single ProseMirror node — `qmdVerbatim` — holds the literal markdown source of any block (or inline span) the CommonMark subset can't represent richly. It renders as a **monospace, editable source box** (the rich-editor equivalent of the textarea's "code mode"), via `ReactNodeViewRenderer` + `NodeViewWrapper`. Its `renderMarkdown` emits its stored text **verbatim** (the semantic-preservation escape hatch); its `parseMarkdown` captures the raw token text.

**Block-kind detection (route reliably, don't rely on `marked` mis-tokenizing):** before handing `value` to the engine, sniff the block kind from the source so unsupported whole-blocks go straight to `qmdVerbatim` rather than through `marked` (which would mis-parse `:::`/`$$`/`` ```{lang} ``/`{{<` into uncontrolled literal text). The sniff is a cheap prefix test (a fenced div `:::`, a display-math `$$`, an attributed/exec code fence ```` ```{ ````, a raw block, a shortcode `{{<`). Inline unsupported constructs (`$x$`, `@cite`, `[s]{.cls}`) are captured by the engine's per-token fallback into inline `qmdVerbatim` atoms.

> **Honest scope note (record in the plan, surface to the user):** for a *fully* unsupported block, the tiptap surface degenerates to "a heavier textarea" (same monospace code-mode UX, more bundle). TipTap's payoff is realized only on (a) richly-editable blocks and (b) *mixed* blocks (rich text containing an inline verbatim atom). This is acceptable for v1 because the keystone makes the surface a single global choice — it cannot defer a block to the textarea without a per-block surface protocol that does not exist.

### Fidelity contract (the acceptance criterion)

Because commit goes **TipTap → markdown text → parent parse + splice → reconcile/normalize → CRDT**, pampa re-normalizes the whole top-level block before it lands. Therefore:

- **Byte-normalization is a non-issue** (TipTap's marker choices never persist). Do **not** tune TipTap's `renderMarkdown` to match pampa.
- **The contract is semantic:** TipTap's emitted markdown must be **valid qmd that pampa re-parses to the *intended* AST**. The round-trip test is `parse → (edit) → serialize → pampa-re-parse yields the equivalent AST`, **not** byte-equality. The verbatim node guarantees this for unsupported constructs (literal source re-parses as intended).

---

## Boundary with Plans 1, 2, 6 (read before starting)

- **Plan 1** publishes the `EditingSurface` contract (`EditingSurfaceProps`/`EditingSurfaceHandle`/`EditingSurfaceComponent`, `MeasuredBox`, `CaretHint`), the in-tree `TextareaSurface` reference, and the renderer-API primitives + **React** on `window.__Q2_PREVIEW_RENDERER__` (`renderMeasuredEdit`, React/`forwardRef`/hooks). Plan 7 consumes these by keystone name (`// PLAN1:`). **Plan 7 adds NO primitive to the renderer surface** — `caretGeometry` is Plan 6's (textarea-internal); the tiptap surface implements the handle via ProseMirror, importing nothing of Plan 6's geometry.
- **Plan 2** defines the `editing-surface:` manifest contribution + discovery + the two-axis selection that installs the selected surface as `ViewControllerProps.surface`, AND (Plan 7-specific) the **lazy provisioning of `window.__Q2_TIPTAP__` when this surface is selected** (`// PLAN2:` integration point).
- **Plans 4/5 (modes)** render "the active surface" — they receive `surface: EditingSurfaceComponent` and mount it; they delegate caret/edge to the surface handle and **never assume textarea**. Plan 7's surface is what they render when `tiptap` is selected. Plan 7 does **no mode work**.
- **Plan 6 (textarea surface)** is the sibling and template. Plan 7 mirrors its packaging, vitest-discovery, React-boundary, and orthogonality-demo conventions. **Reuse, do not re-establish**, whatever Plan 6 settled for vitest discovery of `resources/extensions/quarto/*/src/__tests__/` and the React-on-renderer-surface boundary.

---

## Proposed contract notes (Plans 1/2/6) — edits this plan makes outside Plan 7

These are the only changes outside Plan 7. They are **clarifications / constraints on already-open seams + one deferred delivery seam** — no new fields, no reshaped lifecycle. Each is applied as a marked note in the target plan by Phase 0.5 of this plan (and the edits are committed alongside this plan).

1. **Caret/exit-column queries are best-effort/optional (Plan 1 + Plan 6).** Plan 6's open question #2 (exit-column channel) and Plan 1's candidate `getLogicalColumn()` handle method are **source-text-column** shaped. A rich surface cannot report an exact source column. **Constraint to bake in when that seam is resolved:** type the cross-surface landing column as an **advisory hint** (the mode treats it as approximate), define `initialCaret.column` as **advisory** (a surface MAY approximate to a line edge), and do **not** require `getLogicalColumn`-style exact source columns as a handle method. Name the edge queries neutrally (e.g. `isAtFirstLine()`/`isAtLastLine()`), not in textarea-internal terms.
2. **Heavy-dependency provisioning for surfaces (Plan 1 + Plan 2).** The delivery rail is globals-only (no npm import resolution). Plan 6 already flags React exposure; Plan 7 escalates: a surface may need **additional heavy deps** (`@tiptap/*`). **Note to bake in:** Plans 1/2 will need a **dependency-provisioning seam for surfaces** (not just React). v1 uses a global-injection stopgap (`window.__Q2_TIPTAP__`, lazy); the durable mechanism is the in-flight sandbox/package-import work (SANDBOX INTEGRATION POINT). This is **deferred, not blocking**.
3. **`MeasuredBox` should carry the block type (Plan 6, open question #1).** Plan 6 debates whether `MeasuredBox` carries the block type / `leftInsetStripped`. Plan 7 gives a second reason to resolve it toward **carrying the block type**: it lets the tiptap surface route to the verbatim NodeView from `box.blockType` rather than re-sniffing source. **Note to bake in:** strengthen Plan 6's open-Q #1 toward "carry block type"; not a new requirement.

---

## Global Constraints (bake into every step)

- **TDD, test-first, bite-sized.** Each step writes/loads the failing test BEFORE code, watches it fail for the intended reason, writes the minimal code to green, runs the relevant suite, commits. One step at a time. Use `prevalidating-test-seams` when prepping each test phase and `fail-on-revert` after building to prove each test binds to the surface code (a green suite after a scaffold can be vacuous).
- **No hacks / no scope-undoing TODOs.** If a step wants a hack or a TODO that undoes prior work, STOP and ask (CLAUDE.md).
- **Consume Plan 1/Plan 2 by keystone name.** `EditingSurfaceComponent`, `EditingSurfaceProps` (`value`, `box`, `initialCaret`, `onChange`, `onCommit`, `onCancel`, `onEdgeReached`), `EditingSurfaceHandle` (`focus(caret?)` + best-effort edge/caret queries), `MeasuredBox`, `CaretHint`; `renderMeasuredEdit` + React off the renderer surface; the Plan 2 `editing-surface:` key (`PLAN-2-KEY`). Where the exact path is unknown, reference by name + `// PLAN1:`/`// PLAN2:`.
- **The active-region id is sacred.** The surface's measure-and-set wrapper MUST emit `id="q2-active-edit-region"` exactly, or every `assertNoReflowOnActivation` caller breaks (`ts-packages/preview-e2e-helpers/src/index.ts`). The surface reuses Plan 1's `renderMeasuredEdit` (which emits it) — do not re-implement it.
- **Markdown-string in/out — no commit-wire knowledge.** The surface fires `onCommit(text)`; the mode routes to `DocumentStore.commit`. The surface holds no commit/delete-policy knowledge.
- **TipTap acquired only via `tiptapRuntime.ts`.** Never a bare `import` of an `@tiptap/*` package in any surface module other than the typed accessor. Importing TipTap any other way is a plan failure (the rail can't resolve it).
- **Sync contract.** Do not introduce an async `EditingSurface` lifecycle. The only awaited thing is the one-time `loadTiptapRuntime()` on mount; the editor's own parse/serialize/focus are synchronous (rAF/`setTimeout(0)` inside `focus` is internal).
- **VFS path convention.** `/project/` prefix (CLAUDE.md) if any path is constructed.
- **Verification gates (CLAUDE.md).** This plan touches TS render paths + `quarto-core`-reachable resources (the extension ships under `resources/`) + SPA bundling, so the **full** gate applies before declaring done:
  - `cargo build --workspace` + `cargo nextest run --workspace` (backstop — no Rust changed, but the monorepo rule requires green).
  - `cargo xtask verify` (full — WASM leg in scope because the extension ships under `resources/` and the SPA embeds the bundle).
  - `cd hub-client && npm run build:all` — the production `tsc -b && vite build` is stricter than `tsc --noEmit`/`vitest`.
  - vitest from the renderer project (never pipe through `tail`); the extension's `__tests__/` run under the discovery mechanism Plan 6 established.
  - Playwright e2e (both hosts) for geometry/edge behaviour — **browser-only** (jsdom returns zero rects); do NOT declare geometry/edge done on vitest alone.
  - **Stale-WASM / stale-SPA trap:** for any `q2 preview` end-to-end check, rebuild the SPA chain (`cd hub-client && npm run build:wasm` → `cargo xtask build-q2-preview-spa` → `cargo build --bin q2`) so the embedded SPA carries the new TS + the tiptap bundle.
- **hub-client changelog (two-commit workflow).** Any commit touching `hub-client/` MUST add a `hub-client/changelog.md` entry in a second commit carrying the first's short hash.
- **Bundle weight.** Track the size delta the tiptap chunk adds to the SPA; it must be a **separate lazy chunk**, not in the main SPA entry (Phase 2/8). Record the measured chunk size in Notes.

---

## Consumes / Produces (inter-plan interface)

**Consumes (from Plan 1):**
- The `EditingSurface` contract types (`EditingSurfaceComponent`, `EditingSurfaceProps`, `EditingSurfaceHandle`, `MeasuredBox`, `CaretHint`) — `// PLAN1:` module path.
- `renderMeasuredEdit` (the measure-and-set wrapper emitting `#q2-active-edit-region`) + **React**/`forwardRef`/hooks on `window.__Q2_PREVIEW_RENDERER__`.
- The clean per-block markdown `value` (the mode produced it via `EditBufferCache.editableTextFor`); `box: MeasuredBox` (ideally carrying `blockType` — Proposed contract note #3).

**Consumes (from Plan 2):**
- The `editing-surface:` manifest contribution schema (`PLAN-2-KEY`) + discovery of the dir's `.tsx`.
- The Rust→iframe delivery channel merging the extension's components into `customComponentsCode`.
- The two-axis selection that resolves `editing-surface-tiptap` → `EditingSurfaceComponent` → `ViewControllerProps.surface`, **and** the lazy provisioning of `window.__Q2_TIPTAP__` when this surface is selected (`// PLAN2:`).

**Produces (a leaf — no later plan depends on it):**
- The bundled `editing-surface-tiptap` extension (manifest + TSX) = a second, radically different `EditingSurfaceComponent` proving the contract generalizes.
- The `tiptapRuntime.ts` accessor (with the SANDBOX integration point) + the lazy SPA chunk.
- The `MarkdownEngine` port + the v1 `MarkedMarkdownEngine` (CommonMark subset) + the `qmdVerbatim` NodeView.
- The orthogonality demo (any mode × this surface) + the textarea↔tiptap swap-with-zero-mode-change proof, as durable tests.
- The three Proposed-contract-note edits applied to Plans 1/2/6.

---

## Phases

> Ordering: lay the dependency/acquisition + engine foundations (Phases 1–3) before the surface body (Phase 4), then the verbatim/edge/caret behaviours (Phases 5–6), then orthogonality + selection + e2e (Phases 7–8). Each phase ends green and committed.

### Phase 0 — Pre-flight: confirm deps landed; apply contract notes; baseline

- [ ] **0.1** Confirm Plan 1's `EditingSurface` contract exists and is importable: grep `ts-packages/preview-renderer/src/framework/` for `EditingSurfaceComponent`, `EditingSurfaceProps`, `EditingSurfaceHandle`, `MeasuredBox`, `CaretHint`, and the in-tree `TextareaSurface`. Record actual module paths into a `// PLAN1:` block at the top of `index.tsx`. **If absent, STOP** — Plan 7 is blocked on Plan 1 (the contract does not exist in code yet as of 2026-06-21).
- [ ] **0.2** Confirm Plan 1 exposes **React**/`forwardRef`/hooks + `renderMeasuredEdit` on `window.__Q2_PREVIEW_RENDERER__` for transpiled extensions (verified today React is set on `window.React` in `entry.tsx:280`, NOT on the renderer object — confirm Plan 1's final placement). Record accessor names. **If React is not exposed for transpiled surfaces, STOP and coordinate with Plan 1** (two React copies break hooks; a surface is a React component).
- [ ] **0.3** Confirm Plan 2's `editing-surface:` schema (`PLAN-2-KEY`) + two-axis selection + the surface-install point exist: grep `crates/quarto-core/src/extension/types.rs` for the editing-surface variant; grep the host for where a selected surface's `EditingSurfaceComponent` is installed into `ViewControllerProps.surface`. **If absent, STOP** — blocked on Plan 2.
- [ ] **0.4** Baseline the renderer vitest suite green and record Plan 6's vitest-discovery mechanism for `resources/extensions/quarto/*/src/__tests__/` (reuse it; do not re-establish). Record whether `editing-surface-textarea` (Plan 6) has landed (the orthogonality demo prefers a real sibling surface to swap against).
- [ ] **0.5 (contract notes — applied + committed with this plan)** Apply the three **Proposed contract notes** to the sibling plans, as marked notes (no code): add note #1 to Plan 1's `EditingSurfaceHandle` section and Plan 6's open-Q #2; add note #2 to Plan 1's renderer-API/React section and Plan 2's delivery-channel section; add note #3 to Plan 6's open-Q #1. Each note references "Plan 7, Proposed contract notes". Commit these edits in the same commit that adds this plan file.

### Phase 1 — Scaffold the extension dir + entry + manifest + smoke

- [ ] **1.1 (test, then code)** Create `src/index.tsx` exporting the contract shape Plan 2 installs (`export const editingSurface: EditingSurfaceComponent` — exact field name per Plan 2's surface installer; mark `// PLAN2:`). Add `src/__tests__/index.smoke.test.tsx` asserting the export is present and is a `forwardRef` component (`editingSurface.$$typeof` is the forward-ref symbol, or mounting it with a `ref` does not warn). Stub `TiptapSurface` as a bare `forwardRef` that renders an empty `<div id="q2-active-edit-region" />` so the smoke test compiles. Run → fail (module absent) → green.
- [ ] **1.2** Author `_extension.yml` per Plan 2's `editing-surface:` schema (`PLAN-2-KEY`): declare the surface entry (`component: src/index.tsx`) + `render-components` listing every `.tsx` the surface needs delivered on the rail (`index.tsx`, `TiptapSurface.tsx`, `tiptapRuntime.ts`, `markdownEngine.ts`, `verbatimNode.ts`, `edgeKeymap.ts`, `caret.ts`, `extensions.ts`). Add a focused assertion that an editing-surface manifest parses (co-locate with Plan 2's discovery tests, or leave a `// PLAN2:` note + checklist item rather than duplicating Plan 2's parser test). Run discovery test → green.

### Phase 2 — TipTap acquisition port (`tiptapRuntime.ts`) + lazy provisioning

- [ ] **2.1 (test)** `src/__tests__/tiptapRuntime.test.ts`: with no `window.__Q2_TIPTAP__`, `loadTiptapRuntime()` rejects with the provisioning error; with a stub global set, it resolves to that object. Run → fail (module absent).
- [ ] **2.2** Author `tiptapRuntime.ts` exactly as in **D1** (the typed `TiptapRuntime` interface + the stopgap `loadTiptapRuntime` reading `window.__Q2_TIPTAP__`, with the `// SANDBOX INTEGRATION POINT` comment). Run → green. `fail-on-revert`: corrupt the global key string; confirm the resolve test REDs.
- [ ] **2.3 (host lazy-provision, test)** In the SPA host (`q2-preview-spa` and/or `hub-client`), add the code-split provisioner: when `editing-surface-tiptap` is the selected surface (Plan 2 selection), `import()` the tiptap bundle chunk and assign `window.__Q2_TIPTAP__` before the surface activates. Test (vitest, host): selecting the tiptap surface triggers the dynamic import + sets the global; selecting textarea does not. Mark `// PLAN2:` at the selection hook. Build the curated extension chunk (see Phase 3.2) as a **separate lazy chunk** (verify via the Vite/esbuild chunk graph; record the chunk size in Notes). Run → green. Apply the two-commit changelog if `hub-client/` changed.

### Phase 3 — `MarkdownEngine` port + CommonMark subset parse/serialize

- [ ] **3.1 (test)** `src/__tests__/markdownEngine.roundtrip.test.ts`: for each v1-subset construct (paragraph, heading 1–6, bullet list, ordered list, blockquote, inline `**`/`*`/`` ` ``/`~~`/link), assert `serialize(parse(md))` yields markdown that **re-parses to an equivalent doc** (semantic round-trip, NOT byte-equality). Use a small AST-equivalence helper (normalize the JSONContent: drop positions, compare node types + text). Run → fail (engine absent).
- [ ] **3.2** Author `extensions.ts` (the curated TipTap extension set: Document, Paragraph, Text, Heading, BulletList, OrderedList, ListItem, Blockquote, Bold, Italic, Code, Strike, Link, HardBreak, History — the CommonMark subset, **plus the `qmdVerbatim` node from Phase 5**) and `markdownEngine.ts` (`MarkdownEngine` interface + `MarkedMarkdownEngine` wrapping `rt.MarkdownManager` configured with `extensions.ts`). Run 3.1 → green. `fail-on-revert`: drop the Heading extension; confirm the heading round-trip REDs.
- [ ] **3.3 (test)** Round-trip a *mixed* block: a paragraph containing one unsupported inline (`$x^2$`) → assert the math survives as a verbatim inline atom whose serialized form is the literal `$x^2$` (semantic preservation). (Depends on Phase 5's verbatim node; if running 3 before 5, mark this step and complete it after 5.) Run → green.

### Phase 4 — The `EditingSurfaceComponent` body (measure-and-set + value + handle)

- [ ] **4.1 (test)** `src/__tests__/surface.body.integration.test.tsx`: mount `<TiptapSurface value="**hi**" box={…} ref onCommit onCancel onChange onEdgeReached />` (with `window.__Q2_TIPTAP__` stubbed to a real TipTap runtime in the test). Assert: (1) the wrapper carries `id="q2-active-edit-region"`; (2) the editor renders the parsed content (a `<strong>hi</strong>` in the contenteditable); (3) typing + `Mod-Enter` fires `onCommit` with serialized markdown; (4) `Escape` fires `onCancel`; (5) `editor.on('update')` fires `onChange`. Run → fail.
- [ ] **4.2** Author `TiptapSurface.tsx` as the `forwardRef` `EditingSurfaceComponent`:
  - `await loadTiptapRuntime()` once on mount (state: `runtime | null`); render the measured wrapper immediately (so `#q2-active-edit-region` exists with zero reflow) and the `<EditorContent>` once the runtime resolves. Re-seed `content` from `markdownEngine.parse(value, rt)` when `value` changes (mode-driven remount).
  - **Measure-and-set:** wrap `<EditorContent>` in the imported `renderMeasuredEdit` (`// PLAN1:`, NOT re-implemented), preserving the `id` and the box-reproduction. Use `box.blockType` for left-inset/verbatim routing if Plan 6's `MeasuredBox` carries it (Proposed note #3); else sniff (Phase 5).
  - **Callbacks:** `editor.on('update')` → debounced `onChange(serialize)`; `Mod-Enter` keymap + `editor.on('blur')` → `onCommit(serialize)`; `Escape` → `onCancel()`. Serialize via `markdownEngine.serialize(editor.getJSON(), rt)`.
  - **Handle (`forwardRef` → `EditingSurfaceHandle`):** `focus(caret?)` → `editor.commands.focus()` + caret mapping (Phase 6); plus the best-effort edge/caret queries.
  - Teardown is React-driven: unmount → `useEditor` cleanup → `editor.destroy()`.
  - Run 4.1 → green. `fail-on-revert`: drop the `renderMeasuredEdit` wrapper; confirm the `#q2-active-edit-region` assertion REDs.

### Phase 5 — Verbatim NodeView ("code mode") + block-kind detection

- [ ] **5.1 (test)** `src/__tests__/verbatim.integration.test.tsx`: (a) a whole-block unsupported `value` (`$$E=mc^2$$`) mounts the surface showing a **monospace** verbatim NodeView containing the literal source; editing it + commit serializes the literal text back; (b) `detectVerbatimBlock("::: callout-note\n…")` returns true and `detectVerbatimBlock("# heading")` returns false. Run → fail.
- [ ] **5.2** Author `verbatimNode.ts`: the `qmdVerbatim` `Node` (via `rt.Node.create`) with `ReactNodeViewRenderer` + a monospace `NodeViewWrapper`, `parseMarkdown` (capture raw token text) + `renderMarkdown` (emit stored text verbatim), and `detectVerbatimBlock(src): boolean` (prefix sniff for `:::`, `$$`, ```` ```{ ````, raw blocks, `{{<`). Wire it into `extensions.ts` and into `TiptapSurface`'s value-routing (sniff before parse; whole-block-unsupported → mount a single `qmdVerbatim`). Run → green. `fail-on-revert`: break `renderMarkdown` to emit `''`; confirm the literal-round-trip test REDs.
- [ ] **5.3 (test)** Inline verbatim: a paragraph with `[span]{.cls}` and `@cite` → each becomes an inline `qmdVerbatim` atom (monospace chip) whose serialized form is the literal source; surrounding text stays rich. Run → green.

### Phase 6 — Edge detection → `onEdgeReached`; caret landing; `focus`

- [ ] **6.1 (test)** `src/__tests__/edge.integration.test.tsx`: with the caret in the **last** textblock and `view.endOfTextblock('down')` true (mock the seam as `caretGeometry.test.ts` documents the jsdom rect-mock pattern), a bare `ArrowDown` fires `onEdgeReached('down')` and is `preventDefault`ed; a non-edge `ArrowDown` does NOT fire it and moves the caret natively. Mirror for `ArrowUp`/first textblock. Run → fail.
- [ ] **6.2** Author `edgeKeymap.ts`: a ProseMirror keymap (added via a TipTap extension in `extensions.ts`) that, on a bare arrow (no Shift/Ctrl/Alt/Meta), tests doc-edge = (selection in first/last textblock) && `view.endOfTextblock(dir)`; on an edge → `preventDefault` + `onEdgeReached(dir)` (the surface passes the callback in via editor storage/props); else returns false (native move). Wire up/down now; wire left/right as contract-complete (fire on doc-start/doc-end) only if a test demands. Run → green. `fail-on-revert`: invert the edge test; confirm the demo REDs.
- [ ] **6.3 (test + code)** Author `caret.ts`: `applyCaret(editor, hint, rt)` mapping a `CaretHint` to a selection — `{edge:'first'}` → `TextSelection.atStart`, `{edge:'last'}` → `TextSelection.atEnd`, `{line/column}` → **best-effort** (clamp to the nearest textblock start/end; column approximated, per the locked decision). Wire into `EditingSurfaceHandle.focus(caret?)`. Test: `ref.current.focus({edge:'last'})` lands the caret in the last textblock (assert via `editor.state.selection`). Run → green. Record in Notes that column is advisory (Proposed contract note #1).

### Phase 7 — Orthogonality demo (any mode × this surface) + textarea↔tiptap swap

- [ ] **7.1 (test)** `src/__tests__/orthogonality.integration.test.tsx`: drive **this surface** under both a flat (`block-editing`-style) and a tree-aware (`nesting-cursor`-style) `ViewController` (real bundled modes if Plans 4/5 landed, else minimal stub controllers exercising the contract — mirror Plan 6 §4.1). Assert the surface behaves identically under both: same `#q2-active-edit-region`, same `onCommit(text)` for the same keystrokes, `onEdgeReached(dir)` fires identically (the mode's *response* differs; the surface's emission is mode-independent). Run → fail → green after Phases 4–6.
- [ ] **7.2 (test)** Swap-with-zero-mode-change: mount the same mode + fixture once with `editing-surface-textarea` (Plan 6) and once with `editing-surface-tiptap` selected as `ViewControllerProps.surface`; assert the **mode code path is identical** (no branch on surface kind) and both produce `onCommit` for the same edit. (If Plan 6 has not landed, assert against Plan 1's in-tree `TextareaSurface`.) Run → green. `fail-on-revert`: make the surface assume a mode shape (e.g. fire `onEdgeReached` only under the flat stub); confirm the tree-aware case REDs.

### Phase 8 — Selection wiring + e2e + end-to-end verification

- [ ] **8.1** Confirm Plan 2's two-axis selection resolves `editing-surface-tiptap` → `EditingSurfaceComponent` → `ViewControllerProps.surface`, and triggers the lazy provisioning (Phase 2.3). If Plan 2's surface picker is not yet wired, leave a `// PLAN2:` note + a thin host shim selecting `tiptap` for the e2e fixture. Two-commit changelog if `hub-client/` changed.
- [ ] **8.2 (Playwright)** A geometry/edge e2e (browser-only): with `tiptap` selected + a mode active, activate a paragraph → assert `#q2-active-edit-region` appears with the box height preserved (no reflow; reuse `assertNoReflowOnActivation`); type + `Mod-Enter` → block commits; `ArrowDown` on the last line → `onEdgeReached('down')` → mode advances; a `$$`-block activates into the verbatim code-mode NodeView. Keep under the SPA host's e2e dir.
- [ ] **8.3 (E2E through the binary — required, CLAUDE.md)** Honor the stale-WASM/stale-SPA chain (`cd hub-client && npm run build:wasm` → `cargo xtask build-q2-preview-spa` → `cargo build --bin q2`), then drive a real `q2 preview` session with `tiptap` selected on a fixture mixing paragraphs/headings/lists (rich) + a math/callout block (verbatim). Inspect the DOM: rich blocks edit WYSIWYG; unsupported blocks show monospace code-mode; commit updates the block; **switch the MODE (flat↔nesting) with `tiptap` unchanged** and confirm identical surface behaviour (the orthogonality proof, live). Record the exact invocation + an output snippet + an explicit "inspected" note in this plan file.
- [ ] **8.4** Full gate: `cargo xtask verify` (full) + `cd hub-client && npm run build:all`. Capture logs to a `/tmp` file once; grep for failures. Record the lazy tiptap chunk size.
- [ ] **8.5** Confirm `resources/extensions/quarto/editing-surface-tiptap/` is self-contained: its `src/` imports React + `renderMeasuredEdit` + the contract types off the renderer surface (`// PLAN1:`), acquires TipTap **only** via `tiptapRuntime.ts`, and imports **nothing** of Plan 6's `caretGeometry`. Confirm it emits `#q2-active-edit-region`.
- [ ] **8.6** Update `hub-client/changelog.md` (second commit, with hash) for the user-visible change (a TipTap WYSIWYG editing surface, swappable with textarea). Update the epic index ("Plan 7 produces") + record the Engine-B-readiness (markdown-engine-as-port) and the SANDBOX integration point as a follow-up strand (braid, `discovered-from` this work — side-issue only).

---

## Test surfaces this plan OWNS (surface-level — orthogonal to mode tests)

- **Vitest (in `editing-surface-tiptap/src/__tests__/`):** `index.smoke`, `tiptapRuntime`, `markdownEngine.roundtrip` (semantic round-trip of the CommonMark subset), `surface.body.integration` (measure-and-set + value + commit/cancel/change), `verbatim.integration` (code-mode + block-kind detection + inline atoms), `edge.integration` (`onEdgeReached` via `endOfTextblock`), `caret` (best-effort landing), `orthogonality.integration` (any mode × this surface + textarea↔tiptap swap).
- **Playwright (SPA):** the tiptap surface's geometry/edge/verbatim e2e (no-reflow `#q2-active-edit-region`, WYSIWYG vs code-mode, edge→advance).
- **NOT this plan's:** activation/policy/landing/nesting (the modes'); `caretGeometry`/textarea geometry (Plan 6's); the Rust manifest/discovery parser tests (Plan 2's).

---

## Open boundary questions (resolve against Plan 1/2's published contract; do NOT guess)

1. **Exact handle method names + the exit-column channel** (Plan 1 / Plan 6 open-Q #2). Conform to whatever Plan 1 publishes, holding the constraint from Proposed contract note #1 (column advisory; neutral edge-query names).
2. **`MeasuredBox` block-type carriage** (Plan 6 open-Q #1). If carried, route verbatim from `box.blockType`; else sniff the source. Proposed contract note #3 strengthens "carry block type".
3. **React/`forwardRef` exposure for transpiled surfaces** (Plan 1; Plan 6 open-Q #4). Blocking if not exposed.
4. **Lazy-provision hook location** (Plan 2). Where the host code-splits + sets `window.__Q2_TIPTAP__` on tiptap selection.
5. **SANDBOX dependency-provisioning mechanism** (Elliot's work). Replaces the `tiptapRuntime.ts` stopgap body; surface untouched.

---

## Risks & notes

- **Delivery is the headline risk, and it is deferred by decision.** The rail cannot import npm; v1 stopgaps with `window.__Q2_TIPTAP__` (lazy, code-split). If the sandbox work changes the import model, only `tiptapRuntime.ts` changes. Do NOT bake TipTap imports anywhere else.
- **Fully-unsupported blocks degenerate to "a heavier textarea."** Acceptable for v1 (single global surface); TipTap's value is rich + mixed blocks. Recorded honestly.
- **Fidelity is semantic, not byte.** pampa re-normalizes on commit; do NOT tune TipTap's serializer to pampa. The round-trip test asserts AST-equivalence after re-parse, not bytes.
- **Geometry/edge are browser-only.** jsdom returns zero rects; gate Phases 4/6 correctness on Playwright (8.2), not vitest alone.
- **Two React copies break hooks** (Plan 6's lesson). The surface uses the host React via the renderer surface; never bundle a second React into the tiptap chunk (TipTap's React is a *peer* dep — provision it against the host React).
- **Engine B is the robust endpoint, recorded not built.** The `MarkdownEngine` port makes the `pampa.wasm` swap a drop-in. Keep the port boundary clean.
- **Orthogonality is a real obligation** (keystone §2). The demo (7.1) + the swap proof (7.2) + their `fail-on-revert` are the proof the mode↔surface boundary is complete for a radically different surface.
- **Provisional names** (keystone §15): contract type names + the `PLAN-2-KEY` may be renamed by global find-replace; author against keystone names + keep greppable.

---

## References

- Keystone: `claude-notes/designs/2026-06-20-editing-mode-contract.md` (§2 two-axis matrix, §5 `EditingSurface` contract + the geometry boundary, §10 surface selection, §11 "active surface ❌ stackable", §13 tiptap-as-embedded-surface, §15 provisional names).
- Epic: `claude-notes/plans/2026-06-20-editing-mode-epic.md` ("Plan 7 (tiptap surface)"; test-ownership split).
- Plan 1: `…-plan-1-core-services-and-seams.md` (the `EditingSurface` contract + in-tree `TextareaSurface` + renderer-API React/primitive exposure Plan 7 consumes).
- Plan 2: `…-plan-2-extension-type-and-delivery.md` (the `editing-surface:` manifest + discovery + two-axis selection + the `LOAD_CUSTOM_COMPONENTS` rail; `PLAN-2-KEY`; the lazy-provision integration point).
- Plan 6 (sibling surface, template): `…-plan-6-textarea-surface-extension.md` (packaging, vitest-discovery, React boundary, orthogonality-demo conventions; open questions #1/#2/#4 this plan constrains).
- TipTap (verified 2026-06-21, `/Users/gordon/src/tiptap`, v3.27.x): `packages/core/src/Editor.ts:123-187,532-578` (sync constructor + deferred `mount`/`createView`); `packages/react/src/useEditor.ts:319-377` + `EditorContent.tsx:88-181` (headless create + DOM mount in `componentDidMount`); `packages/markdown/src/MarkdownManager.ts:302,333,939-1083` (sync parse/serialize; unknown HTML → literal text); `packages/core/src/commands/{focus,setTextSelection}.ts` (focus + `TextSelection.create`); `EditorView.endOfTextblock` (ProseMirror, via `@tiptap/pm/view`) for edge detection; package deps: core→`@tiptap/pm` only, markdown→`marked` only, react→React peer.
- q2 delivery rail (verified): `ts-packages/preview-renderer/src/q2-preview/entry.tsx:107-126,276-306` (`__Q2_PREVIEW_RENDERER__`, `window.React`/`window.katex`, blob-import); `hub-client/src/services/tsxTranspiler.ts` (Babel-standalone, no import map); `hub-client/src/components/render/ReactRenderer.tsx` (`customComponentsCode`).
- pampa AST→markdown writer (the on-commit normalizer): `crates/pampa/src/writers/qmd.rs` (`write_single_block` at ~:2590); call chain `wasm-quarto-hub-client::regenerate_nested_buffers` → `pampa::regenerate_nested_buffers` → `write_single_block`.
- Skills: `prevalidating-test-seams`, `fail-on-revert` (mandatory per epic for surface plans).
- Rules: `.claude/rules/cross-platform.md`, `.claude/rules/integration-tests.md` (moot — no Rust tests change), `.claude/rules/wasm.md` (moot for v1; relevant to the out-of-scope Engine B).

---

## Notes / discovery log

- Fill in Phase 0 baselines: contract/React-exposure/selection confirmations (or the STOP that blocks on Plan 1/2), and Plan 6's vitest-discovery mechanism + whether `editing-surface-textarea` has landed.
- Record the lazy tiptap chunk size (Phase 2.3 / 8.4) and confirm it is a separate chunk, not in the SPA main entry.
- Record the resolution of Open boundary questions 1–5 against Plan 1/2's published contract.
- Record the orthogonality-demo mode source (bundled Plans 4/5, in-tree controllers, or stubs) — Phase 7.1.
- Record the Phase 8.3 e2e invocation + observation (CLAUDE.md end-to-end rule), including the live mode-switch orthogonality check and a verbatim-block (code-mode) screenshot/DOM snippet.
- Record the SANDBOX integration point status (Elliot's package-import work) and file the Engine-B-readiness follow-up strand.
