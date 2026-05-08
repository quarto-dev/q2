# Listings — implementation epic

**Date:** 2026-05-05
**Status:** Filed. Epic `bd-61cd`. Sub-issues filed; ids inline in
each phase header below.

**bd issue mapping and sub-plan files:**

Per CLAUDE.md, each phase gets its own `claude-notes/plans/YYYY-MM-DD-*.md`
sub-plan **before** implementation begins. None of the per-phase
sub-plans exist yet — they will be authored at the start of each
phase's work session, resolving the open questions listed in this
epic plan's §"Open questions". The dates in the filenames below are
placeholders; the real date is the day the sub-plan is written.

The epic-level open questions (custom-template extension naming,
image-fallback heuristic, `list.min.js` defer, HTML-parser choice,
schema placement, stage-file location) live in this epic plan,
under §"Open questions (to resolve in sub-plans, not now)" — not
in any per-phase sub-plan yet.

This document — `claude-notes/plans/2026-05-05-listings-epic.md` —
is the parent plan that every sub-plan will reference. The
companion design rationale is
`claude-notes/plans/2026-05-05-listings-design-discussion.md`.

| Phase | bd id     | Type    | Title                                                                  | Sub-plan file                                        | Status              |
|-------|-----------|---------|-------------------------------------------------------------------------|------------------------------------------------------|---------------------|
| Epic  | `bd-61cd` | epic    | Listings feature epic                                                  | `2026-05-05-listings-epic.md` (this file)            | Filed               |
| —     | —         | —       | Design discussion (rationale)                                           | `2026-05-05-listings-design-discussion.md`           | Filed               |
| L0    | `bd-n8a4` | task    | ListingItemInfo profile extension                                      | `2026-05-05-listings-L0-profile-extension.md`        | **Closed** (impl `ab28ea00`, merge `57671f9b`) |
| L1    | `bd-izqh` | feature | ListingItemInfoStage (auto-fill, pre-checkpoint)                       | `2026-05-05-listings-L1-autofill-stage.md`           | **Closed** (impl `38749998`, merge `a56e0e91`) |
| L2    | `bd-j60g` | task    | Listing data model + YAML schema                                       | `2026-05-06-listings-L2-data-model.md`               | **Closed** (reference doc; types impl in L3, merge `b4f2238c`) |
| L3    | `bd-ml8z` | feature | ListingResolveTransform (Pass-2, built-ins via doctemplate)            | `2026-05-06-listings-L3-resolve-transform.md`        | **Closed** (impl `3b8dd645`…`ff23a2a2`, merge `b4f2238c`) |
| L4    | `bd-b5jm` | task    | quarto-doctemplate enhancements (pipes, ConfigValue bridge, resolver)  | `2026-05-06-listings-L3-resolve-transform.md` (bundled w/ L3) | **Closed** (impl `3b8dd645`, merge `b4f2238c`) |
| L5    | `bd-5vsr` | feature | Categories sidebar                                                     | `2026-05-06-listings-L5-categories-sidebar.md`       | **Closed** (impl `2750546b`+`67a985f4`, merge `9e8afa0d`) |
| L6    | `bd-xbnf` | task    | Dependency-graph integration (`listing_content_globs`)                 | `2026-05-07-listings-L6-dep-graph.md`                | **Closed** (impl `ffa4d227`, merge `8b5efb91`) |
| L7    | `bd-qf7r` | feature | Post-render placeholder upgrade (engine-rendered previews; BRACKETED)  | `2026-05-07-listings-L7-postrender-upgrade.md`       | **Closed** (impl `d4877142`, merge `dc3a0f7b`) |
| L8    | `bd-rqgx` | feature | Custom listing templates                                               | `2026-05-07-listings-L8-custom-templates.md`         | **Closed** (impl `92ca4c52`, merge `cd2410fa`) |
| L9    | `bd-o90m` | feature | RSS feeds                                                              | `2026-05-08-listings-L9-rss-feeds.md`                | **Closed** (impl `8b7a9286`…`0bdd219e` (7 phases), merge `f5475bb2`) |
| L10   | `bd-hzsi` | task    | Q1 → Q2 listing template migration docs + LLM skill                    | `YYYY-MM-DD-listings-L10-migration-docs.md`          | not yet written     |
| L11   | `bd-qb4o` | task    | Listings epic close-out                                                | `YYYY-MM-DD-listings-L11-close-out.md`               | not yet written     |
**Parent design discussion:**
`claude-notes/plans/2026-05-05-listings-design-discussion.md` —
contains the rationale for every architectural decision below.
**Parent epic (related, not parent-child):** website epic
`bd-0tr6` (closed). Listings was explicitly deferred from that epic.

## Settled decisions (from the discussion document)

These are inputs to this plan, not open questions:

1. **Item-info data model is the C5 design.** A new
   `DocumentProfile.listing_item: ListingItemInfo` field with curated
   typed sub-fields plus an `extra: BTreeMap<String, ConfigValue>`
   bag. Listings are the *only* feature allowed to consume
   `listing_item`; non-listing consumers must use the typed top-level
   profile fields. Discipline written into the contract doc.
2. **Auto-fill follows the generate/render decomposition.** A new
   pre-checkpoint stage (`ListingItemInfoStage`) auto-fills standard
   fields when the author hasn't supplied them. Author values always
   win.
3. **Templating is `quarto-doctemplate`, not EJS.** Pandoc-style
   `$var$` syntax for both built-ins and custom templates. No JS
   runtime. Hub-client safe. Pre-compute helper outputs server-side
   into the per-item map.
4. **Built-ins are doctemplate templates embedded via
   `MemoryResolver`.** The three built-ins (`default`, `grid`,
   `table`) ship as embedded templates so authors can read them as
   reference; "custom" is exactly the same surface with a different
   `PartialResolver`.
5. **Q1 → Q2 template migration is not seamless.** Documented + LLM
   skill assistance (user signed off on the trade-off).
6. **Cross-document mechanics reuse existing Q2 substrates:**
   `ProjectIndex` for item discovery, Phase-8 dependency graph for
   incremental rebuild edges, `WebsiteProjectType::post_render` for
   the placeholder-substitution step, sitemap-style emission for
   RSS feeds.
7. **`DocumentProfile` is now at v4** (after L0 / `bd-n8a4`).
   The original epic plan called for "v2 → v3"; in fact the
   field already sat at v3 (`bd-o8pr` / `resources`), so L0
   bumped to **v4**. L0 also added `categories_raw:
   Option<ConfigValue>` on both `DocumentProfile` and
   `DocumentProfile.listing_item` to preserve merge tags
   (decision D7 in the L0 sub-plan); listings consumers feed
   both into `MergedConfig` for tag-aware category merging
   (default `Concat`, override via `!prefer`). Future bumps in
   this epic target **v5+**.
8. **Placeholder substitution is bracketed as a website-only,
   CLI-only feature (decided 2026-05-05).** The
   description-preview-from-rendered-content and
   preview-image-from-rendered-content features (Q1's
   `completeListingItems`) require reading sibling pages' rendered
   HTML output. This means the feature *only* works in
   environments that complete a full project render, with engine
   execution, before the listing host page is finalized.
   Specifically:
   - **Available**: `quarto render` on a website project (CLI).
   - **Not available**: hub-client preview, future `quarto
     preview` (which is planned to be a local hub-client). These
     environments must not block on engine execution of sibling
     pages just to render a preview snippet.
   - **Safeguard requirement**: every listing item must have a
     usable fallback for `description` and `image` even when
     placeholder substitution does not run. Pass-1 auto-fill
     (L1) populates these defaults; the L7 step *upgrades* them
     when sibling rendered content is available, but the
     unupgraded form is always a valid, displayable listing.
   - **Discipline**: the placeholder mechanism is intentionally
     leak-resistant. It lives in one named post-render step, in
     one file, with a header comment explaining why it exists
     and why future features should not reach for the same
     pattern. See L7's "bracketing rules" subsection.

## What this epic delivers

- Q1-feature-parity listings on `default` / `grid` / `table`
  built-in types, against the C5 `listing_item` profile field.
- Custom listing templates via `quarto-doctemplate`.
- Categories sidebar (the right-margin category list Q1 emits when
  `categories: true` is set on a listing).
- RSS feed generation per listing host, gated on
  `website.site-url`.
- Description-preview and preview-image post-render placeholder
  substitution (Q1's `completeListingItems` equivalent).
- Phase-8 dependency-graph integration: editing a content file
  pulls its listing host(s) into Mode B's render set
  automatically.
- Schema definitions under `quarto-yaml-validation` matching Q1's
  `website-listing` shape, with custom-template support.

## What this epic does *not* deliver

- **Listing index (`listings.json`)** — Q1 emits this for the
  search infrastructure. Search itself is its own epic; this lands
  when search lands.
- **`field-required` validation diagnostics beyond a basic
  warning.** Q1 throws on missing required fields; v1 emits a
  diagnostic and drops the item. Stricter validation is a follow-up.
- (Removed from "not delivered" 2026-05-05.) `list.min.js`
  client-side sort/filter interactivity *is* in scope for L3
  per decision 3. See L3 for the artifact-store integration
  rules and the fallback path if integration proves invasive.
- **Q1-to-Q2 migration tooling.** Documentation + LLM skill, not
  an automated converter.
- **Multi-format listings.** HTML-only, mirroring Q1's
  `formats: [$html-doc]` constraint.
- **`feed.xml` styling via `xml-stylesheet`.** Listed in Q1
  schema; defer until someone asks.

## Architecture summary

Pipeline shape after this epic:

```
Pass 1 (per file):
  Parse → Merge → IncludeExpansion →
  ListingItemInfoStage    ← new, fills listing_item from frontmatter + AST
  → DocumentProfileStage   ← profile now carries listing_item
  → … rest of Pass 1 …

Pass 2 (per file, project context):
  PreEngineSugaring → Engine → ThemeCSS → UserFilters(pre) →
  AstTransformsStage:
    - existing transforms …
    - ListingResolveTransform   ← new; reads ProjectIndex, materializes items, emits markdown via doctemplate
    - CategoriesSidebarTransform ← new; right-margin category list
  → UserFilters(post) → CodeHighlight → RenderBody → ApplyTemplate

Project post-render:
  WebsiteProjectType::post_render:
    - flush_site_libs (existing)
    - copy_favicon (existing)
    - write_sitemap (existing)
    - write_robots_txt (existing)
    - substitute_listing_placeholders   ← new; firstPara + previewImage
    - write_rss_feeds                    ← new; per-listing-host feeds

Dependency graph (Phase 8):
  - body_link_targets (existing)
  - nav_dependencies (existing)
  - sidebar co-membership (existing)
  - listing_content_globs ← new edge source (`bd-xbnf`, L6)
```

The `ListingItemInfoStage` lives between `IncludeExpansionStage` and
`DocumentProfileStage` for the same reason `IncludeExpansion` does:
the AST it reads must be post-include but pre-sugar, and the profile
extracted at the checkpoint must reflect the auto-filled
`listing_item`.

## Phasing

Each phase is intended to land as a separate bd issue with its own
sub-plan. They are in dependency order; some can parallelize once
L1 and L2 are in.

### L0 — `ListingItemInfo` profile extension (foundation)

**bd type:** task. **Profile version:** 2 → 3.

Scope:
- Add `pub listing_item: ListingItemInfo` to `DocumentProfile`.
- New struct `ListingItemInfo` with the curated fields (`title`
  override, `subtitle`, `description`, `image`, `image_alt`,
  `date`, `date_modified`, `categories`, `reading_time_minutes`,
  `word_count`) plus `extra: BTreeMap<String, ConfigValue>`.
  All optional / default-empty; serializer skips empty.
- `is_empty()` helper for `serde(skip_serializing_if = ...)`.
- Read these fields in `DocumentProfile::extract` from
  `meta.listing-item` (frontmatter).
- Bump `DOCUMENT_PROFILE_VERSION` 2 → 3.
- Update `claude-notes/designs/document-profile-contract.md`:
  - New row in the field table.
  - **New §"Scoped feature surfaces"** documenting the
    `listing_item` discipline: only the listings feature reads
    this field; non-listing consumers must use top-level fields.
    Specifically forbid `listing_item.extra` reads outside listing
    code paths. This is the contract paragraph the user signed off
    on as worth writing down explicitly.
  - Change-log entry with the version bump.
- Update `quarto-yaml-validation` schema to recognize
  `listing-item:` at frontmatter top level.
- Tests: round-trip serialization, version-mismatch rejection,
  field extraction from frontmatter, default-empty behavior.

End-to-end verification: render a fixture `.qmd` that declares
`listing-item: { reading-time: "10 min" }` and assert the
extracted profile carries that value.

### L1 — `ListingItemInfoStage` (auto-fill)

**bd type:** feature. Depends on L0.

Scope:
- New stage at
  `crates/quarto-core/src/stage/stages/listing_item_info.rs`
  (decision 6 — matches sibling stage modules).
- Pipeline position: after `IncludeExpansionStage`, before
  `DocumentProfileStage`. Insert into both
  `build_html_pipeline_stages_with_apply_config` and
  `build_wasm_html_pipeline`.
- Behavior: for each unset standard field on
  `meta.listing-item`, compute and inject:
  - `reading-time-minutes`: word-count divided by 200wpm
    constant (matches Q1's `estimateReadingTimeMinutes`).
  - `word-count`: tokenize the post-include AST text content.
  - `date-modified`: from filesystem mtime if available; else
    skip.
  - `description`: **always populate a fallback** from the AST.
    First plain-text paragraph after include expansion,
    truncated to the configured `max-description-length`. This
    is the *safe baseline* the L7 post-render step may later
    upgrade with engine-rendered content; if L7 doesn't run
    (hub-client, `quarto preview`), this fallback is what the
    listing displays. Never `None` for a real document.
  - `image`: **always populate a fallback** from the AST. First
    `Image` node `src` from the body, in document order, after
    include expansion (decision 2 — confirmed). If no images are
    present in the static AST, leave `None` — the listing
    template will render its existing `image-placeholder`
    empty-div fallback. Same role as `description`: a safe
    baseline that L7 may later upgrade.
- **Safeguard contract**: after L1, a document is *always*
  presentable as a listing item without any post-render
  upgrade. L7 is purely an enhancement. The CLI/website-render
  environment gets engine-output previews; every other
  environment gets the static AST previews and renders
  correctly.
- Author values always win — the stage only fills holes.
- Stage must be idempotent so resume-from-AtProfile in tests
  doesn't double-fill.
- Tests: each field individually (auto-filled vs. author-supplied),
  the stage's idempotence, an integration test that reads the
  profile after the checkpoint and confirms `listing_item` is
  populated.

End-to-end: a fixture with no `listing-item:` key still has
`profile.listing_item.reading_time_minutes` set after Pass 1.

### L2 — Listing data model + schema

**bd type:** task. Depends on nothing (can run parallel to L0/L1).

Scope:
- New module `crates/quarto-core/src/project/listing/` (mod.rs,
  config.rs, item.rs).
- Port Q1's `Listing`, `ListingItem`, `ListingDescriptor`,
  `ListingType`, `ListingSort`, `ListingFeedOptions`,
  `ListingSharedOptions` types. Adapt to Q2 idioms (no JS
  defaults, `Option<T>` over sentinel values, types over strings
  where useful).
- YAML schema in `quarto-yaml-validation`: port
  `external-sources/quarto-cli/src/resources/schema/definitions.yml`
  `website-listing` and `website-listing-contents-object`
  definitions.
- Surface `listing:` as a top-level document-frontmatter key
  (decision 5 — matches Q1 and Q2's current `navbar` placement;
  reconcile with `bd-n9dr` if needed).
- `template:` field accepts a path with canonical extension
  `.template` (decision 1). `.ejs.md` is accepted with a
  deprecation diagnostic surfacing the Q1-migration documentation
  link.
- No rendering yet; this is data plumbing only.
- Tests: schema validation positive/negative cases, deserialization
  round-trip, default value coverage.

### L3 — `ListingResolveTransform` (Pass-2, built-ins only)

**bd type:** feature. Depends on L0, L1, L2.

Scope:
- New Pass-2 transform that runs inside `AstTransformsStage`.
- Reads the host page's `listing:` config from frontmatter;
  uses `ProjectIndex` + glob expansion to resolve `contents:` into
  matched profiles; filters via `include` / `exclude`; sorts via
  `sort:`; truncates via `max-items:`.
- Builds the per-item template binding:
  - Pulls curated fields from `profile.listing_item` (with
    fallback to the standard `profile.title` etc. when
    `listing_item.<field>` is unset).
  - Builds pre-rendered helper strings server-side
    (`image_html`, `metadata_attrs`, etc.).
  - Pulls custom fields from `profile.listing_item.extra`.
- Renders via `quarto-doctemplate` against the chosen built-in
  template (`default` / `grid` / `table` only in L3).
- Built-in templates ship as `include_str!`-embedded resources
  via `MemoryResolver`.
- Output is markdown that splices into the host page's AST as a
  `RawBlock` or via the same markdown-pipeline pattern Q1 uses,
  inserted at the document's `<div id="<listing.id>">` slot or
  appended.
- Description-preview placeholders and preview-image
  placeholders are emitted with the *same regex format* Q1 uses
  (`<!-- desc(5A0113B34292)[max=…]:path -->`,
  `<!-- img(9CEB782EFEE6)[…]:id:path -->`) — substituted in L7.
- **Bundle `list.min.js`** for client-side sort/filter
  interactivity (decision 3). The built-in templates emit the
  same markup as Q1, so Q1's `list.min.js` slots in unchanged.
  The script is registered as a `Project`-scoped artifact via
  Phase-5's artifact store and emitted into `_site/site_libs/`
  alongside other shared JS. L3's sub-plan must verify this is
  a clean integration; if the artifact-store wiring forces
  cross-cutting churn, fall back to deferring `list.min.js` to
  a follow-up and re-open decision 3.
- Glob expansion uses the project's existing deterministic
  glob expander (verify which one in L3's sub-plan; pampa or
  `quarto-core` likely already has one for `_quarto.yml`
  `project.render`).
- Tests: per-template-type rendering snapshots; filter / sort /
  max-items behavior; missing-field handling; empty-listing
  handling; multiple listings on a single page; placeholder
  emission for description and image.

End-to-end: a two-page fixture (one host page with
`listing: default`, three content posts) renders an HTML file
with three listing items, each linking to the rendered post.

### L4 — `quarto-doctemplate` enhancements

**bd type:** task. Depends on nothing; **L3 depends on L4**.

Scope:
- Implement the pipe evaluator (today both
  `evaluator.rs:render_variable` and
  `evaluator.rs:evaluate_partial` have `// TODO: Apply pipes`).
- Minimum viable pipe set:
  - `escape` (HTML escape; for table cells, descriptions).
  - `escape_xml` (RSS feed payload).
  - `date_format <fmt>` (configurable date formatting; use
    `chrono` or whatever the rest of the codebase uses).
  - `first` / `rest` (helpful for varied item layouts; not
    strictly needed for built-ins).
- Add `ConfigValue → TemplateValue` conversion. Likely shape:
  `impl From<&ConfigValue> for TemplateValue` in
  `quarto-core` (lives there because `quarto-doctemplate` does
  not depend on `quarto-pandoc-types`'s `ConfigValue`).
- Add a project-scoped partial resolver flavor that combines
  `MemoryResolver` (for built-ins) with a `FileSystemResolver`
  rooted at the host-page directory (for custom templates).
  Likely just a `ChainedResolver` + small ergonomic constructor.
- Tests: each pipe's behavior + edge cases; pipe + partial
  composition; resolver chain ordering; the
  `ConfigValue → TemplateValue` round-trip; deeply-nested
  `extra` map access.

L4 can land independently of the listing-specific work; it's
pure templating-engine improvement and unblocks L3.

### L5 — Categories sidebar

**bd type:** feature. Depends on L3.

Scope:
- New transform `CategoriesSidebarTransform` (or extend
  `ListingResolveTransform`; decide in L5's sub-plan based on
  code shape).
- Reads the resolved item set produced by `ListingResolveTransform`,
  groups items by category, emits a margin-sidebar markdown
  contribution into the host page.
- Q1-parity styles: `category-default`, `category-unnumbered`,
  `category-cloud`. Templates embedded via `MemoryResolver`.
- Click-to-filter behavior on the page is an interactive feature
  scoped out of the v1 epic (see "What this epic does not
  deliver"); category items render as static links/spans.
- Tests: grouping correctness, three category styles, empty-
  categories handling.

### L6 — Dependency graph integration

**bd type:** task. Depends on L0, L2.

Scope (as implemented; original epic-plan design renamed
`listing_content_targets` → `listing_content_globs` per the L6
sub-plan, since we store unresolved glob strings rather than
resolved paths — resolution can't safely cache on a per-doc
profile that doesn't see the full project source set):
- Add `listing_content_globs: Vec<String>` to `DocumentProfile`.
  **Profile version bumped 4 → 5** (`bd-xbnf`) so stale Phase-8
  caches invalidate cleanly. Populated by `DocumentProfile::extract`
  via the new `crate::project::listing::config::extract_content_globs`
  helper.
- New automatic edge source in
  `crates/quarto-core/src/project/dependency_graph.rs`: for each
  host page with non-empty `listing_content_globs`, expand each
  glob against `ProjectIndex.profiles()` (host-relative first,
  project-relative fallback — same rule L3 uses) and add a forward
  edge from the host to each match. Each listing host with a
  non-empty glob list is added to `force_render`.
- Mode B picks up listing hosts when any of their content files
  is targeted. The existing
  `augment_targets_with_always_render` primitive does the work
  unchanged — L6 only feeds `force_render`.
- Tests: edge-add correctness, Mode B selection includes listing
  hosts, sentinel for "listing host with no matches" (no edges,
  no errors). Unit + integration coverage in `quarto-core` (+26
  tests over baseline). End-to-end CLI Mode-B verification
  recorded in the L6 sub-plan.

### L7 — Post-render placeholder upgrade (engine-rendered previews)

**bd type:** feature. Depends on L3. **Bracketed feature** —
see "Bracketing rules" below.

**Purpose.** *Upgrade* the description-preview and preview-image
fields of listing items, replacing the static-AST fallbacks
populated by L1 with content extracted from siblings' fully-
rendered HTML output (post-engine, post-filter, post-highlight).
The listing renders correctly without this step; L7 is purely an
enhancement. This matches Q1's behavior, which the Quarto
ecosystem relies on (e.g. ggplot output as the "above the fold"
preview image for a blog post).

**Why this exists in the architecture.** The data L7 reads —
sibling pages' rendered HTML — does not exist in any in-memory
form during the per-file Pass-2 of the listing host. It only
exists as bytes on disk after every sibling has finished
rendering. To use that data, *something* must read those bytes
after they exist. L7 is that step.

**Scope.**
- New step in `WebsiteProjectType::post_render`:
  `substitute_listing_placeholders(outputs: &[ProjectOutputFile])`.
- Place in the `post_render` composition alongside
  `flush_site_libs`, `copy_favicon`, `write_sitemap`,
  `write_robots_txt`. **Not** a new pipeline pass; one more
  named step inside the existing project-level post-render
  hook.
- The L3 listing transform emits placeholder comments
  (`<!-- desc(5A0113B34292)[max=…]:posts/foo.html -->`,
  `<!-- img(9CEB782EFEE6)[…]:id:posts/foo.html -->`) **alongside**
  the L1 fallback content, not replacing it. The placeholder is
  a marker for "if engine-rendered content is available, swap
  this out; otherwise leave the fallback."
- Port Q1's `readRenderedContents` and `completeListingItems`:
  - Regex-find the two placeholder shapes in each rendered HTML
    output.
  - For each placeholder, open the referenced sibling output
    file, extract `firstPara` + `previewImage`, substitute. If
    the sibling has neither (e.g. engine produced nothing
    visible), the L1 fallback remains in place.
  - HTML parsing: **`scraper`** (decision 4). Its CSS-selector
    API maps directly to Q1's `querySelector(...)` patterns and
    aligns with the user's planned future use in
    `_quarto.tests`. L7's sub-plan must verify `scraper`'s
    transitive dependencies do not break the existing WASM
    build even though L7 itself is CLI-only — pulling
    incompatible deps into `quarto-core` would break hub-client
    via shared dep resolution. Fall back to `tl` only if a hard
    blocker is found.
  - Cache: read each sibling once per `post_render` invocation.
- Tests: placeholder upgrade against a real rendered fixture;
  L1-fallback preservation when sibling has no preview content;
  graceful skip when sibling output file is missing; explicit
  "L7 disabled" path produces correct-but-static listings.

**End-to-end:** a host page with descriptions sourced from
sibling content shows engine-rendered first-paragraphs of each
sibling after `quarto render`. Disabling L7 (or running a
non-CLI environment) on the same fixture produces correct
listings using L1 fallbacks.

#### Bracketing rules (load-bearing)

The user-flagged constraint (2026-05-05): this feature reads
sibling rendered output, which is conceptually a small leak in
the otherwise-clean per-file rendering model. To prevent the
pattern from spreading, L7 is bracketed by *convention* (not
type-system enforcement, since that would be over-engineering
for one feature):

1. **Single home.** All L7 code lives in
   `crates/quarto-core/src/project/listing/post_render_upgrade.rs`
   (or equivalent — confirm in L7's sub-plan). The
   `WebsiteProjectType::post_render` composition calls into a
   single named function. No other module reaches into rendered
   output files for sibling-content reasons.
2. **File header documents the discipline.** The module's
   top-of-file comment explicitly says: *"This module reads
   sibling rendered HTML to upgrade listing previews. This is
   the only place in Q2 that does this. Do not add more
   features here; if you find yourself wanting to read sibling
   rendered HTML for a different reason, that is a signal to
   redesign rather than to extend this module."*
3. **CLI-only by construction.** L7 runs only inside
   `WebsiteProjectType::post_render` on the native render path
   (`quarto render`). Hub-client and any future `quarto
   preview` *do not* invoke this step. Listings in those
   environments display the L1 fallbacks. This must be:
   - asserted in tests (a hub-client smoke test exercises a
     listing fixture and confirms L1 fallbacks are visible);
   - documented in user-facing docs (the listings reference
     page in `docs/` carries a callout: "Engine-rendered
     previews are available in `quarto render` only. In
     interactive environments, listings show static-AST
     previews — set `description:` and `image:` explicitly if
     you need a specific preview to appear during preview");
   - documented in the file header above.
4. **No cross-feature reuse.** If a future feature (search
   indexing, social meta, etc.) needs sibling rendered content,
   it gets its own named step in `post_render`, not a hook
   into L7's machinery. The substitution-by-regex pattern is
   scoped to listings.
5. **Mandatory L1 fallback contract.** Per L1's "Safeguard
   contract": every listing item must be presentable without
   L7 running. L7 is an *upgrade*, not a *requirement*.
   Reviewers of L1 and L3 must verify this property holds:
   removing L7 from `post_render` produces correct (if less
   pretty) listings.

The user signed off on the feature's bracketing instead of its
removal, with the trade-off explicit: keep Q1 parity for
`quarto render`, accept that hub-client / `quarto preview`
listings show static-AST fallbacks. Bracketing rules above are
the form of the trade-off.

#### Out of scope for L7

- Re-rendering siblings to get fresh preview content (would
  re-introduce the cross-page render dependency we are
  explicitly avoiding).
- Pre-extracting preview content during sibling Pass-2 and
  caching it (would couple per-file Pass-2 to a
  listings-specific contract).
- Making L7 work in hub-client (would require sibling engine
  execution; explicitly out of scope per user direction).

### L8 — Custom templates

**bd type:** feature. Depends on L3, L4.

Scope:
- Wire the `template:` config path through to
  `ListingResolveTransform`'s template resolver: when the user
  sets `template: my-listing.template` (decision 1), resolve via
  `FileSystemResolver` rooted at the host-page directory, falling
  back to `MemoryResolver` for built-in partials referenced
  from within the custom template.
- Custom templates receive the same data binding the built-ins
  do, including `item.extra`.
- The `template-params:` config key is exposed as
  `listing.template_params` in the binding.
- Diagnostic when `template:` points at a missing file (with
  source span on the YAML key).
- Deprecation diagnostic when the path ends in `.ejs.md`,
  pointing at the L10 migration documentation.
- Tests: custom template rendering against a fixture; access to
  `item.extra`; access to `listing.template_params`; missing-file
  diagnostic; deprecated-extension diagnostic.

End-to-end: a fixture with `listing: { template: custom.qmd-template }`
that reads `item.extra.status` renders correctly.

### L9 — RSS feeds

**bd type:** feature. Depends on L7.

Scope:
- New step in `WebsiteProjectType::post_render`:
  `write_rss_feeds(outputs, project_index, listings_index)`.
- One feed file per listing host that opts in via
  `feed: true` or `feed: { … }`.
- Output: `<output_dir>/<host-stem>.xml`. Emit `link rel=alternate`
  pointing at it from the host's `<head>`.
- Three `type:` modes from Q1: `full`, `partial`, `metadata`.
  `full` and `partial` reuse `readRenderedContents` from L7;
  `metadata` only reads from the profile.
- Per-category sub-feeds when `feed.categories:` is configured.
- Templates as embedded doctemplate (`feed/preamble`,
  `feed/item`, `feed/postamble`) using L4's `escape_xml` pipe.
- Gated on `website.site-url` (matches sitemap behavior).
- Tests: feed XML against a snapshot; partial vs. full vs.
  metadata content; per-category sub-feeds; site-url-absent
  graceful skip.

End-to-end: a fixture with `feed: true` produces a syntactically
valid `feed.xml` (validated against the W3C feed validator's
expected shape, in offline form).

### L10 — Q1 → Q2 migration documentation + LLM skill

**bd type:** task. Depends on L8 (custom templates) being shippable.

Scope:
- User-facing migration doc in `docs/` (the user-facing Quarto
  website tree): "Migrating Q1 listing templates to Q2."
  Covers: `<%= … %>` → `$…$`, control flow rewrites, helper
  functions → server-pre-rendered fields, `item.extra` for
  custom-author fields.
- LLM skill (in `.claude/skills/`) that, given a Q1 EJS listing
  template, suggests a Q2 doctemplate equivalent. Pairs with the
  doc above.
- Worked examples covering the three built-in shapes plus a
  representative custom template from the wild (find one in
  the Quarto user-extension catalogue).

L10 is documentation-grade work; could land before L9 if convenient.

### L11 — Epic close-out

**bd type:** task. Last.

Scope:
- Compile the per-phase follow-up `bd` log into a single epic
  report (matching the website-epic close-out pattern).
- Confirm `cargo xtask verify` runs clean on a fresh checkout.
- Update `claude-notes/designs/document-profile-contract.md`
  change log if any v3-additive fields were added beyond
  `listing_item` (L6 added `listing_content_globs` and bumped
  the profile version 4 → 5 — already documented in the
  contract doc).
- Confirm hub-client renders listings end-to-end via WASM
  (real browser session, per CLAUDE.md §"End-to-end
  verification"). The key claim being verified: hub-client
  loads a multi-page project that uses listings, edits a
  content page, sees the listing host's preview update.

## Dependency graph (between phases)

```
L0 (profile field) ──► L1 (auto-fill stage)
                       │
                       ▼
                  L3 (built-in render) ──► L5 (categories sidebar)
                       ▲                ──► L7 (post-render subst.)
                       │                       │
              L2 (data model + schema) ────────┤
                       │                       ▼
                       │                  L9 (RSS feeds)
                       │
                       │     L4 (doctemplate pipes + bridge)
                       │           │
                       └───────────┼─► L8 (custom templates)
                                   │
                                   └─► L10 (migration docs / LLM skill)

L0, L2 ──► L6 (dep graph integration; can parallelize w/ L3+)
L9, L8 ──► L11 (close-out)
```

## Ordering recommendation

1. **L4** in parallel with L0/L1/L2 — it's pure templating-engine
   work and unblocks L3.
2. **L0 → L1 → L2** in sequence (L1 reads the field added in L0;
   L2 is independent but small enough to slot in here).
3. **L3** is the first user-visible deliverable. Built-ins only.
   At end of L3, `quarto render` on a website fixture produces
   listing pages.
4. **L7** next — without it, L3's output has unresolved
   `<!-- desc -->` placeholders. L3 is *demoable* without L7 but
   not *complete*.
5. **L5 + L6** can interleave; both are smaller than L3 and L7.
6. **L8** is the second major deliverable: full Q1-feature-parity
   custom templates.
7. **L9** rounds out user-visible features.
8. **L10** docs/skill — could start in parallel with L8.
9. **L11** close-out.

A reasonable working assumption is L0 + L1 + L4 ship in one
session; L2 + L3 ship in the next; subsequent phases are one per
session.

## Risks and mitigations

- **Risk:** `quarto-doctemplate` pipe implementation is more
  involved than expected (e.g. evaluator architecture doesn't
  cleanly accommodate pipes).
  *Mitigation:* L4 carries its own sub-plan and tests; if pipes
  prove invasive, ship a smaller pipe set (just `escape` +
  `date_format`) and revisit. Built-ins can mostly avoid pipes by
  pre-formatting server-side.
- **Risk:** glob expansion semantics drift between `_quarto.yml`
  `project.render` and listings `contents:`.
  *Mitigation:* L2's sub-plan must explicitly identify the shared
  expander and route both through it. If they diverge, file a
  follow-up to consolidate.
- **Risk:** `ListingItemInfoStage` running pre-checkpoint changes
  the profile shape for every document, not just listing-host
  documents. Cache invalidation cascade.
  *Mitigation:* L0 + L1's tests must cover the "no listing
  anywhere in project" case as a no-op (no profile changes).
  `listing_item` defaults empty + `skip_serializing_if` keeps the
  on-disk profile shape unchanged for non-participating
  documents.
- **Risk:** post-render placeholder substitution is sensitive to
  the rendered HTML's structure (Q1's `readRenderedContents` reads
  `main.content`, `#title-block-header`, etc.).
  *Mitigation:* L7's sub-plan inventories the structural
  assumptions and adds a fixture-rendered fixture that locks
  them in.
- **Risk:** L7's "read sibling rendered HTML" pattern leaks into
  other features over time, eroding the per-file render
  isolation the rest of the architecture maintains.
  *Mitigation:* the bracketing rules in L7 (single module,
  header-comment discipline, CLI-only invocation, mandatory L1
  fallback). Reviewers of any future post-render step must check
  that they are not reaching for the same pattern. The fact that
  this risk is explicitly flagged here, in L7, and in the
  module's file header is itself a mitigation: we have written
  down "do not extend this" in three places.
- **Risk:** authors come to depend on L7-upgraded previews and
  are confused when hub-client shows different content.
  *Mitigation:* the listings reference doc carries an explicit
  callout (per L7 bracketing rule 3); the L1 fallback is
  *always* a sensible-looking preview, not a placeholder
  string; the gap between L1 and L7 should be "engine-output
  appears in CLI render, doesn't appear in preview" — both
  states are correct, just different.
- **Risk:** custom templates using `item.extra` access keys that
  don't exist (typo, schema drift).
  *Mitigation:* doctemplate already emits Q-10-2 "Undefined
  variable" diagnostics; verify they propagate to the user with
  source span. Add a listing-render-time diagnostic that names
  the host page if a referenced extra key is missing across all
  items.
- **Risk:** v3 profile bump invalidates every Phase-8 cache on
  upgrade.
  *Mitigation:* this is correct behavior — `VersionMismatch`
  silently regenerates the cache. Document in the change log.
- **Risk:** Hub-client WASM build doesn't pick up new
  doctemplate features cleanly (WASM's restricted Lua stdlib has
  been a recurring issue).
  *Mitigation:* `cargo xtask verify` covers this. L4's sub-plan
  includes a WASM-build verification step.

## Resolved decisions (formerly "open questions"; user confirmed 2026-05-05)

All six epic-level open questions have been answered. Recording here
for the audit trail; sub-plans inherit these as inputs.

1. **Custom-template extension: `.template`.** Shortest readable
   token. Acknowledges the relationship to Pandoc's templating
   language without implying full compatibility (Q2 is currently a
   strict subset of Pandoc doctemplate features, with extensions
   under consideration). Schema accepts `.template` as the
   canonical extension; `.ejs.md` is accepted with a deprecation
   diagnostic for Q1 migration.
2. **Image-from-first-body-image heuristic in L1: ship in v1.**
   Common use case. L1's auto-fill scans the post-include AST for
   the first `Image` node and uses its `src` as the listing-item
   `image` fallback when the author hasn't supplied one. The
   exact "first" semantics (literally first in document order)
   are confirmed in L1's sub-plan; no scoring heuristic.
3. **Bundled `list.min.js` interactivity: include in L3** unless
   a strong technical reason emerges. The markup the built-in
   templates emit is the same as Q1's, so the same `list.min.js`
   slots in. JS bundling routes through Phase-5's artifact-store
   `Project` scope. L3's sub-plan must verify the artifact-store
   integration is straightforward; if it forces cross-cutting
   churn, fall back to deferring `list.min.js` to a follow-up
   and re-open this question.
4. **HTML-parsing crate for L7: `scraper` (preferred).** The CSS
   selector API matches Q1's `querySelector(...)` patterns
   directly and the user has previously evaluated it for a future
   `_quarto.tests` implementation, so adopting it here aligns
   with that planned use. L7's sub-plan must verify WASM
   compatibility — though L7 itself is CLI-only by construction
   per the bracketing rules, the `scraper` crate must not pull
   transitive dependencies that break the existing WASM build.
   Fall back to `tl` only if a hard blocker is found.
5. **Schema placement: top-level `listing:` frontmatter key.**
   Matches Q1 and Q2's current `navbar` placement. L2's sub-plan
   must reconcile with the still-open `bd-n9dr` nav-config-
   placement decision; the listings work does not pre-empt that
   decision but must be migratable if `bd-n9dr` ultimately
   chooses a namespaced placement.
6. **`ListingItemInfoStage` location:**
   `crates/quarto-core/src/stage/stages/listing_item_info.rs`.
   Matches sibling stage modules.

## Test-strategy threads (cross-cutting)

- **Fixture projects** under
  `crates/quarto-core/tests/fixtures/listings/`:
  - `minimal-default` — host page + three posts, default type.
  - `grid-with-images` — exercises image-html pre-rendering.
  - `table-sortable` — exercises field-types + sort UI markup.
  - `custom-template` — author-provided custom template reading
    `item.extra`.
  - `categories-sidebar` — three category styles.
  - `rss-feed` — full / partial / metadata feed types.
  - `incremental` — touching a content file rebuilds only the
    listing host.
- **End-to-end CLI verification** for every phase that produces
  user-visible output, per CLAUDE.md.
- **Snapshot tests** for rendered HTML and feed XML; explicit
  call-outs when snapshots change.
- **Hub-client smoke test** in L11: real browser session showing
  a listing-host page rendering through the WASM API and updating
  on sibling-content edit.
- **Full-workspace verification** (`cargo xtask verify`) before
  every commit touching `quarto-core`, `quarto-pandoc-types`, or
  `quarto-doctemplate`.

## Documentation outputs

By the end of the epic:

- Updated `claude-notes/designs/document-profile-contract.md`
  with `listing_item` row and §"Scoped feature surfaces".
- New user-facing reference page in `docs/` covering the
  listings YAML schema (mirrors Q1's reference docs but for
  Q2 syntax differences).
- Migration doc: "Q1 listing templates → Q2 doctemplate."
- LLM skill: `q1-listing-template-migration` (or similar) under
  `.claude/skills/`.
- Per-phase commit messages document sub-plan, profile-version
  bumps, and any contract-doc changes (per existing practice).

## Filing plan

Once the user approves this epic plan:

1. File the epic itself: `br create "Listings feature epic" -t epic
   -p 1 -d "<short summary>" --json` — get the bd id.
2. File each phase L0–L11 as a sub-issue with
   `--deps parent-child:<epic-id>` plus inter-phase
   `blocks` deps as in the dep graph above.
3. Reference this plan file from each phase's bd description and
   add the bd ids back into this plan's phase headers (matches
   the website-epic pattern).
4. `br sync --flush-only && git add .beads/ && git commit`.
