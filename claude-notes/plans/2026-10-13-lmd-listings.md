---
title: 'lmd-driven listings: declare the items in YAML, build the listing in an lmd cell (bd-2oqen84t)'
date: 2026-10-13
description: 'How q2 listings resolve and render today, how lmd cells run, where the two do not meet, and the mechanisms that would let a listing publish its resolved items to metadata so an lmd cell can iterate over them and build the markup. Design and spikes; not a replacement for doctemplate custom listings.'
status: implemented  # S1 to S5 landed 2026-10-13 on the topic branch, full `cargo xtask verify` green; awaiting review and merge.
braid:
  strand: bd-2oqen84t
---

**Strand:** bd-2oqen84t (exploration; discovered from bd-lmd-exploration-jqv2nbl4). **Related:** bd-0fd0 (Lua filter slot between generate and render transforms), bd-61cd (listings epic), bd-0t4e07jk (listing items cannot read bare front-matter keys).

**Branch:** `braid/bd-2oqen84t-lmd-driven-listings-declare` (topic branch in the main checkout, based on `main` @ `b976329ed`; no worktree). **PR:** [#826](https://github.com/quarto-dev/q2/pull/826).

**What landed (2026-10-13), one commit per spike.** Every resolved listing is published at `meta.listings.<id>` in the template binding\'s vocabulary and native `ConfigValue` kinds (`project/listing/metadata.rs`); a `listing-resolve` stage runs before `lmd-execute` and `user-filters-pre`, sharing `resolve_listings` with the generate transform; `type: metadata` resolves and renders nothing; the `lmd-blog` fixture pins the motivating example through the real binary; the listings guide documents the metadata contract for Lua filters. `cargo xtask verify` (full, WASM included) green at the last commit. Details per spike under § Spikes; the two follow-up strands are bd-0o82ly9c (string-typed grammar positions) and bd-u1rxccq7 (date helper).

## The idea

Quarto 2 custom listings are doctemplates: logic-less, interpolate-branch-loop over a binding that Rust pre-computes. Everything a template cannot express (grouping, counts, string surgery, conditional structure beyond `$if$`) has to be pre-computed into metadata, and the user docs say so at length ("What doctemplates cannot do"). lmd (PR #823) now gives a document a real language with markdown fragments as expressions and the document\'s metadata in scope. The question this plan studies is whether the two can meet like this:

````qmd
---
title: Blog
listing:
  id: posts
  type: metadata              # resolve the items, render nothing
  contents: posts/*.qmd
  sort: "date desc"
---

```{lmd}
return meta.listings.posts.items:map(function(it)
  return {=:
::: {.post-card}
### `{lmd} pandoc.Link(it.title, it.path)`

`{lmd} it.date` · `{lmd} table.concat(it.categories, ", ")`

`{lmd} it.description`
:::
:=}
end)
```
````

Three pieces: blog pages under a globbable location as usual; a listing declaration that only *resolves* items into a known metadata location; an lmd cell that iterates over them and builds the DOM. Built-in and custom doctemplate listings keep working unchanged.

Going through `meta.*` is the load-bearing choice, and it is independent of lmd. A plain Lua filter can read the items in `Meta` and build the listing in `Div` (or wherever it likes); lmd is one consumer of that metadata, not the mechanism. The two designs compose: the metadata contract gives any Lua code the items, and lmd gives the author a place to write that code next to the markdown that wants it. Either half is useful without the other.

## How listings work today

The listings feature (epic bd-61cd, plans `2026-05-05-listings-*` through `2026-10-09-listing-*`) is built on the project\'s two-pass architecture.

**Pass 1, the profile.** Every document is parsed once up to `DocumentProfileStage` and summarized into a `DocumentProfile` (`crates/quarto-core/src/document_profile.rs`): title, subtitle, description, abstract, authors, date, categories, image, draft, `order`, the `listing-item:` block (`ListingItemInfo`, with `extra` for custom fields), derived fallbacks (first paragraph, preview image; `ListingItemInfoStage`), and `listing_content_globs`, the host page\'s `listing.*.contents:` globs resolved to project-relative form so the dependency graph (L6) knows the host depends on its items. The profiles are collected into a `ProjectIndex`.

**Pass 2, the render.** Two AST transforms inside `AstTransformsStage`, both in the `Navigation` phase:

- `ListingGenerateTransform` (`transforms/listing_generate.rs`) parses `meta.listing` (`project/listing/config.rs`, `parse_listings`), resolves each listing\'s `contents:` globs against `ProjectIndex::profiles()` (no filesystem walk; identical on native and WASM), overlays inline records, hydrates one `ListingItem` per hit (`project/listing/item.rs`), applies `include`/`exclude`, sorts, truncates, and stores `Vec<ResolvedListing { listing, items }>` on **`RenderContext::resolved_listings`**. It bails out if that field is already populated, which the code describes as "another generate path (today: hand-edited, tomorrow: a Lua filter post-bd-0fd0) already populated the resolved set".
- `ListingRenderTransform` (`transforms/listing_render.rs`) builds a doctemplate `TemplateContext` per listing (`project/listing/binding.rs`: `listing.*`, `items[]`, `project.*`), applies the built-in template for the type or the author\'s `template:` file for `type: custom`, appends the hidden "No matching items" div, **re-parses the output markdown with the qmd reader**, appends the List.js pagination nav and init script for `default`/`grid`, and splices the blocks into the author\'s `::: {#<id>}` slot or a new `#listing-<id>.quarto-listing` div at the end of the page.

Downstream consumers of `resolved_listings`: `CategoriesSidebarTransform`, the RSS feed stage and link injector (L9), and `llms.txt`. After the whole site renders, the native-only L7 post-render upgrade (`project/listing/post_render_upgrade/`) rewrites the host HTML inside `<!-- desc-begin(…) -->`/`<!-- img-begin(…) -->` envelopes with the first paragraph and preview image of the rendered item pages.

**The per-item binding** (`binding.rs`, the "load-bearing public contract for L8 custom templates") has these keys: `title`, `subtitle`, `description` (markdown, each with an `-html` twin), `author`, `authors`, `date`, `date-modified`, `categories`, `category-html`, `image`, `image-alt`, `image-html`, `path` (host-directory-relative `.qmd` source path, rewritten to `.html` later by `LinkRewriteTransform`), `filename`, `order`, `reading-time`, `reading-time-minutes`, `word-count`, `metadata-attrs`, `table-row`, `show.*`, `show-description`, `extra.*`, and the four placeholder markers `description-placeholder-begin/end`, `image-placeholder-begin/end`.

**Why the data is not in metadata.** The L3 plan\'s original D2 put the resolved listings at `meta.listings.<id>` so a Lua filter could mutate them. It was revised in-flight (L3 §D2, §D13) because there is no Lua filter slot between generate and render inside `AstTransformsStage` (`UserFiltersStage::pre` runs before the whole stage, `::post` after it), and a `ConfigValue` round-trip of `Listing`/`ListingItem` looked like 200 lines of boilerplate with nothing to consume it. bd-0fd0 tracks the slot; `RenderContext::resolved_listings`, `listing_generate.rs`, `listing_render.rs`, `categories_sidebar.rs` and the pipeline\'s NAVIGATION PHASE comment all carry `TODO(bd-0fd0)` markers naming "a meta serialize/deserialize bridge at the injection boundary" as the integration point. The `"listings"` key is otherwise unused in the codebase (only an unrelated reconcile test uses the word).

## How lmd works today

From `claude-notes/plans/2026-10-12-lmd-exploration.md` (spikes 1 to 7, merged as PR #823):

- `LmdDesugarStage` runs right after `PreEngineSugaringStage` and before the engine; it rewrites `{lmd}` cells and inline code into desugared Lua (`{=: :=}` to `pandoc.Blocks`, `{=[ ]=}` to `pandoc.Inlines`, `{= =}` to `pandoc.Nodes`) and marks them pending.
- `LmdExecuteStage` runs **after the engine and before `user-filters-pre`**, that is, before every AST transform. One Lua state per document (`pampa::lua::lmd::LmdRuntime`), the filter environment plus `quarto.metadata.get` and the `meta` proxy over **a snapshot of `ast.meta` taken as the stage starts**. Cells run in document order; a block cell returns a `Block`, a list of blocks, or `nil`; an inline cell returns inlines or a string.
- `meta` navigation: maps are lazy proxies, arrays are real `pandoc.List`s (so `:map` works), YAML strings in document metadata come back as `Inlines` (the `MetaInlines` convention), `Scalar` strings as Lua strings.
- Errors are located at document lines (spike 7); the snapshot suite in `crates/quarto/tests/integration/lmd_snapshots.rs` pins output and diagnostics for four fixtures.

## The gap, precisely

Two independent things stand between the sketch above and a working page:

1. **Data location.** Resolved listings live in a typed Rust field that nothing Lua-visible reads. An lmd cell (or a user filter) has no way to see the items.
2. **Order.** Even if the data were in `ast.meta`, `lmd-execute` runs before `ast-transforms`, and `listing-generate` is in the `Navigation` phase of `ast-transforms`, long after. The cell\'s `meta` snapshot would predate the data.

Both are plumbing rather than design holes, and the second turns out to be a choice we made for a reason that no longer binds. `ListingGenerateTransform` needs `ast.meta.listing`, `ProjectIndex::profiles()`, the project directory, the host page\'s path and the `SourceContext` for glob provenance. None of those are produced by an AST transform. It sits in `Navigation` because "a future Lua-filter slot (bd-0fd0) should see the full generated set in one place" alongside the navbar/sidebar generates, not because anything upstream of it in the phase feeds it. `StageContext` already carries `project_index` (it is how `AstTransformsStage` hands it to `RenderContext`), so a pipeline *stage* placed before `lmd-execute` can resolve listings.

One thing that genuinely must stay late: the *render* side. `ListingRenderTransform` reads `date-format` and the localized "No matching items" term from normalized metadata, and the link-rewrite and resource-collection transforms that make `path` and `image` work run inside `ast-transforms` after it. An lmd cell that emits `pandoc.Link(..., "posts/a.qmd")` and `pandoc.Image(..., "posts/a/cover.png")` at `lmd-execute` gets the same treatment, since both transforms run later; the listing-templates guide\'s two silent-failure rules (links and images must be AST nodes, not raw HTML) apply to lmd output exactly as they apply to doctemplate output.

## Proposal

Three mechanisms, each useful on its own, in the order they would be built.

### M1. Publish every resolved listing to `meta.listings.<id>`

A serializer `ResolvedListing -> ConfigValue`, written once, alongside the binding (`project/listing/binding.rs` is the right neighbour; the key names and semantics should match the template binding so the two contracts do not drift, and the listing-templates guide can document one vocabulary). Shape:

```yaml
listings:
  posts:
    id: posts
    type: metadata
    fields: [date, title, description]     # effective_fields()
    sort: ...                              # as declared
    items:
      - title: !md "Fix `_scope`: a *lexical* regression"   # PandocInlines
        subtitle: ...                                       # PandocInlines, absent if none
        description: ...                                    # PandocInlines; the L1 fallback when derived
        description-source: authored | derived | ...        # FieldSource, so a cell can decide
        author: "A, B"                                      # Scalar
        authors: [A, B]
        date: "Oct 1, 2026"                                 # Scalar; the display string, formatted as the binding does (decision 2)
        date-iso: "2026-10-01"                              # Scalar; the stored value, for sorting and grouping
        date-modified: ...
        categories: [a, b]
        image: posts/a/cover.png                            # Scalar; project-relative as in ListingItem
        image-alt: ...
        path: posts/a.qmd                                   # Scalar; host-dir-relative .qmd, rewritten later
        filename: a.qmd
        order: 3
        reading-time-minutes: 4
        word-count: 812
        extra: { ... }                                      # ConfigValue passthrough
```

`ConfigValue` already distinguishes `Scalar` strings (which reach Lua as strings) from `PandocInlines` (which reach Lua as `Inlines`), so `it.path` is a string usable as a link target and `it.title` is inlines usable as link content, with no `stringify` dance. Lists (`authors`, `categories`) are always present, empty when the item has none, so `#it.categories` and `ipairs` never need a guard; the binding omits empty lists so `$if$` works, and that is the one presence rule the two contracts do not share (found by the first end-to-end cell, which indexed nil). This is the detail the L3-era objection ("round-trip boilerplate") did not have in view: the serializer is one direction only, and `ListingItem` already holds prose as `Inlines`.

The write happens in the generate step for **every** listing type, not only the new one. That makes M1 the "meta serialize bridge" bd-0fd0 asked for, minus the deserialize half: a user filter or lmd cell can *read* any listing on the page. Whether `ListingRenderTransform` should *read back* `meta.listings.<id>` when it differs from `resolved_listings` (so a filter can mutate a built-in listing) is bd-0fd0\'s own question and stays out of scope here (decision 4).

What is deliberately **not** in the shape: `title-html` and friends (a cell has `pandoc.write` if it wants HTML), `metadata-attrs`, `table-row`, `show.*` and `show-description` (artifacts of the logic-less template), and the four L7 placeholder markers (see M3). `outputHref` is spelled `output-href`. Custom fields are published under `extra` only, never flattened into the item map as the binding does, so a custom field cannot shadow a curated key in Lua. Three keys exist only here: `description-source` and `image-source` (`listing-item`, `document`, `abstract`, `record`, `derived`, `absent`: which step of the hydration chain the value came from, so a cell can tell authored from derived) and `origin` (`document`, `record`, `record-over-document`). The listing map also carries `page-size`, `max-items`, `max-description-length`, `date-format`, `image-placeholder`, `categories` (the mode name) and `template-params`.

Prose is prepared exactly as the binding prepares it before writing it out (links and notes dropped, one line, description truncated at the listing\'s `max-description-length`), then published as `PandocInlines` instead of markdown text: the same value as a template sees, in its native type. The one judgement call in that rule is dropping links from titles; the binding does it because every template wraps the title in the item link, and a cell will too, so parity won.

### M2. Resolve listings before `lmd-execute`

Move item resolution out of the `Navigation` phase into a new pipeline stage, `listing-resolve`, placed in `build_html_pipeline_stages_with_options` between the engine stage and `LmdExecuteStage`. It does what `ListingGenerateTransform::transform` does today, writes `meta.listings.<id>` (M1), and stashes the typed `Vec<ResolvedListing>` on `StageContext`; `AstTransformsStage` copies it into `RenderContext::resolved_listings` the way it already copies `project_index`. `ListingGenerateTransform` stays registered and keeps its existing "already populated, treat as override" early return, so every test that drives the transform directly still passes and the `Navigation` ordering comments stay true for the render side.

Why a stage and not an earlier transform: `lmd-execute` is a stage, and the only hook before it is the stage list. (An alternative is to move `lmd-execute` itself into `ast-transforms` as a transform in the `Navigation` phase after `listing-generate`. That would also put it after shortcode resolution and `heading-id`, which changes spike 5\'s ordering story and the "cells see engine output, filters see cell output" contract. Not recommended; see Alternatives.)

What has to hold for this to be safe:

- Item hydration reads only pass-1 profiles and the host\'s own `listing:` YAML. Verified in `listing_generate.rs`: the inputs are `ast.meta`, `ctx.project_index`, `ctx.project.dir`, `page_relative_source(ctx)` and `ctx.source_context`.
- `meta.listing` is read before `ConfigMarkdownTransform`/shortcode resolution normalize metadata. Globs and record scalars are plain strings; a shortcode inside `contents:` is not supported today either. Records\' `title`/`description` are parsed as markdown at record-parse time (`record.rs`), not by the normalization transforms. To confirm in the spike: render the existing listing fixtures with the stage moved and diff the snapshots.
- Book pipelines pause/resume `ast-transforms` by phase (`pause_stages_from`, `finishing_stages_from`). A stage before `lmd-execute` sits in the prefix and runs once; the resumed tail\'s `ListingGenerateTransform` sees `resolved_listings` populated only if the stash survives on `StageContext`. Listings on book pages are not a supported combination today, so a regression test that the book pipeline still builds is enough.
- WASM: `ListingGenerateTransform` already runs in the hub-client preview; the stage must not pull native-only code. `LmdExecuteStage` runs on both targets too. (Checked after landing: `build_q2_preview_pipeline_stages`, which both `q2 preview` and the hub client use, is the HTML stage list minus `Q2_PREVIEW_STAGE_EXCLUDED`, and `listing-resolve` is not on that list, so preview renders see `meta.listings.<id>` exactly as `q2 render` does.)

Side effect worth having: `user-filters-pre` runs after the new stage, so Lua filters see `meta.listings.<id>` too. That is most of bd-0fd0\'s read half for free.

### M3. A listing type that resolves but does not render

`type: metadata` (decision 1; `data` and `none` were the other candidates, and `metadata` won for saying exactly what the listing does) is a fourth `ListingType`. The generate step treats it like any other type (globs, records, filters, sort, `max-items`, `fields`); the render step skips the template, the "No matching items" div, the List.js bootstrap and the container, and emits nothing. The categories sidebar, the feed stage and `llms.txt` keep working, since they read `resolved_listings`, which is a real benefit: an lmd-built blog index still gets an RSS feed and a category sidebar for nothing.

Interaction with the rest of the listing machinery, type by type:

| Mechanism | Built-in / custom today | `type: metadata` + lmd cell |
|---|---|---|
| Item links (`path`) | `.qmd` in the template, rewritten by `LinkRewriteTransform` | the same, provided the cell emits `pandoc.Link`, not raw HTML |
| Item images | markdown image, collected and copied | the same, with `pandoc.Image` |
| Derived description/image (L7) | envelope markers in the template, substituted post-render from rendered item HTML | **not in v1.** The cell gets the L1 fallback (`description` = first paragraph *as written*, `image` = preview image found before rendering), which is exactly what the hub-client preview shows today. The markers are HTML comments; exposing them as `Scalar` strings for a cell to wrap in `pandoc.RawBlock("html", …)` is mechanical, but the L7 regex also expects the envelope to sit inside a known structure, so this needs its own look. Decision 3: the L1 fallback is accepted for v1. |
| Container `#listing-<id>.quarto-listing` and listing SCSS | emitted by the render transform | the author\'s business: wrap the cell result in `{=: ::: {#listing-posts .quarto-listing .quarto-listing-container-default} :=}` if the built-in CSS is wanted |
| List.js pagination/category filter | `default`/`grid` only | none, as for `custom` today |
| Dependency graph (host rebuilds when an item changes) | via `listing_content_globs` on the host\'s profile | the same; `flatten_content_globs` does not look at `type` |
| Incremental render cache key | includes listing config | unchanged |
| `Q-12-*` diagnostics for bad globs, records, fields | from generate | the same; template diagnostics (`Q-12-7`, `Q-12-9`, `Q-12-24`, `Q-12-10`) never fire |

Why a type rather than `type: custom` with no `template:`: that combination today falls back to the default template with `Q-12-7`, and overloading it would make a configuration mistake indistinguishable from an intent.

Noticed while reading the S2 output, before `type: metadata` existed: the lmd cards and the built-in listing on the same page both produced `id="first"` (the card\'s section from the heading-id transform, the listing\'s `h3` from its re-parsed template). The built-in listing is spliced in the Navigation phase, after `heading-id` has run, so its reader-derived heading ids are never de-duplicated against the page. Pre-existing (any `## First` heading on a listing page collides the same way) and out of scope here; `type: metadata` sidesteps it for lmd listings by not rendering the built-in markup at all.

### What the author writes, end to end

With M1 to M3 in place the sketch at the top of this document works as written. Dates (decision 2): the serializer pre-formats `date` and `date-modified` the way the template binding does (listing `date-format`, then document `date-format`, then `medium`), so `` `{lmd} it.date` `` is the display string, and `date-iso` / `date-modified-iso` carry the stored values for sorting and grouping. A Lua-side formatting helper (`quarto.listing.format_date(iso, style)` over `dates::format_date`) is filed as bd-u1rxccq7 and not part of this plan.

A fuller example, to make the ergonomics concrete (grouping by year, which no doctemplate can do):

````qmd
```{lmd}
local by_year = {}
for _, it in ipairs(meta.listings.posts.items) do
  local y = it["date-iso"]:sub(1, 4)
  by_year[y] = by_year[y] or {}
  table.insert(by_year[y], it)
end
local years = {}
for y in pairs(by_year) do years[#years + 1] = y end
table.sort(years, function(a, b) return a > b end)

local out = pandoc.Blocks({})
for _, y in ipairs(years) do
  out:extend({=:
## `{lmd} y`
:=})
  for _, it in ipairs(by_year[y]) do
    out:extend({=:
- `{lmd} pandoc.Link(it.title, it.path)` — `{lmd} it.description`
:=})
  end
end
return out
```
````

## Findings about lmd itself, from writing the examples

These are gaps in lmd that a listing is the first real consumer to hit. None blocks the plan; each is a candidate follow-up strand.

1. **No interpolation into link targets, image sources or attributes.** The desugarer sees inline lmd code only as a `Code` node in an inline list. `[text](url)` has `url` as a string field of `Link`, so `` [`{lmd} it.title`](`{lmd} it.path`) `` does not do what it looks like: the target is the literal text. The author has to drop to `pandoc.Link({=[ … ]=}, it.path)`, which is what the examples above do. Same for `![alt](src)` and `{.class key=value}`. This is a limitation of the qmd grammar, not of lmd: those fields are strings by design (full markdown there makes no sense; `[link](_foo_)` should not read as an `Emph`), and today only shortcodes reach into them, through the text-context expansion in `transforms/shortcode_resolve.rs` (`expand_text_in_place`, bd-fz6gwfq0: results stringified, attribute values only, never ids or classes). Code that returns a *string* does make sense there. The idea on the table is a string-typed qmd grammar node for those positions: string literals, executable `{lang}` code that must return a string, and shortcodes limited to the ones that return strings. Filed as **bd-0o82ly9c** (decision 6); separate from this plan, which uses `pandoc.Link(...)` directly.
2. **Strings versus inlines at the boundary.** `it.title` is `Inlines`, `it.path` is a string. That is the right split and `ConfigValue` carries it, but the rule ("YAML document strings are markdown, generated scalars are strings") needs to be written down in the user-facing docs for `meta`, or every listing author will discover it by `pandoc.utils.stringify`.
3. **Heading ids.** A grouped listing emits headings from a fragment; spike 5 already makes their ids follow the final text, so `## 2026` gets `#section-2026`-style ids with no further work. Worth a pin in the fixture.
4. **Echo of a long cell.** `--| echo: true` on a listing cell shows the line-preserving desugared form, which for a multi-fragment cell is hard to read (spike 7, finding 2). Cosmetic.
5. **Numbers in inline splices.** `` `{lmd} #it.categories` `` inside a fragment fails with `Q-11-3: Inline userdata, string, or Inline-like value expected, got number`: the splice lands as a raw element of `pandoc.Para({ … })`, and pampa\'s inline coercion takes strings but not numbers. Spike 3 chose the same rule for top-level inline cells. A number in inline position has only one sensible meaning (`Str(tostring(n))`); accepting it in the coercion layer would remove a `tostring` from most listing cells. Candidate follow-up, with bd-0o82ly9c as the natural home since both are about what may stand in a text position.
6. **A failing cell shows its source.** When a cell fails at run time, the page keeps the cell as a plain `lmd` code block holding the *original* fragment syntax, and the diagnostic points at the fragment\'s closing line rather than the inline cell inside it that failed (spike 7 said this is the honest resolution without a column map). Both were fine for debugging the cells above; noted so nobody is surprised.

## Alternatives considered

- **A pull API instead of metadata**: `quarto.listing.items("posts")` or `quarto.listing.resolve({ contents = "posts/*.qmd" })` backed by the project index. Lazy and typed, no serializer. Rejected as the primary mechanism because the YAML declaration is what feeds `listing_content_globs` and thus the dependency graph and incremental rebuilds; a listing resolved only from Lua would not rebuild the host when an item changes. A thin `quarto.listing.get(id)` returning the same table as `meta.listings[id]` is harmless sugar and can come later.
- **Move `lmd-execute` into `ast-transforms` after `listing-generate`**: fixes the order with no new stage, but relocates every cell past shortcode resolution, heading ids and the engine/filter boundary spikes 2 and 5 reasoned about. Too much for one consumer.
- **A second, opt-in execution point** (`--| after: listings`): cells marked this way would run in a later slot. It splits the one-Lua-state-per-document model (which cells share state with which?) for no gain over M2.
- **Doing it all in doctemplate**: the template is logic-less by design; the user docs already send authors to metadata pre-computation for anything beyond interpolation. That is the problem statement, not a solution.

## Spikes

Each spike is one commit on a topic branch off `main`, with its tests, and ends with the plan updated. Order matters: S1 and S2 are independent of M3 and already useful on their own.

- [x] **S1, the serializer (M1).** Done 2026-10-13. `project/listing/metadata.rs`: `publish_listings` / `listing_to_config_value` / `item_to_config_value`, called at the end of `ListingGenerateTransform`. `key_parity_with_the_template_binding` walks the binding\'s item map against the metadata item map with explicit omitted / renamed / added lists, so neither contract can drift without a line changing there. The binding lends `effective_date_style`, `display_date`, `host_relative_qmd` and a new `prose_inlines` (the inlines `prose_markdown` writes out). The end-to-end pin moved to S2: before the stage exists nothing Lua-visible runs after the generate transform, so a filter reading `meta.listings.<id>` is the honest test and it only passes once S2 lands.
- [x] **S2, the stage (M2).** Done 2026-10-13. The generate transform\'s body became `resolve_listings(listing_value, &ListingResolveInputs, &mut diags) -> ListingResolution` (same file), shared by the transform and the new `stage/stages/listing_resolve.rs`, registered as `listing-resolve` right before `lmd-execute` (30 stages on the HTML list). The stage stashes the set on `StageContext::resolved_listings` and sets `listings_resolved`; `AstTransformsStage` bridges both into the inner `RenderContext` and back, like `code_block_decorations`. The flag is what makes the override sound: `resolved_listings.is_empty()` alone cannot tell "not resolved yet" from "resolved, and the page\'s `listing:` was unusable", and without it the transform would parse and diagnose the same bad `listing:` a second time. Pinned end to end in `tests/integration/listing_meta_bridge.rs`: a Lua filter at `user-filters-pre` reads `doc.meta.listings.posts.items` (titles as inlines, paths as strings, resolved order), and the motivating lmd cell renders cards with `pandoc.Link(it.title, it.path)` whose targets the link rewriter turns into `b.html` / `a.html`. Whole `quarto-core` and `quarto` suites green, listing snapshots unchanged.
- [x] **S3, the type (M3).** Done 2026-10-13. `ListingType::Metadata`, spelled `metadata`, parses beside the other four names (the two `Q-12-1` messages and the `Q-12-1` docs page list it). It takes the `custom` defaults: no field set of its own (`effective_fields` then reports every field some item carries, as for `custom`), page size 25, no layout knobs. `ListingRenderTransform` skips it entirely, so no template, container, "No matching items" div or List.js bootstrap, and the List.js artifacts are registered only when some listing on the page actually rendered. The resolved set stays on the context, so the categories sidebar, the feed and `llms.txt` see it unchanged. Pinned in the render transform tests (nothing appended, an author\'s slot left as written, no artifacts; a mixed page still gets the artifacts for its rendered listing), the config tests, and the end-to-end lmd page, which now declares `type: metadata` and asserts no `quarto-listing` markup and no `list.min.js`. The dependency-graph claim needed no new test: `flatten_content_globs` never looks at `type`, and the existing L6 tests cover it.
- [x] **S4, the motivating example.** Done 2026-10-13. `crates/quarto/tests/integration/fixtures/lmd-blog/`: a website with three posts (two years, one authored description, one multi-author, markup in a title) and `posts/index.qmd` declaring a `type: metadata` listing with three cells: cards (`pandoc.Link(it.title, it.path)`, display date, `it.author or "anonymous"`, category count, `description-source`, description), one `pandoc.BulletList` per year grouped over `date-iso`, and an inline cell counting the items. The single-file harness has no project index, so `lmd_snapshots.rs` grew `render_blog`, which copies the project to a scratch directory, runs `q2 render`, and snapshots the host page\'s `<main>` and the normalized stderr (`blog.main.html.snap`, `blog.stderr.snap`). The stderr snapshot is one line: the page renders with no diagnostics. Observations from the output: links are `third.html` and friends (the rewriter did its job on `pandoc.Link` targets); the title inlines keep their `<em>` and `<code>`; year headings get pandoc\'s `section` / `section-1` ids since their text is all digits; and a fragment that *is* a list yields one `<ul>` per item, so the idiomatic form is a `Blocks` fragment per item handed to `pandoc.BulletList`, which the fixture now shows.
- [x] **S5, docs (the half that stands on its own).** Done 2026-10-13. `docs/guides/projects/listings.qmd` gained "Listings as data": `type: metadata`, the `meta.listings.<id>` shape as a table of keys with their Lua types, a `Pandoc(doc)` filter that builds a bullet list from the items, what keeps working (categories sidebar, feed, dependency tracking) and what a hand-built listing goes without (the post-render description upgrade, in-browser paging), and the links-and-images rule. `listing-templates.qmd` points there from "What doctemplates cannot do". The lmd cell form is deliberately absent: lmd has no user docs yet, and the listing docs should not be where it is introduced. When the lmd docs land, "Listings as data" is where one paragraph and the `lmd-blog` fixture\'s cells belong.

Rough size: S1 and S3 are each half a day; S2 is a day with the verification; S4 half a day; S5 depends on lmd docs. (Actual: all five landed in one session on 2026-10-13, S1 through S4 as one commit each.)

## Decisions (2026-10-13)

The six open questions of the first draft, answered by the user the same day. The text above has been updated to match; this list is the record.

1. **The type name is `metadata`.** `data` and `none` were the alternatives; `metadata` is the most explicit about the effect, which is the point of the type.
2. **Dates are pre-formatted**, as the template binding does, with `date-iso` and `date-modified-iso` alongside for sorting and grouping. The straightforward version lands first; the Lua formatting helper is bd-u1rxccq7.
3. **L7 derived descriptions and images:** the L1 fallback is accepted for v1. The envelope markers stay out of the metadata shape.
4. **bd-0fd0\'s write half** stays with bd-0fd0. M1 is built so as not to foreclose it: one key, the binding\'s vocabulary, no second data path.
5. **M1 publishes every listing**, not only `type: metadata`. Listing pages carry a few KB more metadata; q2 is memory-efficient enough that this is not worth gating.
6. **String interpolation into link targets, image sources and attributes** is filed as bd-0o82ly9c, a standalone grammar strand (finding 1 above), rather than waiting for a second consumer.

Also recorded: the `meta.*` route is right *because* it is independent of lmd. A plain Lua filter with `Meta` and `Div` handlers gets the same items; lmd listings are the composition of the two designs, not a feature of either.

## Verification plan

- Per spike: `cargo clippy -p quarto-core --all-targets -- -D warnings`, `cargo nextest run -p quarto-core -p quarto -E 'test(listing) | test(lmd)'`.
- Before merge: `cargo xtask verify` (full, not `--skip-hub-build`; `quarto-core` feeds the WASM leg and both the listing generate transform and the lmd stages run in the hub-client preview).
- The listing snapshot fixtures and the lmd snapshot suite are the regression guard for S2: if moving resolution earlier changes any listing output, the diff says where.
