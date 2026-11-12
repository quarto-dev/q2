---
title: "Rich-editor prototype: edit plain-text headings"
---

## Overview

The prototype (`q2-demos/rich-editor`) edits the title block and the body\'s
paragraphs of plain text. Every other block is a read-only line (`ATOM`).
This plan adds the next simplest thing: **headings whose text is plain**
(`## Results`), editable in place, creatable and removable from the keyboard.

Why headings, and not something else:

- A heading is the only other block that is exactly "one line of inlines":
  no nesting, no multi-line text, no new inline types. Its text edits are the
  ones paragraphs already have.
- It is the smallest change that breaks the assumption "an editable line is a
  paragraph". After it, **a line has a kind**. List items, block quotes and
  code blocks all need that same idea (kind, later plus depth), so the work
  carries forward.
- It needs the first *structural* commands (turn this line into that kind and
  back) and the first input rule (`## ` + space), which every later block
  reuses.
- pampa is ready: the incremental writer already splices a `Header`\'s inlines
  in place and keeps its `{#id .class}` suffix verbatim, and the qmd writer
  omits auto-generated ids.

Considered and deferred:

- **Inline formatting** (emphasis, strong). This is the other axis (marks
  inside a line) and is a much bigger step: the model, rebase, the DOM offset
  mapping and write-back all stop being plain text. It should come after the
  line-kind axis is settled, and is where the text-only engine gets its real
  test.
- **A paragraph between two read-only blocks** (the README\'s "not done yet").
  Worth doing, but it adds no new kind of content.
- **Bullet list items.** The natural follow-up; needs kind *and* depth, and
  list tightness in the writer.

## The design decision: where a line\'s kind lives

The engine\'s model is text: a field is a string, the body is a line per
block, and rebase, undo, `settle` and collaboration are all text operations.

**Recommended: keep the kind in the text, as a line prefix.** A heading line
is one marker character followed by its text. The marker is one of six
Unicode noncharacters (`U+FDD1`–`U+FDD6`, level 1–6), which can never
legitimately be in a document. This mirrors how `ATOM` already works, and how
markdown itself works (block structure is line prefixes).

- Rebase, undo/redo, `settle`, history and cross-field edits need no change:
  a kind change is a one-character text edit, so it travels with the line
  through every operation, including a collaborator turning a paragraph into
  a heading under the caret.
- The DOM mapping barely changes: `Lines` renders the line\'s element with the
  text after the prefix and `data-start` pointing past it. The offset before
  the marker has no DOM position, as an atom line has none.
- Much of the behaviour falls out: Backspace at the start of a heading\'s text
  deletes the marker (the heading becomes a paragraph, as in every editor);
  Enter inside a heading leaves the second half without a marker (a
  paragraph).
- The engine stays generic. It knows only "a run of marker characters at the
  start of a line is a prefix that can\'t be typed, selected into or moved
  off the line start"; `editing/body.ts` decides what a prefix means.

The alternative is a structured body (`{ kind, text }[]`) through `Fields`,
`FieldSource`, rebase, `settle` and history. It is cleaner to read but
rewrites most of the engine for no behaviour gain at this step, and inline
formatting will force a model rethink anyway. Decide then, with a real
second use case, rather than now.

**Confirmed 2026-11-12; implemented as described.**

## Behaviour (the spec to write first)

Showing:

- A heading with plain text is an editable line, rendered as `h1`–`h6` by
  its level. A heading with formatting, like a paragraph with formatting,
  stays read-only.
- Explicit attributes (`{#first .unnumbered}`) show as `q2 render` shows
  them (id and classes on the element) and survive every text edit.

Editing:

- Typing changes only the heading\'s own source line; `{...}` stays verbatim.
- Markdown characters typed in a heading are text (written escaped).
- Enter at the end starts a paragraph below. Enter in the middle splits into
  two headings (as ProseMirror and Google Docs do). Enter at the start of the
  text puts an empty paragraph above and the heading stays a heading.
- Backspace at the start of the text turns the heading into a paragraph;
  the source line loses its `#`s and nothing else. Backspace again joins as
  paragraphs do.
- Delete at the end of a line joins the next line into it; the result keeps
  the first line\'s kind.
- An empty heading stays a heading (`##` reads back as an empty `Header`).

Making one:

- In a paragraph, one to six `#` at the start followed by a space turns the
  line into a heading of that level. Undo gives back the typed `## `.
- Seven or more `#`, or `#` not at the line start, is text (as the reader
  says).

Clipboard, collaboration, undo:

- Copying across a heading puts `## Text` on the clipboard, never a marker.
  Pasted text never contains a marker (pasting markdown as structure is out
  of scope).
- A collaborator changing `Text` to `## Text` in the source changes the line
  to a heading here without moving the caret off its text; and the reverse.
- Undo of a kind change restores source exactly.

Out of scope: setext headings (the reader doesn\'t have them), a heading
toolbar or shortcuts, changing level other than Backspace-and-retype,
section wrappers (see Parity below).

## Checklist

### Phase 0: tests

- [x] Fixture `e2e/fixtures/headings.qmd`: plain headings at two levels,
      one with `{#res .big}`, one with formatting, paragraphs between.
- [x] `e2e/headings.spec.ts` covering the Behaviour section. Every test
      checks the page *and* the source.
- [x] `body.qmd`\'s heading is now `## *Results*`, so the body specs keep a
      heading that can't be edited.
- [x] Unit tests: `engine/prefix.test.ts` (the generic prefix rules, the
      input rule, the clipboard, rebasing a kind change under the caret),
      `body.test.ts` (read, write, attributes), and
      `fieldSource.wasm.test.ts` (what the incremental writer makes of it).

### Phase 1: read and show

- [x] `engine/model.ts`: `isPrefix`, `linePrefix`, `pastPrefixes`.
- [x] `editing/body.ts`: `lineOf` reads a `Header` of plain inlines as
      prefix + text; text holding a prefix or `ATOM` is read-only.
- [x] `editBody`: a prefixed line writes a `Header`. Changed lines between
      two unchanged ones replace the old blocks there in order
      (`replacedBlocks`); a heading made from an editable old heading keeps
      its attributes and `attrS`, with the id dropped when the reader made
      it up.
- [x] `Lines`: `lineElement(prefix, index)` chooses the element; text and
      `data-start` exclude the prefix. `VisualEditor` renders `h1`–`h6` with
      the block's written classes.
- [x] `select` and `rebaseState` move a position before a prefix to after
      it.

### Phase 2: the prefix rules in `commands.ts`

- [x] A prefix is never typed or pasted; a line joined to the one before
      loses its prefix unless that line is left empty.
- [x] Enter: before the prefix at the start of the text, prefix on both
      halves in the middle, none at the end.
- [x] Backspace at the start of the text removes the prefix (this fell out
      of deleting the character before the caret). It shares an undo step
      with deletions just before it, like any other deletion; it was not
      worth a special case.
- [x] An empty heading round-trips (`##` reads as an empty `Header`).

### Phase 3: making a heading

- [x] `Schema.inputRule` and `applyInputRule`; `react.tsx` applies it after
      `insertText` as its own undo step, so undo restores `## `.

### Phase 5: complete the spec

- [x] e2e specs for what Phases 0-4 left unspecified: joins and selection
      deletes in both directions around a heading, attributes on Backspace
      and on a split, empty headings (left, in the source, Enter on one), a
      body that starts with a heading, `#` in the abstract, what a copy from
      a heading takes in, and level, class and text changes from
      collaborators, and Up/Down through headings.
- [x] They found a bug: a level changed in the source (`##` to `###`)
      showed an empty heading, because `Field` only rewrote its element\'s
      text when the text or offset changed, not when React swapped the
      element for one of another type. Fixed in `engine/react.tsx`.

### Phase 6: Backspace takes back the heading `#` made

- [x] Backspace as the very next action after `## ` made a heading undoes
      the conversion, leaving `## ` as text (`\\#\\# ` in the source), as
      undo already did. After anything else (a caret move, more typing) it
      behaves as usual. `react.tsx` remembers the editor state the input rule
      left and undoes on Backspace only while that is still the state.

### Phase 4: clipboard, parity, docs

- [x] `Schema.prefixText`: a selection that takes in a heading's start
      copies `## Text`.
- [x] `render-parity.spec.ts`: a `fixme` test for headings (sections and
      `anchored`).
- [x] README.
- [x] `npm run test:unit` (129), `test:wasm` (25), `typecheck`, `build`,
      and `test:e2e` in Chromium, Firefox and WebKit (436 passed, 26
      skipped: the existing `fixme`s and the Chromium-only parity specs).

## Details

### What the reader and writers do (checked 2026-11-12)

- `## Two words` parses as `Header [2, ["two-words", [], []], …]` with
  `attrS.id: null`: the auto id is in the AST but not in the source. An
  explicit `{#first .unnumbered}` has non-null `attrS.id` and `classes`.
- `##` and `## ` parse as an empty `Header`. `####### seven` and `#no space`
  are paragraphs. A closing `###` is literal text. No setext headings.
- A `Header` with new inlines and an empty id writes as `## B`.
- `incremental.rs` (`block_attrs_eq`, inline-content blocks) splices changed
  `Header` inlines and compares the id only when it was explicit.

### End-to-end check

`/fixture.html?fixture=headings` in Chromium, screenshot inspected: `Results`
and `Details` render as the theme's `h2` and `h3`, identical to the
read-only `*Formatted* heading` beside them. The e2e specs drive the real
app on a sync hub and assert on the page and the source text, e.g. typing
` so far` in `## Results {#res .big}` gives `## Results so far {#res .big}`,
and typing `#### ` at the start of `Middle.` gives `#### Middle.`.

### Parity

`q2 render` nests each heading and what follows it in
`<section id class="section levelN">`, and adds `class="anchored"` and
`data-anchor-id` from level 2. The editor renders flat lines. Nesting
`section` elements inside one `contenteditable` is possible (it is pure
rendering in `Lines`) but re-parents line elements whenever a level
changes, so it is left as a known gap with a `fixme` parity test rather
than taken on here.

### What this sets up

- **List items, block quotes:** more prefix kinds, plus depth as a longer
  prefix, plus Tab/Shift-Tab. The pairing logic in `editBody` and the input
  rule hook are reused.
- **Inline formatting:** independent of all of the above, and the point at
  which to revisit text-with-sentinels versus a structured model.
