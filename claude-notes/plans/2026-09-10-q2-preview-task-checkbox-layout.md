# q2-preview: task-list checkbox renders on its own line above the item text

**Strand:** bd-qif9l4cx
**Related:** bd-q2wqj24c (CommentBlock wrapper `<div>` breaks parent > child
parity in general), bd-tvtknbhx (interactive checkboxes; its open polish item
(2) — loose/Para-leading task items — is fixed by this plan as a by-product).
**Status:** proposed — awaiting review before execution.

## Overview

Reported 2026-09-10 on quarto-hub.com: in the `q2-preview` format a task item
such as

```markdown
* \[Elliot\]
  * looking into QH file sync bug a bit
  * [x] working with Julia on getting her work on replacing vdocs in Positron merged
```

renders the checkbox on an empty line by itself, with the item text (and the
list bullet) on the line below.

### Reproduction (done, 2026-09-10)

Fixture: the snippet above as `tasklist.qmd`, served with
`target/debug/q2 preview tasklist.qmd --no-browser`, inspected with a headless
Playwright script (`inspect.mjs`, scratchpad) that finds the checkbox in the
preview frame and dumps its `<li>`'s outerHTML plus bounding rects. Live DOM of
the item:

```html
<li>
  <label>
    <input disabled="" type="checkbox" checked="">
    <div style="position: relative; box-shadow: none; transition: box-shadow 0.15s;">
      working with Julia on getting her work on replacing vdocs in Positron merged
    </div>
  </label>
</li>
```

The checkbox's rect is at y=180.8 (13px tall) and the label's at y=175.8 with
height 51px: two line boxes. A second fixture with only task items
(`- [x] done item` / `- [ ] todo item`) shows the same DOM under
`<ul class="task-list">`, so this is not specific to nested or mixed lists —
**every task item in q2-preview is broken**, hub-client and `q2 preview` alike
(both mount `PreviewRoot` from `@quarto/preview-renderer`).

The native writer (`pampa -t html`) is fine:
`<li><label><input type="checkbox" checked="" />working with …</label></li>`.
There is no CSS involved; the only rules matching are bootstrap's
`label { display: inline-block }` and `input[type="checkbox"] { margin-right: 0.5ch }`.

### Root cause

`ts-packages/preview-renderer/src/q2-preview/blocks/taskList.tsx`
(`TaskItemBody`) builds the task `<li>` at the *list* level:

```tsx
<label>
  <input type="checkbox" …/>
  <Node node={strippedTaskHead(item[0])} …/>   // the item's Plain, marker removed
</label>
```

`<Node>` for a block routes through the registry's `Block` entry, which since
Comments v1 (#441, 2026-07-30 — nine days *after* the task-list work in #407,
2026-07-21) is `CommentBlock`. For every commentable block whose source
resolves, `CommentBlock` renders a positioned `<div>` wrapper around the block
so the comment bubble can anchor to it (`custom/CommentBlock.tsx` ~line 980).
That `<div>` lands *inside* the inline `<label>`, after the `<input>`: a
block-level box inside an inline container forces the text onto its own line.
The `<li>` marker attaches to the first line box, which is now the text line
(a block-level replaced element like a bare `<input>` creates no line box of
its own), which is why the bullet sits beside the text, not the checkbox.

`CommentBlock` is not the only block wrapper that can appear at that spot:

- `dispatchers.tsx` `Block` wraps its output in `AttributionWrap … as="div"`
  whenever attribution is resolved (Authors overlay on);
- `renderMeasuredEdit` replaces the block with a `<div id="q2-active-edit-region">`
  + textarea while it is being edited, which today would mount the editor
  *inside* the `<label>`.

So the existing design — "wrap `<Node node={Plain}>` in a `<label>` from the
outside" — is fragile against every block-level decoration the dispatcher
stack adds, present or future. The `task-list.integration.test.tsx` suite did
not catch it because its assertions are structural on `li > label > input`
only; the wrapper `<div>` sits *after* the input and violates nothing the tests
check.

(bd-q2wqj24c tracks removing the CommentBlock wrapper element altogether for
parent > child CSS parity. Even if that lands, the attribution and edit
wrappers remain, so this fix must not depend on it.)

## Fix design

Move the `<label>` **inside** the item's head block renderer instead of
outside it, so every block-level wrapper (comment chrome, attribution, edit
surface) stays *around* the label and the checkbox + text always share one
inline formatting context. This mirrors the native writer, which rewrites the
item's head `Plain`/`Paragraph` inlines to
`RawInline "<label><input …/>" … RawInline "</label>"`
(`crates/pampa/src/writers/html.rs` `rewrite_task_item`): tight items render
`<li><label>…</label></li>`, loose items `<li><p><label>…</label></p></li>`.

Mechanism: a small React context carried from the `<li>` to its head block.

```
BulletList / OrderedList
  └─ <li data-block-pool-id=…>              (borrow logic unchanged)
       └─ <TaskItemContext.Provider value={{checked, onToggle}}>   only around item[0], only when taskItemChecked(item) !== null
            └─ <Node node={item[0]}>        (UNstripped — the head block strips the marker itself)
                 └─ CommentBlock → AttributionWrap → Plain | Para
                                                      └─ <label><input …/>{inlines.slice(2)}</label>
       └─ item.slice(1) blocks              (outside the provider, as today)
```

- `taskList.tsx` keeps `taskItemChecked`, `allTaskItems`, `makeTaskToggle`
  (unchanged) and gains `TaskItemContext` + a `TaskLabel` component holding the
  current `<label>`/`<input>` markup and the pointer-event guards (checkbox
  clicks must not open the block editor; label-text clicks must not toggle).
  `TaskItemBody` and `strippedTaskHead` go away.
- `Plain.tsx`: read `TaskItemContext`; if set **and** this node's inlines start
  with `Str "☐"|"☒"` + `Space`, render `<TaskLabel …>{inlines.slice(2)}</TaskLabel>`;
  otherwise the current fragment. The label re-provides `TaskItemContext`
  as `null` so nothing nested (a footnote's blocks, a custom inline) can
  inherit it.
- `Para.tsx`: same check inside the `<p>` (`<p …><TaskLabel …>…</TaskLabel></p>`),
  which fixes the loose-item gap left open in bd-tvtknbhx ("label can't wrap
  `<p>`" — it never needed to; the `<p>` wraps the label, as in the writer).
- `BulletList.tsx` / `OrderedList.tsx` (both the incremental and the plain
  branches): replace the `checked !== null ? <TaskItemBody/> : …` fork with
  the provider around `item[0]`; the `task-list` class logic and the pool-id
  borrow are untouched. The incremental (reveal) branch provides
  `{checked, onToggle: undefined}` — inert checkbox, as today.
- The edit path needs no change: when the head `Plain` becomes the edit target,
  the `Block` dispatcher swaps it for the measured-edit wrapper *before* Plain
  runs, so the label is simply absent while editing (today it would host the
  textarea). The rich-text editor still shows the raw ballot glyph — that is
  bd-tvtknbhx polish item (3), out of scope here.

Why not the alternatives:

- **Bypass the Block dispatcher for the head block** (render its inlines
  directly inside the label): loses comment extraction — `[>> …]` comment
  spans would leak into the label text — and loses the comment bubble,
  attribution colouring, and edit activation for task items.
- **CSS (`label > div { display: inline }`) or a "no wrapper inside labels"
  flag on CommentBlock**: patches one wrapper; the attribution and edit
  wrappers would still break it, and it fights the CSS-parity direction of
  bd-q2wqj24c.

## Work items

### Phase 1 — tests first (must fail on `main` before Phase 2)

All in `ts-packages/preview-renderer/src/q2-preview/` (run with
`npx vitest run task-list` from that package; fixtures generated verbatim with
`pampa <(printf …) -t json` like the existing ones).

- [ ] `task-list.integration.test.tsx`: add a structural assertion to the
      existing tight-list test — the `<label>` wrapping each checkbox contains
      **no block-level element** (`label.querySelector('div, p') === null`)
      and the input's next sibling is the text. This is the regression test
      for the reported bug; it fails today with the CommentBlock `<div>`.
- [ ] Same file: new test with the reporter's nested/mixed fixture
      (`* \[Elliot\]` / nested bullets / one `[x]` item). Asserts: the inner
      `<ul>` has no `task-list` class (writer parity, all-items rule), and the
      one checkbox's label passes the same no-block-descendant check.
- [ ] Same file: new test that any block-level wrapper is an **ancestor** of
      the label, i.e. `input.closest('li') > * … > label` — the comment
      wrapper `<div>` (present in `show` mode) now sits between `<li>` and
      `<label>`; documents the intended DOM shape.
- [ ] Same file: loose task items (`- [ ] a\n\n- [x] b\n`) render
      `li > p > label > input` and no ballot glyph in `textContent`
      (bd-tvtknbhx item 2; fails today — glyph renders as text).
- [ ] Same file: toggle tests keep passing unchanged (they fire `click` on the
      input and inspect the subtree commit) — they are the guard that the
      context plumbing delivers `onToggle` correctly. Add one for a loose
      item toggle.
- [ ] `OrderedList` coverage: one test with `1. [ ] a` / `2. [x] b` asserting
      `ol` has no `task-list` class and labels are block-free.

### Phase 2 — implementation

- [ ] `taskList.tsx`: add `TaskItemContext` and `TaskLabel`; remove
      `TaskItemBody` / `strippedTaskHead`; update the module comment (the
      "tight items only … until Para grows a slot" paragraph is obsolete).
- [ ] `Plain.tsx`, `Para.tsx`: consume the context; render `TaskLabel` when the
      head-marker check passes.
- [ ] `BulletList.tsx`, `OrderedList.tsx`: provide the context around
      `item[0]` in both branches; drop the `TaskItemBody` fork.
- [ ] `npx vitest run` in `ts-packages/preview-renderer` green; then
      `cargo xtask verify` (hub-client build + `test:ci` cover the consumer;
      no Rust change expected, but the TS package is bundled by hub-client and
      q2-preview-spa).

### Phase 3 — end-to-end verification (required before declaring done)

- [ ] Rebuild the embedded SPA so `q2 preview` picks up the TS change:
      `cargo xtask build-q2-preview-spa && cargo build --bin q2`.
- [ ] `target/debug/q2 preview tasklist.qmd --no-browser` + the Playwright
      inspect script: the input and the text now share one line
      (label height ≈ one line box; input rect and label rect on the same y),
      `li` outerHTML shows `<li><div …><label><input …>text</label></div></li>`.
      Record the invocation and the observed DOM in this plan.
- [ ] Same for the all-task fixture and a loose-item fixture
      (`li > p > label`).
- [ ] hub-client: `cd hub-client && npm run dev`, open a project with the
      reporter's snippet as a `format: q2-preview` document, confirm visually
      (screenshot into the plan) and confirm toggling still writes `[x]`/`[ ]`
      back (the `--allow-edit`/edit-toggle surface) — this is the surface the
      bug was reported on.
- [ ] Optional parity check: `/preview-parity` harness fixture for a task list
      if the harness mounts with `PreviewContext` (bd-q2wqj24c notes it does
      not — if so, skip and say so).

### Phase 4 — wrap-up

- [ ] `braid comment` the strand with the e2e evidence; close it.
- [ ] Comment on bd-tvtknbhx that polish item (2) (loose task items) is done
      here; items (3) and (4) remain.
- [ ] hub-client changelog entry is **not** needed unless `hub-client/` files
      change (the fix lives in `ts-packages/preview-renderer`); re-check the
      diff before committing.

## Decisions (reviewed 2026-09-10)

1. **Scope: tight and loose items both.** The `Para` slot is in scope; loose
   task items must render `li > p > label > input` like the writer.
2. **Reveal/incremental branch stays inert.** Provide `onToggle: undefined`
   in the incremental branch; revealjs task-list behaviour is left for later.
3. **No guard for a leading comment span** (`[>> note] [x] item`). The reader
   does not recognise a task marker there, so nothing changes; the simpler
   implementation is the accepted trade-off and that configuration is
   unsupported.
