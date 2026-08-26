# Rich inline rendering in comment bubbles

**Strand:** bd-y66gbfs4 (discovered-from bd-wcz4x7y0, PR #612)
**Date:** 2026-08-26
**Status:** in progress on `braid/bd-y66gbfs4-render-rich-inlines-emphasis`

**Decisions locked (Carlos, 2026-08-26):** rich rendering in BOTH
surfaces (compact + expanded); link handling ships with the
`scrollIntoView` fallback (no host `scrollToAnchor` wiring); plain-text
authoring from the bubble input for v1; chip text is the generic
`[unsupported content]` + `title` tooltip (narrow beats specific).

## Context

After bd-wcz4x7y0, comment bubbles show the *full plain text* of a
`[>> ...]` comment span via `inlinesToPlainText` — content is no longer
dropped, but styling is: `*this*` shows as `this`, `` `code` `` as
`code` without monospace, links as dead text. This strand upgrades the
bubble to render the comment's inlines through the normal q2-preview
inline renderers.

Surfaces (all in
`ts-packages/preview-renderer/src/q2-preview/custom/CommentBlock.tsx`):

1. **Compact bubble** — first comment, single line,
   `nowrap + ellipsis` (`commentSpanText(comments[0])`, ~line 995).
2. **Expanded rows** — one row per comment in expand mode / clicked-open
   bubbles (`commentSpanText(c)`, ~line 908).

Out of scope: the experimental example components
(`comments.tsx.txt`, `comments_rc.jsx`) stay plain-text — they are
user-authored examples, and their emoji-reaction detection *needs* the
plain-text form. The `inlinesToPlainText` helper itself is untouched
(alt text, tooltips, and meta coercion still want plain text).

## How to render: the framework already has the entry point

The framework's `<Node>` (`framework/dispatch.tsx:402`) is the
sanctioned recursion entry — it dispatches to the format's `Inline`
wrapper (registry lookup + attribution wrap + the atomic-subtree gate).
CommentBlock is rendered inside the `<Ast>` tree, so `RegistryContext`
is available. The change is essentially:

```tsx
function commentSpanContent(span: InlineNode, onNavigateToDocument?: …) {
    return (span as SpanInline).c[1].map((inline, i) => (
        <Node key={i} node={inline}
              onNavigateToDocument={onNavigateToDocument}
              setLocalAst={NOOP} />
    ));
}
```

- **`setLocalAst` is a deliberate no-op**: bubble content is a
  *display* of the comment, not an edit surface. Comment mutation goes
  through `addComment`/`resolveCommentAtIndex` on the source node; a
  live `setLocalAst` here would write into the stripped clone and be
  lost (or worse). Read-only matches how `<Ast>` treats atomic
  subtrees.
- `onNavigateToDocument` is already in CommentBlock's `NodeArgs`; it
  needs threading into `CommentWrapper` (which currently receives only
  `comments`/`block`/`edit`/`mode`).
- Rendering the *content* inlines (not the comment `Span` itself)
  avoids the Span renderer re-emitting `quarto-edit-comment` markup
  inside the bubble.

What each inline then does, for free: `Emph`/`Strong`/`Strikeout` etc.
render their elements; `Code` gets monospace; `Quoted` emits the same
curly quotes as today's plain text (shared `QUOTE_CHARS`); `Math`
renders KaTeX; attribution wraps appear when the Authors overlay is on
(consistent with the row's author dot, which comes from the same span).

## The two real problems

### 1. Links inside the bubble bypass the preview's link interception

The preview intercepts link clicks with a **delegated bubble-phase
listener on `document.body`** (`installLinkHandlers`,
`utils/iframeLinkHandlers.ts:82`): external links →
`window.open(_blank)`, `#frag` → scroll, artifact/qmd hrefs →
`onQmdLinkClick`. But the comment chrome deliberately calls
`e.stopPropagation()` on click (CommentBlock ~line 847) so bubble
clicks don't reach the *other* document-level delegate — click-to-edit.
A bare `<a>` rendered inside the bubble therefore never reaches the
body listener, and its click falls through to **native navigation of
the preview iframe** — the worst outcome (external URL replaces the
preview; `#frag` does a raw jump).

Letting anchor clicks propagate is not an option (they'd hit
click-to-edit and open the enclosing block's editor). Proposal:

- Export the per-click routing from `iframeLinkHandlers.ts` as a
  reusable `routeLinkClick(ev, opts): boolean` (refactor the existing
  body listener to call it — no behavior change there).
- In the bubble's click handler, *before* the existing
  `stopPropagation`: if the click target has an `<a>` ancestor within
  the bubble, `preventDefault` + `stopPropagation` and call
  `routeLinkClick`. Cross-document routing uses the threaded
  `onNavigateToDocument`; same-document `#frag` falls back to
  `getElementById(frag)?.scrollIntoView({behavior:'smooth'})` (the
  bubble doesn't have `scrollToAnchor`; note as a known simplification).
- A link click must also *not* trigger the bubble's own
  expand/open-input `onClick` (guard: `closest('a')`).

### 2. Un-inline-ish content can break the bubble's geometry

The bubble is a ~140–160px-wide floating chip that participates in the
force layout (it re-measures on mount/expansion, not on async content
growth). Two offenders:

- **`Image`**: renders an unconstrained `<img>` (the renderer applies
  no max sizing) that loads async — the bubble would balloon *after*
  the relayout pass measured it.
- **`Note`**: block content in a tiny chip.

**Width containment is NOT automatic** (reviewed with Carlos —
requirement: image blast radius must end at the bubble border). The
existing `maxWidth: 140px`/`160px` constraints sit on the *inner text
containers*, not the chip: the bubble div is a shrink-to-fit
`position: absolute` box with no width bound of its own, `<img>` has
no default `max-width`, and `text-overflow: ellipsis` clips text but
not replaced elements — so an unconstrained image widens the whole
chip to its intrinsic width. And even a clamped image loads async,
changing the chip's height *after* the force layout measured it.

Proposal — three-part containment, so no comment content of any kind
can move layout outside the chip:

1. **Clamp the image**: scoped rule injected alongside the existing
   `.q2-comment-input` style (idempotent style-tag IIFE, ~line 98);
   bubble gets a `q2-comment-bubble` class and
   `.q2-comment-bubble img { max-width: 100%; max-height: 2.5em;
   object-fit: contain; }` (aspect preserved; `100%` resolves against
   the 140/160px inner containers).
2. **Clamp the chip**: the bubble div itself gets a `maxWidth` +
   `overflow: hidden`, so no descendant — image, KaTeX display box,
   anything a renderer emits — can widen the chip past its design
   width. Blast radius ends at the bubble border by construction.
3. **Re-solve the scaffolding after async growth**: a capture-phase
   `load` listener on the bubble (`img` load events don't bubble;
   capture is required) that calls `scheduleBubbleRelayout()`, so the
   force layout re-solves once the (bounded) growth lands instead of
   keeping stale overlap geometry until the next hover/mount.

### 3. "Weird" AST content gets an affirmative `[unsupported content]` chip

(Reviewed with Carlos — nested comments / editorial marks must not
render as if supported; show an explicit indicator instead.)

What the AST allows inside a comment span: nested editorial marks
parse fine — `[>> outer [>> inner] [!! hl] [++ ins] [-- del] ^[note]]`
produces a comment span whose content contains more marks and a
`Note`. **Wire-format fact that shapes the design**: in the JSON the
preview receives, *all four* editorial marks serialize as plain
`t: 'Span'` nodes distinguished only by class (`quarto-edit-comment`,
`quarto-insert`, `quarto-delete`, `quarto-highlight`) — verified with
`pampa -t json`. So a tag-keyed registry entry can't intercept them;
interception must be class-aware inside a `Span` override. Note also
that comment *extraction* only strips top-level spans from the block's
inline slot — nested marks genuinely reach the bubble renderer.

Mechanism — a **bubble-scoped registry override** (nested
`RegistryContext.Provider` is an established pattern; RevealDeck does
it; preserve the outer `sourceInfoPool` when re-providing):

```tsx
const outer = useContext(RegistryContext);
const bubbleRegistry = useMemo(() => ({
    ...outer.registry,
    Span: BubbleSpan,        // class-aware interceptor
    Note: UnsupportedChip,   // block content has no place in a chip
}), [outer.registry]);
```

- `BubbleSpan`: if the span's classes intersect
  `EDITORIAL_MARK_CLASSES` (the four above) → `<UnsupportedChip/>`;
  otherwise delegate to the outer registry's `Span` (ordinary
  `[text]{.cls}` spans render normally).
- `UnsupportedChip`: renders the literal `[unsupported content]` in a
  visibly different typeface — monospace, muted, slightly smaller —
  with a `title` tooltip naming the node kind (e.g. "nested comment").
  The unsupported node's *content is not rendered* — affirmative
  replacement, not partial rendering.
- Because the provider wraps the whole bubble subtree, arbitrarily
  *nested* unsupported content (`*emph with [!! mark]*`) is
  intercepted through normal registry recursion — no per-level
  checking in CommentBlock itself.
- Unknown inline tags (future AST additions with no renderer) already
  hit the Inline dispatcher's muted "(not yet implemented)"
  placeholder — a same-spirit affirmative indicator; left as-is.

This is the general "weird markdown" valve: anything we later deem
bubble-unsafe becomes one more entry in the bubble registry override.

`Note` is intercepted as unsupported (block content in a tiny chip);
its extraction/round-trip behavior is untouched.

The compact bubble keeps `nowrap + ellipsis`; inline elements truncate
fine. Multi-line content (`LineBreak`, KaTeX display math) is clipped
to one line there and fully visible in the expanded row — acceptable.

## Proposed changes (summary)

- `utils/iframeLinkHandlers.ts`: extract `routeLinkClick(ev, opts)`
  from the body listener; export it. No behavior change for existing
  callers.
- `CommentBlock.tsx`:
  - `commentSpanText(span)` (string) → `commentSpanContent(span, nav)`
    (ReactNode via `<Node>` + noop `setLocalAst`), used at both bubble
    sites; thread `onNavigateToDocument` into `CommentWrapper`.
  - Bubble click handler routes `<a>` clicks through `routeLinkClick`
    (with `onNavigateToDocument` for qmd links) and suppresses the
    expand/open-input action for them.
  - Containment: `q2-comment-bubble` class + scoped `img` clamp in
    the injected style tag, chip-level `maxWidth`/`overflow: hidden`
    on the bubble div, and a capture-phase `load` listener that
    schedules a bubble relayout.
  - Bubble-scoped registry override (`BubbleSpan` class-aware
    interceptor + `Note`) rendering `[unsupported content]` chips for
    nested editorial marks and other bubble-unsafe nodes.
- Tests (see below).

Nothing changes in `framework/plainText.ts`, the qmd writer, the
add/resolve commit paths, or the Rust side. The DOM-parity harness is
unaffected (comment chrome only exists on the preview side and parity
fixtures don't carry comments — verify no `dom-parity: true` fixture
contains `[>> ...]` before landing).

## Test plan (TDD — tests first, verify they fail)

Extend `CommentBlock.bubbleText.integration.test.tsx` (rename stays;
it is the bubble-content contract suite):

- [ ] `Emph`/`Code` comment renders `<em>` and `<code>` elements inside
      the bubble (assert elements, not just text) — fails today
      (plain text only).
- [ ] `Quoted` still shows `“world”` (now via the Quoted renderer) —
      guards against regressing bd-wcz4x7y0.
- [ ] Expanded mode (`PreviewContext.commentsMode = 'expand'`): each
      row renders rich content.
- [ ] Link in a comment: renders `<a href>`; clicking it does **not**
      collapse/expand the bubble and does not navigate natively
      (assert `defaultPrevented`; mock `window.open` for the external
      case).
- [ ] Image in a comment: `<img>` renders with the clamp class in
      scope (assert the injected style rule exists and the bubble
      carries `q2-comment-bubble`); the bubble div carries its own
      `maxWidth` + `overflow: hidden` (chip-level containment).
- [ ] Image `load` inside the bubble schedules a force-layout pass
      (spy on the relayout scheduling; dispatch a synthetic `load`
      event on the `<img>` under jsdom).
- [ ] Nested comment span inside a comment renders exactly one
      `[unsupported content]` chip and none of the inner text.
- [ ] Each editorial-mark class (`quarto-insert`, `quarto-delete`,
      `quarto-highlight`) and `Note` renders the chip; the chip is
      intercepted even when nested inside `Emph` (registry recursion,
      not top-level filtering).
- [ ] An ordinary `[text]{.mark}` span in a comment renders normally
      (interceptor is class-scoped, not all-Spans).
- [ ] New unit test for `routeLinkClick` (external / `#frag` /
      qmd-path cases) in `iframeLinkHandlers`' existing test home, plus
      an assertion that the body listener still works (existing tests).

Verification gates: preview-renderer `npm test` + `test:integration`;
hub-client `test:ci` + `npm run build:all`; e2e via `q2 preview` on a
fixture with `[>> see *this* and [a link](https://example.com) and
"quotes"]`, inspecting the bubble DOM (record here, per the e2e
policy).

## Work items

- [x] Phase 1: failing tests — 16 new cases across
      `CommentBlock.bubbleText.integration.test.tsx` (rich elements,
      expanded rows, link routing, image containment + load relayout,
      unsupported chips ×6) and
      `iframeLinkHandlers.integration.test.ts` (`routeLinkClick` ×5);
      all ran and failed for the intended reasons. Three deliberate
      pre-fix guards pass (Quoted text, non-link bubble click expands,
      ordinary classed span renders).
- [x] Phase 2: `routeLinkClick` extraction in `iframeLinkHandlers.ts`
      — behavior-identical (all 19 pre-existing delegated-listener
      tests pass unchanged) + 5 new unit tests. One test expectation
      fixed during TDD: `resolveRelativePath` returns VFS-absolute
      paths (`/dir/other.qmd`), matching the delegated listener.
- [x] Phase 3: implemented — `CommentSpanContent` component (bubble
      registry override: class-aware `BubbleSpan` + `Note` chip;
      `<Node>` recursion with noop `setLocalAst`), `CommentWrapper`
      gains `onNavigateToDocument`, chrome-level `handleBubbleLinkClick`
      (unroutable hrefs are swallowed — a bubble link never natively
      navigates the iframe), bubble `q2-comment-bubble` class +
      `maxWidth: 260px` + `overflow: hidden`, scoped img clamp rule,
      capture-phase `load` listener → `scheduleBubbleRelayout`.
      All 17 bubble tests + full preview-renderer suites green
      (578 unit / 628 integration); `tsc` clean.
- [x] Phase 4: full verification + e2e record (below). hub-client
      `test:ci` green (1005 + 114 + 133), `build:all` exit 0.
- [x] E2E record: rebuilt preview chain (`build:all` →
      `cargo xtask build-q2-preview-spa` → `cargo build --bin q2`);
      fixture `Stuff.[>> see *this* and [a link](https://example.com/)
      and "quotes" and [!! marked]]`; ran `cargo run --bin q2 --
      preview <fixture> --no-browser` and inspected the DOM in Chrome
      via the devtools MCP. Observed (output inspected): bubble
      textContent `see this and a link and “quotes” and [unsupported
      content]` with a real `<em>this</em>` and
      `<a href="https://example.com/">`; chip is monospace with
      tooltip "unsupported in comment bubbles: highlight mark";
      bubble has class `q2-comment-bubble`, `max-width: 260px`,
      `overflow: hidden`; clamp rule present in the injected style
      tag. Live click on the link inside the iframe:
      `defaultPrevented === true`, `window.open('https://example.com/',
      '_blank', 'noopener,noreferrer')` called, inline comment input
      NOT opened.
- [x] Phase 5: PR #615 merged to main as 24f98a507 (all 10 CI checks
      green); bd-y66gbfs4 closed. Changelog entry: 1976f2aa2;
      implementation: ca959a674. Live-site pickup still needs a
      hub-client deploy.

## Open questions for review

1. **Compact bubble: rich or plain?** Proposed rich in both surfaces
   (one code path, consistent look). The alternative — plain text in
   the compact one-liner, rich only when expanded — keeps the chip
   maximally simple but forks the code path. I recommend rich in both.
2. **Link targets**: proposed behavior is external → new tab (same as
   document links), `.qmd` → `onNavigateToDocument`, `#frag` →
   `scrollIntoView` fallback. OK to ship without wiring the host's
   `scrollToAnchor` (smooth-scroll + highlight) into CommentBlock?
3. ~~Editing round-trip~~ **Resolved (Carlos, 2026-08-26)**: plain-text
   authoring from the bubble input is fine for v1 — rich comments are
   authored by editing the `.qmd` source directly.
4. **Chip wording**: single generic `[unsupported content]` with a
   `title` tooltip naming the kind, vs. kind-specific text like
   `[unsupported: nested comment]`. Proposed: generic text + tooltip
   (keeps the chip narrow in a 140px bubble).
