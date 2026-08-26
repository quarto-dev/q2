# Comment bubbles drop Quoted (and all non-Str) inlines

**Strand:** bd-wcz4x7y0 (follow-up for rich bubble rendering: bd-y66gbfs4)
**Date:** 2026-08-26
**Status:** implemented on `braid/bd-wcz4x7y0-comment-bubbles-drop-quoted`; e2e verified

## Symptom

On quarto-hub.com (live) and in `q2 preview`, an editorial comment
`[>> Hello "world"]` renders its bubble as just `Hello` — the quoted
word is silently gone. Minimal reproduction:
`~/Desktop/daily-log/2026/08/26/hello.qmd`:

```qmd
---
title: hello
---

Stuff.[>> Hello "world"]
```

## Diagnosis

The parse is **correct**. `pampa -t native` on the fixture gives:

```
[ Para [Str "Stuff.", Span ( "" , ["quarto-edit-comment"] , [] )
    [Str "Hello", Space, Quoted DoubleQuote [Str "world"]]] ]
```

and pampa's own HTML writer emits
`<span class="quarto-edit-comment">Hello “world”</span>` — fine.
Smart typography turns `"world"` into `Quoted DoubleQuote [Str "world"]`;
that's expected.

The bug is in the React preview renderer (confirmed by Carlos).
`CommentBlock.tsx` extracts `quarto-edit-comment` spans from the block
and renders their text in the bubble via a local stringifier,
`commentSpanText` (`ts-packages/preview-renderer/src/q2-preview/custom/CommentBlock.tsx:133-141`):

```ts
function commentSpanText(span: InlineNode): string {
    return (span as SpanInline).c[1]
        .map((o: InlineNode) => {
            if (o.t === 'Str') return (o as StrInline).c;
            if (o.t === 'Space') return ' ';
            return '';          // <-- Quoted, Emph, Code, Link, ... vanish
        })
        .join('');
}
```

Any inline that is not `Str`/`Space` contributes the **empty string,
without recursing into its content**. So this is not a quotes-only bug:
`[>> use *this* one]` drops "this", `[>> see `code`]` drops the code,
links drop their text, etc. Quotes are just the most common trigger
because smart typography rewrites every `"..."` into a `Quoted` node.

### Why the rest of the document is unaffected

Comment spans stripped from the block are the only inlines routed
through `commentSpanText`. Normal document content goes through the
per-node dispatchers, whose `Quoted` renderer
(`q2-preview/inlines/Quoted.tsx`) correctly emits `“…”`/`‘…’` around
recursively rendered children.

### The near-miss: a shared, almost-correct helper already exists

`framework/plainText.ts` exports `inlinesToPlainText`, the
"Pandoc-Stringify equivalent" used for image alt text, Note tooltips,
title-block and meta-string coercion. It **recurses correctly** into
`Quoted` (and Emph/Code/Link/...), but it currently drops the quote
*marks*: `Quoted DoubleQuote [Str "q"]` → `q`, not `“q”` (the unit test
at `plainText.test.ts:65` pins this: expects `'hiq'`).

Pandoc's real `stringify` (Text.Pandoc.Shared) de-sugars `Quoted` via
`deQuote` into `“ … ”` / `‘ … ’` (U+201C/D, U+2018/9) — the quote marks
are part of the plain text. The preview's own `Quoted.tsx` renderer uses
the same characters. So `plainText.ts` deviates from both its stated
contract and the visual renderer.

## Affected surfaces

1. **`CommentBlock.tsx` `commentSpanText`** — the shipped bug: both the
   compact bubble preview (line ~999) and the expanded rows (line ~912).
2. **`framework/plainText.ts` `Quoted` case** — quote marks missing
   from alt text / tooltips / meta strings (secondary, low-visibility,
   but same "quotes disappear" class; fixing it makes helper and
   renderer agree).
3. **Experimental render-component examples** (same shallow pattern,
   copy-pasted): `hub-client/src/components/render/experimental-components/comments.tsx.txt`
   (lines ~40, ~298) and `.../new/comments_rc.jsx` (lines ~40, ~309).
   These are user-facing example components, not compiled into the app.

Not affected: pampa HTML writer, `q2 render` output, the AST, the qmd
round-trip (add/resolve commits go through the qmd writer, which
serializes `Quoted` correctly).

## Proposed fix

Replace the local `commentSpanText` with the shared walk, and teach the
shared walk to emit quote marks (Pandoc-stringify parity):

**A. `framework/plainText.ts`** — in `inlineText`, split `Quoted` out
of the `Span` case and wrap the recursed content in the kind-appropriate
curly quotes:

```ts
case 'Quoted': {
    const [kind, inlines] = n.c as [{ t: string }, InlineNode[]];
    const [open, close] =
        kind?.t === 'SingleQuote' ? ['‘', '’'] : ['“', '”'];
    return open + inlinesToPlainText(inlines ?? []) + close;
}
```

(Reuse/mirror the `QUOTE_CHARS` table from `inlines/Quoted.tsx`; decide
during implementation whether to export it from one place.)

**B. `CommentBlock.tsx`** — delete `commentSpanText`; both call sites
become `inlinesToPlainText((span as SpanInline).c[1])`.

**C. Experimental examples** — apply the same substitution in
`comments.tsx.txt` / `comments_rc.jsx`. The render-components runtime
already exposes `inlinesToPlainText` (`q2-preview/entry.tsx:89,140`),
so the examples can call it instead of carrying a private stringifier.

### Why extend the shared helper rather than patch locally

- The helper's documented contract is "Pandoc Stringify-equivalent";
  Pandoc stringify includes quote marks. This is a fidelity fix, not a
  behavior fork.
- A local fix in CommentBlock would leave alt text / tooltips / meta
  strings silently dropping quote marks — the same user-visible class.
- One recursive walk, one truth; the copy-pasted local stringifier is
  exactly how this bug shipped.

### Blast radius of (A)

Consumers of `inlinesToPlainText`/`blocksToPlainText`: `framework/meta.ts`
(meta-string coercion), `Image.tsx` (alt text), `Note.tsx`,
`PreviewTitleBlock.tsx`, plus the render-components runtime export.
The only behavior change is quote characters now appearing where quoted
text already appeared — strictly closer to both Pandoc and the visual
renderer. Risk: snapshot/unit-test churn (at minimum
`plainText.test.ts:65`'s `'hiq'` expectation, deliberately updated to
`'hi“q”'`). Will run the full hub-client + preview-renderer suites and
report any other snapshot deltas explicitly.

## Test plan (TDD — tests first, verify they fail)

- [x] Update `framework/plainText.test.ts` Quoted expectation to
      include curly quotes (Double and Single kinds); ran, failed as
      expected (`'hiq'` / `'tis'` without quote marks).
- [x] Add `CommentBlock.bubbleText.integration.test.tsx` rendering a
      block whose comment span contains `Str + Space + Quoted` (plus
      SingleQuote and `Emph`/`Code` cases); ran, all 3 failed as
      expected (`'Hello '`, `''`, `'use  '`).
- [x] Implement (A) and (B); both tests pass.
- [x] Apply (C) to the two example files, via the debug-renderer
      global (which now exposes the plain-text helpers; the
      framework-primitive parity test locks them on both globals).
- [x] preview-renderer `npm test` (578 passed) + `npm run
      test:integration` (609 passed); hub-client `npm run test:ci`
      (exit 0: unit + integration + wasm suites).
- [x] `cd hub-client && npm run build:all` — exit 0.
- [x] End-to-end (see record below).

### End-to-end verification record

- Rebuilt the preview chain: `npm run build:all` (includes WASM) →
  `cargo xtask build-q2-preview-spa` → `cargo build --bin q2`.
- Invocation: `cargo run --bin q2 -- preview
  ~/Desktop/daily-log/2026/08/26/hello.qmd --no-browser` →
  `http://127.0.0.1:61594/?page=hello.qmd`, inspected in Chrome via
  the devtools MCP.
- Observed DOM inside the preview iframe (output was inspected):
  the bubble `div[title="1 comment"]` has
  `textContent === 'Hello “world”'` (curly quotes present), and the
  stripped paragraph reads `Stuff.`. Before the fix the bubble read
  `Hello ` with the quoted word missing.

## Work items

- [x] Phase 1: failing tests (plainText Quoted marks; CommentBlock bubble text)
- [x] Phase 2: `plainText.ts` Quoted fix (+ shared `QUOTE_CHARS`,
      now also imported by `inlines/Quoted.tsx`)
- [x] Phase 3: `CommentBlock.tsx` uses `inlinesToPlainText`
- [x] Phase 4: experimental example components updated
      (`comments.tsx.txt`, `new/comments_rc.jsx`); debug-renderer
      global gained `inlinesToPlainText`/`blocksToPlainText`;
      parity test extended to lock them
- [x] Phase 5: full verification (vitest, build:all, e2e preview inspection)
- [ ] Phase 6: close bd-wcz4x7y0; note whether a hub deploy is needed for the live site

## Open questions for review

1. **Quote marks in alt text / meta strings**: agree that (A) extending
   the shared helper is right, vs. keeping the helper as-is and doing a
   comment-local wrapper? (I recommend (A) for Pandoc parity.)
2. **Emphasis etc. in bubbles stays plain text** — the bubble is a
   plain-text surface; `*this*` will show as `this` (content kept,
   styling dropped). Rendering rich inlines inside the bubble is out of
   scope here; file a follow-up strand if wanted.
3. The `.txt`/`.jsx` example files: fix in the same PR (proposed) or
   file a separate low-priority strand?
