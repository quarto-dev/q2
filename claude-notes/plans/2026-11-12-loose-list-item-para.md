---
title: 'HTML writer drops `<p>` in loose list items (bd-nnvvomg6)'
date: 2026-11-12
description: 'The pampa HTML writer collapses a single-Para list item to bare inlines; key the `<p>` on Plain vs Para, as Pandoc does.'
status: done
braid:
  strand: bd-nnvvomg6
---

**Branch:** `braid/bd-nnvvomg6-loose-list-item-para` (topic branch in the main
checkout, based on `main` @ `0d7b3cfab`)

## Triage verdict

**Ready to design.** The bug reproduces at HEAD, the reader already produces
Pandoc's AST, and the fix is a few lines in one function; the open questions
are about scope and the snapshot churn, not about the mechanism.

## Issue context

bd-nnvvomg6 (bug, P2, label `parity`), filed 2026-11-12 during the GH #892
triage. `write_blocks_inline` in `crates/pampa/src/writers/html.rs` writes a
list item whose only block is a `Para` without the `<p>`. A loose list
(`- a` blank `- b`) therefore renders `<li>a</li>`, where Pandoc emits
`<li><p>a</p></li>`. The behavior dates from the first HTML writer
(`c866e8191`). It also affects loose task lists: Pandoc emits
`<li><p><label><input/>…</label></p></li>`, and q2 drops the `<p>`.

## Dependency graph

- **related**: bd-2ydvebkc (closed; GH #892 task-list rendering, PR #1191).
  That work noticed the bug while comparing loose task lists against Pandoc.
  It deliberately left the fix out of scope, because the bug affects every
  list, not only task lists. The `rewrite_task_item` doc comment already
  promises `<li><p><label>…</label></p></li>` for loose items, so after this
  fix the writer will match its own comment.
- No `blocks` or `discovered-from` edges. Nothing depends on this strand.

## What the code looks like today

- `write_blocks_inline` (`crates/pampa/src/writers/html.rs:1945`) has one
  caller, `write_list_item` (`:1413`). That function serves both
  `BulletList` and `OrderedList` (`:1612`, `:1631`). `write_blocks_inline`
  unwraps `blocks.len() == 1` for both `Plain` and `Paragraph`.
- **The AST is correct.** On the repro, pampa's JSON matches
  `pandoc -t json` item for item: loose items are `Para`, tight items are
  `Plain`, and the mixed list is `[[Para, Para], [Para]]`. So only the
  writer is wrong.
- **The preview is already correct.** q2-preview's `BulletList`/`OrderedList`
  render each item block through `Node`, and its `Para` component always
  emits `<p>`. `listBorrow.ts` already relies on loose items having an inner
  `<p>` ("Amendment A1"). The native writer is the outlier, so the fix
  brings render into line with preview.
- **Footnotes are unaffected.** `q2 render` wraps each note in a `Div`
  (`<li><div id="fn1"><p>…</p></div></li>`), so the `Div` path is used.

Repro at HEAD: `loose-list-item-para-investigation/repro.qmd`.
`pandoc.html` (Pandoc 3.13) and `pampa.html` (`pampa -t html`) are the two
outputs. The diff:

```
Loose:        pandoc <li><p>a</p></li>        pampa <li>a</li>
Mixed, item 2: pandoc <li><p>b</p></li>       pampa <li>b</li>
Loose ordered: pandoc <li><p>one</p></li>     pampa <li>one</li>
Loose task:   pandoc <li><p><label>…</label></p></li>
              pampa  <li class="task-list-item"><label>…</label></li>
Tight:        identical
```

A minor whitespace difference also shows up: for a multi-block item, pampa
writes `<p>second</p>\n</li>`, and Pandoc writes `<p>second</p></li>`.

## Decisions (user, 2026-11-12)

1. Whitespace: leave it unchanged. A multi-block item keeps `\n</li>`.
2. Snapshot updates go in a separate commit from the fix.
3. lmd block fragments: option (b). Change the fixture and
   `components.qmd:141` to `pandoc.Plain({=[ … ]=})`, and add a note to
   `lmd.qmd` that a block fragment per item makes a loose list. lmd builds
   the AST in code, so authors can choose tight or loose easily.
4. A hoisted trailing attr stays on the `<li>`.

## Checklist

### Phase 0: tests (must fail first)

- [x] `crates/pampa/tests/integration/test_html_list_item_para.rs`: loose
      bullet, loose ordered, mixed, tight (unchanged), and a hoisted attr on
      a loose item (stays on the `<li>`; the `<p>` has no attr)
- [x] `test_task_list.rs`: a loose task item renders
      `<li class="task-list-item"><p><label>…</label></p></li>`
- [x] End-to-end: a `q2 render` test that asserts `<li><p>` for a loose list
- [x] Run them and confirm that they fail

### Phase 1: lmd fixture and docs (before the fix, so that no snapshot moves)

- [x] The lmd-blog fixture and `components.qmd:141` use
      `pandoc.Plain({=[ … ]=})`
- [x] `lmd.qmd`: add a note that a `{=: :=}` per item gives a loose list
- [x] Check that the `blog.main.html` snapshot is unchanged (today Plain and
      a single-Para item render the same)

### Phase 2: the fix

- [x] `write_blocks_inline`: unwrap only `Plain`
- [x] Phase 0 tests pass; workspace suite green
- [x] Snapshot updates, if any, go in their own commit. None were needed:
      with the fixture on `Plain` first, no `.snap` file changes

### Phase 3: verification

- [x] `q2 render` of the repro, with the output inspected (loose list, loose
      task list with the checkbox hang)
- [x] `cargo xtask verify --skip-hub-build`, plus a clippy check of pampa

## Open design questions for the user

1. **Multi-block whitespace.** Should this fix also match Pandoc's
   `</p></li>` (no newline before `</li>`) for multi-block items, or should
   it stay strictly about the missing `<p>`? Changing the whitespace adds
   snapshot churn that has no visual effect.
2. **Snapshot churn tolerance.** The measured churn is recorded below. Do
   you want it in one commit with the fix, or the fix and the snapshot
   updates as two commits for easier review?
3. **lmd block fragments as list items.** `{=: … :=}` per item produces a
   `Para`, so after the fix such lists render loose. That matches Pandoc,
   but authors probably expect a tight list (see the churn measurement).
   Options:
   (a) Accept it. Change the fixture and `components.qmd:141` to
   `pandoc.Plain({=[ … ]=})`, as `lmd.qmd` already teaches.
   (b) Accept it, and also add a sentence to `lmd.qmd` warning that a
   block fragment per item makes a loose list.
   (c) Have lmd turn a single-`Para` block fragment into `Plain` when it is
   used as a list item. This departs from Pandoc, and the recommendation is
   against it.
   The recommendation is (b).
4. **Trailing hoisted attr on a loose single-Para item.** `write_list_item`
   hoists a trailing `{.class}` from the last block onto the `<li>`
   (bd-aeyss6p5). Once the `<p>` is emitted, should the attr stay on the
   `<li>` (the current behavior; the code strips it before the inner block
   is written), or should it move to the `<p>`? The recommendation is to
   keep it on the `<li>`, which needs no change.

## Risks / tradeoffs (draft)

- **Visual change for users.** Every loose list rendered by q2 will gain
  paragraph spacing. That matches Pandoc and Q1, but it is visible, so it
  should get a changelog or release-note line.
- **Snapshot churn** (measured below) could hide an unrelated regression.
  Review each change.
- **Source-location attrs.** Once the inner `<p>` is written via
  `write_blocks`, it gets its own `data-loc` when source locations are on.
  The `<li>` does not borrow it, which matches the preview's Amendment A1.

## Snapshot churn measurement

Throwaway patch (since reverted): drop the `Paragraph` arm of
`write_blocks_inline`, then run `cargo nextest run --workspace
--no-fail-fast`. Result: 19,932 tests, **1 failure**, 1 snapshot,
`crates/quarto/tests/integration/snapshots/lmd/blog.main.html.snap`.

```
-<li><a href="third.html">Third</a> (Mar 5, 2026)</li>
+<li><p><a href="third.html">Third</a> (Mar 5, 2026)</p>
+</li>
```

The churn is small, but this one failure is a real finding. The fixture
(`crates/quarto/tests/integration/fixtures/lmd-blog/posts/index.qmd`) builds
each item with a block fragment, `{=: … :=}`, which parses to a `Para`. The
AST is therefore loose, and only the writer bug makes it look tight. The
same pattern appears in the user docs at
`docs/guides/authoring/components.qmd:141`. In contrast,
`docs/guides/authoring/lmd.qmd` § A tight list already teaches
`pandoc.Plain({=[ … ]=})` for tight items, and `listings.qmd` uses
`pandoc.Plain`. After the fix, lists that users build with `{=: :=}` per
item will gain paragraph spacing, as they would under Pandoc.

The snapshot also shows the `\n</li>` whitespace from Q1.

Also noted: the unit tests contain no loose-list HTML assertion at all. Phase
0 fills that gap.

Pre-flight `cargo xtask verify --skip-hub-build` passed on `main` @
`0d7b3cfab` before the investigation.

## End-to-end verification (2026-11-12)

`cargo run --bin q2 -- render repro.qmd` (a copy of
`loose-list-item-para-investigation/repro.qmd`). The `<main>` list markup was
inspected:

```
<ul>
<li><p>a</p>
</li>
...
<ol type="1">
<li><p>one</p>
</li>
...
<ul class="task-list">
<li class="task-list-item"><p><label><input type="checkbox" disabled="" />loose one</label></p>
</li>
...
<ul>
<li>a</li>
<li>b</li>
```

The checkbox hang from PR #1191 already targets
`li.task-list-item > p:first-child > label > input`
(`resources/scss/bootstrap/_bootstrap-rules.scss:3600`), so loose task items
still hang the checkbox. I checked the selector against the new markup but
did not compare screenshots in a browser.

Workspace: `cargo nextest run --workspace` passed 19,939 tests (7 new), and
`cargo clippy -p pampa -p quarto --all-targets -D warnings` was clean. No
snapshot files changed.
