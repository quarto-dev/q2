# Handoff: remove `CommentBlock`'s per-block wrapper (q2-preview comment chrome as an overlay layer)

**Strand:** bd-q2wqj24c (re-scoped 2026-09-10; `related` → bd-j3764r9a parity epic; discovered-from bd-kltzdhle)
**Written:** 2026-09-10, for a fresh session. Everything below was verified in the
session that made hub-client default to q2-preview (PR for bd-kltzdhle; plan
`2026-09-09-hub-client-default-q2-preview.md`, Phase 4b).
**Status:** not started. This note is the recommendation, not a decision record —
the implementer should re-verify the facts marked *(verified)* and challenge the
design where the code disagrees.

## The defect

`CommentBlock` (`ts-packages/preview-renderer/src/q2-preview/custom/CommentBlock.tsx`,
1245 lines) is registered as the q2-preview `Block` renderer
(`q2-preview/registry.ts:51-56`): it wraps the block dispatcher for **every**
block, extracts `[>> …]` comment spans, and renders the comment bubble chrome.
To do that it wraps each commentable block in

```html
<div style="position: relative; box-shadow: none; transition: box-shadow 0.15s">
  <!-- chrome (position: absolute; top: -11px; right: -10px), only when visible -->
  <p …>the block</p>
</div>
```

(`CommentBlock.tsx` ~981-1010: the wrapper `<div>` with `onMouseMove` /
`onMouseLeave`; ~1000-1030: the absolutely positioned chrome container.)

*(verified)* The wrapper is present in the live q2-preview DOM in **both**
hub-client (editing on) and read-only `q2 preview`: the smoke-all fixture
`toc-containers/div-heading-becomes-section.qmd` renders as
`blockquote > div[style=…] > h4#quoted`, and a 4-heading document had 15 wrappers.

Consequences:

1. Every theme rule keyed on a direct child stops matching the block:
   `blockquote > h4`, `.callout-body > :first-child` (2 rules),
   `.callout-body > div` (3), `li > p:last-of-type`, `li > a/span/ul`,
   `.tab-pane > p / pre:last-child`, `section > section`
   (`grep -rhoE '… *> *…' resources/scss`). These are real visual parity
   drifts against `q2 render`, not just failing test selectors.
2. The parity harness (`hub-client/src/services/smokeAllParity.wasm.test.tsx`)
   cannot see it: its read-only mount has no `PreviewContext`, so the chrome
   never renders and fixtures carry `dom-parity: true` while the live iframe
   diverges. Note this blind spot in whatever test you add.
3. The code already fights the symptom: the chrome is mounted *before* the
   content so the block keeps matching `:last-child` rules (comment at
   ~1003).

The e2e runner currently skips the DOM assertions for that one fixture
(`DOM_ASSERTIONS_PENDING_PARITY` in `hub-client/e2e/helpers/smokeAllDiscovery.ts`,
keyed `'toc-containers/div-heading-becomes-section.qmd' → 'bd-q2wqj24c'`).
Removing that entry is the last step of this work.

## What does NOT work

- **`display: contents` on the wrapper.** Removes the wrapper's *box* (so grid
  placement is unaffected — that is why the chrome slots use it, bd-xdier), but
  the element stays in the DOM tree and the CSS `>` combinator matches the DOM
  tree. `blockquote > h4` still fails. The wrapper element itself must go.
- **A sentinel sibling element** (a hidden `<span>` before the block to find
  it): it becomes the parent's `:first-child`, breaking `.callout-body > :first-child`
  the same way.
- **Portalling the chrome inside the block's own element**: a `<div>` inside
  `<p>`, chrome text in `textContent`, inline `position: relative` on theme
  elements. Rejected.

## Recommended design: one overlay layer, no wrapper

`CommentBlock` renders the block untouched — `<>{children}</>` plus its
chrome portalled into a single positioned layer — and positions each bubble
from the block's measured rect.

1. **Overlay layer.** `PreviewDocument` renders one
   `<div data-q2-comment-layer style="position:absolute; inset:0; pointer-events:none">`
   inside `#quarto-content` (which gets `position: relative`; confirm that
   does not disturb the theme's grid — a positioned grid container is fine,
   but check `page-columns` rules). Bubbles are `position:absolute` children
   of the layer with `pointer-events:auto`. Provide the layer element through
   a context so each `CommentBlock` can `createPortal` into it. Reveal decks:
   the layer must live inside `.reveal .slides` scaling context or the
   counter-scale math changes — read the `DECK_BUBBLE_FUDGE` / `RevealScaleSync`
   comments (~370-400) before deciding where the layer sits for decks.
2. **Geometry.** The existing relayout pass (`scheduleBubbleRelayout`,
   ~400-560) already measures every bubble's anchor rect and solves overlaps
   in viewport px; today the anchor is `e.el.parentElement` (the wrapper).
   Change `BubbleEntry` to carry the **block element** as the anchor and have
   the pass write each bubble's `top`/`left` (anchor rect minus layer rect,
   plus the `-11px` / `-10px` offsets × scale) instead of relying on CSS
   anchoring. Trigger the pass from a `ResizeObserver` on each anchor and on
   the layer (content height changes above a block move it without resizing
   it; the layer/container resize catches those), plus the existing triggers
   (register, hover, mode switch, image load, `q2-reveal-scale`).
3. **Hover.** Delegated `mousemove`/`mouseleave` on `#quarto-content`
   (there is precedent: `useBlockEditHover` delegates pointer handling at the
   root and resolves `[data-block-pool-id]` hosts). Map the pointer to the
   nearest anchor (via the entry set — anchors are known elements; use
   `el.contains(target)` or `elementFromPoint`) and drive the per-block
   `isHovered` (right-half test, ~989) and `bubbleHovered` through the
   registry entries. The glow (`box-shadow` on the wrapper) becomes an
   overlay outline over the anchor's rect (`pointer-events:none`), so no
   style is written onto theme elements.
4. **Finding the block's DOM element.** Two options; recommend the first:
   - **Host-ref contract.** React 19 (`react@^19.2`) passes `ref` as an
     ordinary prop to function components. Add an optional `hostRef` to
     `NodeArgs` (`framework/types.ts:149`) and have each block component spread
     it onto its host element next to `dataLocProps(...)`. 14 of the 16
     block components already spread `dataLocProps` on one host element
     (`grep -c 'dataLocProps(' q2-preview/blocks/*.tsx`). Two do not:
     `Plain` renders a fragment (`<>{renderChildren(args)}</>`, no element —
     comments on `Plain` blocks (tight list items) need a decision: anchor to
     the parent `<li>`, or give `Plain` no chrome), and `taskList` renders
     into a borrowed `<li>`. Custom nodes (Callout, Theorem, …) and user
     render-components also need the prop spread or they get no chrome —
     decide whether "no chrome" is acceptable for user components.
   - **`data-loc` lookup.** Every block host already carries
     `data-loc="f:l:c-l:c"` (`framework/sourceLoc.ts:24`), computable from the
     node's `l`. `CommentBlock` could `querySelector` it under `#quarto-content`
     after mount. Touches nothing else, but is indirect (two blocks can't share
     a loc, but synthesized blocks have none) and costs a query per block per
     render.
5. **Comment containers.** Blocks without an inline slot (code blocks, mermaid)
   are wrapped in a `Div.quarto-edit-comment-container` *in the AST* (see
   `CONTAINER_CLASS`, ~77-83, and `InsideCommentContainer`); that Div is a real
   block and stays. Only the React-side `<div style=position:relative>` goes.

## Tests to write first (red before the change)

- `CommentBlock.structure.integration.test.tsx` (new): mount with a
  `PreviewContext` (see how `CommentBlock.resolveLast.integration.test.tsx`
  builds `tree(texts, ctx)` and the `commitSubtreeEdit` stub, ~40-95) and
  assert **DOM shape**: `blockquote > h4`, `blockquote > p`,
  `div.callout-body > p:first-child`-style parent/child relations hold with
  comments present and absent, and with the bubble visible; the layer holds
  the bubbles; no element between a block and its parent.
- Rewrite the existing suites' DOM helpers: `wrapper()` /
  `wrapperGlows()` in `CommentBlock.resolveLast.integration.test.tsx` (~103-131)
  assume `para.parentElement` is the positioned wrapper; the glow assertion
  moves to the overlay outline. `CommentBlock.defensive` (`host = para.parentElement`,
  line ~96) likewise. `CommentBlock.bubbleText` is mostly bubble-internal and
  should survive.
- The parity harness blind spot: add a test that mounts **with**
  `PreviewContext` and compares the article body's element structure against
  a read-only mount (same AST) — that is the guard this bug lacked.
- End: remove the `div-heading-becomes-section` entry from
  `DOM_ASSERTIONS_PENDING_PARITY`; the fixture's `ensureHtmlElements`
  (`blockquote > h4#quoted`, no `blockquote section`) then runs live.

## Verification (what "done" means)

1. `cd ts-packages/preview-renderer && npm test && npm run test:integration`
   (578 + 655 tests today).
2. hub-client tiers under the pinned Node (see gotchas): `npm run test`,
   `test:integration`, `test:wasm`; `npm run typecheck`; `npm run build:all`.
3. e2e: rebuild the e2e dist (`VITE_E2E=1 npm run build` in hub-client, with
   the hub binary built) and run
   `npx playwright test --config playwright.smoke-all.config.ts -g "div-heading-becomes-section"`
   plus the full smoke-all config (157 tests, ~2-6 min) and the interactive
   comment specs (`q2-preview-render-components-comment.spec.ts`,
   `q2-preview-edit-toggle.spec.ts`).
4. Real browser: `cargo xtask build-hub-client-embed && cargo build --bin q2`,
   then `q2 preview <project> --ui editor --port 4322 --no-browser` and drive
   it with the Chrome DevTools MCP (the Claude-in-Chrome extension was not
   connected in the previous session). Check: hover the right half of a
   paragraph → "+" bubble; add a comment → bubble + `[>> …]` in the source;
   bubbles in a reveal deck (`format: revealjs`) still counter-scale; open
   Bootstrap dropdown in the navbar is unaffected. Also `q2 preview` without
   `--ui editor` (read-only chrome still shows existing comments).
5. Visual parity spot-check that motivated the change: a callout whose body
   starts with a paragraph — `.callout-body > :first-child` margin in preview
   should now match `q2 render`.
6. `cargo xtask verify` (full — `preview-renderer` is bundled into hub-client
   and the q2-preview SPA).

## Gotchas from the previous session

- **Node.** The non-interactive shell resolves `node` to Homebrew's v26; the
  repo pins 24 (`.nvmrc`). Under 26 about 23 hub-client unit tests fail
  spuriously (`localStorage.clear` undefined). Run every npm/npx/vitest
  command as `fnm exec --using=24 <cmd>` (`fnm use` only works with cwd at
  the repo root).
- **e2e configs.** `npx playwright test smoke-all` finds nothing — smoke-all is
  `testIgnore`d in the default config; use
  `--config playwright.smoke-all.config.ts`. The iframe kind is now
  `'q2-html-render' | 'q2-debug' | 'q2-preview'` with no default
  (`e2e/helpers/previewExtraction.ts`); plain fixtures mount the q2-preview
  iframe.
- **Stale artefacts.** `q2 preview` embeds the SPA and the editor UI at build
  time; after a `preview-renderer` change rebuild with
  `cargo xtask build-q2-preview-spa` (viewer) and
  `cargo xtask build-hub-client-embed` (editor), then `cargo build --bin q2`.
- **Don't reach for the wrapper's `parentElement`** anywhere new — the whole
  point is that the block's parent is the theme element again.

## Related strands still on the e2e skip-list (not this work)

bd-c3dtpe36 (mermaid), bd-b3oq2fsy (user `css:` links), bd-fandfn60
(repo actions in the TOC), bd-bg0jze2i (callout body heading sectionize —
diagnose before assuming it is a sectionize change; the blockquote item
turned out to be this wrapper), bd-47afd5ro (tabsets).
