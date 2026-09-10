# q2-preview: remove `CommentBlock`'s per-block wrapper `<div>` (bd-q2wqj24c)

**Date:** 2026-09-10
**Braid:** bd-q2wqj24c (P2 bug; labels `parity`, `preview-renderer`)
**Branch:** `feature/bd-kltzdhle-hub-client-default-q2-preview` — the branch under
PR #670 (https://github.com/quarto-dev/q2/pull/670). No worktree; the work is
designed and done in the main checkout and pushed to that branch so it merges
through #670 (user's instruction, 2026-09-10).
**Status:** Investigation — pending design alignment with user. **Do not start
implementation until the user gives the go-ahead.**
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

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Tests first (red before the change).**
  - New `custom/CommentBlock.structure.integration.test.tsx`: mount with a
    `PreviewContext` (pattern: `CommentBlock.resolveLast.integration.test.tsx:40-95`)
    and assert DOM shape — `blockquote > h4`, `blockquote > p`,
    `li > p`, `div.callout-body > p:first-child`-style relations hold with
    comments absent, present, and with the bubble visible; no element sits
    between a block and its parent; bubbles live in the overlay layer.
  - Parity guard the harness lacks: a test that mounts the same AST twice
    (read-only vs with `PreviewContext` + a visible bubble) and asserts the
    article-body element structure is identical.
  - Rewrite the wrapper-dependent helpers: `wrapper()` / `wrapperGlows()` in
    `CommentBlock.resolveLast.integration.test.tsx:103-131`, the
    `host = para.parentElement` assertion in `CommentBlock.defensive.integration.test.tsx:96`.
    `CommentBlock.bubbleText` is bubble-internal and should survive.
- **Phase 1 — Anchor discovery.** Per Q2/Q3: block components expose their host
  element to the enclosing `CommentBlock`; `Plain` resolves to its parent host
  or renders passthrough.
- **Phase 2 — Overlay layer + geometry.** `CommentWrapper` renders
  `<>{children}</>` plus its chrome portalled into the layer; `BubbleEntry`
  carries the anchor element; the relayout pass writes `top`/`left` from the
  anchor rect; `ResizeObserver` on anchors and the layer's container joins the
  existing triggers.
- **Phase 3 — Hover + glow.** Delegated `mousemove`/`mouseleave` at the root
  (or on the layer's container) mapped to the nearest registered anchor;
  right-half test against the anchor's rect; glow becomes an overlay outline
  over the anchor rect (Q4).
- **Phase 4 — Decks.** Verify bubbles in `format: revealjs` under the new
  layer; decide what survives of `RevealScaleSync`, the `q2-reveal-scale`
  listener, `.present` gating, and `DECK_BUBBLE_FUDGE` (Q5).
- **Phase 5 — Close out.** Remove the `div-heading-becomes-section` entry from
  `DOM_ASSERTIONS_PENDING_PARITY`; run the smoke-all e2e config and the
  interactive comment specs; browser verification per the handoff's
  "Verification" list (hover `+`, add, resolve, deck bubbles, read-only
  `q2 preview`); callout-body spot-check against `q2 render`; update the
  handoff note and bd-kltzdhle's Phase 4b table; `hub-client/changelog.md`
  entry (the skip-list edit is under `hub-client/`, so the two-commit rule
  applies); full `cargo xtask verify`.

## Open design questions for the user

1. **Where the overlay layer lives.** (a) *Recommended:* one body-level layer
   (`position: absolute; top: 0; left: 0; width: 0; height: 0; overflow: visible;
   pointer-events: none`) lazily created by the comment module and portalled
   into, with bubbles at **document coordinates** (anchor rect + `scrollX/Y`).
   No scroll listener needed; works identically under `PreviewDocument` and
   `RevealDeck`; bubbles sit outside reveal's transform. (b) A layer rendered by
   each document root and handed down through a context — cleaner ownership,
   but both roots must opt in and decks need their own placement. Which?
2. **How a bubble finds its block's element.** (a) *Recommended:* a
   `CommentAnchorContext` provided per `CommentBlock` carrying `{ node, register }`;
   the five chrome-eligible components call a small hook that returns a ref
   callback only when `ctx.node === args.node` (the identity check keeps a
   nested `Para` inside a comment-container `Div` from registering as the
   Div's anchor). No change to `NodeArgs`, nothing threads through the
   dispatcher or `AttributionWrap`. (b) An optional `hostRef` prop on
   `NodeArgs` (React 19 passes `ref` as a plain prop). (c) A `data-loc`
   `querySelector` after mount — touches nothing else but fails for synthesized
   blocks and in jsdom tests without locations. Under (a) or (b), a user
   `render-components` override of `Para`/`Header` that does not adopt the hook
   gets no bubble; is "no chrome for non-adopting overrides" acceptable?
3. **`Plain` blocks (tight list items, definition bodies, table cells).**
   `Plain` renders a fragment. Options: (a) the list/definition/table
   components provide their `<li>`/`<dd>`/`<td>` element through a
   `PlainHostContext` so the Plain's bubble anchors to it; (b) no bubble on
   Plain blocks. And the fallback when no anchor is found (whichever option):
   render passthrough with the comment spans left **in the text** (nothing
   silently disappears), or strip and show nothing as `hide` mode does?
4. **The block glow.** Today bubble-hover paints a `box-shadow` on the wrapper.
   Keep it as an overlay outline positioned over the anchor rect
   (`pointer-events: none`), or drop the glow and keep only the bubble-side
   glow on block hover?
5. **Deck sizing.** With bubbles outside the `.slides` transform, the
   counter-scale, the measured `scale` field, and `DECK_BUBBLE_FUDGE` become
   unnecessary; the `.present`-slide gating and the `q2-reveal-scale` relayout
   trigger stay. OK to delete the scale machinery, keeping a single "bubbles on
   decks are 1.2× larger" constant only if a real browser check shows they read
   small?
6. **Position freshness.** With document-coordinate placement, a bubble's
   position updates on the next relayout pass (ResizeObserver, register, hover,
   mode switch, image load, deck signal) rather than by CSS. Content growth
   above a block that does not change the block's size is caught by the
   observer on the layer's container (`#quarto-content` / `.reveal`), one
   frame late. Acceptable, or do you want the pass also driven by a
   `MutationObserver`?
7. **`AttributionWrap` (finding 3).** File a separate strand under
   bd-j3764r9a for the `div.q2-attr-wrap` wrapper, and leave it out of this
   change?

## Risks / tradeoffs (draft)

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
