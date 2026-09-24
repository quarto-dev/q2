# q2-preview scroll sync (bd-9kzfi)

## Overview

Scroll sync (editor ↔ preview) works for the HTML preview (`MorphIframe`)
but is a no-op for the q2-preview format (`ReactPreview` → `Q2PreviewIframe`).
Two gaps:

1. **No source-line attributes in the q2-preview DOM.** `MorphIframe` maps
   editor line ↔ preview position via `data-loc="fileId:startLine:startCol-endLine:endCol"`
   attributes the Rust HTML writer stamps on each element. The q2-preview React
   renderer stamps only `data-sid` (and only under attribution). `findElementForLine`
   has nothing to match.
2. **No scroll plumbing on the React path.** `ReactPreview` never calls
   `useScrollSync`; `Q2PreviewIframe` exposes no `scrollToLine`/`getScrollRatio`
   handle and attaches no `scroll`/`click` listeners; `ReactRenderer` threads none.

Per-node line numbers already ride the wire: `renderPageInProject` emits
`include_inline_locations: true`, so every node carries an `l` field
(`{f, b:{o,l,c}, e:{o,l,c}}`). The q2-preview iframe is `allow-same-origin`,
so the parent can reach `contentDocument` directly and reuse `MorphIframe`\'s
exact scroll logic. **No Rust change, no new postMessage protocol.**

Scope (confirmed with user): **block-level scroll sync only.** No inline
granularity, no selection mirroring.

## Design

- `data-loc` must land on each leaf's **own** semantic element (no wrapper):
  wrappers (even `display:contents` / `position:relative`) break theme CSS
  child/adjacency/`:nth-child` selectors and margin collapse — a parity risk
  this repo guards hard.
- New helper `dataLocProps(node)` (framework) → `{ 'data-loc'?: string }`,
  spread onto each q2-preview block leaf's root element.
- Extract `parseDataLoc` / `findElementForLine` / `isElementVisible` into a
  shared `iframe/scrollSyncDom.ts`; `MorphIframe` and `Q2PreviewIframe` both
  import them.
- `Q2PreviewIframe` gains a `ref` handle (`scrollToLine`, `getScrollRatio`)
  + `onScroll`/`onClick` props, using direct same-origin `contentDocument`
  access (mirrors `MorphIframe`).
- `ReactRenderer` forwards the handle ref + callbacks to `Q2PreviewIframe`
  (q2-preview branch only).
- `ReactPreview` wires `useScrollSync` exactly like `Preview.tsx`.

## Work Items

### Phase 1 — data-loc emission (TDD)
- [x] Test: `dataLocProps` returns correct string for node with `l`, `{}` without
- [x] Test: Para/Header/CodeBlock/Div with `l` render `data-loc`; node without `l` has none
- [x] Implement `framework/sourceLoc.ts` + export
- [x] Spread `dataLocProps(node)` into block leaves: Para, Header, CodeBlock
      (both paths), BulletList, OrderedList, BlockQuote, Div, HorizontalRule,
      RawBlock, Figure, LineBlock, DefinitionList, Table (skipped Plain — Fragment)

### Phase 2 — scroll plumbing (TDD where feasible)
- [x] Test: `findElementForLine` / `parseDataLoc` in shared module (jsdom)
- [x] Extract `iframe/scrollSyncDom.ts`; refactor `MorphIframe` to import
- [x] `Q2PreviewIframe`: ref handle + scroll/click listeners (same-origin)
- [x] `ReactRenderer`: thread ref + onScroll/onClick to Q2PreviewIframe
- [x] `ReactPreview`: wire `useScrollSync`

### Phase 3 — verify
- [x] `npm run build` (production tsc -b + vite) green; WASM untouched (TS-only)
- [x] vitest suites green (preview-renderer 186+192; hub-client 556 unit + 66 integ)
- [x] Added pampa regression test proving real parser output carries `l` on
      *block* nodes (`json_location_test::test_json_location_on_block_nodes`),
      and jsdom tests proving the real `<Ast>` render stamps `data-loc` from it.
      **NOT verified:** live browser scroll interaction (same-origin iframe DOM
      access + Monaco cursor events) — needs a running hub + browser session.
- [ ] changelog.md entry (two-commit workflow) — pending commit
- [ ] beads bd-9kzfi close + sync — pending commit
