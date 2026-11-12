# Issue #892: task lists render a bullet *and* a checkbox

- **GitHub**: <https://github.com/quarto-dev/q2/issues/892>
- **Reporter**: \@cscheid (Carlos Scheidegger), 2026-10-21
- **Triage date**: 2026-11-12
- **Worktree**: `.worktrees/issue-892` (branch `issue-892`, based on `main` @ `b83d7a7ad`)
- **Braid strand**: bd-2ydvebkc (main); related: bd-nnvvomg6 (loose-list `<p>` dropped)
- **Scope**: broadened by the reporter at triage time to "handle checklist
  rendering bugs" in HTML output (`q2 render` and `q2 preview`, which share
  the theme CSS). Interactive toggling is out of scope; it is already
  tracked by bd-tvtknbhx.

## Summary

The issue reports that task-list items in the preview look wrong ("wonky")
and show both a bullet and a checkbox. It also asks what GFM does. The
screenshot attached to the issue needs an authenticated browser session, so
it was not inspected. The text alone is enough to reproduce the problem.

The symptom reproduces in `q2 render` as well as in the preview. It is a
real bug and its cause is localized: Q2 never ported the CSS rule that hides
the list marker. A side-by-side render with Pandoc 3.13 found two more
divergences, listed under Reproduction below. One of them, the dropped `<p>`
in loose lists, is a general writer bug and is filed separately. Before
choosing a fix, we surveyed how 20 or so markdown renderers and SSGs emit
and style task lists ([survey.md](survey.md)). The survey shows that the
fix involves a real design choice between matching Pandoc and Quarto 1 and
matching the rest of the ecosystem.

## Reproduction

The fixture is `repro.qmd` (next to this file). It covers tight, mixed, ordered,
loose, nested, long-wrapping, and edge-case lists. `issue-reports/` is
excluded from the notes render, so the fixture was rendered from a scratch
copy:

``` bash
cp claude-notes/issue-reports/892/repro.qmd "$SCRATCH"/ && cd "$SCRATCH"
q2 render repro.qmd                       # q2 @ b83d7a7ad
pandoc -f markdown -t html repro.qmd      # pandoc 3.13, for comparison
grep -o 'ul\.task-list[^}]*}' repro_files/styles.css
```

### Markup: q2 vs. Pandoc 3.13

| case | q2 | pandoc 3.13 |
|---|---|---|
| tight, all tasks | `<ul class="task-list"><li><label><input type="checkbox" />one</label></li>` | identical |
| mixed list | `<ul>` (no class), task items get `<label><input …>` | identical |
| ordered | `<ol type="1"><li><label><input …` (no class) | identical |
| **loose** | `<li><label><input type="checkbox" />loose one</label></li>` | `<li><p><label><input type="checkbox" />loose one</label></p></li>` |
| nested | class on both levels | identical |
| **`- [ ]` with no text** | `<li></li>` | `<li><label><input type="checkbox" /></label></li>` |
| `[ ]no space` / mid-text `[ ]` | empty `<span></span>` with a Q-2-49 warning | literal `[ ]` |

Neither renderer emits `disabled`.

### CSS: the actual cause of the issue

The compiled `styles.css` contains only the following task-list rules:

``` css
ul.task-list{padding-left:1em}
input[type=checkbox]{margin-right:.5ch}   /* from _bootstrap-rules.scss */
```

There is no `list-style: none` and no negative margin to hang the checkbox
into the gutter, so every task item shows the bullet followed by the
checkbox.

## Localization

- **Missing rule.** Quarto 1 hides the marker in its patched Pandoc template
  partial, `external-sources/quarto-cli/src/resources/formats/html/pandoc/styles.html:202-207`:

  ``` css
  ul.task-list{list-style: none;}
  ul.task-list li input[type="checkbox"] {
    width: 0.8em;
    margin: 0 0.8em 0.2em -1em; /* quarto-cli#4556 */
    vertical-align: middle;
  }
  ```

  This partial is always emitted, because it sits outside
  `$if(document-css)$`. Q2's task-list work (bd-obkvhlam) ported only the
  two `_quarto-rules.scss` rules. They are at
  `resources/scss/bootstrap/_bootstrap-rules.scss:3565-3576`.
- **Why pampa looks fine on its own.** Pampa's standalone template
  `crates/pampa/resources/templates/html/styles.html:193` *does* contain
  Pandoc's `ul.task-list[class]{list-style:none}`. The Quarto HTML pipeline
  (`q2 render`, and through the shared theme CSS, `q2 preview`) does not use
  that partial.
- **Revealjs.** The revealjs layer has its own `.reveal ul.task-list` rules.
  The `grep` hit was `resources/scss/revealjs/quarto-revealjs.scss`, and it
  was not re-verified in this triage.
- **Markup emitters.**
  - Writer: `crates/pampa/src/writers/html.rs:1333-1383`
    (`task_item_checked`, `rewrite_task_item`) and `:1590-1598` (the
    all-or-nothing `class="task-list"`).
  - Preview: `ts-packages/preview-renderer/src/q2-preview/blocks/taskList.tsx`.
    It mirrors the writer, and `task-list.integration.test.tsx` asserts the
    parity.
- **Loose `<p>` dropped.** `crates/pampa/src/writers/html.rs:1916`
  (`write_blocks_inline`) collapses any list item whose only block is a
  `Para`. This is not specific to task lists (bd-nnvvomg6).
- **Empty `- [ ]`.** The reader in `crates/pampa/src/pandoc/treesitter.rs:297`
  prepends `Str "☐"` and `Space` only when there is content to prepend to.
  This was not dug into further.

## Ecosystem survey: findings that drive the fix

The full report, with verbatim markup and CSS per renderer and with
evidence levels, is [survey.md](survey.md). The parts relevant here:

- **Consensus: the checkbox replaces the marker.** Every project that ships
  CSS hides the bullet for task items, and nobody intends to show both. The
  checkbox then hangs into the marker gutter through a negative inline-start
  margin, so the item text lines up with non-task siblings. GitHub uses
  −1.4em, Pandoc −1.6em, Quarto 1 −1em, and VitePress −1.25rem.
- **Pandoc is the outlier on mixed lists.** Pandoc puts `class="task-list"`
  only on an all-task `<ul>`, and puts no class on `<li>`. Its CSS can
  therefore act only per list, so Pandoc and Quarto 1 show a bullet *and* a
  checkbox on task items in mixed lists, and a number *and* a checkbox in
  ordered lists. GitHub, VitePress, MkDocs Material, minima, and GitLab
  mark each task `<li>` (GitHub uses `task-list-item`) and hide the marker
  per item. Plain siblings keep their bullet in those renderers.
- **Markup.** Q2 already matches Pandoc 3.1.3+ and emits
  `<label><input type="checkbox">…</label>`, which is the most accessible
  variant in the survey. `disabled` is emitted by default everywhere except
  Pandoc (since 3.0.1). Loose items put the checkbox inside `<p>` almost
  everywhere.
- **The empty `- [ ]` case is split.** Pandoc, pulldown-cmark, and comrak
  treat it as a task. GitHub, cmark-gfm, remark, markdown-it, and the
  others do not.
- **The issue's question "is GFM's behavior specified?"** The GFM spec
  specifies only the parse (`[ ]`/`[x]`/`[X]` followed by whitespace, first
  paragraph only) and gives example output with `disabled=""`. It says
  nothing about presentation. Bullet hiding is github.com's CSS
  (`github-markdown-css`), not part of the spec.

## Open design questions, escalated to the user

These decide the fix scope of bd-2ydvebkc and cannot be settled by
investigation alone.

1. **Per-list vs. per-item marker hiding.**
   - *(a) Quarto 1 parity.* Port the `styles.html` rule as is. The
     all-task `ul.task-list` loses its bullets, and mixed and ordered lists
     keep bullet or number plus checkbox.
   - *(b) Ecosystem behavior.* Additionally emit a per-item hook. One
     option is `class="task-list-item"` on task `<li>`, an additive class
     that leaves `ul.task-list` Pandoc-compatible. The other is CSS
     `li:has(> label > input[type=checkbox])`, with no markup change. The
     marker is then hidden per item.

   Recommendation: (b) with the additive class. Two reasons:
   - The issue says outright that bullet plus checkbox is wrong.
   - The markup change is purely additive. Writer and preview change in
     lockstep, and the parity tests already pin the shape.
2. **Ordered lists.** Under (b), should task items in an `<ol>` hide their
   number? GitHub, VitePress, and Material hide it. GitLab keeps it on
   purpose. Recommendation: keep numbers. A number carries information
   that a bullet does not.
3. **Empty `- [ ]`.** Should it render an empty checkbox (Pandoc parity)
   or a plain item (GitHub parity)? Recommendation: Pandoc parity, since
   Q2 follows Pandoc's AST convention.
4. **`disabled`.** Keep Pandoc's enabled-but-inert checkboxes, or emit
   `disabled` as nearly everyone else does? The interactive preview
   (bd-tvtknbhx) would then have to un-disable them. Recommendation: emit
   `disabled` in static `q2 render` output. A clickable checkbox that does
   not persist is misleading.

### Decisions (user, 2026-11-12)

1. **Per-item marker hiding (b).** The writer and the preview emit an
   additive `class="task-list-item"` on each task `<li>`. `ul.task-list`
   keeps its Pandoc all-or-nothing rule. The CSS hides the marker and
   hangs the checkbox per item.
2. **Ordered lists keep their numbers by default, with a user escape
   hatch.** The hatch is a theme SCSS variable, declared `!default` in
   the bootstrap layer and settable from a user theme's
   `/*-- scss:defaults --*/` layer. The name is provisional:

   ``` scss
   /*-- scss:defaults --*/
   $task-list-ordered-markers: false;   // default true: keep numbers
   ```

   One variable gates **both** `list-style` and the checkbox's negative
   hang margin on `ol > li.task-list-item`. The two must flip together:
   - hiding the number without hanging the checkbox leaves an empty
     gutter;
   - hanging the checkbox while keeping the number makes them overlap.

   Plain user CSS also works, because the per-item class gives a stable
   selector. It needs both properties overridden, which is why the
   variable is the documented route. The revealjs layer must honor the
   same variable. Document it in `docs/guides/formats/html/themes.qmd`,
   or wherever theme variables are listed. Check that `q2 preview`
   compiles user themes the same way, so the escape hatch works there
   too.
3. **An empty `- [ ]` renders an empty checkbox** (Pandoc parity). This
   matters for the collaborative editor: a user typing `- [ ] ` expects
   the preview to show the box immediately. The reader currently drops
   the marker when the item has no content.
4. **Static `q2 render` output emits `disabled`.** Edit-capable surfaces
   (`q2 preview --allow-edit` and hub-client, bd-tvtknbhx) render them
   enabled.

## Implementation (bd-2ydvebkc, same branch)

- [x] **Grammar.** In `tree-sitter-qmd/.../grammar.js`, everything after a
  task marker is optional, so `- [ ] ` with nothing after it is a task item
  instead of an ERROR. Two corpus tests were added in
  `test/corpus/task_list.txt`. The pampa parse-error table
  (`_autogen-table.json`) was regenerated with
  `crates/pampa/scripts/build_error_table.ts`, because parser state numbers
  shifted. `- [ ]` with *no* trailing space is still an ordinary item, as
  in GFM.
- [x] **Reader.** An empty task item is `Plain [Str "☐"]` (Pandoc's
  shape), and the `Str` covers the bracket bytes.
- [x] **HTML writer.** It emits `li.task-list-item` and a `disabled`
  attribute, and accepts the lone-marker form. It also fixes a bug that
  predates this change: a hoisted item attr (`- [ ] todo {.x}`) was
  dropped on task items, because the attr is now split off *before* the
  task rewrite.
- [x] **qmd writer.** The empty item round-trips as `[ ] ` with the
  trailing space the grammar requires.
- [x] **q2-preview.** It renders the `task-list-item` class (through
  `liItemAttrProps`) and the lone marker. The incremental (reveal) `<ol>`
  path now renders task checkboxes, matching `<ul>`.
- [x] **SCSS.**
  - The marker is hidden per item, and the checkbox hangs by
    `width + gap = 1.2em`. `ul.task-list` padding was raised from 1em to
    1.2em so the checkbox sits flush with the surrounding text.
  - `$task-list-ordered-markers` (default `true`) is honored by both the
    Bootstrap and revealjs layers.
- [x] **Docs.** Task lists are now described in
  `guides/authoring/markdown-basics.qmd`, and the variable is in
  `guides/formats/html/themes.qmd#task-lists`.

End-to-end evidence (inspected):

- `q2 render repro.qmd` emits
  `<li class="task-list-item"><label><input type="checkbox" disabled="" />task</label></li>`
  next to `<li>plain item</li>` in the mixed list. The compiled CSS has
  `ul>li.task-list-item{list-style-type:none}` and
  `...>input[type=checkbox]{width:.8em;margin:0 .4em .2em -1.2em;vertical-align:middle}`.
- `hub-client/e2e/q2-preview-task-list.spec.ts` (Chromium, real q2-preview
  iframe) passes. Every task `<li>` computes `list-style-type: none`, its
  checkbox sits left of the `<li>` box, its text starts within 2px of the
  `<li>` content edge, and the plain siblings in the mixed list keep their
  bullets.

## Outcome / recommended next step

- Filed **bd-2ydvebkc**. Fix scope: port the marker-hiding and hang
  rules into `_bootstrap-rules.scss`, in the form chosen in Q1 above, plus
  the matching revealjs check and the writer and preview markup changes
  that follow from Q1–Q4. Tests first: a quarto-sass compile assertion
  for the new rules, writer and preview parity tests for any markup
  change, and an e2e check in `q2 preview` for the computed
  `list-style-type`.
- Filed **bd-nnvvomg6** (related). The HTML writer drops `<p>` for loose
  single-paragraph items. This is general Pandoc parity and has snapshot
  impact, so it is kept separate.
- `[ ]` not followed by a space, and a mid-text `[ ]`, are working as
  intended. qmd parses `[ ]` as span syntax and warns with Q-2-49.

## Verification commands used

``` bash
gh issue view 892 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
cargo xtask verify --skip-hub-build    # main @ b83d7a7ad: Rust green (19373 tests);
                                       # 2 hub-client test:wasm failures were a stale WASM:
cd hub-client && npm run build:wasm && npm run test:wasm   # 478/478 pass
q2 render repro.qmd ; pandoc -f markdown -t html repro.qmd
pampa -i loose.qmd -t json | jq '.blocks[0].c'   # AST has Para; writer drops <p>
grep -n -A8 task-list external-sources/quarto-cli/src/resources/formats/html/pandoc/styles.html
grep -rn task-list crates/pampa/resources/templates/ resources/scss/
```

## Cross-references

- bd-obkvhlam (closed): original task-list rendering and the SCSS port.
  Plan: `claude-notes/plans/2026-07-21-task-list-rendering.md`.
- bd-qif9l4cx (closed): preview checkbox on its own line (CommentBlock
  wrapper inside `<label>`). It was fixed before #892 was filed, so it is
  not the cause here.
- bd-tvtknbhx (open): interactive checkbox toggling. Question 4 above
  interacts with it.
- quarto-cli#4556: origin of Quarto 1's `-1em` checkbox margin.
