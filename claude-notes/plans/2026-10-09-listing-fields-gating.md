---
title: 'Grid and default listings ignore fields: for everything but categories (bd-p80b9jy9)'
date: 2026-10-09
description: 'The built-in grid and default item templates gate only categories on the effective field set; gate every field block on its `show.*` flag as Quarto 1 does, so fields: [date, title, description] stops drawing image placeholders.'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-p80b9jy9
---

**Branch:** `braid/bd-p80b9jy9-listing-fields-gating` (main checkout, topic branch, based on `main` @ `ea72d68aa`)
**Do not start implementation until the user gives the go-ahead.**

**Pre-flight (2026-10-09):** `cargo xtask verify --skip-hub-build` passed all Rust tests (16029/16029). The ts-packages step failed because `@bjorn3/browser_wasi_shim` and `@myriaddreamin/typst.ts` are missing, which is a stale `node_modules` in this checkout (run `npm install`), not a problem at HEAD.

## Triage verdict

**Ready to design.** The bug reproduces at HEAD exactly as described, the fix
direction in the strand holds up (the `show` map is already there and the
default field sets already contain `image`), and the only real design
decisions are scope (how much more of Q1's template structure to port) and
sequencing against the in-flight description-precedence work, which edits the
same two templates.

## Issue context

P2 bug (labels `listings`, `parity`), filed 2026-10-09 by Carlos Scheidegger.
`item-grid.template` and `item-default.template` consult the per-item `show`
map only for categories (`$if(show.categories)$`). Image, title, subtitle,
description, author, date and reading time render whenever the item has a
value, whatever `fields:` says. The visible case: a grid listing with
`fields: [date, title, description]` draws a `card-img-top` with a
`listing-item-img-placeholder` on every card.

## Dependency graph

- **discovered-from** bd-fvcip3t5 (claude-notes plans listing page, epic
  bd-uk8zgkha, in progress). Trying `type: grid` with
  `fields: [date, title, description]` on `claude-notes/plans/index.md`
  produced 1053 cards each topped by an empty grey placeholder bar. The plans
  page is a table today, so nothing is blocked; grid is just not usable yet.
- No `blocks` or `related` edges. Informally related to
  bd-listing-description-precedence-x4bh6w3m and
  bd-listing-default-no-derived-desc-m0wrr8ty, which another agent is
  implementing now (plan `2026-10-09-listing-description-precedence.md`, on a
  branch in a different checkout). That plan's Phase 3 rewrites the
  description block (`$if(description)$` → `$if(show-description)$`) and the
  `$if(image-html)$` branch (adds an image envelope) **in these same two
  templates**. See Q2.

## What the code looks like today

Repro: `claude-notes/plans/listing-fields-gating-investigation/` (README has
commands and results). Two posts with every field set (one with an image),
and default/grid listings with and without `fields: [date, title, description]`.

- **Templates** (`crates/quarto-core/src/project/listing/templates/`).
  `item-grid.template` and `item-default.template` gate image (both the
  `image-html` and the placeholder branch), title, subtitle, reading-time,
  description, author and date on value presence only. `show.categories` is
  the only `show.*` read, and that has been so since the listings feature
  landed (`ccb220023`).
- **Binding** (`binding.rs` ~503). Every item gets `show.<field> = true` for
  each field in the *effective* field set.
- **Effective fields** (`binding.rs` `effective_fields`, ~97). Explicit
  `fields:` is used verbatim. Defaulted sets are presence-filtered, but
  `image` always survives (Q1 parity).
- **Default field sets** (`config.rs` `apply_type_defaults`, ~926). Default and
  Grid both include `image`, `title`, `subtitle`, `author`, `date`,
  `description`, `categories`, `reading-time`, `filename`, `file-modified`.
  So gating on `show.*` changes nothing for listings without `fields:`, which
  answers the strand's "check the default includes image" item.
- **Quarto 1** (`external-sources/quarto-cli/src/resources/projects/website/listing/`).
  - `item-grid.ejs.md`: image block under `fields.includes('image')`
    (placeholder when the item has none); every other field under
    `showField(f)` = `fields.includes(f) && item[f] !== undefined`; categories
    under `fields.includes('categories') && item.categories`.
  - `item-default.ejs.md`: the same idea with `fields.includes(...)` checks
    (title/subtitle/description gated on the field alone; date/author/
    reading-time on field and value).
  - Q1 also has structure q2's templates lack: the grid omits `.card-body`
    when no body field shows; a `.card-footer` for `filename` /
    `file-modified`; an "other fields" table (grid) or metadata list
    (default) for any field without a dedicated slot; author/date flex
    justification classes; `image-align`. None of that is in this strand.
- **Existing tests.** `transforms/listing_render.rs` has template-level unit
  tests (e.g. `item_default_renders_category_chips_when_categories_field_enabled`)
  and a `show.*` test for custom templates (~1250). No test covers
  `fields:` narrowing for the built-in grid/default templates.

## Proposed phases (draft)

- **Phase 0 — Tests first (red).** In `listing_render.rs` unit tests: for
  grid and default, an item with every field set and a listing with
  `fields: [date, title, description]` renders no image/placeholder,
  subtitle, author, reading time or categories; a listing without `fields:`
  still renders the image and the placeholder for an image-less item.
- **Phase 1 — Gate the templates.** Wrap each block in `$if(show.<field>)$`
  (image block including the placeholder branch under `$if(show.image)$`),
  keeping the inner value checks.
- **Phase 2 — Verification.** Repro vs. Quarto 1; the plans page with
  `type: grid`; snapshot churn review; `cargo xtask verify`.
- **Phase 3 — Docs.** `docs/guides/projects/listings.qmd`: say that `fields:`
  controls which parts of a built-in card render.

## Open design questions for the user

1. **Scope.** Gate the existing blocks only (the strand's fix direction), or
   also port Q1's missing structure: empty `.card-body` omission,
   `filename`/`file-modified` footer, the "other fields" table/list,
   author/date justification? My recommendation: gating only here, and file
   the structural parity as a separate strand.
2. **Sequencing with the description-precedence work.** That branch edits the
   description and image blocks of both templates. Options: (a) land this
   small change first and let that branch rebase (the conflict is mechanical:
   their `$if(show-description)$` ends up nested inside `$if(show.description)$`);
   (b) wait for it to land and do this on top; (c) hand this strand to that
   agent to fold into its Phase 3. I lean (a), because this change is small
   and self-contained.
3. **`image` in `fields:` with no item image.** Q1 draws a placeholder per
   card when `image` is in the field set, even if no item has an image (and
   q2's `effective_fields` keeps `image` in defaulted sets for that reason).
   Keep that parity, so the plans page needs explicit `fields:` to avoid the
   grey bars? I assume yes.

## Risks / tradeoffs (draft)

- **Merge conflict** with the description-precedence branch on both
  templates (Q2).
- **Custom field sets that omit `title`** now render cards without titles.
  That is Q1 behaviour and what the author asked for, but it is a visible
  change for anyone relying on the current behaviour.
- **Snapshot churn** should be small: explicit `fields:` in existing fixtures
  is mostly on table listings.
