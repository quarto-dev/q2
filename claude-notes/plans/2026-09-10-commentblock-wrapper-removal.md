# q2-preview: remove `CommentBlock`'s per-block wrapper `<div>` (bd-q2wqj24c)

**Date:** 2026-09-10
**Braid:** bd-q2wqj24c (P2 bug; labels `parity`, `preview-renderer`)
**Branch:** `feature/bd-kltzdhle-hub-client-default-q2-preview` — the branch under
PR #670 (https://github.com/quarto-dev/q2/pull/670). No worktree; the work is
designed and done in the main checkout and pushed to that branch so it merges
through #670 (user's instruction, 2026-09-10).
**Status:** Implemented and verified 2026-09-10 (Phases 0–5; only the changelog entry and the push remain). Committed on the PR #670 branch.
**Precursor:** `2026-09-10-commentblock-overlay-handoff.md` (the previous
session's recommendation). This plan re-verifies it and narrows it in three
places (see "What the code looks like today").
**Investigation artifacts:** `commentblock-wrapper-removal-investigation/`.

## Triage verdict

**Ready to design.** The defect is reproduced and diagnosed at HEAD, the handoff
note's facts check out, and the change is contained to
`ts-packages/preview-renderer/src/q2-preview` plus one e2e skip-list entry. The
remaining decisions are genuine design choices (where the overlay layer lives,
how a bubble finds its block's DOM element, what happens to `Plain` blocks and
to the block glow), listed under "Open design questions".

## Issue context

`CommentBlock` is registered as the q2-preview `Block` renderer
(`registry.ts:56`). For every block that can carry a `[>> …]` comment it renders

```html
<div style="position: relative; box-shadow: none; transition: box-shadow 0.15s">
  <div style="position: absolute; top: -11px; right: -10px; …">…bubble…</div>  <!-- only when visible -->
  <p>…the block…</p>
</div>
```

(`custom/CommentBlock.tsx:983-1010` wrapper; `1017-1060` chrome container.)
The wrapper exists so the bubble can be `position: absolute` against it and so
its `onMouseMove`/`onMouseLeave` can drive the right-half hover test and the
glow. The theme CSS is written for the native HTML writer's DOM, where a block
is a *direct* child of its container, so every `parent > child` rule stops
matching in the live preview: `blockquote > h4`, `.callout-body > :first-child`,
`.callout-body > div`, `li > p:last-of-type`, `.tab-pane > p`, `section > section`
(full inventory with counts: `commentblock-wrapper-removal-investigation/theme-direct-child-rules.md`).
These are visual parity drifts against `q2 render`, now visible by default since
PR #670 made q2-preview the hub-client default.

Filed 2026-09-09 by Carlos Scheidegger during the bd-kltzdhle work as "heading
inside blockquote not `blockquote > h4`"; re-scoped 2026-09-10 after diagnosis
(sectionize is correct; the wrapper is the cause). A comment on the strand
records that `display: contents` does **not** fix it — the `>` combinator
matches the DOM tree, not boxes — so the wrapper *element* must go.

## Dependency graph

- **discovered-from bd-kltzdhle** (in_progress, PR #670): "hub-client: q2-preview
  is the default renderer". Its plan's Phase 4b works the e2e DOM-assertion
  skip-list back down; this strand owns the
  `toc-containers/div-heading-becomes-section.qmd` entry in
  `DOM_ASSERTIONS_PENDING_PARITY` (`hub-client/e2e/helpers/smokeAllDiscovery.ts:453`).
  Removing that entry is the last step of this work. The plan explicitly hands
  this item to a fresh session and keeps it on the same PR.
- **related bd-j3764r9a** (open epic): React ↔ HTML DOM parity. Its enforcement
  tool is the parity harness (`hub-client/src/services/smokeAllParity.wasm.test.tsx`),
  which mounts the AST **read-only with no `PreviewContext`** — so the comment
  chrome never renders there and the harness is structurally blind to this bug.
  Any fix must add the guard the harness lacks.
- No `blocks` edges in either direction. Urgency comes from the parent PR, not
  from dependents.

## What the code looks like today

Everything the handoff note marks *(verified)* still holds at branch HEAD
(`7e67fb034`): wrapper at `CommentBlock.tsx:983`, chrome-before-content ordering
to keep `:last-child` matching (`:1010`), relayout pass anchoring on
`e.el.parentElement` (`:414`, `:455`), hover half-test on the wrapper's rect
(`:995`). Five commits touch the file; none since the handoff.

Three findings that **narrow or redirect** the handoff's design:

1. **The anchor problem is much smaller than stated.** The handoff counts "14 of
   16 block components" needing a host-ref. But `CommentBlock` returns
   passthrough for everything except blocks with an inline slot (`Para`,
   `Plain`, `Header`), `CodeBlock` (incl. the `MermaidCodeBlock` override), and
   the `Div.quarto-edit-comment-container` (`canHoldComment`, `:267-270`).
   Callout, Theorem, Figure, lists-as-a-whole, tables, and every other custom
   node never get chrome today. So the contract touches **five** components,
   and only `Plain` (a fragment, `blocks/Plain.tsx`) lacks a host element.
2. **Reveal decks do not render `PreviewDocument`.** `RevealDeck.tsx:396-437`
   renders `@revealjs/react`'s `<Deck>` directly under a `RegistryContext`; there
   is no `#quarto-content` in a deck. A layer "inside `#quarto-content`" would
   simply not exist for decks. This pushes the design toward a layer that does
   not depend on either document root (Q1 below), which as a side effect puts
   bubbles *outside* reveal's `.slides` transform and would let the
   counter-scale + `DECK_BUBBLE_FUDGE` machinery (`:359-374`, `:1027`) go.
3. **There is a second wrapper of the same kind, out of scope.**
   `AttributionWrap` (`framework/attribution.tsx:143`) inserts
   `div.q2-attr-wrap` between a block and its parent whenever the Authors
   overlay resolves attribution for that node. Same defect class, but opt-in
   (only while the overlay is on) and shared with q2-debug. Not this strand;
   proposed as a new strand linked to bd-j3764r9a (Q7).

Precedents already in the tree for the pieces the design needs:

- Delegated pointer handling at the document root resolving hosts by
  `closest('[data-block-pool-id]')`: `useBlockEditHover.tsx` (spread onto
  `#quarto-content` via `hostProps`).
- A badge positioned from a measured rect at `position: fixed`, rendered by the
  root: `useAttributionHover` overlay (`attribution.tsx:245-253`).
- Element lookup by `data-loc`: `iframe/scrollSyncDom.ts:54,137`.
- The relayout pass already works entirely in viewport px and re-measures every
  entry each pass (`scheduleBubbleRelayout`, `:400-560`).

Repro at HEAD: the fixture `crates/quarto/tests/smoke-all/toc-containers/div-heading-becomes-section.qmd`
(`ensureHtmlElements: blockquote > h4#quoted`) carries `dom-parity: true` and
passes the read-only harness, while the e2e runner skips its DOM assertions via
the skip-list entry. No new repro fixture is needed; the existing one is the
regression test once the entry is removed.

Pre-flight (`cargo xtask verify --skip-hub-build` under Node 24): Rust build,
tests and lints green. The hub-client WASM tier failed 4 tests in
`previewFormatSubstitution.wasm.test.ts` with "Unknown format: q2-html-render"
because the checked-in WASM artifact predates this branch's format
registration — a stale-artifact issue, not a defect at HEAD (the parent plan's
Phase 7 records the full verify green after `npm run build:wasm`). Confirmed:
after `npm run build:wasm` the file passes 7/7 (2026-09-10).

## Decisions (2026-09-10, with the user)

- **D1 — Layer.** One body-level layer (`position: absolute; top: 0; left: 0;
  width: 0; height: 0; overflow: visible; pointer-events: none`), lazily
  created by the comment module (same idempotent pattern as the
  `data-q2-comment-styles` style tag), portalled into by every `CommentWrapper`.
  Bubbles are `position: absolute` children at **document coordinates**
  (anchor rect + `scrollX/Y`), `pointer-events: auto`. No scroll listener.
  Works identically under `PreviewDocument` and `RevealDeck`.
- **D2 — Anchor discovery.** A `CommentAnchorContext` provided per
  `CommentBlock` with `{ node, register }`; the chrome-eligible host components
  (`Para`, `Header`, `CodeBlock`, `MermaidCodeBlock`, `Div`) call a hook that
  returns a ref callback only when `ctx.node === args.node`. No `NodeArgs`
  change; nothing threads through the dispatcher or `AttributionWrap`. A user
  `render-components` override that does not adopt the hook gets no chrome —
  accepted; the override infrastructure is expected to change and is not worth
  designing around now.
- **D3 — `Plain`.** Option (a): the components that host a `Plain` in an
  element provide that element through a `PlainHostContext`, and a `Plain`'s
  `CommentBlock` anchors to it. Blast radius today (see below): `BulletList`
  and `OrderedList` `<li>`s are the only sites where a `Plain` can *get* chrome;
  `DefinitionList` `<dd>`/`<dt>` only matter for read-only display of comments
  that already exist. **Fallback when no anchor is found:** passthrough with
  the comment spans left in the text — nothing silently disappears.
- **D4 — Glow.** Keep it as an overlay outline over the anchor rect
  (`pointer-events: none`, in the same layer); evaluate in the browser.
- **D5 — Decks.** Delete the measured `scale`, the counter-scale transform and
  `DECK_BUBBLE_FUDGE`; keep `.present`-slide gating and the `q2-reveal-scale`
  relayout trigger. Re-introduce a single deck size constant only if the
  browser check shows deck bubbles read small.
- **D6 — Freshness.** Relayout on `ResizeObserver` (anchors + the layer's
  container) plus the existing triggers; one-frame lag for content growth
  above a block is acceptable. **No `MutationObserver`.**
- **D7 — `AttributionWrap`.** Out of scope; filed as bd-ijlb2yui
  (related → bd-j3764r9a, discovered-from → bd-q2wqj24c).

### Q3 blast radius (what "Plain gets chrome" means today)

`CommentBlock` gives a comment-less block chrome only when `resolveSource`
returns a non-Opaque node of the *same commentable kind* (`:272-289`). For a
`Plain` that means:

| host of the `Plain` | resolves to | chrome today | after D3 |
|---|---|---|---|
| `BulletList` / `OrderedList` item (`<li>`, incl. task items) | the `Plain` itself | yes (`li > div[pos:rel] > text`) | anchor = the `<li>` — `blocks/BulletList.tsx:38,87`, `blocks/OrderedList.tsx:53,95,106` |
| `DefinitionList` term/definition (`<dt>`/`<dd>`) | the `DefinitionList` → kind mismatch | no (read-only display only if a comment already exists) | anchor = `<dt>`/`<dd>` (`blocks/DefinitionList.tsx:34,62`) so existing comments still show |
| `Table` cell | Opaque | no | unchanged (no anchor → passthrough, spans in text) |
| `Figure` caption | the `Figure` → kind mismatch | no | unchanged |

So D3 touches two files for the real case (five `<li>` sites) and one more for
read-only parity; `taskList.tsx` renders inside the list's `<li>` and needs
nothing.

## Phases

### Phase 0 — Tests first (red before the change)

- [x] `custom/CommentBlock.structure.integration.test.tsx` (new; 10 tests, ran red first: 10/12 failed). Mount with a
      `PreviewContext` (pattern: `CommentBlock.resolveLast.integration.test.tsx:40-95`,
      generalised to arbitrary block trees) and assert DOM shape with comments
      absent, present, and with the bubble visible (hover): `blockquote > h4`,
      `blockquote > p`, `li > p` (loose item), `li` text directly (tight item),
      `div.callout-body > p:first-child`; no element between a block and its
      parent; bubbles live under `[data-q2-comment-layer]`; `#quarto-content`
      contains no bubble.
- [x] Parity guard (in the structure suite): same AST mounted read-only and with `PreviewContext` +
      visible bubbles — `main#quarto-document-content` element structure
      (tag names + classes, text-free) identical.
- [x] Rewrote wrapper-dependent helpers: `wrapper()` / `wrapperGlows()`
      (`resolveLast` `:103-131`) → glow read from the overlay outline;
      `defensive` `:96` host assertion → anchor registered / bubble present.
- [x] Geometry test (`CommentBlock.geometry.integration.test.tsx`, 2 tests, red first): with stubbed rects, a bubble's
      `top`/`left` equal anchor rect + scroll + the `-11px`/`-10px` offsets;
      the force-layout nudge still separates two overlapping bubbles.
- [x] Plain-in-list test (in the structure suite): a tight bullet item with a comment renders
      `li > text` (no wrapper) and its bubble is anchored to the `<li>` rect;
      a commented `Plain` with no host context renders passthrough with the
      span in the text.

### Phase 1 — Anchor discovery (D2, D3)

- [x] `q2-preview/commentAnchor.tsx`: `CommentAnchorContext`,
      `useCommentAnchorRef(node)`, `PlainHostContext`.
- [x] Adopted in `Para`, `Header`, `CodeBlock` (both return paths),
      `MermaidCodeBlock` (diagram + error + fallback paths), `Div`.
- [x] `BulletList`, `OrderedList`: provide `PlainHostContext` per `<li>`;
      `DefinitionList`: per `<dt>`/`<dd>`.
- [x] `CommentBlock`: provides the context around `B`; a `Plain` resolves its
      anchor from `PlainHostContext`; no anchor → passthrough (D3 fallback).

### Phase 2 — Overlay layer + geometry (D1, D5, D6)

- [x] Layer host: lazily created body-level element, re-created if detached
      (tests wipe `document.body`).
- [x] `CommentWrapper` renders `<>{children}</>` + `createPortal(chrome, layer)`.
- [x] `BubbleEntry` carries the **anchor element**; the pass measures the
      anchor rect, computes natural `top = rect.top - 11`, `left`/`right` from
      the anchor's right edge, solves as today (viewport px), and writes
      `top`/`left` in document coordinates (+ `translateY(nudge)` stays for the
      animated nudge).
- [x] Deleted `scale`/`setScale`, the counter-scale transform, `DECK_BUBBLE_FUDGE`;
      keep `.present` gating (via the anchor's `closest('.reveal section')`)
      and the `q2-reveal-scale` reset trigger.
- [x] `ResizeObserver` (built lazily on first use) on each registered anchor and on the layer's container
      (`#quarto-content` or `.reveal`, whichever exists) → `scheduleBubbleRelayout()`.

### Phase 3 — Hover + glow (D4)

- [x] Delegated `mousemove` / `mouseleave` on `document` (installed once while
      any entry is registered): resolve the anchor under the pointer by walking
      `target` ancestors against a `WeakMap<Element, entry>`; right-half test
      on the anchor rect; pointer inside a bubble → `bubbleHovered` for that
      entry (bubbles are outside the anchor subtree now, so containment is
      checked against the bubble element).
- [x] Glow: an outline element in the layer positioned over the anchor rect,
      shown while `bubbleHovered`.
- [x] Removed the wrapper `<div>` and the "chrome before content" ordering note.

### Phase 4 — Decks

- [x] Browser check on a `format: revealjs` doc (see the verification record: deck bubbles adopt the deck font and are scaled 1.2×): bubbles on the current slide
      only, normal size, positioned at the slide-local block; decide on a size
      constant (D5).

### Phase 5 — Close out

- [x] Removed `'toc-containers/div-heading-becomes-section.qmd'` from
      `DOM_ASSERTIONS_PENDING_PARITY`; run
      `npx playwright test --config playwright.smoke-all.config.ts` (full) and
      the interactive comment specs.
- [x] Browser verification per the handoff "Verification" list (hover `+`,
      add, resolve, read-only `q2 preview`, callout-body spot-check vs
      `q2 render`), after `cargo xtask build-q2-preview-spa`,
      `cargo xtask build-hub-client-embed`, `cargo build --bin q2`.
- [ ] `hub-client/changelog.md` (two-commit rule; after the first commit); updated
      `2026-09-10-commentblock-overlay-handoff.md` (superseded pointer) and
      bd-kltzdhle's Phase 4b table.
- [x] Full `cargo xtask verify` under Node 24 (all 14 steps green 2026-09-10, after the grammar-cache rebuild noted below); push to the PR branch on approval — pending.

## Risks / tradeoffs

- **The hover/positioning core is being rewritten**, including the nudge
  round-trip the comments warn is fragile ("never read back from our own
  transform"). Mitigation: the force-layout solver itself (sort, relax, settle,
  no-reorder) is untouched — only the anchor measurement and the write-back
  change.
- **Deck behaviour is under-tested** (no jsdom coverage of `RevealScaleSync`);
  Phase 4 relies on browser verification.
- **The parity harness stays blind** to chrome by design (read-only mount);
  the Phase 0 guard test is what prevents recurrence.
- **Stale embeds.** `q2 preview` embeds the SPA and the editor bundle; after
  the change, `cargo xtask build-q2-preview-spa`, `cargo xtask build-hub-client-embed`,
  then `cargo build --bin q2` before any browser check.
- **Node.** All npm/vitest commands must run under Node 24
  (`fnm exec --using=24 …`); the shell's default Node 26 fails the verify
  preflight and breaks ~23 hub-client unit tests spuriously.

## Verification record (2026-09-10)

Chain rebuilt before every browser check: `cargo xtask build-q2-preview-spa`,
`cargo xtask build-hub-client-embed`, `cargo build --bin q2`; e2e bundle via
`VITE_E2E=1 npm run build` in `hub-client` with `cargo build --bin hub`.

**Tests.** preview-renderer: 578 unit + 669 integration (14 new: 10 structure,
2 geometry, plus the rewritten resolveLast/defensive assertions) green;
hub-client: typecheck, 1102 unit, 128 integration, 140 WASM (incl. the parity
harness) green. e2e: smoke-all 156 passed / 1 skipped (pre-existing), with the
`div-heading-becomes-section` fixture's DOM assertions live; the interactive
comment specs (`q2-preview-render-components-comment`, `q2-preview-edit-toggle`)
4/4.

**Browser (hub-client editor UI).** `cargo run --bin q2 -- preview
<scratch>/verify-proj --ui editor --port 4322 --no-browser`, driven through
the Chrome DevTools MCP. Project: `_quarto.yml` + `doc.qmd` (blockquote with a
heading, titled callout, tight/loose lists, code block, a paragraph with two
comments) + `deck.qmd` (`format: revealjs`, two slides). Screenshots:
`bd-q2wqj24c-doc-bubbles.png`, `bd-q2wqj24c-deck-bubbles.png` (before the
deck font/size fix).

- DOM: `blockquote > h4` and `blockquote > p` hold; `.callout-body-container.callout-body`
  has `p` as first child with computed `margin-top: 0px` and last child
  `margin-bottom: 0px` — the same values `q2 render`'s output computes for
  the same document (checked in the browser on `doc.html`); 3 `li > p`;
  tight `<li>`s have no child elements; no `.q2-comment-bubble` and no
  `position: relative` div inside `main#quarto-document-content`.
- Layer: exactly one `[data-q2-comment-layer]`, a child of `body`,
  `position:absolute; 0×0; overflow:visible; pointer-events:none`.
- Geometry (viewport px): quote paragraph top 195.3 / right 699.8 → bubble
  top 184.3 / right 709.8; tight item top 450.2 → 439.2; closing paragraph
  top 802.8 → 791.8 (`−11` / `+10` everywhere).
- Hover: a synthetic `mousemove` on the right half of a comment-less tight
  item showed the `+` bubble at top 464.7 / right 731 (item top 475.7 /
  right 721) without inserting anything into the `<li>`; a real click on
  `+` opened the inline input with focus and did **not** activate the block
  editor (`#q2-active-edit-region` absent); typing "Added from the overlay
  (bd-q2wqj24c)" + Enter made the Monaco line read
  `* tight two[>> Added from the overlay (bd-q2wqj24c)]` and the bubble show
  the text with ✓; clicking ✓ returned the line to `* tight two` and the
  bubble to the hover-only `+`.
- Glow: pointer over a bubble mounts `[data-q2-comment-glow]` in the layer
  with a rect equal to the block's (top 68.8 / left 50.8 / 649×25.5) and no
  inline style on the block; moving onto the block's left half removes it.
- Deck (`deck.qmd`, `format: revealjs`, editor UI): `.slides` at
  `matrix(0.64, …)`; on the present slide the paragraph (top 230.8 / right
  709.4) has its bubble at top 219.8 / right 719.4 and the tight item (top
  271.7 / right 166.8) at 260.7 / 176.8 — placed against the *scaled* block
  rects with no counter-scale math. First pass showed two things the old
  in-tree chrome got for free: the bubbles inherited the page's default serif
  (they no longer sit under `.reveal`) and read small next to slide type. Fixed
  by adopting the anchor's computed `font-family` on every layout pass (a
  once-per-anchor cache was wrong: the first placement ran before the deck
  theme's stylesheet applied and measured `Times`) and scaling deck chrome by
  `DECK_BUBBLE_SCALE = 1.2` (D5). After the fix both bubbles report
  `"Source Sans Pro", Helvetica, sans-serif` and `translate(-100%, 0px) scale(1.2)`.
  Screenshot: `bd-q2wqj24c-deck-bubbles.png` (after the fix).
- Read-only `q2 preview` (`--port 4323`, no `--ui editor`, `?page=doc.qmd`):
  the five existing comments render as bubbles in the body-level layer, none
  inside `main`; `blockquote > h4` computes `margin-top: 25.5px` — the same
  as `q2 render`'s `doc.html` in the same browser; callout body margins 0/0
  as above; the chrome's `font-family` equals the body's.
- Gotcha met on the way: after rebuilding the embed, the already-open tab
  kept a **browser-cached** `q2-preview.html` whose asset URL now fell
  through to the SPA fallback, so the iframe ran the old code. Reload with
  the cache bypassed before trusting a re-check.
- Final e2e on the final bundle: `div-heading-becomes-section` 1/1 with live
  DOM assertions; the two interactive comment specs 4/4.
- Gotcha met during the full verify: Step 4 (`tree-sitter test`) failed 8
  corpus cases (all YAML-metadata / `---` related, producing `(metadata (yaml))`
  where the corpus expects `(metadata)`) with **no grammar change in this
  checkout**. Cause: the tree-sitter CLI caches the compiled parser at
  `~/.cache/tree-sitter/lib/markdown.dylib`, keyed by grammar *name* only, so
  a sibling checkout (`rooms/room-5/q2`, whose `parser.c` was regenerated at
  13:53) rebuilt the shared library and this checkout's tests ran a foreign
  parser. `tree-sitter test --rebuild` from this directory: 613/613. Worth
  a strand if it bites again: `cargo xtask verify` could pass `--rebuild`
  (or `-p <dir>`) so the grammar step is hermetic across checkouts.
