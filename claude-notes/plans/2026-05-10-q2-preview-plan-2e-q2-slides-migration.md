# Plan 2E — q2-slides + revealjs as sibling formats

**Date:** 2026-05-10
**Branch:** feature/q2-preview (post-2D)
**Status:** Implementation plan — DRAFT (open design questions §1, §3, §4, §5, §6 unresolved)
**Milestone:** Completes the hub-client format-restructure that 2pre started. After 2E, every React-side render path lives under a sibling format directory (`q2-debug/`, `q2-preview/`, `q2-slides/`) and follows the framework registry contract; no "ghost format" code remains at the top of `components/render/`.

## Goal

Migrate `ReactAstSlideRenderer.tsx` (the carousel `SlideAst`) and `RevealjsReactAstSlideRenderer.tsx` (the reveal.js `RevealjsSlideAst`) into a shared `hub-client/src/components/render/q2-slides/` directory. Establish the **two-registries-sharing-leaves** pattern: both `format: q2-slides` and `format: revealjs` are React-side slide formats today, and they already share their per-block / per-inline rendering by importing it across files; the migration makes that sharing explicit by moving the leaves into a single set of files that both registries spread from.

After 2E:

- `q2-slides/blocks/*.tsx` and `q2-slides/inlines/*.tsx` hold the per-Pandoc-tag leaves once. Both `q2SlidesRegistry` and `revealjsRegistry` spread from the same set.
- `q2-slides/SlideAst.tsx` is the carousel document-root, registered as `'Ast'` in `q2SlidesRegistry`.
- `q2-slides/RevealjsAst.tsx` is the reveal.js document-root, registered as `'Ast'` in `revealjsRegistry`.
- `parseSlides`, `extractSections`, `splitByHeaders`, `flattenBlocks` consolidate into `q2-slides/parseSlides.ts`.
- `attributesToProps`, `parseStyleString` consolidate into `q2-slides/attributesToProps.ts` (used by every leaf that handles HTML attribute passthrough).
- `AspectRatioScaler.tsx` moves from the top-level into `q2-slides/`; q2-slides is the only consumer.
- `q2-slides/SlideContext.tsx` carries slide-control state (`currentSlide`, `setCurrentSlide`, `totalSlides`) — the parent (Editor) mounts the Provider above the registered `Ast` component, mirroring the precedent set by `q2-preview/PreviewContext.tsx`.
- `ReactRenderer.tsx`'s slide branch (lines 209-233) collapses to one mount: `<Ast registry={isRevealjs ? revealjsRegistry : q2SlidesRegistry} {...astProps} />`.
- External consumers (`useCursorToSlide.ts`, `useSlideThumbnails.tsx`) retarget their imports from `./ReactAstSlideRenderer` to `./q2-slides`.
- `ReactAstSlideRenderer.tsx` and `RevealjsReactAstSlideRenderer.tsx` are deleted at the end of Phase 2.
- The `parity-with-q1` question for revealjs (whether to ever route `format: revealjs` through a Rust-side HTML renderer instead of the React-side reveal.js wrapper) is **explicitly out of scope** — Plan 2E preserves both React paths as first-class.

## Locked design decisions

### §2. Two registries, shared leaves — RESOLVED

`q2-slides/` is one directory housing two registries:

```ts
// q2-slides/registry.ts
import { SlideAst } from './SlideAst';
import { RevealjsAst } from './RevealjsAst';
import { Block, Inline } from './dispatchers';
import * as Blocks from './blocks';
import * as Inlines from './inlines';

const sharedLeaves = { ...Blocks, ...Inlines };

export const q2SlidesRegistry: FormatRegistry = {
    ...sharedLeaves,
    Block,
    Inline,
    Ast: SlideAst,
};

export const revealjsRegistry: FormatRegistry = {
    ...sharedLeaves,
    Block,
    Inline,
    Ast: RevealjsAst,
};
```

The two registries differ on exactly one entry — the document-root `'Ast'`. Everything else is the same `sharedLeaves` object spread into both. This mirrors the structural reality today: `RevealjsReactAstSlideRenderer.tsx:13` imports `parseSlides` and `renderBlock` directly from `ReactAstSlideRenderer.tsx`. The migration makes that sharing explicit at the registry level.

**Why this beats one-registry-with-a-meta.format-switch.** A single registry that branches inside the registered `'Ast'` component would couple two doc-root concerns into one file, mix two sets of dependencies (reveal.js + plain React) into one bundle entry, and obscure which format owns which chrome. Two narrow registries keep the dependencies on each side honest: a user who never opens a `format: revealjs` document still pays for the reveal.js library because of the static import (TS module-graph semantics), but the *intent* of which leaf belongs to which format is plain at the registry level.

**Why this beats two iframes / two HTML pages / two entry.tsx files.** The two slide formats share parent-side state plumbing (slide picker, cursor-to-slide, thumbnails). Mounting them via the in-page `<Ast>` component lets that plumbing stay shared. Splitting at the iframe boundary (option (c) in §1's open question) would require duplicating it. We can still iframe the whole `q2-slides/` later as a single iframe that branches internally on format — that's a downstream design call for §1, not a constraint here.

### Naming consistency

| Concern | Today | After 2E |
|---|---|---|
| Format registry | none (imperative) | `q2SlidesRegistry`, `revealjsRegistry` (per-format const) |
| Carousel doc root | `SlideAst` (line 57) | `SlideAst` (unchanged) — registered as `'Ast'` in `q2SlidesRegistry` |
| Reveal.js doc root | `RevealjsSlideAst` (line 68) | `RevealjsAst` — registered as `'Ast'` in `revealjsRegistry` |
| Slide segmentation | `parseSlides` exported from `ReactAstSlideRenderer.tsx` | `parseSlides` exported from `q2-slides/parseSlides.ts` |
| Slide-control state | controlled-mode props on `SlideAst` / `RevealjsSlideAst` | `SlideContext` provider mounted by parent |

**`SlideAst` keeps its name** for symmetry with `PreviewDocument` / `AstRenderer` (each format's `'Ast'` entry has a format-meaningful name); the rename `RevealjsSlideAst` → `RevealjsAst` strips the redundant suffix the same way 2pre stripped `Slide` from intermediate symbols. No public-API impact: neither symbol is exposed on a window global like `__REACT_AST_DEBUG_RENDERER__` is. (Verified: no `__REACT_AST_SLIDE_RENDERER__` or similar global; user TSX has no slide-side surface.)

## Open design questions

The five questions below are unresolved. Each has a recommended pick, but the user has not committed. Pick before the corresponding phase lands; resolutions can land mid-implementation as long as the affected phase hasn't started.

### §1. Iframe boundary — in-page, single iframe, or two iframes?

q2-debug and q2-preview each have their own iframe + HTML page + `entry.tsx`. The slide renderers today render in-page directly inside `ReactPreview`'s React tree. Three options:

- **(a) Stay in-page** *(Recommended for v1)*. `<Ast registry={…}>` mounts inside `ReactRenderer.tsx`'s slide branch the same way `SlideAst`/`RevealjsSlideAst` mount today. No iframe, no postMessage, no `q2-slides.html` / `revealjs.html` needed. The parent's slide-control hooks (`useCursorToSlide`, `useSlideThumbnails`) keep working without cross-frame plumbing. Iframing is a follow-up plan.
- **(b) Single iframe `/q2-slides.html`** that branches on `meta.format` to mount either `SlideAst` or `RevealjsAst`. One HTML page, one `entry.tsx`, cross-frame plumbing for slide-control state. Symmetric with q2-preview / q2-debug.
- **(c) Two iframes** (`/q2-slides.html`, `/revealjs.html`). Maximum separation; doubled boilerplate. Useful only if the two formats' iframe-host concerns end up genuinely different (e.g. revealjs needs `<script type="module">` setup that q2-slides doesn't).

Recommended (a) for 2E. Rationale: the iframe boundary's main wins (sandboxing, theme isolation, separate bundle entry) don't currently apply to the slide formats — there's no per-format theme injection, no user-supplied TSX surface, no security boundary justifying it. Adding it would inflate Plan 2E from "format-registry migration" to "format-registry migration + cross-frame slide-control plumbing" without solving an active problem. If iframing becomes desirable later (e.g. when slide editing lands and we want the same sandboxing q2-preview has), it's a single follow-up plan against the post-2E directory layout.

### §3. SlideContext shape — narrow triplet, or richer?

```ts
// Recommended (option A): narrow
interface SlideContextValue {
    currentSlide: number;
    setCurrentSlide: (n: number) => void;
    totalSlides: number;
}
```

vs.

```ts
// Option B: richer
interface SlideContextValue {
    currentSlide: number;
    setCurrentSlide: (n: number) => void;
    totalSlides: number;
    goToPrevSlide: () => void;
    goToNextSlide: () => void;
    onSlideClick?: (slideIndex: number) => void; // optional thumbnail-click callback
    presenterMode?: boolean; // future reveal.js feature
}
```

Recommended **(A)**. Keyboard/prev/next navigation stays local to `SlideAst` / `RevealjsAst`'s internal state (each consumes the narrow triplet and computes prev/next inline). External consumers (`useCursorToSlide`, `useSlideThumbnails`) only need to read `currentSlide` / `setCurrentSlide`; the narrow shape covers them.

Trade-off: if a future feature (e.g. presenter mode, slide-jump-from-thumbnail UI) wants to reuse the navigation primitives across multiple sibling components, option (B) lifts that wiring once. But adding fields later is mechanical (`SlideContext.tsx` is small); narrowing fields once they're consumed is harder.

### §4. Image asset resolution — VFS-direct, or migrate to manifest pattern?

`SlideAst`'s `Image` case (`ReactAstSlideRenderer.tsx:780-835`) reads VFS files synchronously inside the render path: `vfsReadFile` for `/.quarto/` paths, `vfsReadBinaryFile` for project-relative paths, base64-encodes the result, sets a `data:` URL. q2-preview pre-walks images in the parent and distributes a `Record<origPath, blobUrl>` via `AssetManifestContext`.

- **(a) Keep VFS-direct** *(Recommended for v1)*. Mechanical migration: `q2-slides/inlines/Image.tsx` carries the same `vfsReadFile` / `vfsReadBinaryFile` calls. Stays sync, no parent-side walker added.
- **(b) Migrate to manifest pattern.** Adds an `AssetManifestContext` consumer to q2-slides; requires a parent-side walker (which q2-preview has via `assetWalker.ts`) to populate the manifest before mount. Symmetric with q2-preview but adds non-trivial wiring.

Recommended (a) on minimum-scope grounds. Manifest migration is most valuable when there's an iframe boundary (cross-frame asset-URL distribution); since §1 keeps slides in-page, the symmetry win is academic.

### §5. Editing — read-only stays read-only, or wire `setLocalAst`?

Slide leaves currently can't edit content. The framework's `NodeArgs<T>` type includes `setLocalAst`; q2-preview's leaves use it for live edits.

- **(a) Read-only — pass `setLocalAst: () => {}`** *(Recommended for v1)*. Each q2-slides leaf accepts the framework's `NodeArgs` shape but doesn't wire writes back. Mirrors the slide renderer's current "read-only preview" semantics.
- **(b) Wire `setLocalAst` per-leaf for parity with q2-preview.** Each leaf implements the spread-and-replace pattern (e.g. `Header` rewrites `node.c[2]` when its inlines change). Rote work: ~22 leaves × ~5 LOC of write-back per leaf. Enables future "edit slide titles in-place" without a structural follow-up.

Recommended (a). Slide editing is a feature, not a refactor concern. (b)'s plumbing inflates the plan ~110 LOC for a feature that has no consumer today; landing it as part of the slide-editing feature plan is cleaner.

### §6. Migration strategy — phased shim like 2pre, or single-shot?

- **(a) 2pre-style shim** *(Recommended)*. Phase 1 builds `q2-slides/` additively while `ReactAstSlideRenderer.tsx` becomes a re-export barrel exposing `parseSlides`, `renderBlock`, `Slide`, `SlideAst` under their old names. Phase 2 migrates each consumer (ReactRenderer, RevealjsReactAstSlideRenderer first since it's adjacent, then useCursorToSlide and useSlideThumbnails) one commit at a time. Phase 3 deletes the shim. Tree green after every commit.
- **(b) Single big-bang.** Create `q2-slides/`, retarget all four consumers in one commit, delete the old files. Faster (~5 commits vs ~12), harder to revert.

Recommended (a). 2pre's pattern worked; reusing it costs little and gives bisection / partial-revert affordance for a 1140-LOC migration touching four external consumers.

## Checklist

(Phase numbering continues from 2D's Phase 8. Phases assume §6 option A — phased shim. If §6 flips to (b), Phase 9 collapses to a single commit.)

### Phase 9 — Pre-flight

- [ ] **9.1** Verify the two slide formats share rendering symmetrically today. Diff `RevealjsReactAstSlideRenderer.tsx`'s `renderSlideContent` (lines 27-63) against `ReactAstSlideRenderer.tsx`'s `renderSlide` for `type === 'title'` (lines 382-414). Expectation: title-slide rendering is duplicated nearly verbatim — the same inline styles for `<h1>` (72px) and `<p>` author (36px). Confirm. If a divergence exists, log it; the migration's "one title-slide component shared between SlideAst and RevealjsAst" pattern needs to handle it.
- [ ] **9.2** Confirm no user TSX in `~/docs/demo-playground/` reaches `SlideAst` / `RevealjsSlideAst` / `parseSlides` / `renderBlock` by name. (2pre verified the same for q2-debug.) If a demo does, the migration needs a window-global passthrough analogous to `__REACT_AST_DEBUG_RENDERER__` — but the slide formats had no such global before, so this is unlikely.

### Phase 10 — Build q2-slides directory behind a shim

- [ ] **10.1** Create `q2-slides/` directory. Add `q2-slides/index.ts` as the barrel that re-exports public symbols (`SlideAst`, `RevealjsAst`, `parseSlides`, `Slide` type, `q2SlidesRegistry`, `revealjsRegistry`, `SlideContext`).
- [ ] **10.2** Create `q2-slides/styles.ts`. Pull the inline-style constants out of `renderBlock`'s switch arms: `paraStyle` (margin/lineHeight), `headerStyles` (per-level fontSize), `codeBlockStyle`, `bulletListStyle`, etc. ~50 LOC of style constants moved verbatim from the switch.
- [ ] **10.3** Create `q2-slides/attributesToProps.ts`. Move `attributesToProps` (lines 441-510) and `parseStyleString` (lines 485-510). Drop the leading underscore prefix on locals where present. Keep the function signatures — leaves consume them as before.
- [ ] **10.4** Create `q2-slides/parseSlides.ts`. Move `parseSlides`, `extractSections`, `splitByHeaders`, `flattenBlocks` (lines 203-345). Drop the local `extractMetaString` definition (lines 350-371) — replaced by the framework helper post-2D.
- [ ] **10.5** Create `q2-slides/AspectRatioScaler.tsx`. Move from top-level `components/render/AspectRatioScaler.tsx`. Files are character-identical; only the path changes.
- [ ] **10.6** Create `q2-slides/SlideContext.tsx`. Define `SlideContextValue = { currentSlide: number; setCurrentSlide: (n: number) => void; totalSlides: number }` and the React context. Mirror the `q2-preview/PreviewContext.tsx` shape (default `null`, leaves treat absence as a bug). See §3 for the open question about which fields belong here.
- [ ] **10.7** Create `q2-slides/blocks/*.tsx` per-block leaves: `Para.tsx`, `Plain.tsx`, `Header.tsx`, `CodeBlock.tsx`, `BulletList.tsx`, `OrderedList.tsx`, `BlockQuote.tsx`, `Div.tsx`, `RawBlock.tsx`, `HorizontalRule.tsx`, `Figure.tsx`. Each takes `NodeArgs<T>` from the framework. Each consumes `attributesToProps` and `styles` from siblings. Each renders inline children via `renderChildren(args)` (the framework's traversal helper, which dispatches per-tag through the registry's `'Inline'` and per-block-tag entries). Eleven files, ~30 LOC each. **Per §5, `setLocalAst` is a no-op today; leaves accept the field but don't wire writes back.** Add `q2-slides/blocks/index.ts` re-exporting all eleven by Pandoc-tag name (`Para`, `Plain`, `Header`, …).
- [ ] **10.8** Create `q2-slides/inlines/*.tsx` per-inline leaves: `Str.tsx`, `Space.tsx`, `SoftBreak.tsx`, `LineBreak.tsx`, `Emph.tsx`, `Strong.tsx`, `Quoted.tsx`, `Code.tsx`, `Link.tsx`, `Image.tsx`, `Span.tsx`, `Math.tsx`. Twelve files. Same NodeArgs shape; `Math.tsx` is the only one with an external dep (`katex.renderToString`). `Image.tsx` keeps the `vfsReadFile` / `vfsReadBinaryFile` synchronous reads (per §4 option A). Add `q2-slides/inlines/index.ts`.
- [ ] **10.9** Create `q2-slides/dispatchers.tsx`. Define `Block` and `Inline` per the framework contract — each does `registry[node.t]` lookup; on miss, render the slide-formats' default fallback (currently a gray `[NodeType]` span — `ReactAstSlideRenderer.tsx:876-879` for inlines; the block path doesn't have an equivalent today, so define one symmetrically). Registered under the framework-reserved keys `'Block'` and `'Inline'` in both registries.
- [ ] **10.10** Create `q2-slides/SlideAst.tsx`. The carousel document-root, registered as `'Ast'` in `q2SlidesRegistry`. Reads `currentSlide` / `setCurrentSlide` / `totalSlides` from `SlideContext` (or from controlled-mode props during the migration if §1 keeps the parent-side state shape unchanged — see §3 for the precise context shape). Mounts `<AspectRatioScaler>` + the dark-frame chrome + the prev/next buttons + the slide counter. For each slide's content, walks `slide.blocks` via the framework's `renderChildren` — which dispatches through `q2SlidesRegistry['Block']` → `q2SlidesRegistry[node.t]`. Title-slide rendering uses the same inline-styled `<h1>` / `<p>` as today (extracted to a local helper component shared with `RevealjsAst`).
- [ ] **10.11** Create `q2-slides/RevealjsAst.tsx`. The reveal.js document-root, registered as `'Ast'` in `revealjsRegistry`. Identical to today's `RevealjsSlideAst` (the reveal.js Deck, the plugin imports, the menu CSS override) but: walks `slide.blocks` via `renderChildren` instead of importing `renderBlock` from the old top-level file. Title-slide rendering shares the helper from 10.10.
- [ ] **10.12** Create `q2-slides/registry.ts` per the §2 source listing above. Both registries spread the same `Blocks` / `Inlines` modules and the same `Block` / `Inline` dispatchers; they differ only on `'Ast'`.
- [ ] **10.13** Convert `hub-client/src/components/render/ReactAstSlideRenderer.tsx` into a re-export barrel that exposes the new internals under their old names: `export { SlideAst, parseSlides, renderBlock, type Slide } from './q2-slides';`. The old `renderBlock` export becomes a wrapper that uses the new framework dispatch internally — its signature `(block, key, currentFilePath, onNavigateToDocument) => ReactNode` is preserved so the four external consumers don't break during the migration. Convert `RevealjsReactAstSlideRenderer.tsx` similarly: `export { RevealjsAst as RevealjsSlideAst } from './q2-slides';`. **End of Phase 10**: tree compiles green; no consumers have moved yet.

### Phase 11 — Migrate consumers (one commit each)

- [ ] **11.1** `RevealjsReactAstSlideRenderer.tsx`-side migration is already covered by Phase 10.13's shim conversion. Verify by deleting the file's body (keeping only the re-export) and confirming `npm run build:all` is green. Commit.
- [ ] **11.2** Migrate `ReactRenderer.tsx`'s slide branch. Replace lines 209-233 with:

  ```tsx
  const ast = JSON.parse(astJson);
  const isRevealjs = extractMetaString(ast?.meta?.format) === 'revealjs';
  const slideRegistry = isRevealjs ? revealjsRegistry : q2SlidesRegistry;
  return (
      <ErrorBoundary>
          <SlideContext.Provider value={slideContextValue}>
              <Ast registry={slideRegistry} {...astProps} />
          </SlideContext.Provider>
      </ErrorBoundary>
  );
  ```

  `slideContextValue` carries the parent-owned `currentSlide` state (read from existing `currentSlideIndex` / `onSlideChange` props) shaped to whatever §3 picks. Imports update from `./ReactAstSlideRenderer` / `./RevealjsReactAstSlideRenderer` to `./q2-slides`. Commit.
- [ ] **11.3** Migrate `hub-client/src/hooks/useCursorToSlide.ts`. The hook today imports `parseSlides` from `'../components/render/ReactAstSlideRenderer'` and walks slides to find which one a cursor position belongs to. Update the import path to `'../components/render/q2-slides'`. No logic changes. Commit.
- [ ] **11.4** Migrate `hub-client/src/hooks/useSlideThumbnails.tsx`. Imports `parseSlides`, `renderBlock` (and types). Update paths to `'../components/render/q2-slides'`. The hook also calls `renderBlock(block, key, currentFilePath, onNavigateToDocument)` directly to render thumbnails — verify this signature is preserved by the new exports (the public `renderBlock` function in `q2-slides/index.ts` should preserve it for thumbnail compatibility). If thumbnails want to switch to the registry-based dispatch, that's a follow-up; for v1, the imperative path stays as a sibling export. Commit.

### Phase 12 — Delete shim, retire old files

- [ ] **12.1** Once all four consumers reference `./q2-slides`, delete `ReactAstSlideRenderer.tsx` and `RevealjsReactAstSlideRenderer.tsx`. Delete top-level `AspectRatioScaler.tsx` (it moved in 10.5). Verify `ReactRenderer.integration.test.tsx` (the only other file that referenced `AspectRatioScaler`) updates its import to point at the new location. Commit.
- [ ] **12.2** Delete the slide-side branch in `getQ2Format.ts` if anything became dead. (Today `getQ2Format` returns the format string; the routing in `ReactRenderer.tsx` does the slide-vs-revealjs split. After 11.2 the split is internal to the slide branch; `getQ2Format` is unaffected.) Verify.

### Phase 13 — Verification

- [ ] **13.1** Run `cargo xtask verify --skip-hub-build && cd hub-client && npm run build:all && npm run test:ci`. The slide tests at `q2-debug.integration.test.tsx` are q2-debug-only; q2-slides has no integration tests today. Add a minimal smoke test for `q2-slides/SlideAst.tsx`: renders a 2-slide doc, asserts the prev/next buttons exist, asserts `slides.length === 2`. Add a minimal smoke test for `q2-slides/RevealjsAst.tsx`: renders the same doc, asserts the reveal.js Deck is mounted (via a `data-testid` on a wrapper).
- [ ] **13.2** Manual browser session. Open a `format: q2-slides` doc and a `format: revealjs` doc through a running hub. Verify visual parity with pre-migration: same dark frame, same prev/next behavior for q2-slides; same reveal.js menu / arrow keys / theme for revealjs. Record both invocations and observed-output snippets in the implementation transcript.

## Out of scope

- **Iframing q2-slides** — left for §1's open-question follow-up. If 2E ships in-page (recommended) and a later need (slide editing, sandboxing) wants iframing, it's a single-format plan against the post-2E directory layout.
- **Asset-manifest migration for slide images** — left for §4's open-question follow-up. The VFS-direct path is functional; switching to manifest distribution is a symmetry win not a correctness fix.
- **Slide editing (`setLocalAst` plumbing)** — left for §5's open-question follow-up. Lands as a feature plan, not a refactor.
- **Slide transitions / themes / presenter mode** — reveal.js features that already work via plugin imports; the migration preserves them without expanding scope.
- **Routing `format: revealjs` through a Rust-side HTML pipeline** — explicitly NOT planned. q2-slides and revealjs both stay React-side after 2E. If a future plan adds a Rust revealjs HTML output, retiring the React revealjs path is a separate cleanup.
- **Format-specific theme injection** (q2-preview has `themeFingerprint` / `applyTheme` for Bootstrap-flavored CSS) — slides have hardcoded inline styles and reveal.js's own theme CSS today; no theme-injection contract exists for them. Adding one is its own feature plan.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `q2-slides/styles.ts` (NEW — extracted constants) | ~50 |
| `q2-slides/attributesToProps.ts` (NEW — moved) | ~70 |
| `q2-slides/parseSlides.ts` (NEW — moved + slimmed; minus `extractMetaString`) | ~140 |
| `q2-slides/AspectRatioScaler.tsx` (moved verbatim from top-level) | ~92 |
| `q2-slides/SlideContext.tsx` (NEW) | ~25 |
| `q2-slides/blocks/*.tsx` (NEW — 11 leaves @ ~30 LOC each) | ~330 |
| `q2-slides/inlines/*.tsx` (NEW — 12 leaves @ ~25 LOC each, except Image and Math which are ~50) | ~340 |
| `q2-slides/blocks/index.ts` + `inlines/index.ts` | ~25 |
| `q2-slides/dispatchers.tsx` (NEW — Block, Inline) | ~50 |
| `q2-slides/SlideAst.tsx` (NEW — carousel chrome only; leaves are dispatched) | ~120 |
| `q2-slides/RevealjsAst.tsx` (NEW — reveal.js chrome only) | ~110 |
| `q2-slides/registry.ts` (NEW — both registries + sharedLeaves spread) | ~30 |
| `q2-slides/index.ts` (NEW — public barrel) | ~10 |
| `ReactAstSlideRenderer.tsx` shim → deletion | -885 |
| `RevealjsReactAstSlideRenderer.tsx` shim → deletion | -163 |
| Top-level `AspectRatioScaler.tsx` deletion | -92 |
| `ReactRenderer.tsx` slide-branch rewrite | ~10 |
| `useCursorToSlide.ts` import path update | ~1 |
| `useSlideThumbnails.tsx` import path update | ~1 |
| **Net** | **~+250 LOC, distributed across 30+ small files** |

The line-count *grows* slightly because per-tag leaves are explicit (one file per Pandoc tag) rather than buried in a switch arm. The structural payoff is exactly that: each leaf is independently editable, importable, testable, and overridable. q2-debug and q2-preview already pay that file-count cost; q2-slides catching up is the point.

## Dependencies

### Hard dependencies

- **Plan 2pre** ✅ (landed) — the framework / format / registry contract.
- **Plan 2D Phase 6.0** — `framework/meta.ts` and `framework/plainText.ts`. `q2-slides/parseSlides.ts` imports `extractMetaString` from `framework`; the slide-renderer's private copy retired in 2D's 6.0d. **2E cannot land before 2D's 6.0 phase**; once 6.0 is in, 2E's other phases (2D's 6.1+ for body-container / title-block) are independent of 2E.
- **`@revealjs/react`** + reveal.js peer deps — already installed (introduced 2026-03-20 commit `0cfd9e71`). Plan 2E does not change them.

### Soft / activation dependencies

None. Both slide formats already function today; the migration is structural.

### Blocks

Nothing immediately. Possible follow-ups it unblocks:
- Iframing q2-slides (§1).
- Manifest-based asset distribution for slides (§4).
- Slide editing (§5).
- Slide-format theme-CSS injection (parallel to q2-preview's `applyTheme` pattern).
- A future Rust-side revealjs HTML output, if/when desired (separate plan; doesn't conflict with 2E).

## Risk areas

- **External-consumer signature drift**. Phase 10.13 keeps the old `renderBlock(block, key, currentFilePath, onNavigateToDocument) → ReactNode` signature exposed for `useSlideThumbnails`'s use. After Phase 11.4 nothing should be calling that old signature, but the q2-slides `index.ts` continues to export it as a thumbnail-rendering helper. If a future plan wants to switch thumbnails to the registry-based dispatch, that's the natural follow-up; until then the imperative export is a deliberate convenience. Document the dual contract in `index.ts`'s doc-comment.
- **Slide-control state ownership**. The parent (`Editor.tsx`) owns `currentSlideIndex` state today and threads it through `ReactPreview` → `ReactRenderer` → `SlideAst` / `RevealjsSlideAst` as props. Switching to `SlideContext` keeps the parent ownership but moves the read-side from props to context. Verify there's no stale-closure bug when `currentSlideIndex` updates while a leaf is mid-render. (Should be fine — context updates re-render consumers; the existing prop-drilling did the same.)
- **AspectRatioScaler tests**. The top-level `ReactRenderer.integration.test.tsx` references `AspectRatioScaler` directly. After Phase 12.1's move, the import path changes. Verify the test still passes against the new location.
- **Reveal.js dependency footprint**. `revealjsRegistry`'s `Ast: RevealjsAst` static-imports `@revealjs/react`, reveal.js plugins, and reveal.js CSS. Even if a user only ever opens `format: q2-slides` documents, the bundle pulls reveal.js because `q2-slides/registry.ts` exports both registries side-by-side. Today's tree has the same coupling (`ReactRenderer.tsx` imports `RevealjsSlideAst` unconditionally), so this is not a regression — just a known cost of the unified module. If bundle size becomes a concern, dynamic-import `RevealjsAst` in a follow-up.
- **Title-slide rendering shared between SlideAst and RevealjsAst**. Pulled into a local helper in 10.10 / 10.11. Verify the helper doesn't accidentally pull format-specific styling from one chrome that doesn't apply in the other.

## References

### hub-client side (modified by 2E)

- `hub-client/src/components/render/ReactAstSlideRenderer.tsx` — converted to shim in 10.13, deleted in 12.1.
- `hub-client/src/components/render/RevealjsReactAstSlideRenderer.tsx` — converted to shim in 10.13, deleted in 12.1.
- `hub-client/src/components/render/AspectRatioScaler.tsx` — moved to `q2-slides/AspectRatioScaler.tsx` in 10.5; top-level deleted in 12.1.
- `hub-client/src/components/render/ReactRenderer.tsx` — slide branch rewrite in 11.2 (lines 209-233 collapse to one `<Ast>` mount with conditional registry).
- `hub-client/src/hooks/useCursorToSlide.ts` — import path update in 11.3.
- `hub-client/src/hooks/useSlideThumbnails.tsx` — import path update in 11.4.
- `hub-client/src/components/render/ReactRenderer.integration.test.tsx` — `AspectRatioScaler` import path update in 12.1.
- `hub-client/src/components/render/q2-slides/` — entire new directory.

### hub-client side (read-only references during implementation)

- `hub-client/src/components/render/q2-preview/PreviewContext.tsx` — precedent for `SlideContext`'s shape.
- `hub-client/src/components/render/q2-preview/registry.ts` — precedent for the registry layout (single registry; q2-slides has two but the layout is parallel).
- `hub-client/src/components/render/q2-debug/registry.ts` — second precedent.
- `hub-client/src/components/render/framework/index.ts` — what the leaves and dispatchers consume (`Node`, `renderChildren`, `RegistryContext`, `extractMetaString` post-2D 6.0).
- `hub-client/src/components/Editor.tsx:244, :278-281, :1051-1052` — the parent-side slide-control state (`currentSlideIndex`, `setCurrentSlideIndex`, `useSlideThumbnails`, `useCursorToSlide`). Reading these confirms the SlideContext shape is sufficient.

## Revision history

- **2026-05-10**: initial draft. §2 (two registries sharing leaves) locked per discussion; §1, §3, §4, §5, §6 left open with recommendations. Plan structure mirrors 2pre's two-phase shim approach (build new → migrate consumers → delete shim) since 2pre's pattern worked and 2E's migration is comparable in scope.
