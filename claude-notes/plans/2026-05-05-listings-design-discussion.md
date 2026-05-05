# Listings — design discussion against the DocumentProfile architecture

**Date:** 2026-05-05
**Status:** Discussion document **promoted to an epic plan** at
`claude-notes/plans/2026-05-05-listings-epic.md`. This file remains
the rationale reference for *why* each design decision in the epic
plan was made; the epic plan is *what* gets implemented. Read this
first if you're new to the listings work.
**Parent epic (when this becomes one):** website project epic
(`bd-0tr6`). The listings feature is explicitly out of scope for that
epic (see §"Out of scope" in
`claude-notes/plans/2026-04-23-website-project-epic.md`).

## Why this conversation now

Q1 listings have been on the website epic's deferred list since
Phase 0. The website epic itself was scoped to *avoid* every feature
that had non-trivial cross-document coordination beyond what a sidebar
needs, because we wanted the snapshot/two-pass design to settle first.

Now that Phases 0–9 of the website epic are landed, the architecture
that those phases produced — `DocumentProfile`, `ProjectIndex`,
Pass-1/Pass-2 split, dependency graph, post-render hooks — covers
*more* of what Q1 listings need than was obvious when the epic started.
This document audits how much, where the residual gaps are, and
specifically whether the part of Q1 listings that motivated the defer
(custom EJS templates with full metadata access) is still the
showstopper.

## What Q1 listings actually do

A listing is a page declaration that materializes a list of "items"
sourced from sibling documents (or YAML files, or raw files), then
renders that list into the host page using one of four templates
(`default`, `grid`, `table`, `custom`).

Read top-down from Q1:

1. **Item discovery** —
   `external-sources/quarto-cli/src/project/types/website/listing/website-listing-read.ts`,
   `readContents`. Each listing's `contents:` field is a list of globs
   (default: every `.qmd` next to the host page, minus the host
   itself) plus optional inline metadata records. Globs match against
   the project's input files; YAML paths are read as
   metadata-document items; `.qmd` paths produce `document` items.
2. **Item hydration** — `listItemFromFile`. For each `.qmd` matched,
   Q1 reads the merged `(project + directory + frontmatter)` metadata,
   plus the input-target index (which carries `outputHref`,
   pre-rendered `markdown.markdown` for word-count/reading-time).
   The hydrated item is a flat record with the union of standard
   listing fields (`title`, `subtitle`, `description`, `author`,
   `date`, `image`, `categories`, …) **plus** every other field the
   author put in the frontmatter (`...documentMeta` is spread first;
   the standard fields override). This last detail is the one that
   complicates EJS-custom-template support — see §"The custom
   template problem".
3. **Filtering, sorting, hydration** — `hydrateListing` resolves
   field defaults per type (table vs. grid vs. default), `include` /
   `exclude` filters apply, `sort` is applied.
4. **Template render** —
   `external-sources/quarto-cli/src/project/types/website/listing/website-listing-template.ts`,
   `templateMarkdownHandler`. An EJS template is rendered into
   markdown, fed into Pandoc through the markdown pipeline, then the
   resulting HTML is grafted into a `<div id="listing-…">` slot on the
   host page during the post-render HTML pipeline.
5. **Two kinds of placeholder for "rendered" data** — Q1 deliberately
   does *not* try to read sibling rendered HTML from inside the host
   page's Pass-2 (it would force an ordering between sibling renders).
   Instead, the EJS template emits comment placeholders
   (`<!-- desc(5A0113B34292)[max=…]:relative/path.html -->` and
   `<!-- img(…)[…]:id:relative/path.html -->`). Then in
   `completeListingItems`, the project's post-render walks every
   output file, regex-finds those placeholders, opens each referenced
   sibling output file, extracts `firstPara` + `previewImage` from its
   rendered HTML, and substitutes back. This is a **third pass** in
   all but name, and it depends on every sibling already being on
   disk.
6. **Categories sidebar, RSS feeds, listing index, supplemental
   render set for incremental** — all derived from the same
   item set; lifecycle is similar (Pass-2 produces stub, post-render
   substitutes).

The key shape: Q1 has Pass-1-style metadata access (via
`inputTargetIndex`), Pass-2-style template rendering (EJS into the
host's markdown pipeline), and Pass-3-style content substitution
(scanning sibling output files for `firstPara` and preview images).

## How that maps onto Q2's existing machinery

Phases 0–9 of the website epic produced exactly the seams Q1 listings
were missing in 2020. Quick crosswalk:

| Q1 piece                                         | Q2 equivalent                                                                                   |
|--------------------------------------------------|--------------------------------------------------------------------------------------------------|
| `inputTargetIndex(project, path).markdown.yaml`  | `ProjectIndex.lookup_by_source(path) → &DocumentProfile`                                         |
| `inputTarget.outputHref`                         | `DocumentProfile.output_href`                                                                    |
| Project-relative input path                      | `DocumentProfile.source_path` (project-relative, forward slashes; same invariant Q2 enforces)    |
| `target.markdown.markdown` (raw md for reading time / word count) | **Not present in profile today** — pampa knows how to compute a reading-time outline, but the profile snapshots are deliberately small. See §"Profile gap analysis". |
| `documentMeta.image`, `meta[kImageAlt]`, `meta[kImageLazyLoading]` | `DocumentProfile.image` already; alt/lazy-loading not snapshot today.                            |
| `parseAuthor(documentMeta.author)`               | `DocumentProfile.authors: Vec<String>` (flat names, structured author info deliberately deferred — §"Profile gap analysis" item 2).  |
| Per-page Pass-2 EJS rendered into markdown pipeline | A Pass-2 transform reading `ProjectIndex` and emitting block-level markdown for the host. Same shape sidebars/navbars now use. |
| `completeListingItems` post-render placeholder substitution | The website project type's `post_render` hook (Phase 7) is the natural home for this — it already walks rendered output files for sitemap, favicon, etc. |
| `listingSupplementalFiles` incremental rendering | Phase 8's `ProjectDependencyGraph` already supports the edge type we need: "listing host page depends on every page matched by its `contents:` globs". The `body_link_targets` field is the working precedent. |
| RSS feed file production                          | Same pattern as `WebsiteCanonicalUrlTransform` + `write_sitemap` from Phase 7 — produces a per-listing `feed.xml` in `post_render`. |

The mapping is clean enough that the listings feature reads more like
"a fifth Pass-2 transform plus a post-render hook" than "a separate
project subsystem".

## What's *not* covered by what we have today

These are the residual gaps. Some are fillable by extending the
profile contract; one is genuinely hard.

### Profile gap analysis

Listing items in Q1 expose ten standard fields. Eight are already in
`DocumentProfile`:

- `title`, `subtitle`, `description`, `image`, `author(s)`, `date`,
  `categories`, `path`/`outputHref`.

Two are not snapshot today and would need to be:

1. **Reading time / word count.** Q1 derives these from raw markdown
   text (`estimateReadingTimeMinutes` runs a tokenizer on
   `target.markdown.markdown`). pampa has the AST at the profile
   checkpoint, so a word-count pass is straightforward. Reading time
   is just `word_count / words_per_minute` with the same constant
   Q1 uses. Both fields are pure functions of the same AST that
   produced `outline`, so there's no engine-output dependency. **Add
   `word_count: Option<u32>` + `reading_time_minutes: Option<u32>`
   to `DocumentProfile` (additive, no version bump if defaulted).**
2. **Date-modified, file-modified.** `date-modified` (from
   frontmatter) and `file-modified` (mtime). The first is metadata
   and lives naturally on the profile. The second is filesystem state
   and changes outside the AST — Q1 reads it at hydration time per
   request. We can either (a) snapshot mtime alongside the profile
   (cheap; need to invalidate cache when mtime changes — already
   handled by Phase 8's source hash), or (b) compute it lazily at
   listing-resolve time. Either works; (a) is more uniform.
3. **`image-alt`, `image-lazy-loading`.** Two scalar fields. Add to
   profile or read via a "raw frontmatter pass-through" mechanism
   (see custom-template discussion below).
4. **Author preview / structured authors.** Q1 produces a `string[]`
   of author display names through `cslNames` / `parseAuthor`. The
   profile already does flat names via `authors: Vec<String>`; the
   contract explicitly defers structured author metadata. For
   listings v1 we accept the flat list — Q1's `default` / `grid` /
   `table` templates only display joined strings anyway.
5. **`description` fallback to `abstract`.** Q1 falls back from
   `description` → `abstract` → rendered-content placeholder. The
   profile has `description: Option<String>`; we either widen
   profile extraction (`description ?? abstract`) or do the fallback
   in the listing resolver. Either is fine; profile extraction is
   the cleaner cut because it makes the listing resolver
   data-source-agnostic.

None of these is hard. All are additive on the v3 profile (we are
already at v2 from Phase 8; bumping is cheap when the next change is
the Phase 9 follow-up that needs it anyway).

### The custom template problem

This is the genuinely hard part, and it's the reason the listings
feature was deferred when the website epic was scoped.

**Q1 lets a custom `template:` author write any EJS expression they
want against an item.** Look at the default item template
(`item-default.ejs.md`):

```
const readField = (item, field) => {
  let value = item[field];
  if (field.includes(".") && !field.endsWith(".") && !field.startsWith(".")) {
    const fields = field.split(".");
    value = item;
    for (const deref of fields) {
      value = value[deref];
    }
  }
  return value;
}
```

Q1 documents this pattern: a custom template can read **any frontmatter
field at any depth**, including arbitrary author-defined keys, by
walking `item.<dotted.path>`. The `listItemFromFile` hydration
deliberately does `{ ...documentMeta, ...standardFields }`, so every
frontmatter field is in scope.

Three things make this incompatible with a curated `DocumentProfile`:

1. **Open-world metadata.** `DocumentProfile` is a closed shape. We
   cannot enumerate ahead of time which frontmatter keys a custom
   template will read.
2. **Read-only by construction.** Even if we wanted to add a generic
   `extra: HashMap<String, ConfigValue>` field, the profile's whole
   selling point is its narrow, versioned, serializable contract.
   Adding an open-world bag would dilute that.
3. **No EJS engine in Q2.** The 2025-12-20 EJS analysis
   (`claude-notes/plans/2025-12-20-ejs-usage-analysis.md`) lays out
   the four candidates: (a) static / hardcoded, (b) Tera, (c)
   Pandoc-doctemplate-style ($var$), (d) embedded QuickJS. We have
   `quarto-doctemplate` in (c) and it's used for the Pandoc
   document-template path; we have nothing in (a) / (b) / (d).

The resulting design tension: do we let users reach into arbitrary
frontmatter from listing templates, or do we lock the templating
language to the curated profile fields?

#### Option C1 — Defer custom templates, ship the three built-ins

Most of Q1's listing usage in the wild is `default`, `grid`, or
`table`. The `custom` slot is the long-tail. Phase 1 of Q2 listings
could ship the three built-in types *only*, written against the
profile contract. The built-in templates use only the standard fields
already in `DocumentProfile` (post-extension: word-count, reading-
time, date-modified). No EJS engine needed; we can compile the
built-ins down to Rust functions or to the existing
`quarto-doctemplate` syntax.

This is the simplest path and matches the conservative shape of every
prior website-epic phase. **Recommended starting point.**

#### Option C2 — Custom templates with a curated extra-field surface

Add to `DocumentProfile`:

```rust
/// Free-form frontmatter passthrough for listing custom templates.
/// Author-declared at the project level via
/// `website.listing-fields: [tags, status, ...]`. Only declared fields
/// are snapshot. Stable, versioned, but the *list of keys* is
/// configuration rather than schema.
#[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
pub listing_fields: BTreeMap<String, ConfigValue>,
```

Custom templates would read those declared fields by name. Anything
not declared is invisible. This keeps the profile closed-shape (we
know exactly what's in it) while letting authors opt into the
fields a particular listing template needs. Compatible with a
restricted EJS-like template engine *or* with `quarto-doctemplate`
syntax.

Trade-off: requires authors to declare fields; not 100% Q1-compatible.

#### Option C3 — Side-channel: parallel raw-frontmatter index

Store, parallel to `ProjectIndex`, a `RawFrontmatterIndex` keyed by
project-relative path holding parsed YAML for each input file.
Loaded lazily from cache on Pass-2 listing resolution. Custom
templates run against that index, not against `DocumentProfile`.

Trade-offs:
- Re-introduces an "unprincipled" data channel that the profile
  contract was specifically created to replace; risk of feature creep
  ("oh, can the navbar read it too?").
- Doubles the cache-invalidation surface (frontmatter changes that
  don't affect the profile must still invalidate the raw-frontmatter
  cache for that file).
- 100% Q1-compatible: authors can use any frontmatter field they
  declare without further configuration.

If we end up needing this, the right way to introduce it is *as a
narrow side-channel scoped to listings*, not as a general profile
extension. Mirror Phase 8's `IncludeEntry` precedent: a separate
field added to the profile JSON, populated by a dedicated stage.

#### Option C4 — Embed QuickJS, ship Q1-verbatim EJS

The 2025-12-20 analysis covers this path. ~1–2 MB binary cost, full
template compatibility with Q1 templates as written. Pulls in a JS
runtime alongside the Lua runtime we already have. Significant scope
expansion.

I do not think we should do this for listings alone. If we ever add
QuickJS for some other reason (e.g. observable-style runtime), then
listings can ride along. Otherwise the cost-benefit is poor.

#### Option C5 — Named listing-item info object on the profile (user-proposed, 2026-05-05)

The user proposed a fifth shape that I think threads the needle
better than C1–C4. Recording it here as the working baseline.

**Shape.** Add a *single, named, scoped* field to `DocumentProfile`:

```rust
pub struct DocumentProfile {
    // … existing fields …

    /// Information advertised by this document for listings that
    /// include it. Author-declarable in frontmatter; auto-filled by
    /// `ListingItemInfoStage` before the checkpoint for fields the
    /// author left unset. The single feature-scoped surface where
    /// listings consumers reach for per-document data.
    #[serde(default, skip_serializing_if = "ListingItemInfo::is_empty")]
    pub listing_item: ListingItemInfo,
}

pub struct ListingItemInfo {
    /// Override for the title displayed in listings. Defaults to
    /// `profile.title` when unset.
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub description: Option<String>,
    pub image: Option<String>,
    pub image_alt: Option<String>,
    pub date: Option<String>,
    pub date_modified: Option<String>,
    pub categories: Vec<String>,
    pub reading_time_minutes: Option<u32>,
    pub word_count: Option<u32>,

    /// Free-form fields a custom listing template will consume.
    /// Author-declared in `listing-item.extra` (or wherever the
    /// schema lands). Outer profile shape does not change when keys
    /// are added/removed here, so no `profile_version` bump.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, ConfigValue>,
}
```

**Authoring surface.** A document advertises listing info by writing
a top-level frontmatter key:

```yaml
---
title: My post
listing-item:
  reading-time: "15 minutes"   # author override; auto-fill skipped
  extra:
    status: "draft"             # custom field for a custom template
    sponsors: [Foo, Bar]
---
```

**Population pipeline (mirrors sidebar generate/render).**

1. `ListingItemInfoStage` runs *before* the profile checkpoint. For
   each standard `ListingItemInfo` field that's `None`/empty, fill
   it: reading-time + word-count from the AST, date-modified from
   mtime, image from the first image in the body, etc. Author-supplied
   values win — the stage only fills holes.
2. `DocumentProfileStage` extracts `listing_item` like any other
   profile field — it's already on the AST's metadata at that
   point.
3. Listings consumers read `profile.listing_item` from
   `ProjectIndex`. Custom templates have access to the full struct
   *including* `extra`.

**Why this is better than C2/C3:**

- **vs. C2 (open `listing_fields: BTreeMap` directly on the
  profile).** C2's hole is at the top of the profile, naked, with no
  scope. Once it exists, the navbar code can read it, the sidebar
  code can read it, anything can read it. C5 scopes the hole inside
  a named feature surface (`listing_item`). The contract can say:
  "this field exists for listings consumers; non-listings consumers
  must use the typed top-level fields." The discipline is
  enforceable in code review because the feature name is right
  there in the field name.
- **vs. C3 (parallel raw-frontmatter index).** C3 introduces a
  second data plane with its own cache invalidation. C5 stays inside
  the profile, so Phase 8's cache machinery covers it for free —
  one cache key, one version bump.
- **vs. C1 (built-ins only).** C1 punts custom templates entirely.
  C5 makes custom templates *possible from day one*, with a clean
  contract: "whatever you put in `listing-item.extra`, the custom
  template can read." Unblocks the long-tail Q1 use case without
  an EJS engine.

**Why this lines up with prior design discipline.**

- **It uses the generate/render decomposition** that sidebars,
  navbar, footer, and page-nav already use. `ListingItemInfoStage`
  is the "generate" half (auto-fills holes); listings consume a
  resolved struct.
- **Author-supplied values win.** Same affordance Q1 had via
  "spread the document metadata" but *narrower and named*, and the
  frontmatter key documents itself.
- **The `extra` bag's versioning is clean.** `extra` is
  `#[serde(default, skip_serializing_if = ...)]`. When an author
  adds a new key, the outer profile shape does not change.
  Downstream consumers (custom templates) opt in to specific keys.
  Same discipline the existing v2 collection-fields use.
- **Lua-filter influence is forward-compatible.** Today filters can
  only contribute to the profile via the document's own frontmatter
  or the auto-fill stage. If/when we add a pre-checkpoint filter
  position, it gets a write channel into `listing_item`
  specifically — not into the rest of the profile. The named
  surface is a natural extension point.

**Trade-offs / open questions for C5:**

- The `extra` bag *is* an open-shape escape hatch. C5's argument is
  that scoping it inside `listing_item` makes the discipline
  enforceable; the alternative argument is that any open-shape
  field on the profile is a slippery slope regardless of name.
  Worth a contract-doc paragraph specifically forbidding non-
  listing consumers from reaching into `listing_item.extra`.
- Auto-fill ordering matters. Reading-time/word-count auto-fill must
  read the *post-include-expansion* AST, which means
  `ListingItemInfoStage` runs after `IncludeExpansionStage` but
  before `DocumentProfileStage`. The pipeline already has that slot
  available.
- "Auto-fill the listing-item *image* from the first image in the
  body" is the kind of small-value-but-non-trivial heuristic Q1 has
  in scattered places. Decide explicitly whether to do it
  (matches Q1) or skip it for v1.
- Schema naming. `listing-item` is a fine working name; final
  YAML key choice (top-level vs. namespaced under `website:` vs.
  under `meta.project.*`) coordinates with the epic-wide nav-config
  placement follow-up `bd-n9dr`.

#### Recommendation on custom templates

**C5 is the working baseline.** It gives us:
- Custom templates from day one, no EJS engine, no second data
  plane.
- A named, scoped surface that documents itself in code and YAML.
- A clean migration path for Lua-filter influence later.

C1 (built-ins only) becomes the *bring-up phase* of C5: ship the
auto-fill stage and the `listing_item` profile field, but only wire
the three built-in templates as consumers; custom templates land in
a follow-up phase once the field is solid.

C2 / C3 / C4 stay on the table as fallbacks if C5 hits a wall during
implementation, but the prior design discipline (closed contract,
generate/render decomposition, named feature surfaces) lines up
behind C5.

## The "rendered content" problem (description preview, preview image)

Q1's `completeListingItems` (third-pass placeholder substitution)
needs sibling output HTML to exist. That isn't compatible with running
inside Pass-2. In Q2 the natural home is the website project type's
`post_render` hook, which Phase 7 already wires up.

Concrete shape:

1. The Pass-2 listing transform emits placeholder comments using the
   same convention as Q1
   (`<!-- desc(...)[…]:relative/path.html -->`,
   `<!-- img(...)[…]:id:relative/path.html -->`). Fine to reuse the
   exact regex format — no reason to rev it.
2. `WebsiteProjectType::post_render` gains a
   `substitute_listing_placeholders(outputs: &[ProjectOutputFile])`
   step. For each output file, parse out the placeholders, open each
   referenced sibling output file, extract `firstPara` and
   preview image (Q1's `readRenderedContents` is straightforward to
   port — it operates on rendered HTML using a lightweight DOM read,
   which we can do with the `kuchikiki` / `scraper` family of
   crates, or by string scanning if we keep the DOM contract small).
3. The substitution is purely textual (placeholder → HTML fragment).
   No re-render of the host page is needed.

Phase 7's existing `post_render` composition is the template; the new
step slots in alongside `flush_site_libs`, `copy_favicon`,
`write_sitemap`, `write_robots_txt`.

## Incremental rebuild story

Phase 8's dependency graph is the right substrate. A listing host
page declares (implicitly, via its `contents:` globs) a dependency
on every project file the globs match. The Phase-8 dep graph already
supports adding edges from a page to a set of targets; today,
`body_link_targets` and `nav_dependencies` are the two automatic
edge sources, and sidebar co-membership is the third. Add a fourth:
**listing-content edges**.

Concretely:

- Pass-1 already runs an arbitrary "compute things from each
  document's profile" hook. The listing schema is in the host
  page's frontmatter, so the host's profile can record
  `listing_globs: Vec<String>`.
- The dependency graph builder, given the full `ProjectIndex`,
  expands those globs against the project's source paths and adds
  forward edges from the host to each match.
- Mode B (subset render) already pulls in dependents of a target;
  any listing host whose `contents:` matches a touched file is
  automatically pulled into the render set. This *replaces* Q1's
  `listingSupplementalFiles` exactly.

The `body_link_targets` precedent suggests adding a
`listing_content_targets: Vec<PathBuf>` field to the profile (resolved
at the dependency-graph-build phase, the way `body_link_targets` is
resolved during Pass-1's link resolution stage). v3 profile.

## Categories sidebar, RSS feeds, listing index

Three smaller pieces, none architecturally novel given the above:

- **Categories sidebar** — Q1 builds it from the same item set in a
  per-listing post-render step. In Q2 it's a Pass-2 markdown
  contribution into the host page's right margin (the existing
  template slot). No new machinery.
- **RSS feed** — Per Q1, one feed file per listing host page, written
  in `post_render`, gated on `website.site-url` (same as sitemap).
  Reuses `readRenderedContents` for the `full` / `partial` content
  variants. Adds a `feed.xml` artifact alongside `sitemap.xml`,
  `robots.txt`. Same lifecycle as Phase 7's other post-render
  outputs.
- **Listing index (`listings.json`)** — Q1 writes a global
  `listings.json` that the search infrastructure consumes. Search is
  out of scope until that epic happens; we can punt this until
  search lands and decide then whether to keep the pattern.

## Open questions / risks

1. **Where do listings declare their schema in the project tree?**
   Q1 puts the listing config in the host page's frontmatter. That
   stays the same in Q2. The dep-graph integration just means the
   profile of a listing host page has a non-empty
   `listing_content_targets`.
2. **Globbing semantics.** Q1's `filterPaths` + `globToRegExp` from
   Deno's path module. We'd want a deterministic, project-relative
   glob expander; there's likely already one in pampa or `quarto-core`
   for `_quarto.yml` `project.render`. Verify before committing.
3. **Reading-time / word-count cost on Pass-1.** Cheap (~linear in
   AST size); already done during render. Adding it to the profile
   means doing it once per profile build. Acceptable.
4. **Custom template path: do we punt or design now?** Recommendation
   above: punt. But the *schema* should not bake in the assumption
   that custom is impossible — keep `template:` in the YAML schema
   even in v1, and emit a "custom listing templates not yet
   supported" diagnostic when set, rather than rejecting the YAML.
   This keeps Q1 documents merely warning rather than failing parse.
5. **Image preview extraction without a DOM.** Q1 uses deno-dom.
   pampa already has source-tracked HTML output; we can probably
   read rendered HTML files with `tl` / `scraper` / lightweight regex
   scanning depending on how complex the preview-image discovery
   logic ends up.
6. **Drafts.** Q1 has a `draft-mode: visible | unlinked | gone`
   surface that listings consult. Q2's `DocumentProfile.draft` field
   exists; the listings resolver consumes it the same way the
   sidebar does. The website epic has open follow-ups for draft mode
   (`bd-p4sc`, `bd-1hdz`) that listings will inherit — no new design
   needed.
7. **Multi-format.** Listings are HTML-only in Q1
   (`formats: [$html-doc]`). v1 keeps that constraint. The profile's
   `format_id` is enough to gate.
8. **Where does the listings code live?** A new
   `crates/quarto-core/src/project/listing/` mod, parallel to
   `website_post_render`. Or possibly its own crate
   (`crates/quarto-listings/`) if it grows large; defer until we see
   the implementation size.

## Strawman phasing (when we file beads issues)

Not committing to any of this yet — listing here so the discussion
has something concrete to push back on.

- **L0 — Profile extension.** Add `word_count`, `reading_time_minutes`,
  `date_modified`, `file_modified`, `listing_content_targets` to
  `DocumentProfile`. Bump v2 → v3. Update contract doc. Pure
  extension, no behavior change for non-listing pages.
- **L1 — Schema + data model.** Port Q1's `Listing`, `ListingItem`,
  `ListingDescriptor` types. Schema in `quarto-yaml-validation`.
  No rendering yet.
- **L2 — Pass-2 listing resolver.** A new transform that, given the
  host page's listing config + `ProjectIndex`, materializes
  `Vec<ListingItem>` and emits markdown for the `default` / `grid` /
  `table` templates (Rust-side; no EJS). Built-in templates only.
- **L3 — Categories sidebar.** Margin-sidebar contribution from the
  same item set.
- **L4 — Post-render placeholder substitution.** New step in
  `WebsiteProjectType::post_render`. Description preview + preview
  image extraction. Reuses Q1's `readRenderedContents` logic, ported.
- **L5 — Dependency graph integration.** Edges from listing host →
  every matched content file. Mode B picks up listing hosts when any
  of their content files is touched.
- **L6 — RSS feeds.** `post_render` step. Feed config in host
  frontmatter; one feed per listing.
- **L7 — Custom templates (separate epic).** Pick C2 or C3 from
  §"The custom template problem" based on demand. Possibly never;
  the three built-ins cover most of the wild.

L0 is a precondition for L2; L1–L4 can be developed in sequence;
L5 depends on L1's data model. L6 depends on L4. L7 is pure
follow-up.

## What this document is *not*

- An implementation plan. No file paths, no test strategy, no commit
  sketches.
- A commitment to ship listings in a particular release.
- A claim that the C1/C2/C3/C4 trade-offs are settled. I'm
  recommending C1 first because it's the conservative move and lines
  up with the website-epic precedent, but the user's view on the
  custom-template story may shift the answer.

## Why isn't full metadata already on `DocumentProfile`?

Asked during this session: is the closed-shape profile a technical
constraint, or just a "we didn't need it yet" outcome? Investigated
the source and the Phase-0 paper trail. **The narrowness is a
deliberate design choice, not a technical limitation.** Three
findings.

### Finding 1 — the parent epic plan literally proposed `Option<ConfigValue>`

`claude-notes/plans/2026-04-23-website-project-epic.md` line 182 has
the original first-cut profile sketch with `title: Option<ConfigValue>`
("markdown-rich, following Q2 meta interpretation"). The plan was
written knowing `ConfigValue` is the open-shape merged-metadata type
that already lives in `quarto-pandoc-types`. Choosing flat
`Option<String>` for `title` (and equivalent flattening for every
other field) happened during Phase 0 implementation review, not
because of a missing capability. The Pandoc `meta` is sitting right
there at the checkpoint — `DocumentProfile::extract` takes
`ast: &Pandoc` and reads `&ast.meta` to populate each field
(`crates/quarto-core/src/document_profile.rs:299`). Storing the whole
thing would have been *less* code, not more.

### Finding 2 — the rationale is encoded in the contract's "Non-guarantees" and "Mutability" sections

`claude-notes/designs/document-profile-contract.md` is explicit about
what a profile is *for*: a stable, versioned, serializable, read-only
contract that downstream features (sidebars, cross-doc links, Phase-8
cache, eventual `freeze`) can depend on without coupling to whatever
shape the merged-metadata blob happens to have today. The reasons,
in the contract's own framing:

1. **Caching and versioning.** Profile JSON gets cached on disk
   (Phase 8) and round-tripped between WASM and native.
   `DOCUMENT_PROFILE_VERSION` bumps when the serialized shape changes
   in a way a v1 consumer would misread. A field of arbitrary user
   YAML defeats versioning: any frontmatter key change in any
   document is, transitively, a profile-shape change. The cache key
   would have to invalidate on *any* metadata edit, even ones no
   consumer cares about.
2. **Read-only by construction.** The contract says "profiles are
   read-only" specifically so user filters running later in the
   pipeline can read `&[DocumentProfile]` without being able to
   undermine cross-document invariants. An open metadata bag would
   tempt feature authors to write through it ("the listings
   transform stuffs computed fields back into the profile so the
   sidebar transform can read them") and the contract collapses.
3. **Stable cross-format meaning.** `title`, `authors`, `categories`
   each have a defined plain-text projection in the contract.
   Whether the source frontmatter wrote `author: jane` or
   `authors: [{name: jane}]` or `author: { family: doe, given: jane }`,
   downstream code sees a `Vec<String>`. If consumers reach into
   `meta.author` directly, every consumer reimplements that
   normalization and gets a slightly different answer.
4. **Engine-output exclusion.** The contract draws a hard line:
   nothing engine-produced, nothing sugar-produced, nothing
   filter-mutated, nothing shortcode-resolved is in the profile.
   That line is enforced *by what fields exist*. A `meta:
   ConfigValue` field would be re-checking that line at every
   consumer site instead of once at extraction time.

### Finding 3 — the contract has an explicit "what to do when you need more" rule

The "Mutability" section addresses precisely the listings-style use
case: *"If a profile 'should' reflect some piece of state that can
only be computed after the checkpoint today, the fix is to move the
producing logic earlier in the pipeline — not to back-patch the
profile."* That is, the contract anticipates that consumers will want
fields the profile doesn't currently carry, and the prescribed
remedy is **add a typed field with a `profile_version` bump**, not
add a generic-bag escape hatch.

Phase 8 (`bd-fegm` / `bd-r82e`) is the working precedent. When that
phase needed include-set tracking, body-link targets, nav-dependency
declarations, and an `always_render` flag, the answer was four new
typed fields with `DOCUMENT_PROFILE_VERSION` bumped 1 → 2 — not "add
a `HashMap<String, ConfigValue>` for filter-defined keys." The same
discipline applied to listings would mean: add `word_count`,
`reading_time_minutes`, `date_modified`, `file_modified`,
`listing_content_targets`. v3 bump.

### Implication for the custom-template story

This sharpens the trade-off in §"The custom template problem":

- **Option C2 (declared listing fields, `BTreeMap<String, ConfigValue>`
  on the profile)** is more honest about what it is — a deliberate
  hole punched through the closed-shape contract for one specific
  feature. The contract would document the hole: "this map is
  populated only with author-declared keys from
  `website.listing-fields`; all other consumers must use the typed
  fields."
- **Option C3 (parallel `RawFrontmatterIndex`)** keeps the contract
  intact and isolates the open-shape exception to a single
  feature. The cost is a second cache layer with its own
  invalidation logic.
- **Option C1 (built-ins only)** sidesteps the trade-off entirely
  for v1.

Re-reading the Phase-0 paper trail with this question in mind, my
mild preference shifts from C2 → C3. **C2 looks like a small change
— add a map field, default empty, only populated when configured —
but it sets the precedent that "anything important enough goes on
the profile via a generic bag."** Once that pattern exists, the
sidebar-resolution code, the navbar-active-item code, and every
future feature has an "easy" path that erodes the contract. C3's
ugliness is local; C2's ugliness compounds.

This is exactly the kind of redesign question the discussion
document is meant to surface. The user may legitimately decide the
contract is too strict — but if so, that decision deserves to be
made deliberately at the contract doc, not inherited via a listings
implementation choice.

## Custom listings via `quarto-doctemplate` — feasibility study (2026-05-05)

User asked: assuming the C5 design (named `listing_item` profile
field with `extra: BTreeMap<String, ConfigValue>`), can custom
listings be implemented using `quarto-doctemplate`'s Pandoc-style
`$var$` syntax instead of EJS? This would avoid embedding a JS
runtime and keeps hub-client safe to render listings in a browser
context without sandbox concerns.

**Verdict: yes, this is viable for the *custom-listing* surface, and
it is in fact the cleanest of all the C-series options.** It does
*not* eliminate the need to port the three built-in templates
(default / grid / table) to a Q2-native renderer, but those are
straightforward independent of templating choice.

### What `quarto-doctemplate` actually offers (verified)

Read of `crates/quarto-doctemplate/src/{ast.rs,context.rs,evaluator.rs}`:

- **Variable interpolation** with dotted paths: `$item.title$`,
  `$item.extra.status$`. The evaluator splits on `.` and walks
  `TemplateValue::Map`, so the same dotted-path access pattern Q1
  uses (`item.foo.bar`) works.
- **Conditionals**: `$if(item.image)$ … $else$ … $endif$`.
  Truthiness rules: empty string falsy, non-empty maps truthy,
  arrays containing any truthy element truthy. This matches what
  Q1's templates rely on.
- **For loops**: `$for(items)$ … $sep$, $endfor$`. Loop body has
  the iteration variable bound to its last path component *and* to
  `it`. This is exactly what `for (const item of items)` provides
  in Q1's EJS.
- **Partials**: `$partial("item-default")$` (bare, current
  context) or `$item:partial("item-default")$` (applied, item
  becomes the partial's context). Q1's listings rely on
  `partial('item-default.ejs.md', { listing, item, utils })` —
  applied partials map cleanly onto `$item:partial(...)$`.
- **Array-with-separator interpolation**: `$item.categories[, ]$`
  joins a list with a literal. Q1 has `item.categories.join(", ")`
  in scattered places — this covers it.
- **Pipes** are parsed (`$var/uppercase$`, `$var/left 20 "" ""$`)
  but **not yet implemented in the evaluator** — the partial-pipe
  and variable-pipe paths both have `// TODO: Apply pipes`. For
  listings we'd want at least `escape` (for HTML/RSS), `first` /
  `rest` if we want fancy item layouts, and a date-format pipe.
  None are blockers; they're additive.
- **`TemplateValue` is constructible from `ConfigValue`**. The
  doc says conversion is in the writer layer; for listings we'd
  build a `TemplateValue::Map` directly from the
  `Vec<DocumentProfile>` (filtered/sorted) and the listing config.

### What Q1 EJS templates need that doctemplate lacks

I read all five user-facing listing templates (`listing-default`,
`listing-table`, `listing-grid`, `item-default`, `item-grid`) plus
the helpers (`_filter`, `_pagination`, `feed/item`). Three classes of
divergence:

1. **Helper utilities (`listing.utilities.*`).** Q1 templates call
   `listing.utilities.img(...)`, `listing.utilities.outputLink(...)`,
   `listing.utilities.metadataAttrs(...)`, `localizedString(...)`,
   `sortableFieldData()`, `b64encode(...)`. These are plain string-
   building functions that happen to live JS-side. **In Q2 we don't
   expose them as template functions — we pre-compute them in Rust
   when we build the `TemplateValue::Map`.** I.e., the per-item map
   for a custom template includes pre-rendered fields like
   `item.image_html` (already wrapped in `<img …>`) and
   `item.metadata_attrs` (already a string of `data-…` attributes),
   so the template author writes `$item.image_html$` instead of
   calling a function. This is the same shape Pandoc's writer uses
   for things like `$body$` — pre-rendered markup injected into a
   slot.
2. **Field projection (`fields.includes('foo')`, `showField(...)`).**
   In Q1 the default templates check whether a given field should be
   rendered. doctemplate's truthy-conditional handles the simple
   case (`$if(item.title)$ … $endif$`), and for "is this field in
   the user's `fields:` list?" we project the boolean into a sibling
   field at build time: `item.show.title = true|false`. Then the
   template writes `$if(item.show.title)$`. Slight verbosity tax
   versus Q1's `<% if (showField('title')) %>` but no expressivity
   loss.
3. **JS expressions in attributes.** Q1's grid/table templates have
   inline JS expressions like
   `<div onclick="window.quartoListingCategory('<%= utils.b64encode(category) %>')">`.
   These are *output strings*, not control flow — the template
   embeds a JS runtime call into the generated HTML. doctemplate
   handles this fine by emitting the same string; we just pre-
   compute the b64-encoded category server-side and bind it as
   `category.b64`. No JS-in-template.

There is no construct in any Q1 listing template I read that
*requires* JS evaluation at template-render time. Every JS
expression I found is either control flow (handled by `$if$` /
`$for$`) or string formatting (handled by pre-computation).

### What the listing data binding looks like

For a custom listing, the rendered template context would be:

```
TemplateValue::Map({
  "listing": Map({
    "id":           "my-listing",
    "type":         "custom",
    "fields":       List([String("title"), String("date"), …]),
    "show":         Map({"title": Bool(true), "date": Bool(true), …}),
    "template_params": Map(<author-supplied custom params>),
    // … other knobs
  }),
  "items": List([
    Map({
      // Pulled from DocumentProfile (curated):
      "title":          String("My post"),
      "subtitle":       String("…"),
      "description":    String("…"),
      "date":           String("2026-04-01"),       // formatted
      "author":         String("Jane Doe, John Roe"), // joined
      "authors":        List([…]),                  // raw list
      "categories":     List([…]),
      "image":          String("img.png"),
      "path":           String("/posts/foo.qmd"),
      "outputHref":     String("posts/foo.html"),
      "reading_time":   String("15 min"),
      "word_count":     String("373"),

      // Pulled from listing_item.extra (open):
      "extra": Map({
        "status":    String("draft"),
        "sponsors":  List([…]),
        // anything the author put in `listing-item.extra`
      }),

      // Server-pre-rendered helpers (replace Q1 utilities):
      "image_html":      String("<img src=\"…\" …>"),
      "metadata_attrs":  String("data-index='0' data-categories='…'"),
    }),
    // … one map per item
  ]),
})
```

A template author writes (illustrative, not meant to ship):

```
::: {.list .quarto-listing-default}
$for(items)$
::: {.quarto-post $it.metadata_attrs$}
$it.image_html$
$if(it.show.title)$
### [$it.title$]($it.path$)
$endif$
$if(it.extra.status)$
[Status: $it.extra.status$]
$endif$
:::
$endfor$
:::
```

This is markdown-mode (because the doctemplate output is markdown
that the listing host page splices into its own pipeline), with
inline `$var$` interpolation. The rendered output is markdown that
goes through the same engine as the host page, so all of Q2's normal
markdown features work inside listing items.

### Pieces still needed

1. **Pipe implementations in the evaluator.** Today
   `apply pipes` is a TODO in two places. For listings v1 we'd want
   `escape` (HTML escape for table cells / titles), `escape_xml`
   (RSS feeds), and a `date_format` that takes a format string. These
   are small; the parser already understands the syntax.
2. **A `ConfigValue → TemplateValue` bridge.** The doctemplate
   crate's docstring says this conversion lives "in the writer
   layer." For listings we need it on the *project* side. Likely
   shape: a `From<&ConfigValue> for TemplateValue` or a
   `to_template_value()` method on `ConfigValue`. One-pass walk over
   `ConfigValue::{Scalar, Sequence, Mapping, …}`.
3. **A new partial-resolver flavor**: today
   `quarto-doctemplate::resolver` resolves partials from the
   filesystem (`FileSystemResolver`) or memory (`MemoryResolver`).
   Custom listings need a resolver scoped to the project: the
   author's `template:` is a path relative to the listing host
   page, and any partials it references are likewise project-local.
   The existing `ChainedResolver` should be enough; we just need
   to wire the project root + host-page directory in.
4. **The three built-ins.** `default` / `grid` / `table` need to be
   rewritten as doctemplate templates (or as Rust-side renderers).
   I'd argue **doctemplate** for all three: it makes the built-ins
   editable and the "this is exactly the surface custom templates
   get" property of the design becomes self-evident — the built-ins
   *are* custom templates, just shipped in the binary via
   `MemoryResolver`.

### Why this is strictly better than EJS

- **No JS runtime.** No QuickJS, no rquickjs, no ~1–2 MB binary
  cost. Same renderer drives native and WASM.
- **Hub-client safety.** A doctemplate template cannot execute
  arbitrary code; it can only interpolate values, branch on
  truthiness, loop, and apply named pipes from a fixed allowlist.
  An author who downloads a third-party listing template into a
  hub-client project cannot run JS as a side effect. With EJS,
  `<% any_expression %>` is full JavaScript — a malicious template
  shared as part of a quarto-extension would be a code-execution
  vector in the browser. doctemplate forecloses that entire class
  of risk by construction.
- **One templating language to learn.** Authors already see `$var$`
  in Pandoc partials and the HTML wrapper template; reusing it for
  listings means there's one template syntax in the Q2 surface,
  not two.
- **Source-tracked diagnostics.** doctemplate already emits
  `SourceInfo`-attributed errors via the diagnostic surface. EJS
  errors would be JS stack traces from QuickJS, which we'd have to
  translate. doctemplate just plugs into the existing
  `quarto-error-reporting` machinery.

### What this means for the C-series options

Option **C5 + doctemplate** is now my unconditional recommendation:

- **Built-ins**: ship as doctemplate templates resolved through
  `MemoryResolver` (embedded at compile time via `include_str!`).
- **Custom**: author writes `template: my-listing.ejs.md` (we'd
  rename the convention to drop `.ejs.md`; maybe `.qmd-template`
  or `.q2-template`); resolved through `FileSystemResolver` rooted
  at the host page. The template gets the data binding sketched
  above, with `item.extra` carrying anything the documents
  advertised in `listing-item.extra`.
- **EJS interop story for Q1 → Q2 migration**: not seamless. Q1
  custom templates are JavaScript and don't port automatically.
  We'd document the migration: "rename `<%= item.foo %>` to
  `$it.foo$`, `<% if (cond) { %>` to `$if(cond)$`, `<% for … %>`
  to `$for(…)$`." Most templates are short enough that this is
  manual but tractable. Templates that lean on inline JS computation
  (rare) need to either pre-compute server-side via a custom listing
  field or simplify.

### Unresolved sub-questions

1. **Pipe set.** Need to enumerate the minimum viable pipe set for
   listings (and for built-ins) before committing. Likely: `escape`,
   `escape_xml`, `date_format <fmt>`, `first`, `rest`. Worth a
   small sub-design.
2. **Per-item markdown vs HTML.** Q1 templates produce markdown
   that goes back through the document's pipeline (so `**bold**`
   in a description renders as `<strong>`). doctemplate's output
   is just text; it doesn't care whether the surrounding context is
   markdown or HTML. For Q2 we'd keep the same shape (markdown
   output, host-page pipeline picks up).
3. **`max-description-length` truncation, `description` rendering
   from sibling output files.** These are *not* template-side
   concerns; they happen in the listing-resolve stage (truncation)
   and the post-render placeholder substitution (sibling content
   read). Same architecture as before.
4. **Schema migration for `template: foo.ejs.md`.** Drop the
   `.ejs.md` suffix in the schema, but accept it for back-compat
   with a deprecation warning during parse.

## Discussion seeds

Things I would specifically like to push on:

- Does the "post_render step substitutes placeholders" plan worry
  you? It's a third pass in all but name. Q1 has the same shape and
  it works fine, but it does mean the cache-invalidation story for
  listings is genuinely different from other pages — touching a
  *content* file invalidates the listing host's *post-render* output
  even when the host's profile and Pass-2 output are unchanged. Phase
  8's cache layer is keyed at the Pass-2 boundary; we'd need a
  separate "post-render-output stale" predicate. Not hard, but worth
  surfacing.
- Is L7 acceptable as a long-defer? If users coming from Q1 expect
  custom templates to "just work", we may need to surface a louder
  diagnostic and a clearer migration story than I've sketched.
- Is the `BTreeMap<String, ConfigValue>` "declared listing fields"
  surface (Option C2) the right shape, or would you rather keep
  custom templates entirely off the profile and put them in a
  side-channel (Option C3)? My mild preference is C2 for the same
  reason I prefer the closed profile contract everywhere else, but
  C3 has the advantage of *not* growing the profile.
