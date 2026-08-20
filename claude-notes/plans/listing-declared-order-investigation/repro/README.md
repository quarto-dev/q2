# q2 listings lose the declared order of explicit `contents:` entries

**Observed with:** q2 0.14.0
**Repro:** `q2 render` in this directory.

## Expected (Quarto 1)

With `sort: false` (or no sort), a listing whose `contents:` is a list
of explicit paths preserves the declared order. The Connect cookbook
relies on this: `cookbook/index.qmd` lists Getting Started first, and
Q1 renders it first (see `docs-quarto-1/_site/cookbook/index.html`).

## Actual (q2)

`index.md` declares `contents: [./bravo/index.md, ./alpha/index.md]`
but the listing renders Alpha before Bravo — items come out in
path-alphabetical order regardless of declaration order.

## Root cause area

`contents:` entries are translated to glob patterns and resolved
set-based through the shared glob engine
(`crates/quarto-core/src/project/listing/glob_resolve.rs` →
`crate::glob::resolve_patterns`, single-view matching), which loses
per-pattern declaration order. `apply_sort` with `sort: false` is a
no-op (correct), so the glob engine's ordering leaks through.

A fix likely needs the resolution to remember which pattern matched
each file (first-match wins) and order the item set by pattern index,
Q1-style, at least when patterns are literal paths.

## Downstream effect in the Connect docs

All 15 cookbook listings (now `type: custom`) have curated `contents:`
orders; q2 renders them alphabetized. Also second-order: the listing
auto-description previews extracted from listing-only section pages
pick up the wrong first item.

## Related quirk (noting, not the bug)

`sort:` by a custom field works via `listing-item: extra:` front
matter (`sort.rs::field_value` falls through to `extra`), but
`is_known_sort_field` doesn't know about extra fields, so a *working*
custom-field sort still emits a misleading Q-12-3 "values will compare
as equal" warning.
