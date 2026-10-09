---
title: 'Default and grid listings nest every item one level deeper (bd-mlmkev01)'
date: 2026-10-09
description: 'Fixes default and grid listings that nested each item one level deeper than the last, so listings past about 90 items hit the parser depth limit and vanished; the item templates now end with a blank line.'
status: done  # Implemented on the topic branch (design settled with the user 2026-10-09; see Decisions).
braid:
  strand: bd-mlmkev01
  priority: P1
---

**Branch:** `braid/bd-mlmkev01-listing-nesting-limit` (topic branch in the main checkout, based on `main` @ `15bb0d54f`)

## Triage verdict

**Ready to design.** The root cause is confirmed and the fix is a one-line template change. A table-listing fix already handled the same problem the same way. The remaining questions are about scope: whether to make the fix robust against custom templates, and whether a failed re-parse should keep dropping the listing silently.

## Issue context

P1 bug, filed 2026-10-09 by Carlos Scheidegger. A `type: default` listing with 90 or more items, or a `type: grid` listing with 89 or more, fails to re-parse:

```
Warning [Q-12-10]: Re-parsing rendered listing listing-1 failed with 1 diagnostic(s);
first: The input document is too deeply nested (more than 99 levels). ... Listing skipped.
```

The page then ships with no listing. `type: table` works at 1051 items. The filer suspected that each generated item opens inside the previous one. They also flagged that Q-12-10 is only a warning, so a large listing disappears from the page with nothing more than that warning.

## Dependency graph

- **discovered-from**: bd-fvcip3t5 (claude-notes plans listing page). That work needs one listing of about 1050 plans, so this bug blocks it in practice.
- **related**: bd-61cd (listings feature epic). This bug is in the L3 built-in template layer.
- No `blocks` edges and no children.

## What the code looks like today

The bug reproduces at HEAD and with the installed `q2 0.33.0-nightly.20261009`.

### Root cause

`crates/quarto-core/src/project/listing/templates/listing-default.template` (and `listing-grid.template`) apply the item partial to the whole array with no separator:

```
::: {.list .quarto-listing-default}

$items:item-default()$

:::
```

The doctemplate engine follows Pandoc semantics and **removes the final newline of a partial** (`remove_final_newline` in `crates/quarto-doctemplate/src/resolver.rs:168`, applied at `parser.rs:357`). `item-default.template` ends with `:::\n`, so after that newline is removed the rendered items are joined like this:

```
...
::::::: {.quarto-post .image-right}
```

The previous item's closing `:::` and the next item's opening `::: {...}` end up on one line. Together they form a **6-colon opening fence with attributes**, so the previous item is never closed. Each item therefore nests one level deeper than the one before it. Grid hits the 99-level limit one item earlier because `item-grid.template` adds an extra wrapper (`.g-col-1`).

### Evidence

Files are in `claude-notes/plans/listing-nesting-limit-investigation/repro/`, a website project with its own `_quarto.yml`. It uses `.qmd` files, so the claude-notes site does not render it.

- `index.qmd` has a default listing and a grid listing over 3 docs. Rendered with nightly q2, the `<div>` depth of each item is **3, 4, 5** for `.quarto-post` and **3, 4, 5** for `.g-col-1`. It should be constant.
- `glued.qmd` and `separated.qmd` isolate the parser behavior. `:::\n:::::: {.item}` gives depths 2, 3, 4. Separating the items with blank lines gives 2, 2, 2. pampa is behaving correctly; the template is producing bad markdown.

### Precedent

`listing-table.template` already hit this problem: the stripped newline merged all table rows into one. It was fixed in `d31c4b62c` (bd-listing-table-fields-peg1w3b3) by iterating explicitly:

```
$for(items)$
$it:item-table()$
$endfor$
```

There is a regression test for it in `crates/quarto-core/tests/integration/listing_pipeline.rs` (around line 412). The default and grid templates did not get the same fix. The user docs (`docs/guides/projects/listing-templates.qmd`) already show the `$for(items)$ … $it:item-default()$ … $endfor$` form for custom templates, so custom templates written from the docs are not affected.

## Decisions (2026-10-09)

1. Fix (a): the default and grid wrappers iterate with `$for(items)$`, like the table wrapper.
2. A failed re-parse of a built-in template should be an error. Filed separately as bd-p7eqsmmw.
3. No parser-level detection for custom templates for now; it may not be robustly detectable. Filed as backlog follow-up bd-sgd9desh. The docs show the `$for$` loop and warn about consecutive div fences.

## Checklist

- [x] Phase 0: `default_and_grid_items_are_siblings` and `default_and_grid_listings_with_120_items_render_every_item` in `crates/quarto-core/tests/integration/listing_pipeline.rs`. Both failed before the fix (depths `[3, 4, 5]`; 120-item listing dropped).
- [x] Phase 1: `listing-default.template` and `listing-grid.template` use `$for(items)$ $it:item-x()$ $endfor$`, with a comment in `templates.rs`. Both tests pass; full workspace suite green (16012 passed).
- [x] Docs: callout in `docs/guides/projects/listing-templates.qmd`; fixed the same shorthand in the ejs-listing-port skill's worked example.
- [x] Phase 3: end-to-end check (below).
- [ ] Phase 2: moved to bd-p7eqsmmw.
- [ ] Re-try the 1051-item claude-notes plans listing (bd-fvcip3t5).

## End-to-end verification

Invocation: a copy of `repro/` rendered with `target/debug/q2 render` (built from this branch), then again with 120 docs in `p/`. Inspected `_site/index.html` by counting the unclosed `<div>`s before each item div:

```
3 items:   quarto-post [3, 3, 3]   g-col-1 [3, 3, 3]
120 items: quarto-post 120 items, depths [3]   g-col-1 120 items, depths [3]
```

No Q-12-10 warning at either size. The installed nightly gave `[3, 4, 5]` on the same 3-item fixture.

## Proposed phases (draft, superseded by the checklist)

- **Phase 0: tests first.**
  - Integration test in `listing_pipeline.rs`, run through `render_project` like the existing tests. Render default and grid listings with 3 items and assert that every item div is a direct child of the `.list` container (constant depth). This fails today.
  - Integration test with about 120 trivial items for both types. Assert that no Q-12-10 diagnostic is emitted and that all items appear. This fails today.
- **Phase 1: fix the templates.** Switch `listing-default.template` and `listing-grid.template` to the `$for(items)$ … $endfor$` form used by the table template, with a blank line between items. The exact form depends on Q1.
- **Phase 2: diagnostics (depends on Q2).** Possibly change how a failed listing re-parse is reported.
- **Phase 3: end-to-end check.** Render the repro with `cargo run --bin q2` at N=3 and N=120 and inspect the depths. Then re-try the 1051-item claude-notes plans listing (bd-fvcip3t5).

## Open design questions for the user

1. **Which fix?** Options:
   - (a) Use `$for(items)$` iteration in the two listing templates. This matches the table precedent and the docs.
   - (b) Also end `item-default.template` and `item-grid.template` with an extra blank line, so that after one newline is removed there is still one left. Custom templates that use the unseparated `$items:item-default()$` form would then work too. The downside is that the fix relies on whitespace you can't see.
   - (c) Use a partial separator (`$items:item-default()[sep]$`), if the doctemplate grammar accepts a newline in the separator literal.

   My recommendation is (a). (b) could be added as a safety measure, but only with a comment in the item templates explaining why the blank line is there.
2. **Silent drop.** When the listing re-parse fails, the listing is skipped and only a warning (Q-12-10) is emitted. Should a failed re-parse of a *built-in* template become an error, since it means a q2 bug rather than an author mistake? Or should that be a separate strand?
3. **Custom-template guard.** Should q2 catch this class of bug for custom templates too? For example, it could warn when a rendered listing's item depth grows from item to item, or when it finds a `::::::` fence with attributes immediately after a closing fence. Or is following the documented `$for$` form enough?

## Risks / tradeoffs (draft)

- Existing listing snapshot and HTML tests may have encoded the nested structure. Fixing it changes their DOM, so expect snapshot churn, and report it as the snapshot policy in AGENTS.md requires.
- The CSS and JS (`quarto-listing.js`, list.js filtering) were written for Q1's flat sibling structure, so the fix should improve filtering and sorting behavior. Any code written against the nested Q2 output would need to be found and checked.
