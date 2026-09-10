# q2-preview: remove `CommentBlock`'s per-block wrapper `<div>` (bd-q2wqj24c)

**Date:** 2026-09-10
**Braid:** bd-q2wqj24c (P2 bug; labels `parity`, `preview-renderer`)
**Branch:** `feature/bd-kltzdhle-hub-client-default-q2-preview` — the branch under
PR #670 (https://github.com/quarto-dev/q2/pull/670). No worktree; the work is
designed and done in the main checkout and pushed to that branch so it merges
through #670 (user's instruction, 2026-09-10).
**Status:** Designed (D1–D7 agreed with the user 2026-09-10); implementation not yet started.
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

- [ ] `custom/CommentBlock.structure.integration.test.tsx` (new). Mount with a
      `PreviewContext` (pattern: `CommentBlock.resolveLast.integration.test.tsx:40-95`,
      generalised to arbitrary block trees) and assert DOM shape with comments
      absent, present, and with the bubble visible (hover): `blockquote > h4`,
      `blockquote > p`, `li > p` (loose item), `li` text directly (tight item),
      `div.callout-body > p:first-child`; no element between a block and its
      parent; bubbles live under `[data-q2-comment-layer]`; `#quarto-content`
      contains no bubble.
- [ ] Parity guard: same AST mounted read-only and with `PreviewContext` +
      visible bubbles — `main#quarto-document-content` element structure
      (tag names + classes, text-free) identical.
- [ ] Rewrite wrapper-dependent helpers: `wrapper()` / `wrapperGlows()`
      (`resolveLast` `:103-131`) → glow read from the overlay outline;
      `defensive` `:96` host assertion → anchor registered / bubble present.
- [ ] Geometry unit test for the pass: with stubbed rects, a bubble's
      `top`/`left` equal anchor rect + scroll + the `-11px`/`-10px` offsets;
      the force-layout nudge still separates two overlapping bubbles.
- [ ] Plain-in-list test: a tight bullet item with a comment renders
      `li > text` (no wrapper) and its bubble is anchored to the `<li>` rect;
      a commented `Plain` with no host context renders passthrough with the
      span in the text.

### Phase 1 — Anchor discovery (D2, D3)

- [ ] `q2-preview/commentAnchor.ts(x)`: `CommentAnchorContext`,
      `useCommentAnchorRef(node)`, `PlainHostContext`.
- [ ] Adopt in `Para`, `Header`, `CodeBlock` (both return paths),
      `MermaidCodeBlock` (diagram + error + fallback paths), `Div`.
- [ ] `BulletList`, `OrderedList`: provide `PlainHostContext` per `<li>`;
      `DefinitionList`: per `<dt>`/`<dd>`.
- [ ] `CommentBlock`: provide the context around `B`; a `Plain` resolves its
      anchor from `PlainHostContext`; no anchor → passthrough (D3 fallback).

### Phase 2 — Overlay layer + geometry (D1, D5, D6)

- [ ] Layer host: lazily created body-level element, re-created if detached
      (tests wipe `document.body`).
- [ ] `CommentWrapper` renders `<>{children}</>` + `createPortal(chrome, layer)`.
- [ ] `BubbleEntry` carries the **anchor element**; the pass measures the
      anchor rect, computes natural `top = rect.top - 11`, `left`/`right` from
      the anchor's right edge, solves as today (viewport px), and writes
      `top`/`left` in document coordinates (+ `translateY(nudge)` stays for the
      animated nudge).
- [ ] Delete `scale`/`setScale`, the counter-scale transform, `DECK_BUBBLE_FUDGE`;
      keep `.present` gating (via the anchor's `closest('.reveal section')`)
      and the `q2-reveal-scale` reset trigger.
- [ ] `ResizeObserver` on each registered anchor and on the layer's container
      (`#quarto-content` or `.reveal`, whichever exists) → `scheduleBubbleRelayout()`.

### Phase 3 — Hover + glow (D4)

- [ ] Delegated `mousemove` / `mouseleave` on `document` (installed once while
      any entry is registered): resolve the anchor under the pointer by walking
      `target` ancestors against a `WeakMap<Element, entry>`; right-half test
      on the anchor rect; pointer inside a bubble → `bubbleHovered` for that
      entry (bubbles are outside the anchor subtree now, so containment is
      checked against the bubble element).
- [ ] Glow: an outline element in the layer positioned over the anchor rect,
      shown while `bubbleHovered`.
- [ ] Remove the wrapper `<div>` and the "chrome before content" ordering note.

### Phase 4 — Decks

- [ ] Browser check on a `format: revealjs` doc: bubbles on the current slide
      only, normal size, positioned at the slide-local block; decide on a size
      constant (D5).

### Phase 5 — Close out

- [ ] Remove `'toc-containers/div-heading-becomes-section.qmd'` from
      `DOM_ASSERTIONS_PENDING_PARITY`; run
      `npx playwright test --config playwright.smoke-all.config.ts` (full) and
      the interactive comment specs.
- [ ] Browser verification per the handoff "Verification" list (hover `+`,
      add, resolve, read-only `q2 preview`, callout-body spot-check vs
      `q2 render`), after `cargo xtask build-q2-preview-spa`,
      `cargo xtask build-hub-client-embed`, `cargo build --bin q2`.
- [ ] `hub-client/changelog.md` (two-commit rule); update
      `2026-09-10-commentblock-overlay-handoff.md` (superseded pointer) and
      bd-kltzdhle's Phase 4b table.
- [ ] Full `cargo xtask verify` under Node 24; push to the PR branch on approval.

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
