---
title: "Rich-editor prototype: partial selection across objects"
---

## Overview

How a selection treats an object (a figure, a callout) whose edge it
crosses. Explored in `q2-demos/rich-editor/mockups/selection.html`, which
puts figures and callouts in one document and switches between two models:

- **Partial.** A selection can take part of an object and what is outside
  it, as in ProseMirror editors (Confluence, the Quarto visual editor).
  Deleting keeps the object with what is left in it; copying takes the
  object with only that much in it.
- **Whole objects.** A selection stays in one field, or holds whole objects,
  as in Notion. Crossing an object's edge takes all of it.

**Decision (2026-11-19, for now):** the engine and editor support partial
selection, and each kind of object decides whether a selection that crosses
its edge takes all of it. In the mockup that rule is `wholeFromOutside`.

- **Figures: whole, always.** No real goal wants an image with part of its
  caption; the cases found were all overshoots, or goals better met another
  way. Inside a caption, text is selected as text.
- **Callouts: partial.** The case that decided it is copying out: a passage
  that runs into a callout should paste into another app as the text
  selected (plain text), or as a box holding only that much (rich text).

Supporting partial selection in the engine does not commit every object to
it; the per-object rule is the lever.

## Checklist

### Before building any object for real

- [ ] Inventory the details settled in each mockup the feature draws on
  (`figure.html`, `blocks.html`, `callouts.html`, `selection.html`): their
  code comments, the tips at the top of each page, and the descriptions of
  their options (archived ones included, with the reasons they were
  rejected). One checklist per object, in this plan or one of its own.
  These details took many rounds to get right; none should be lost in the
  move from mockup to editor.
- [ ] Write each inventoried detail as an e2e spec under `e2e/`, before its
  code (the prototype\'s `AGENTS.md`: the specs are the specification).
  Details that cannot be specified yet are `test.fixme`.

### Selection engine

- [ ] Positions: in a line of text, or on an edge between blocks; ordered
  across nested blocks (a callout\'s title, then its paragraphs).
- [ ] The editor owns any selection that leaves a line; within a line it is
  the browser\'s.
- [ ] What a selection covers is decided by where its ends are, never by how
  it got there, and by each object\'s rule (whole, or partial) when an end
  is outside it.
- [ ] Shift with the arrows, Shift-click, and drags (by words and lines
  after a double or triple click) as `callouts.html` settled them.
- [ ] Deleting, typing over, and moving a selection by dragging it.

### Clipboard

- [ ] Copy writes plain text: only the text selected, no fences or markup.
- [ ] Copy writes HTML a document can take: a partly covered callout as a
  box with only that much in it, a figure as a figure.
- [ ] The HTML carries the editor\'s own slice, which paste reads back
  exactly; anything else pastes as plain text.
- [ ] Decide whether pasting qmd text should keep its structure.

## Open questions

- In Partial, deleting a selection that takes part of a callout keeps the
  callout. Is that what people expect when the box is not shown selected?
- A callout title is one field: crossing from it into the callout\'s text
  is "as any lines" in Partial. Keep that, or take the whole callout as
  Whole objects does?
- Overshooting: a drag that slips past an object\'s edge. Partial makes it
  cheap (a little text); whole objects make it visible. Worth a spec either
  way.

## Details

The scenarios behind the decision, from the 2026-11-19 session:

- **Figures.** Moving or deleting text together with a figure wants the
  whole figure; splitting a caption wants text from inside it; copying an
  image with a shorter caption is better done by copying the figure and
  editing. What is left is overshooting.
- **Callouts.** Moving text into or out of a callout is served by selecting
  inside it, by dragging, by the ⌥⇧ arrows, and by Remove the callout. Edits
  across the edge (two sentences that repeat each other) are real but rare.
  Copying a passage out to another app is the case partial selection serves
  best, with plain text and HTML on the clipboard.

The Quarto visual editor (`quarto-dev/quarto`, `packages/editor`) models a
figure as a node whose content is its caption (`content: 'inline*'` in
`src/nodes/image/figure.ts`), with the image as an attribute. A partial
selection there copies the figure while only its caption text shows
selected; that mismatch is what made figures whole here.
