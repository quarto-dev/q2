# L9 — RSS feeds (sub-plan)

**Date:** 2026-05-08
**Beads:** `bd-o90m` (this phase). Parent epic: `bd-61cd`
(`claude-notes/plans/2026-05-05-listings-epic.md`).
**Predecessors:**
- L0–L3 (closed) — listing data model, schema, generate +
  render transforms. `Listing.feed: Option<ListingFeedOptions>`
  is already parsed today; nothing currently consumes it.
- L4 (closed, bundled with L3) — `quarto-doctemplate`
  enhancements (pipes, `ConfigValue → TemplateValue` bridge,
  `project_listing_resolver`). L9 reuses `MemoryResolver`
  for the new feed templates and does **not** add new pipes
  (the `date_format` pipe originally planned for L9 was
  deferred to a follow-up bd; see §"Out of scope for L9").
- L5 (closed) — categories sidebar. Per-category sub-feeds
  (L9) reuse the same per-item category data L5 consumes;
  they don't depend on L5's transform.
- L6 (closed) — Phase-8 dependency-graph integration via
  `listing_content_globs`. Untouched by L9.
- L7 (closed) — `post_render_upgrade` introduced the
  `scraper`-based reader and the per-`post_render`
  cache pattern. **L9 reuses neither verbatim** — L9's reader
  extensions live in a sibling module per the bracketing rule
  ("reader extensions are L9's concern, not L7's"). The
  `scraper` dep gating, `Q-12-13` precedent, and L7's `cargo
  xtask verify` integration are inherited unchanged.
- L8 (closed) — custom listing templates. Untouched by L9
  (L9's templates are built-ins; custom feed templates are
  out of scope).

**Status:** Draft. Awaiting user approval before hand-off.

## Goal of this phase

Match Q1's RSS-feed behavior on `quarto render` for website
projects: every listing host page that opts in via `feed: true`
or `feed: { ... }` produces an RSS 2.0 file alongside its HTML
output, with `<link rel="alternate" type="application/rss+xml">`
in the host page's head pointing at it. Per-category sub-feeds
ship with the same machinery.

L9 ships:

1. **Two new Pass-2 transforms inside `AstTransformsStage`,
   running after `ListingGenerateTransform` /
   `ListingRenderTransform`:**
   - `ListingFeedStageTransform` — for each
     feed-configured `ResolvedListing`, build a *staged*
     RSS XML (preamble + per-item + postamble) using
     embedded doctemplate templates, with placeholder tokens
     for sibling content where engine-rendered description /
     full content is needed. Write to
     `<output_dir>/<host-stem>.feed-{full|partial|metadata}-staged`
     (and per-category variants). Native-only via cfg.
   - `ListingFeedLinkTransform` — inject
     `<link rel="alternate" type="application/rss+xml" title="..." href="<stem>.xml">`
     into the host page's head metadata when the listing has
     `feed:` configured *and* `website.site-url` is set.
     Runs on both native and WASM (the link tag itself is
     harmless in WASM contexts, and gating it would force
     authors to remember "feeds only show up after CLI
     render," which is already documented for L7-style
     features).
2. **A new project post-render step
   `complete_staged_feeds`** in
   `WebsiteProjectType::post_render`, native-only,
   alongside the existing `flush_site_libs` /
   `copy_favicon` / `write_sitemap` / `write_robots_txt` /
   `substitute_listing_placeholders` (L7) hooks. Walks the
   output dir for `*.feed-*-staged` files, substitutes
   placeholders by reading sibling rendered HTML, writes
   final `.xml`, deletes staged. **Runs after L7's
   substitution step** so any host-page HTML that L7 might
   have rewritten is finalized before sibling reads.
3. **L9 reader extension module
   `project/listing/feed/reader_ext.rs`** — listings-RSS
   subset of Q1's `readRenderedContents`. v1 ships
   `extract_first_para_html` (HTML-preserving, for `partial`
   bodies) and `extract_full_contents` (whole `main.content`
   with anchor stripping + urls-to-absolute). Each new
   transform is a private function gated by an
   `RssReaderOptions` flag. Math + syntax-highlight class
   maps are explicitly **not** in v1 (file follow-up bds at
   close-out).
4. **Three embedded RSS templates under
   `project/listing/feed/templates/`:**
   - `preamble.template` — `<?xml ?> <rss> <channel>`
     headers + channel-level metadata (title, link,
     description, generator, lastBuildDate, optional
     image, optional language).
   - `item.template` — one `<item>` per listing item.
     Wraps the description in placeholder tokens that the
     post-render step substitutes (or in `<![CDATA[...]]>`
     verbatim for `metadata` type).
   - `postamble.template` — closing `</channel> </rss>`.
5. **Per-category sub-feeds:** when `feed.categories: [...]`
   is set, one extra staged file per listed category at
   `<host-stem>-<category>.xml`, filtered to items carrying
   that category. Same template machinery; binding builder
   pre-filters items per category. The host-page's head
   carries a `<link rel="alternate">` for the *main* feed
   only — Q1 doesn't emit per-category alternate links and
   we match.
6. **`<media:content>` image emission with `imagesize` crate
   (native-only).** Items whose `image` field resolves to a
   local file get `width="..." height="..." type="..."`
   attributes derived from the on-disk image. Items with
   absolute / data-URI / unreadable images get bare
   `<media:content url="..." medium="image"/>` — same
   degradation Q1 has.
7. **No new doctemplate pipe in L9.** The plan originally
   specified a `date_format <fmt>` pipe; that has been
   **deferred to a follow-up bd** (decided 2026-05-08 at
   impl-start). Rationale: L9's templates do not use
   `date_format` directly — the binding pre-computes
   `pub_date_rfc822` server-side via the `time` crate
   already in `quarto-core`. Adding the pipe would require
   a tree-sitter grammar change to
   `crates/tree-sitter-doctemplate/grammar/grammar.js`
   (the existing pipes are simple aliases; `date_format`
   takes an argument and would need a custom rule like
   `pipe_left`/`pipe_center`/`pipe_right`), plus the
   `tree-sitter generate; tree-sitter build` regeneration
   cycle. Out of scope for L9; filed at close-out. XML
   escaping for feed fields is **not** a pipe (handled
   server-side in the binding per epic decision).
8. **Server-side XML escaping in the feed binding.** The
   binding builder in `feed/binding.rs` escapes title,
   description, category, author, etc. before passing to
   the templates. Templates emit `$title$` verbatim. This
   matches the existing pattern in `listing/binding.rs`
   for `image_html`, `metadata_attrs`, etc.
9. **Pass `cargo xtask verify`** (full, including hub-client
   + WASM build). Most of the feed module is gated to
   `#[cfg(not(target_arch = "wasm32"))]` — the WASM build
   doesn't pull `imagesize` or any of the staged-write /
   reader-extension / completion code. The one piece that
   flows to WASM is `ListingFeedLinkTransform`
   (head-metadata edit, no I/O), which lives in
   `feed/link_inject.rs` outside the cfg gate.

**Out of scope for L9 (deferred):**

- **Math handling in `full` feeds.** Q1 has KaTeX/MathJax
  preservation pathways in `readRenderedContents`. L9 v1
  emits the math as it appears in the rendered HTML
  (typically `<span class="math">$...$</span>` or rendered
  KaTeX HTML). RSS readers may not render it; subscribers
  who want pretty math read the linked-to HTML page. File
  follow-up bd.
- **`inline-code-style` syntax-highlight class-to-style
  mapping in `full` feeds.** Q1 maps highlight classes
  (`token comment`, etc.) to inline `style="color: ..."` so
  feeds render with colors when the subscriber's stylesheet
  doesn't include Quarto's CSS. v1 leaves the classes; if a
  reader doesn't have the CSS, code blocks render in a
  default mono typeface. File follow-up bd.
- **`xml-stylesheet` rendering of feeds in browsers.**
  `Listing.feed.xml_stylesheet` is parsed today; L9 emits
  the `<?xml-stylesheet ?>` PI when the field is set, but
  does **not** copy the stylesheet file to the output dir
  or validate the path. The user is responsible for ensuring
  the stylesheet is present and reachable. Q1 does the same.
- **Validation against the W3C feed validator.** L9 v1 ships
  a snapshot test against a representative output and an
  end-to-end visual inspection; no live W3C validation.
  File follow-up bd if subscribers report parse errors.
- **Custom feed templates.** L9's three feed templates are
  embedded built-ins. L8's `template:` config affects the
  *listing* render, not the feed render. A future epic
  could introduce `feed.template:` for author-supplied
  XML; not in v1.
- **`atom:link` and full Atom 1.0 emission.** L9 emits RSS
  2.0 with the `atom:link rel="self"` extension element
  (matches Q1). Pure Atom output is a follow-up if
  someone asks.
- **Title placeholder substitution.** Q1 substitutes the
  rendered post title (post-engine) for items, because
  engine output may contain math etc. that the metadata
  title doesn't. v1 uses `item.title` from the profile
  directly. Subscribers see the metadata title; if the
  subscribed-to feed item has math in its title, it'll
  show up as the source markup, not the rendered form.
  File follow-up bd if a user reports this.
- **`format.metadata.description` fallback for the channel
  description.** Q1 cascades feed.description →
  format.metadata.description → website.description. v1
  cascades feed.description → website.description (skips
  the per-format layer). The simpler cascade matches Q2's
  configuration model. File follow-up bd if a user needs
  the third level.
- **Hub-client / `quarto preview` feed generation.** Same
  as L7's bracketing — the staged-file write and substitution
  are native-only. Hub-client preview shows the host page
  with a dead `<link rel="alternate">` (the linked file
  doesn't exist in the in-browser VFS). Documented in
  user-facing docs at L11 close-out.
- **`date_format` doctemplate pipe.** Originally listed
  as an L9 deliverable; **deferred** at impl-start
  2026-05-08 (decision D8). L9's binding pre-computes
  `pub_date_rfc822` server-side, so the pipe isn't on
  the critical path. Adding it later requires a small
  `tree-sitter-doctemplate` grammar change (see
  `grammar.js:56`) plus a match arm in
  `quarto-doctemplate/src/pipes.rs`. Filed as a
  close-out follow-up bd.

## Reference material

Read first:

- Parent epic:
  `claude-notes/plans/2026-05-05-listings-epic.md` §"L9" +
  §"Resolved decisions" #4 (`scraper` HTML reader). The
  epic positions L9 as a **follow-up to L7**: the staged-file
  + sibling-substitute pattern Q1 uses for feeds is the same
  pattern L7 introduced for description previews, just with
  a different output (XML next to HTML, vs. inline HTML
  rewrite).
- L3 sub-plan:
  `claude-notes/plans/2026-05-06-listings-L3-resolve-transform.md`
  §"Generate transform" + §"Render transform" — describes
  `ResolvedListing` and the per-item binding. L9's feed
  binding mirrors the structure but with feed-specific
  fields (pubDate, guid, image with width/height).
- L7 sub-plan:
  `claude-notes/plans/2026-05-07-listings-L7-postrender-upgrade.md`
  §"Marker design" + §"Reader extensibility" + §"scraper
  dep gating". L9 inherits the dep-gating pattern: feed
  module is `#[cfg(not(target_arch = "wasm32"))]`,
  `imagesize` is a target-gated crate. L9's reader extension
  is a *sibling* module to L7's (per user's decision in §"Settled
  inputs"), not an extension of L7's `reader.rs`.
- L8 sub-plan:
  `claude-notes/plans/2026-05-07-listings-L8-custom-templates.md`
  §"load_custom_template" — precedent for direct `std::fs`
  calls inside Pass-2 transform code paths. L9 follows the
  same pattern for the staged-file write.
- Q1 reference (read-only):
  - `external-sources/quarto-cli/src/project/types/website/listing/website-listing-feed.ts`
    — full Q1 feed implementation. L9 ports the
    `createFeed` (~`ListingFeedStageTransform`) +
    `completeStagedFeeds` (~`complete_staged_feeds` in
    post_render) split.
  - `external-sources/quarto-cli/src/project/types/website/listing/website-listing-shared.ts:311-597`
    — Q1's `readRenderedContents`. **L9 v1 ports the
    listings-RSS subset only** (firstPara HTML preservation,
    full-content with anchor strip + urls-to-absolute).
  - `external-sources/quarto-cli/src/resources/projects/website/listing/feed/{preamble,item,postamble}.ejs.md`
    — Q1's three EJS templates. L9 ports them to doctemplate
    syntax with the same channel-level + item-level shape.
- Existing Q2 surface L9 builds on:
  - `crates/quarto-core/src/project/listing/config.rs:177`
    — `ListingFeedOptions` struct + `parse_feed`.
    Already complete; L9 doesn't change parsing.
  - `crates/quarto-core/src/project/listing/binding.rs:47`
    — `build_listing_context`. L9 adds a sibling
    `build_feed_context` in `feed/binding.rs` for the new
    template surface. The two contexts share the
    per-item hydration (via `ListingItem`), but the feed
    context exposes feed-specific helper fields
    (`pub_date_rfc822`, `image_url_abs`, `image_width`,
    `image_height`, `image_content_type`, `categories_xml_safe`,
    etc.).
  - `crates/quarto-core/src/transforms/listing_generate.rs:43`
    — `ListingGenerateTransform`. L9's stage transform
    runs *after* this, in the same `AstTransformsStage`,
    consuming `ctx.resolved_listings`.
  - `crates/quarto-core/src/transforms/listing_render.rs`
    — `ListingRenderTransform`. L9 runs in parallel
    (sibling transform). The two are independent: render
    builds in-page listing HTML; stage-feed builds a
    sibling `.xml`.
  - `crates/quarto-core/src/project/listing/post_render_upgrade.rs`
    — L7's `substitute_listing_placeholders`. L9's
    `complete_staged_feeds` runs *after* it (so any
    host-page HTML rewrites L7 made are baked in before
    L9's substitution reads from the host page).
  - `crates/quarto-core/src/project/listing/post_render_upgrade/reader.rs`
    — L7's reader. **L9 does NOT extend this file.** L9
    has a sibling reader at
    `project/listing/feed/reader_ext.rs`. Per L7's
    bracketing rule, L7's reader stays minimal and
    listings-display-only; RSS reader features live in
    L9's tree.
  - `crates/quarto-core/src/project/website_post_render.rs`
    — pattern for native-only post-render hooks
    (`copy_favicon`, `write_sitemap`, `write_robots_txt`).
    L9's `complete_staged_feeds` follows the same shape.
  - `crates/quarto-core/src/project/website_config.rs:53`
    — `website_site_url`. Gates feed emission (no site-url
    → no feeds, with `Q-12-15` warning at first call site).
  - `crates/quarto-doctemplate/src/pipes.rs` — pipe
    registry. L9 originally planned to add `date_format`;
    deferred (see D8). Note that the pipe set is fixed by
    `crates/tree-sitter-doctemplate/grammar/grammar.js:56`,
    so adding a pipe later requires both a grammar change
    (with the `tree-sitter generate; tree-sitter build`
    cycle) and a match arm here.
  - `crates/quarto-error-reporting/error_catalog.json`
    line 827 — `Q-12-14` is the highest existing
    Q-12 code. L9 adds `Q-12-15` (no site-url) and
    `Q-12-16` (sibling output unreadable for full/partial
    feed substitution).

## Settled inputs

These are decisions, not open questions:

- **L9 ships next.** User-confirmed 2026-05-08. Final
  user-visible feature-parity item before L10 (docs/skill).
- **Architecture: Q1-style staged file** (user-confirmed
  2026-05-08). During Pass-2 of each host page,
  `ListingFeedStageTransform` writes
  `<output_dir>/<host-stem>.feed-{type}-staged` synchronously
  via `std::fs::write` (mirrors L8's direct `std::fs`
  precedent). At post_render, `complete_staged_feeds`
  walks the output dir, substitutes placeholders, writes
  final `.xml`, deletes staged file.
- **Feed types: full / partial / metadata, all three**
  (user-confirmed 2026-05-08). `metadata` skips sibling
  reading entirely (description from profile only).
  `partial` extracts firstPara HTML from sibling.
  `full` extracts whole `main.content` HTML with anchor
  strip + urls-to-absolute.
- **Per-category sub-feeds: ship in v1** (user-confirmed
  2026-05-08). Each category in `feed.categories: [...]`
  produces an extra `<stem>-<lowercased-category>.xml`,
  filtered to items carrying that category.
- **XML escaping: server-side in the binding**
  (user-confirmed 2026-05-08). The feed binding builder
  HTML/XML-escapes title, description, category, author,
  etc. before passing to templates. Templates emit fields
  verbatim. Mirrors how `listing/binding.rs` already
  handles `image_html` / `metadata_attrs`.
- **Link injection: new Pass-2 transform**
  (user-confirmed 2026-05-08). `ListingFeedLinkTransform`
  runs after `ListingGenerateTransform` (it reads
  `ctx.resolved_listings` to find feed-configured
  hosts), and appends to `rendered.includes.header`
  (the same slot `WebsiteFaviconTransform` writes to;
  see `crates/quarto-core/src/transforms/website_favicon.rs:74`
  `apply_favicon` for the precedent). Runs on both
  native and WASM (link tag itself is harmless; only
  the file it points to is native-only).
- **Image metadata with `imagesize` crate, native-only**
  (user-confirmed 2026-05-08). The user explicitly noted
  WASM concerns: the dep stays target-gated like
  `scraper`, and the call sites are inside the
  native-only feed module. The `imagesize` crate is
  small (header-parsing only, no full image decode) and
  has no transitive WASM-incompatible deps; L9's sub-plan
  verifies this with a `cargo xtask verify` before
  committing the dep edit.
- **Pipes: none new in L9** (revised at impl-start
  2026-05-08; original plan called for `date_format`).
  The epic's L4 plan called for
  `escape` / `escape_xml` / `date_format` / `first` /
  `rest`. `first` / `rest` already shipped in L4. Escapes
  are server-side. `date_format` is **deferred** — the
  L9 binding pre-computes `pub_date_rfc822` server-side
  via the `time` crate already in `quarto-core`, and
  none of L9's three feed templates need a date-format
  pipe at the template surface. Adding the pipe later
  is non-breaking: a new alias in
  `tree-sitter-doctemplate/grammar/grammar.js` plus a
  match arm in `quarto-doctemplate/src/pipes.rs`. Tracked
  as a close-out follow-up.
- **Templates: three embedded `.template` files**
  (user-confirmed 2026-05-08).
  `feed/templates/preamble.template`,
  `feed/templates/item.template`,
  `feed/templates/postamble.template`. Embedded via
  `MemoryResolver` (same pattern as built-in listing
  templates). Author-readable as reference; not currently
  override-able (would be a separate `feed.template:`
  feature).
- **Full reader scope: urls-to-absolute + anchor stripping
  only** (user-confirmed 2026-05-08). Math + syntax-highlight
  class maps deferred. Each follow-up bd records the
  specific RSS-reader behavior gap so we can prioritize
  later.
- **Module home: `crates/quarto-core/src/project/listing/feed/`**
  (user-confirmed 2026-05-08). New submodule. Files:
  `mod.rs`, `binding.rs`, `stage.rs`, `complete.rs`,
  `reader_ext.rs`, `templates/{preamble,item,postamble}.template`.
  Native-only via `#[cfg(not(target_arch = "wasm32"))]`
  in `mod.rs`.
- **`<link rel="alternate">` link path is host-relative.**
  Format: `href="<stem>.xml"`. Q1 emits the same form. The
  link is interpreted relative to the host page's URL by
  the browser, which is correct for both flat and nested
  layouts.
- **Output filename for sub-feeds: `<stem>-<lowercased-category>.xml`.**
  Q1 lowercases via `.toLocaleLowerCase()`. Q2 uses
  `to_lowercase()` (UTF-8 aware in Rust; close enough to
  Q1's behavior for the kinds of category names we expect).
- **Generator string format: `quarto-2`.** Q1 emits
  `quarto-${quartoConfig.version()}` (e.g. `quarto-1.5.0`).
  Q2's version story is in flux; v1 emits a stable
  `quarto-2` string. A follow-up bd swaps in the real
  version when the version story stabilizes.

## Architecture

### Overall flow

```
Pass 2 (per host page; AstTransformsStage):
  ListingGenerateTransform   ← existing; populates ctx.resolved_listings
  ListingRenderTransform     ← existing; emits in-page listing markdown
  CategoriesSidebarTransform ← existing
  ListingFeedStageTransform  ← NEW; writes <stem>.feed-<type>-staged (native)
  ListingFeedLinkTransform   ← NEW; injects <link rel=alternate> in head

Project post-render (WebsiteProjectType::post_render, native-only):
  flush_site_libs              (existing)
  copy_favicon                 (existing)
  write_sitemap                (existing)
  write_robots_txt             (existing)
  substitute_listing_placeholders (L7)
  complete_staged_feeds        ← NEW; substitutes & finalizes feeds
```

The two-phase pattern is identical to L7's: emit a marker /
staged content during render, substitute it during post-render
when sibling content is available on disk. L7 substitutes
inside the host page's HTML; L9 substitutes inside a sibling
XML file. Both share the gating principle ("CLI-only by
construction; in-browser environments degrade gracefully").

### The staged-file pattern

Q1's reasoning is preserved verbatim: at host-page-render
time, sibling rendered output may not yet exist on disk (Pass 2
renders one page at a time, in arbitrary order). To produce a
feed body that includes sibling content, we need a deferred
substitution step. The staged file is the carrier between
the two phases.

**Naming.** For a host page with output `<output_dir>/posts.html`
and feed `type: full`, the staged file is
`<output_dir>/posts.feed-full-staged`. Per-category sub-feed
for category `"Software"`: `<output_dir>/posts-software.feed-full-staged`.
The extensions match Q1 verbatim:

- `.feed-full-staged`
- `.feed-partial-staged`
- `.feed-metadata-staged`

**Placeholder format.** Inside the staged file, where engine-
rendered description content needs to flow in, the binding
emits a single Q1-verbatim token wrapped in the `<description>`
tag:

```xml
<description>{B4F502887207:posts/foo.html}</description>
```

The token `B4F502887207` matches Q1's `placeholder()` exactly;
the colon-prefixed payload is the project-relative output href
of the sibling file. Post-render regex-matches
`<description>\{B4F502887207:([^}]+)\}</description>` and
substitutes.

For `metadata` type, the description is taken from the
profile's `description` field at staging time, written
inline as `<description><![CDATA[ ... ]]></description>`.
No placeholder, no sibling read needed at post-render. The
post-render walk still touches the file (to delete the staged
extension and rename to `.xml`), but doesn't do regex work.

### Templates

Three doctemplate files under
`crates/quarto-core/src/project/listing/feed/templates/`,
embedded via `include_str!`:

**`preamble.template`** — channel-level metadata. Approximate
shape (see file for exact whitespace):

```
<?xml version="1.0" encoding="UTF-8"?>
$if(channel.xml-stylesheet)$<?xml-stylesheet type="text/xsl" media="screen" href="$channel.xml-stylesheet$"?>$endif$

<rss xmlns:atom="http://www.w3.org/2005/Atom"
     xmlns:media="http://search.yahoo.com/mrss/"
     xmlns:content="http://purl.org/rss/1.0/modules/content/"
     xmlns:dc="http://purl.org/dc/elements/1.1/"
     version="2.0">
<channel>
<title>$channel.title$</title>
<link>$channel.link$</link>
<atom:link href="$channel.feed-link$" rel="self" type="application/rss+xml"/>
<description>$channel.description$</description>
$if(channel.language)$<language>$channel.language$</language>$endif$
$if(channel.image)$<image>
<url>$channel.image.url$</url>
<title>$channel.image.title$</title>
<link>$channel.image.link$</link>
$if(channel.image.height)$<height>$channel.image.height$</height>$endif$
$if(channel.image.width)$<width>$channel.image.width$</width>$endif$
</image>$endif$
<generator>$channel.generator$</generator>
<lastBuildDate>$channel.last-build-date$</lastBuildDate>
```

**`item.template`** — one item. Channel binding doesn't
include the items list; the stage transform iterates and
calls `Template::compile` once per item with a per-item
context:

```
<item>
  <title>$item.title$</title>
$for(item.authors)$  <dc:creator>$it$</dc:creator>
$endfor$
  <link>$item.link$</link>
  $item.description-element$
$for(item.categories)$  <category>$it$</category>
$endfor$
  <guid>$item.guid$</guid>
$if(item.pub-date)$  <pubDate>$item.pub-date$</pubDate>$endif$
$if(item.image)$  <media:content url="$item.image.url$" medium="image"$item.image.attrs$/>$endif$
</item>
```

The `item.description-element` slot is **the placeholder string
or the inline metadata description**. The binding builder
chooses based on feed type:

- `metadata`: `<description><![CDATA[ {profile.description or empty} ]]></description>`
- `partial` / `full`: `<description>{B4F502887207:<output-href>}</description>`

This keeps the template free of conditional logic on feed type.

**`postamble.template`**:

```
</channel>
</rss>
```

Whitespace is significant for human-readable output but not for
RSS parsers. Templates use raw newlines.

### Reader extension (`feed/reader_ext.rs`)

Listings-RSS subset of Q1's `readRenderedContents`. Two
extractors:

```rust
/// Extract the first non-empty `<p>` from `main.content` and
/// return its **HTML**, not just its text. Mirrors Q1's
/// `firstPara` for the `partial` feed type. Caller passes the
/// max-length truncation knob (Q1 uses `max-description-length`,
/// which is `Listing.max_description_length`).
pub fn extract_first_para_html(html: &str, max_length: u32) -> Option<String>;

/// Extract the whole `main.content` element as HTML, with the
/// `RssReaderOptions` transforms applied (urls-to-absolute,
/// anchor strip).
pub fn extract_full_contents(
    html: &str,
    site_url: &str,
    sibling_output_href: &str,
) -> Option<String>;

#[derive(Debug, Clone, Default)]
pub struct RssReaderOptions {
    pub urls_to_absolute: bool,
    pub strip_local_anchors: bool,
    // Forward-compat slots; v1 ignores these:
    #[allow(dead_code)] pub math_handling: bool,
    #[allow(dead_code)] pub inline_code_style: bool,
}
```

**v1 behavior:**

- `extract_first_para_html` returns the inner HTML of the
  first non-empty `<p>` in `main.content`, with anchor tags
  unwrapped (Q1's `partial` mode strips `<a>` so subscribers
  don't navigate into a Quarto-themed page from a feed
  reader). Truncation: char-count on rendered text, cut at
  word boundary, return the corresponding HTML prefix
  (Q1's behavior; trickier than the L7 first-para extractor
  because that one returns plain text).
- `extract_full_contents` returns the inner HTML of
  `<main class="content">`, with:
  - all `<a href="...">` rewritten to absolute URLs (Q1's
    `urls-to-absolute`): `href="../foo/bar.html"` →
    `href="https://example.com/foo/bar.html"`. Computed
    from the sibling's output href + the user's `site-url`.
  - all `<a href="#section-X">` link bodies preserved but
    `href` removed (Q1's anchor strip).
  - `<img src="...">` similarly rewritten to absolute URLs.

**v1 limitations:**

- Math (`<span class="math">$...$</span>` or rendered
  KaTeX HTML) passes through verbatim. Subscribers see what
  they get; if their reader doesn't support MathJax/KaTeX,
  it shows the source notation. Follow-up bd: optional
  fallback to PNG / inline SVG.
- Highlight classes (`token comment`, etc.) pass through.
  Follow-up bd: map to inline `style="color: ..."`.

**`<header id="title-block-header">` skipping:** L9's
extractors skip this header (same as L7's), so feed bodies
don't carry the post title twice (`<title>` already has it).

### Module layout

```
crates/quarto-core/src/project/listing/
  feed/
    mod.rs              ← public API, re-exports, cfg gate
    binding.rs          ← FeedChannel + FeedItem builders;
                          server-side XML escaping;
                          imagesize lookup (per-item)
    stage.rs            ← ListingFeedStageTransform
                          (Pass-2 transform; std::fs::write)
    complete.rs         ← complete_staged_feeds (post_render);
                          regex-substitutes placeholders;
                          renames staged → final .xml
    reader_ext.rs       ← extract_first_para_html,
                          extract_full_contents,
                          RssReaderOptions
    link_inject.rs      ← ListingFeedLinkTransform
                          (Pass-2 head-meta edit; runs both
                          native and WASM)
    templates/
      preamble.template
      item.template
      postamble.template

crates/quarto-core/src/project/orchestrator.rs
  ← add complete_staged_feeds call in
    WebsiteProjectType::post_render after L7

crates/quarto-doctemplate/src/pipes.rs
  ← (deferred) `date_format` pipe — see §"Settled inputs"
    "Pipes" entry. Not part of L9's diff.

crates/quarto-error-reporting/error_catalog.json
  ← +Q-12-15 (no site-url; feeds skipped)
  ← +Q-12-16 (sibling output unreadable; placeholder kept)

crates/quarto-core/Cargo.toml
  ← add `imagesize = "0.13"` under
    [target.'cfg(not(target_arch = "wasm32"))'.dependencies]
```

### `ListingFeedStageTransform` (Pass-2)

```rust
// transforms/listing_feed_stage.rs (or re-exported via
// project/listing/feed/stage.rs — final placement decided
// at impl start based on what reads cleaner alongside
// `listing_render`)

pub struct ListingFeedStageTransform;

#[async_trait(?Send)]
impl Transform for ListingFeedStageTransform {
    async fn apply(&self, ast: &mut Pandoc, ctx: &mut RenderContext)
        -> Result<()>
    {
        // No-op on WASM.
        #[cfg(target_arch = "wasm32")] return Ok(());

        #[cfg(not(target_arch = "wasm32"))]
        {
            let Some(site_url) = website_site_url(&ast.meta) else {
                // Per-host warning is too noisy if many hosts
                // declare feeds in the same project. Surface a
                // single project-level Q-12-15 once, deferred to
                // post_render (where we know we walked the project).
                return Ok(());
            };
            if ctx.resolved_listings.iter().all(|r| r.listing.feed.is_none()) {
                return Ok(());
            }

            let host_output_path = ctx.output_path()?;
            for resolved in &ctx.resolved_listings {
                let Some(feed_opts) = resolved.listing.feed.as_ref() else {
                    continue;
                };
                stage_one_feed(
                    resolved,
                    feed_opts,
                    &site_url,
                    &host_output_path,
                    ctx,
                )?;
                for category in &feed_opts.categories {
                    stage_one_category_feed(
                        resolved, feed_opts, category,
                        &site_url, &host_output_path, ctx,
                    )?;
                }
            }
            Ok(())
        }
    }
}
```

`stage_one_feed`:

1. Build the channel binding via `feed/binding.rs::build_channel_context`
   (XML-escapes title, description, image fields).
2. Compile + render `preamble.template` against the channel
   context.
3. For each item (already filtered + sorted from `resolved.items`,
   then truncated to `feed_opts.items.unwrap_or(20)`):
   - Build the per-item binding via `build_item_context`.
     This is where the `description-element` slot is
     populated — `<![CDATA[...]]>` for `metadata`, the
     placeholder string for `partial`/`full`.
   - Compile + render `item.template`.
   - Append to the in-memory output buffer.
4. Compile + render `postamble.template`.
5. `std::fs::write(<output_dir>/<host-stem>.feed-{type}-staged, output)`.

`stage_one_category_feed` is the same flow with `items`
pre-filtered to those carrying the category and the channel
title / link rewritten per Q1.

### `ListingFeedLinkTransform` (Pass-2; both native + WASM)

Edits `ast.meta.header-includes` to inject:

```html
<link rel="alternate" type="application/rss+xml"
      title="<channel-title>" href="<host-stem>.xml" data-external="1">
```

Runs on both targets — the link tag is harmless in WASM
contexts (the file it points to doesn't exist; the
hub-client preview renders the page without trying to
follow the link). This means hub-client users see the
feed link and learn that "the rendered site has a feed
here." If they click it, they get a 404; that's acceptable
v1 behavior, documented in the listings reference page
(L11).

The transform runs on both targets to keep the rendered
HTML byte-for-byte identical between native and WASM where
possible (helps tests, helps hub-client behavior match
native behavior).

### `complete_staged_feeds` post-render step

```rust
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn complete_staged_feeds(
    project: &ProjectContext,
    runtime: &dyn SystemRuntime,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Result<()> {
    let Some(meta) = project.config.metadata.as_ref() else {
        return Ok(());
    };
    let Some(site_url) = website_site_url(meta) else {
        return Ok(());
    };

    // Per-call cache: HashMap<absolute_sibling_path, Option<RenderedHtml>>.
    let mut cache: HashMap<PathBuf, Option<String>> = HashMap::new();

    let regex = build_placeholder_regex(); // "<description>\{B4F502887207:([^}]+)\}</description>"

    for entry in walk_output_dir(project.output_dir.as_path())? {
        let Some(staged_type) = parse_staged_extension(&entry) else {
            continue;
        };
        let staged_path = entry.path();
        let staged_content = std::fs::read_to_string(&staged_path)?;

        let final_content = match staged_type {
            StagedType::Metadata => staged_content, // no substitution
            StagedType::Partial | StagedType::Full => substitute_descriptions(
                &staged_content, staged_type, &regex,
                &site_url, &project.output_dir, &mut cache, diagnostics,
            ),
        };

        let final_path = staged_path.with_extension("xml");
        std::fs::write(&final_path, final_content)?;
        std::fs::remove_file(&staged_path).ok();
    }
    Ok(())
}
```

**Substitution logic.** For each `<description>{B4F502887207:<href>}</description>`
match:

1. Resolve `<href>` to an absolute filesystem path under
   `project.output_dir`.
2. Read the sibling HTML (cached). If the read fails, emit
   `Q-12-16` *once per missing sibling* (not once per
   placeholder match — multiple feeds in the same project
   could reference the same sibling), strip the placeholder
   bracket, leave an empty `<description></description>`.
3. Run the appropriate L9 reader extractor:
   - `Partial` → `extract_first_para_html(html, max_length)`
   - `Full` → `extract_full_contents(html, &site_url, &href)`
4. Wrap in `<![CDATA[ ... ]]>` and substitute.

### Per-category sub-feeds

`feed.categories: ["Software", "Reproducibility"]` produces
two extra staged files per host:

- `<output_dir>/<stem>-software.feed-<type>-staged`
- `<output_dir>/<stem>-reproducibility.feed-<type>-staged`

The category name is `to_lowercase()`-ed (UTF-8 aware in Rust).
Each sub-feed's items are filtered to those whose
`categories: [...]` list contains the un-lowercased original
category name. The channel binding for sub-feeds uses:

- `link`: same as main feed but with
  `#category=<URI-encoded-category>` appended (mirrors Q1).
- `feed-link`: absolute URL to the sub-feed file.
- `title` / `description` / `image`: same as main feed.

The host page's `<link rel="alternate">` only points at the
main feed (matches Q1; sub-feeds are reachable from category
links inside the main listing UI).

### RFC 822 `pubDate` formatting (server-side, no pipe)

L9's binding pre-computes `pub_date_rfc822` from `item.date`
using the `time` crate (already a `quarto-core` dep). The
template surface receives the formatted string verbatim;
no `date_format` pipe is added in L9 (deferred — see
§"Settled inputs" and §"Out of scope"). Sketch:

```rust
// in feed/binding.rs
fn format_pub_date_rfc822(date_str: &str) -> Option<String> {
    use time::format_description::well_known::{Rfc2822, Rfc3339};
    use time::macros::format_description;

    // Accept (in order): RFC 3339 ("2026-05-08T10:30:00Z"),
    // RFC 2822 ("Thu, 08 May 2026 10:30:00 +0000"),
    // ISO-8601 date-only ("2026-05-08") interpreted as
    // midnight UTC.
    let dt = time::OffsetDateTime::parse(date_str, &Rfc3339)
        .or_else(|_| time::OffsetDateTime::parse(date_str, &Rfc2822))
        .or_else(|_| {
            let date_fmt = format_description!("[year]-[month]-[day]");
            time::Date::parse(date_str, date_fmt)
                .map(|d| d.with_hms(0, 0, 0).unwrap().assume_utc())
        })
        .ok()?;

    dt.format(&Rfc2822).ok()
}
```

The output is the RFC 2822 form `"Thu, 08 May 2026 10:30:00 +0000"`,
which is what RSS 2.0's `<pubDate>` requires (RFC 822 / RFC 2822
date-time — they are interchangeable for the format strings
RSS readers accept).

### Image metadata via `imagesize`

```rust
// in feed/binding.rs

#[cfg(not(target_arch = "wasm32"))]
fn build_item_image(
    item: &ListingItem,
    project_dir: &Path,
    site_url: &str,
) -> Option<FeedImage> {
    let src = item.image.as_ref()?;

    if is_absolute_url(src) || src.starts_with("data:") {
        return Some(FeedImage {
            url: src.clone(),
            attrs: String::new(),  // no width/height/type
        });
    }

    // Local path; resolve relative to project root.
    let abs_path = project_dir.join(src);
    let url = absolute_url(site_url, src);
    let attrs = match imagesize::size(&abs_path) {
        Ok(sz) => {
            let (h, w) = scale_to_feed_dimensions(sz.height as u32, sz.width as u32);
            let mime = mime_for_path(&abs_path);
            let mime_attr = mime.map(|m| format!(r#" type="{}""#, m)).unwrap_or_default();
            format!(r#"{} width="{}" height="{}""#, mime_attr, w, h)
        }
        Err(_) => String::new(),
    };
    Some(FeedImage { url, attrs })
}
```

The `attrs` string is pre-built and dropped into the
template via `$item.image.attrs$` — no template-side
formatting. `mime_for_path` is a small helper covering
the five image formats Q1 supports (png, jpg, gif, webp,
svg).

`scale_to_feed_dimensions` mirrors Q1's
`feedImageSize(height, width)`:

```rust
const MAX_HEIGHT: u32 = 400;
const MAX_WIDTH: u32 = 144;

fn scale_to_feed_dimensions(height: u32, width: u32) -> (u32, u32) {
    if height <= MAX_HEIGHT && width <= MAX_WIDTH {
        return (height, width);
    }
    let h_scale = MAX_HEIGHT as f64 / height as f64;
    let w_scale = MAX_WIDTH as f64 / width as f64;
    let scale = h_scale.min(w_scale);
    (((height as f64) * scale).round() as u32, ((width as f64) * scale).round() as u32)
}
```

### Pipeline placement and stage wiring

L9 doesn't touch the stage graph. Its two transforms
register inside `AstTransformsStage` alongside the existing
listing transforms. Their order in the chain:

```
... existing transforms ...
ListingGenerateTransform          ← populates ctx.resolved_listings
ListingRenderTransform
CategoriesSidebarTransform
ListingFeedLinkTransform          ← NEW; injects <link rel=alternate>
ListingFeedStageTransform         ← NEW; writes staged file (native only)
... existing transforms ...
```

Order rationale:
- `ListingFeedStageTransform` reads `ctx.resolved_listings`
  → must run after `ListingGenerateTransform`.
- `ListingFeedLinkTransform` reads
  `ctx.resolved_listings[*].listing.feed` and the channel
  title from `feed.title || website.title` → must run after
  `ListingGenerateTransform`.
- Either can come first; we put the link transform first
  by convention (head-meta edits typically go before
  side-effect-heavy transforms).

The post-render step is added in
`crates/quarto-core/src/project/orchestrator.rs:266` (right
after L7's `substitute_listing_placeholders`):

```rust
super::listing::feed::complete_staged_feeds(
    project, runtime, diagnostics,
)?;
```

## Diagnostic codes

L9 surface:

- **`Q-12-15`** (NEW) — "Listing feed configured but
  `website.site-url` is not set; feed skipped." Catalog
  text:
  > A listing has `feed:` configured, but the project's
  > `website.site-url` is missing. Feeds require an
  > absolute base URL to construct item links. Set
  > `website.site-url` in `_quarto.yml` to enable feed
  > generation. The listing host page renders correctly
  > otherwise.
  Emitted **once per project**, not per host. Surfaces
  during the first `ListingFeedStageTransform` apply that
  finds a feed-configured listing without `site-url`.
- **`Q-12-16`** (NEW) — "Listing feed: sibling output
  `<path>` could not be read; description left empty for
  this item." Catalog text:
  > While building a listing feed, the substitution step
  > tried to read the rendered HTML for an item but the
  > file could not be opened. The feed was emitted with
  > an empty `<description>` for this item. Re-running
  > the project render usually resolves transient I/O
  > issues; persistent failures suggest a misconfigured
  > output directory.
  Emitted at most once per missing sibling per
  `post_render` invocation (cached).

L9 does **not** introduce any other catalog entries.
Existing codes (`Q-12-1` through `Q-12-14`) are unchanged.

## Test plan (TDD)

Per CLAUDE.md: write tests, watch fail, implement, watch pass.
Tests are organized by L9 module; the largest concentration is
in `feed/stage.rs` and `feed/complete.rs`.

### Phase 1 — diagnostic catalog + scaffolding

1. **`error_catalog_has_q_12_15_and_q_12_16`** — assert both
   new codes are present with the expected titles.
2. **`feed_module_compiles_native_and_is_absent_on_wasm`**
   — `cargo build --workspace` succeeds; `cargo build
   --target wasm32-unknown-unknown -p wasm-quarto-hub-client`
   succeeds and does NOT pull in `imagesize`. Verify via
   `cargo tree --target wasm32-unknown-unknown -p
   wasm-quarto-hub-client | grep -i imagesize` returning
   no output.

### Phase 2 — *(deferred)* `date_format` pipe

The original plan listed four tests (numbered 3–6) for a
new `date_format` doctemplate pipe. **Deferred at impl-start
2026-05-08** to a follow-up bd. See §"Out of scope for L9".
Tests #3–6 are intentionally absent from the L9 diff;
later test numbers below are unchanged for cross-reference
stability.

### Phase 3 — feed binding

7. **`build_channel_context_full_metadata`** — given a
   `Listing` with all `feed.*` fields set + a project
   meta with `website.site-url`/`title`/`description`/
   `image`, the channel binding has the expected
   pre-escaped values (e.g. `<` in title becomes `&lt;`).
8. **`build_channel_context_falls_back_to_website_keys`**
   — `feed.title` unset → uses `website.title`. Same for
   description and image.
9. **`build_item_context_pubdate_rfc822_format`** —
   item with `date: 2026-05-08` produces
   `pub_date_rfc822: "Fri, 08 May 2026 00:00:00 +0000"`.
10. **`build_item_context_xml_escapes_title_description`**
    — item title with `<script>` is escaped before
    template insertion.
11. **`build_item_context_metadata_type_inlines_description`**
    — for `metadata` feed type, item context contains
    `description-element: "<description><![CDATA[...]]></description>"`.
12. **`build_item_context_partial_full_emits_placeholder`**
    — for `partial` and `full`, item context contains
    `description-element: "<description>{B4F502887207:posts/foo.html}</description>"`.
13. **`build_item_image_local_with_imagesize`** — item
    with local `image: posts/cover.png` produces
    `image: { url: "https://example.com/posts/cover.png", attrs: " type=\"image/png\" width=\"...\" height=\"...\"" }`.
    Use a real PNG fixture under `tests/fixtures/`.
14. **`build_item_image_absolute_url_no_attrs`** — item
    with `image: https://example.com/already-abs.png`
    keeps the URL and emits empty attrs.
15. **`build_item_image_unreadable_file_no_attrs`** —
    item with `image: missing.png` (file not on disk)
    emits the URL and empty attrs (no panic).

### Phase 4 — staged-file write (`ListingFeedStageTransform`)

16. **`stage_writes_metadata_feed_with_inline_descriptions`**
    — fixture: 2-post project, `feed: { type: metadata }`.
    After Pass-2, file `_site/posts.feed-metadata-staged`
    exists and contains both items with their descriptions
    inlined as CDATA. No placeholder tokens.
17. **`stage_writes_partial_feed_with_placeholders`** —
    same fixture, `type: partial`. Staged file contains
    placeholder tokens `<description>{B4F502887207:posts/foo.html}</description>`
    for each item.
18. **`stage_writes_full_feed_with_placeholders`** — same
    fixture, `type: full`. Same placeholder shape.
19. **`stage_emits_q_12_15_when_no_site_url`** — fixture
    with `feed: true` but no `website.site-url`. Staged
    file is NOT written; `Q-12-15` is in `ctx.diagnostics`
    once.
20. **`stage_writes_per_category_subfeeds`** — fixture
    with `feed: { type: full, categories: [Software,
    Reproducibility] }` and posts tagged with each.
    Staged files include `posts.feed-full-staged`,
    `posts-software.feed-full-staged`,
    `posts-reproducibility.feed-full-staged`. Each
    sub-feed's body contains only items carrying that
    category.
21. **`stage_truncates_to_feed_items_count`** — fixture
    with 30 posts and `feed: { items: 5 }`. Staged file
    contains 5 `<item>` elements.
22. **`stage_uses_default_20_items`** — fixture with 30
    posts and `feed: true`. Staged file has 20 items.
23. **`stage_skips_when_no_listing_feed`** — fixture with
    a listing but no `feed:`. No staged file is written.
24. **`stage_xml_stylesheet_pi_emitted_when_set`** —
    fixture with `feed: { xml-stylesheet: feed.xsl }`.
    Staged preamble contains
    `<?xml-stylesheet type="text/xsl" media="screen" href="feed.xsl"?>`.

### Phase 5 — link injection (`ListingFeedLinkTransform`)

25. **`link_inject_adds_alternate_for_main_feed`** —
    fixture with `feed: true` + `website.site-url`. The
    rendered HTML's `<head>` contains
    `<link rel="alternate" type="application/rss+xml" title="<feed-title>" href="posts.xml">`.
26. **`link_inject_skips_when_no_feed`** — fixture with
    a listing but no `feed:`. No alternate link in head.
27. **`link_inject_skips_when_no_site_url`** — fixture
    with `feed: true` but no `website.site-url`. No
    alternate link (the file wouldn't exist anyway).
28. **`link_inject_runs_on_wasm`** — fixture rendered
    via WASM API: head still contains the alternate
    link tag. The link points at a non-existent file in
    the VFS, but the tag is harmless.
29. **`link_inject_uses_feed_title_then_website_title`**
    — fixture with `feed: { title: My Feed }`. The
    link's `title` attribute is `"My Feed"`. Without
    `feed.title`, falls back to `website.title`.

### Phase 6 — reader extension (`reader_ext.rs`)

30. **`extract_first_para_html_returns_inner_html`** —
    given `<main class="content"><p>Hello <em>world</em>.</p></main>`,
    output is `"Hello <em>world</em>."`.
31. **`extract_first_para_html_strips_anchors`** — given
    `<p>Click <a href="#x">here</a>.</p>`, output is
    `"Click here."` (anchor unwrapped).
32. **`extract_first_para_html_truncates_at_word_boundary`**
    — long para; max_length=20. Output ≤ 20 visible
    chars, cut at last space.
33. **`extract_full_contents_rewrites_relative_to_absolute`**
    — given `<a href="../foo.html">link</a>` and
    site-url + sibling-href context, output is
    `<a href="https://example.com/foo.html">link</a>`.
34. **`extract_full_contents_strips_local_anchor_hrefs`**
    — given `<a href="#section-1">Top</a>`, output is
    `Top` (anchor element removed; text preserved).
35. **`extract_full_contents_skips_title_block_header`**
    — given a `main.content` with a
    `<header id="title-block-header">` containing the
    post title, the extracted full contents do not
    include the title block.
36. **`extract_full_contents_returns_none_when_no_main`**
    — HTML without `main.content`: returns `None`.

### Phase 7 — post-render completion (`complete_staged_feeds`)

37. **`complete_renames_metadata_staged_to_xml`** —
    fixture with `_site/posts.feed-metadata-staged`
    pre-existing (synthesized in test). After
    `complete_staged_feeds`, `_site/posts.xml` exists
    with the same content; staged file is gone.
38. **`complete_substitutes_partial_descriptions`** —
    fixture with staged file containing placeholders
    + sibling HTML files containing first paragraphs.
    After completion, `_site/posts.xml`'s
    `<description>` tags carry the firstPara HTML wrapped
    in CDATA.
39. **`complete_substitutes_full_descriptions_with_absolute_urls`**
    — same as 38 but `type: full`. CDATA contains the
    full `main.content` HTML, with relative links
    rewritten to absolute.
40. **`complete_emits_q_12_16_when_sibling_missing`** —
    staged file references `posts/missing.html` which
    isn't on disk. After completion, `Q-12-16` is in
    `diagnostics` *once* (cached); the placeholder is
    replaced with empty `<description></description>`;
    final `.xml` is still written.
41. **`complete_caches_sibling_reads_per_call`** — two
    feeds reference the same sibling. The sibling is
    read once (instrument the cache; verify hit count
    on the second placeholder).
42. **`complete_skips_when_no_site_url`** — staged file
    exists but `website.site-url` is unset. Completion
    is a no-op (staged file remains; the
    `ListingFeedStageTransform` should never have
    written it without site-url, but completion is
    defensive).
43. **`complete_handles_concurrent_per_category_files`**
    — staged files for main + 2 categories. All three
    finalize correctly; sub-feed sibling reads share the
    cache with the main feed.

### Phase 8 — End-to-end CLI verification

44. **`pipeline_e2e_metadata_feed`** — fixture project:

    ```
    _quarto.yml         # project.type: website,
                        # website.site-url: https://example.com,
                        # website.title: Example
    posts.qmd           # listing host with feed: { type: metadata }
    posts/foo.qmd       # post with title: Foo, date: 2026-05-01
    posts/bar.qmd       # post with title: Bar, date: 2026-05-02
    ```

    `cargo run --bin q2 -- render` produces
    `_site/posts.xml` with channel + 2 items. Output
    inspected: title, description, link, pubDate present;
    descriptions are the post's metadata `description`
    fields (not engine-rendered).

45. **`pipeline_e2e_partial_feed`** — same fixture but
    `type: partial`. Posts have multi-paragraph bodies.
    `_site/posts.xml`'s descriptions are wrapped in
    CDATA and contain the first paragraph HTML of each
    post. URLs are absolute (e.g. `https://example.com/posts/foo.html`).

46. **`pipeline_e2e_full_feed_with_categories`** —
    fixture with `feed: { type: full, categories:
    [Software, Reproducibility] }`. Each post has
    `categories: [...]`. After render:
    - `_site/posts.xml` contains all items with full
      `main.content` HTML in CDATA.
    - `_site/posts-software.xml` contains only
      Software-tagged items.
    - `_site/posts-reproducibility.xml` contains only
      Reproducibility-tagged items.
    - Each post's HTML contains `<link rel="alternate"
      type="application/rss+xml" href="posts.xml">` in
      `<head>` (only the main feed; not sub-feeds).

47. **`pipeline_e2e_no_site_url_emits_warning_no_files`**
    — fixture with `feed: true` but no `website.site-url`.
    Render succeeds; no `.xml` files in `_site/`;
    `Q-12-15` in stderr.

### Hub-client smoke

48. **WASM build doesn't pull `imagesize` or `scraper`-via-feed.**
    Verified via `cargo xtask verify`: the hub-client
    build succeeds, and the L9 module's `cfg(not(target_arch
    = "wasm32"))` gating excludes `feed/` entirely.
49. **Hub-client preview shows alternate link tag.**
    Real-browser session against a fixture with
    `feed: true`. Confirm:
    - Page renders with the `<link rel="alternate">` in
      head (visible in DevTools).
    - Clicking the link shows a 404 (expected — no
      feed file in the VFS).
    - No console errors, no panic.

    **Browser smoke is recorded in close-out, not gating
    L9 commit.** The CLI verification (#44–#47) is the
    primary correctness signal.

## End-to-end CLI verification record

To be filled in by the L9 implementation session. Per
CLAUDE.md, recording the actual invocation, the inspected
output snippets, and an explicit "output inspected" note.

Tentative record format (mirrors L8):

#### Fixture 1 — metadata feed (`/tmp/l9-fixture-metadata/`)

Layout:

```
_quarto.yml
posts.qmd
posts/foo.qmd
posts/bar.qmd
```

Invocation:
```
cargo run --bin q2 --quiet -- render /tmp/l9-fixture-metadata
```

Expected snippets from `_site/posts.xml`:
- `<title>Example</title>` (channel title)
- `<atom:link href=".../posts.xml" rel="self" .../>`
- `<item>` with `<title>Foo</title>`, `<link>https://example.com/posts/foo.html</link>`,
  `<description><![CDATA[ ... ]]></description>` containing the
  metadata description.
- `<pubDate>` in RFC 822 format.

#### Fixture 2 — full feed with categories (`/tmp/l9-fixture-full-categories/`)

(Layout, invocation, expected snippets to be filled in
by impl session.)

## Branch / worktree

L9 starts from the current `feature/listings` head
(post-L8 merge `cd2410fa`). Worktree:

```
.worktrees/bd-o90m-listings-rss-feeds/
```

Branch: `beads/bd-o90m-listings-rss-feeds`, branched off
`feature/listings`.

Per `.claude/rules/worktrees.md`:

```bash
cd .worktrees/bd-o90m-listings-rss-feeds
echo "../../../.beads" > .beads/redirect
npm install
cargo xtask verify --skip-hub-build  # baseline before changes
```

Before starting, the L9 session must record:

- Current `feature/listings` HEAD hash (`cd2410fa` at
  plan time; verified at impl-start as `b8c9b8b5`
  after committing the L9 plan doc onto the branch
  — the worktree is branched from `b8c9b8b5`).
- Baseline test count: **8907 workspace tests** as of
  impl-start 2026-05-08 (via
  `cargo nextest list --workspace --message-format=json`).
  L9 close-out should report a delta of ≥ +35 new
  tests.

## Pipeline-builder wiring

L9 changes are confined to:

- Two new transforms registered in the Pass-2 transform
  list. `ListingFeedLinkTransform` is registered in
  *both* `build_html_pipeline_stages_with_apply_config`
  and `build_wasm_html_pipeline` (head-meta edit is
  target-agnostic). `ListingFeedStageTransform` is
  registered in the native pipeline only — its body is
  cfg-gated to native and registering it on WASM would
  be a no-op.
- One new post-render call in
  `WebsiteProjectType::post_render` (native-only block).
- `feed/` submodule under `project/listing/`. The
  submodule itself is **not** uniformly cfg-gated —
  most files are native-only, but the link-inject
  transform's binding-reader functions (no I/O) are
  target-agnostic. Concretely: `feed/mod.rs` declares
  `pub mod link_inject;` unconditionally and the rest
  of the submodule (`binding`, `stage`, `complete`,
  `reader_ext`, `templates`) under
  `#[cfg(not(target_arch = "wasm32"))]`.
- `imagesize` dep in `quarto-core/Cargo.toml`
  (target-gated to native).
- Two new entries in `error_catalog.json`.

Not changed in L9 (originally planned, deferred):

- `date_format` pipe in `quarto-doctemplate/src/pipes.rs`
  — see §"Settled inputs" → "Pipes" entry.

No new traits, no new context fields, no new artifact
scopes. The transforms read from `RenderContext` and
`ast.meta`; the post-render step reads from
`ProjectContext` + walks `output_dir`.

## Risks and mitigations

- **Risk: `imagesize` pulls a transitive dep that breaks
  the WASM build despite our cfg gating.** *Mitigation:*
  L9's first commit is the dep edit + a `cargo xtask
  verify` run that confirms the WASM build still
  succeeds. If it doesn't, fall back to bare
  `<media:content url="..." medium="image"/>` (no
  dimensions / type) and file the dep follow-up.
- **Risk: doctemplate whitespace handling produces
  malformed XML.** *Mitigation:* L9's templates use raw
  `\n` newlines and `$if$` / `$for$` constructs that the
  evaluator already handles. The XML format tolerates
  extra whitespace between tags (RSS readers ignore it).
  Snapshot tests lock the output shape.
- **Risk: `extract_full_contents`'s urls-to-absolute
  rewrite hits an edge case (mailto:, javascript:, query
  strings).** *Mitigation:* the rewriter has a guard:
  rewrite only when the href doesn't start with a scheme
  (`^[a-z]+:`) and isn't a fragment-only `#...`.
  Mailto + javascript pass through unchanged. Snapshot
  test covers each case.
- **Risk: per-category sub-feeds duplicate item content,
  inflating output size for large projects.** *Mitigation:*
  each item rendered once into the binding, then
  per-category sub-feeds reuse the rendered item HTML.
  The substitution work *is* duplicated (each sub-feed
  reads sibling outputs again), but the cache is shared,
  so each sibling is read once total per `post_render`.
- **Risk: `Q-12-15` fires once per host but multiple
  hosts can declare feeds.** *Mitigation:* emit
  `Q-12-15` once per *project* (track an
  `already_warned: bool` on a project-scoped state, or
  on `ctx.diagnostics` via dedup). v1 emits once per
  call to `ListingFeedStageTransform`; if the project
  has 5 feed-configured hosts, the user sees 5 warnings.
  *Acceptable* in v1; a follow-up bd consolidates.
- **Risk: `imagesize::size` panics on a malformed image
  file.** *Mitigation:* the call is wrapped in `match
  ...{ Ok(sz) => ... Err(_) => empty attrs }`. An
  unreadable image gets bare `<media:content>` (same as
  Q1's behavior).
- **Risk: pre-existing staged files from a previous run
  collide with this run's output.** *Mitigation:*
  `complete_staged_feeds` deletes each staged file after
  rename. If a previous run aborted before completion,
  re-running picks up the staged file from the broken
  run; v1 trusts the staged file's content (it was
  written by the same code path). The post-completion
  delete then cleans up. Snapshot test covers the
  "stale staged file" case.
- **Risk: file walk in post-render walks into nested
  output directories that contain user data files
  matching the staged-extension pattern.** *Mitigation:*
  the walker filters strictly on `.feed-{full,partial,
  metadata}-staged` extensions. Authors who happen to
  have files matching this exact pattern in their
  output (extremely rare) would see them deleted; this
  is acceptable v1 behavior since the pattern is
  intentionally Q1-verbatim and bizarre. Document.
- **Risk: snapshot churn in the existing rendered-listing
  test suite.** *Mitigation:* L9's two transforms only
  fire when `feed:` is set or when injecting head
  metadata for feed-configured listings. Existing
  fixtures (which don't have `feed:`) are unaffected.
  `cargo insta test` should report new snapshots only,
  no diffs on existing.
- **Risk: hub-client preview's `<link rel="alternate">`
  is confusing — the link exists, but the file behind
  it doesn't.** *Mitigation:* the listings reference
  page in `docs/` (when it lands; bd-u4ow already
  filed for L8) carries a callout: "RSS feed
  generation is a CLI-only feature. The
  `<link rel=\"alternate\">` link tag appears in
  hub-client previews for byte-equivalence with the
  CLI render, but the linked feed file is only
  written by `quarto render`."

## Edge-case behavior (settled)

1. **Listing with `feed: true` but no `contents:`
   (and no posts in the project).** Staged file is
   written with zero `<item>` elements. Final feed has
   channel metadata but no items. RSS readers display
   it as "empty feed." Acceptable; matches Q1.
2. **Listing with `feed: { items: 0 }`.** Treated as
   "default 20 items" (Q1's behavior — `0` is
   indistinguishable from missing in JS).
3. **Item with no `date`.** `<pubDate>` is omitted
   for that item; the `lastBuildDate` falls back to
   "now" when no item has a date. Both match Q1.
4. **Item with `date` in the future.** Emitted
   verbatim; RSS readers usually display future-dated
   items at the top. Q2 doesn't filter; matches Q1.
5. **Item with empty `title`.** Q1 filters such items
   from the feed (`prepareItems` requires `item.title
   !== undefined && item.path !== undefined`). v1
   matches: items without both fields are skipped.
6. **Multiple feeds on one host page (multiple
   listings, each with `feed:`).** Each listing writes
   its own set of staged files. v1 qualifies the
   filename with the listing id when more than one
   listing is on the page; with one listing (the common
   case), the filename is the unsuffixed
   `<host-stem>.feed-...`. With multiple listings,
   each gets `<host-stem>-<listing-id>.feed-...`. No
   warning fires — multi-listing hosts are
   well-supported in L3 and the qualified filename
   avoids collisions cleanly. See §"Decisions log" D7.
   Test #51 covers.
7. **`feed.title` that contains XML-special characters.**
   Server-side escaping handles `<`, `>`, `&`, `"`,
   `'`. Snapshot test #7 covers.
8. **`feed.image` pointing at a 100x100 PNG.** The
   `feedImageSize` scaler returns `(100, 100)` (no
   scale; both dimensions ≤ max). Snapshot covers.
9. **`feed.image` pointing at a 4000x3000 PNG.** The
   scaler returns `(400, 144*3000/4000)` = `(400, 108)`
   — bottlenecked by max-height. Test #13 covers.
10. **`xml-stylesheet` path with absolute URL.** v1
    emits the user's value verbatim into the PI. No
    URL rewriting (Q1 also doesn't, except in
    `kFeedOptions.transform.urlsToAbsolute` which
    only applies to feed *content*, not the
    stylesheet PI).

## Decisions log

- **D1 (staged-file architecture):** user-confirmed
  2026-05-08. Staged file on disk per Q1; substitution
  in post-render. Native-only.
- **D2 (all three feed types in v1):** user-confirmed
  2026-05-08. Metadata fast-path + partial + full.
- **D3 (per-category sub-feeds in v1):** user-confirmed
  2026-05-08.
- **D4 (server-side XML escaping):** user-confirmed
  2026-05-08. No `escape_xml` pipe.
- **D5 (link injection via Pass-2 transform, both
  targets):** user-confirmed 2026-05-08. WASM gets a
  dead but harmless link tag; documented.
- **D6 (`imagesize` for image dimensions, native-only):**
  user-confirmed 2026-05-08, with explicit awareness of
  WASM dep gating. v1 verifies the gating with `cargo
  xtask verify` before committing.
- **D7 (per-listing feed; qualified filename only for
  multi-listing pages):** user-confirmed 2026-05-08.
  **Q1 divergence (verified at impl-start in
  `external-sources/quarto-cli/src/project/types/website/listing/website-listing-feed.ts`):**
  Q1 stores `feed:` on `ListingSharedOptions` (host-page
  level) and merges items from every listing on the page
  into a single feed. Q2's data model (L2) instead stores
  `feed:` on each `Listing` — so we get one feed per
  feed-configured listing. The common case (one listing
  per host) produces the same `posts.xml` Q1 emits.
  Multi-listing pages are a Q2-only capability and emit
  `posts-<listing-id>.xml` per feed-configured listing.
  No collisions; no `Q-12-17` warning needed. If a future
  user reports they wanted Q1's merge semantics, the
  conservative response is to add an opt-in
  `feed-merge: true` page-level config; not in v1.
- **D8 (no new pipes in L9):** user-confirmed 2026-05-08.
  Originally specified `date_format <fmt>`; revised at
  impl-start to "no new pipes" once it became clear the
  L9 templates don't use the pipe (binding pre-computes
  `pub_date_rfc822` server-side via the `time` crate).
  Adding a pipe later would also require a tree-sitter
  grammar change (the pipe set is grammar-fixed; see
  `crates/tree-sitter-doctemplate/grammar/grammar.js:56`).
  Filed as a close-out follow-up.
- **D9 (three embedded feed templates):** user-confirmed
  2026-05-08. Mirrors Q1's preamble/item/postamble
  layout; user-readable for future docs.
- **D10 (full-reader transforms in v1: urls-to-absolute
  + anchor-strip only):** user-confirmed 2026-05-08.
  Math + syntax-highlight class maps deferred.
- **D11 (`reader_ext.rs` lives under `feed/`, not
  extending L7's `reader.rs`):** user-confirmed
  2026-05-08. Preserves L7's bracketing rule.
- **D12 (generator string `quarto-2`):** user-confirmed
  2026-05-08. v1 stable hardcoded string. Follow-up bd
  swaps in real version when the version story
  stabilizes.
- **D13 (worktree on `feature/listings`):** branch
  `beads/bd-o90m-listings-rss-feeds` at
  `.worktrees/bd-o90m-listings-rss-feeds/`, branched
  off the current `feature/listings` head
  (`cd2410fa` at plan time — confirm at impl start).
  Same convention as L1 / L3 / L5 / L6 / L7 / L8.

## Implementation steps

Follow CLAUDE.md TDD: write tests, watch fail, implement,
watch pass.

### Preparation

- [x] Re-read `claude-notes/instructions/testing.md` and
      `claude-notes/instructions/coding.md`.
- [x] Re-read `.claude/rules/wasm.md` (cfg gating; `?Send`
      on async traits).
- [x] Re-read the L7 sub-plan §"scraper dep gating" and
      L8 sub-plan §"WASM behavior" — L9 follows both
      precedents.
- [x] Confirm `feature/listings` head is the post-L8
      merge. Verified at impl-start: branch tip is
      `b8c9b8b5` (this plan-doc commit on top of the
      `cd2410fa` L8 merge).
- [x] Create the worktree at
      `.worktrees/bd-o90m-listings-rss-feeds/` per
      §"Branch / worktree". Branch
      `beads/bd-o90m-listings-rss-feeds`.
- [x] `npm install` in the worktree.
- [x] Add `.beads/redirect` per worktree rules.
- [x] Baseline: `cargo xtask verify --skip-hub-build
      --skip-hub-tests` clean; recorded **8907 workspace
      tests** as the baseline (via
      `cargo nextest list --workspace --message-format=json`).

### TDD phase 1 — diagnostics + dep edit (module skeleton deferred)

- [x] Write test #1 (`error_catalog_has_q_12_15_and_q_12_16`
      in `crates/quarto-error-reporting/src/catalog.rs`).
- [x] Test #2 is a build-system check, not a Rust test:
      verified manually via
      `cargo tree --target wasm32-unknown-unknown -p wasm-quarto-hub-client | grep imagesize`
      (empty output) and a successful
      `cargo build --target wasm32-unknown-unknown -p wasm-quarto-hub-client`.
      Recorded in `claude-notes/plans/...`.
- [x] Add `Q-12-15` and `Q-12-16` to `error_catalog.json`.
- [x] Add `imagesize = "0.13"` to
      `crates/quarto-core/Cargo.toml` under
      `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`.
- [ ] ~~Create `feed/` skeleton with empty files~~ —
      deferred. Phase 1's goal is "the dep + diagnostic
      edits land cleanly without breaking WASM"; the
      empty-stub scaffold would create half-finished
      files. Each subsequent phase creates its own
      `feed/<file>.rs` when the test for it is written.
- [x] Confirm WASM build succeeds and `cargo tree` shows
      `imagesize` is NOT pulled into the WASM build.
- [x] Tests pass: `cargo nextest run -p quarto-error-reporting`
      → 45 passed, 0 failed.

### TDD phase 2 — *(deferred)* `date_format` pipe

Deferred at impl-start 2026-05-08. Tests #3–6 are not
written; the pipe itself ships in a follow-up bd. The
RFC 822 `pubDate` formatting L9 needs is server-side in
`feed/binding.rs` (test #9 covers the binding's
`pub_date_rfc822` field).

### TDD phase 3 — feed binding

- [x] Write tests #7–15. Implementation landed alongside
      tests in `feed/binding.rs`: 32 unit tests cover XML
      escaping (text + attr forms), URL joining, RFC 2822
      / RFC 3339 / date-only parsing, image dimension
      scaling, MIME mapping, channel-builder cascade
      (feed.* → website.* → empty), per-item description-
      element shape (CDATA for metadata feeds; placeholder
      envelope for partial/full), local-PNG imagesize
      integration with synthetic 100x100 and 4000x3000
      fixture headers, absolute-URL / data-URI / unreadable-
      file fallback to empty `attrs`, and `xml-stylesheet`
      plumbing.
- [x] Implement `feed/binding.rs` as `build_feed_channel`
      (channel-level) and `build_feed_item` (per-item)
      returning typed `FeedChannel` / `FeedItem` structs
      (ergonomic for the upcoming stage transform — the
      template-context conversion will be a thin wrapper
      built in phase 4 alongside the templates). Server-
      side XML escaping via small inline `xml_escape_text`
      / `xml_escape_attr` helpers (no new dep).
- [x] Implement `build_item_image` with `imagesize` lookup
      + scaling. The scaling helper `scale_to_feed_dimensions`
      mirrors Q1's `feedImageSize` exactly (max 400h × 144w,
      bottleneck on the smaller axis ratio).
- [x] Tests pass: `cargo nextest run -p quarto-core
      'project::listing::feed::binding::tests'` →
      32 passed, 0 failed.
- [x] Added `parsing` feature to the `time` crate in
      `quarto-core`'s `Cargo.toml` (already had
      `formatting` + `macros`); needed for
      `OffsetDateTime::parse` / `Date::parse`.

### TDD phase 4 — staged-file write transform

- [x] Write tests #16–24 plus an extra
      `stage_qualifies_filename_for_multi_feed_hosts` for D7.
- [x] Embed the three `.template` files via `include_str!`
      (in `feed/stage.rs`, not `feed/mod.rs` — keeps the
      template constants alongside the transform that
      consumes them; same outcome). Templates files live
      under `feed/templates/{preamble,item,postamble}.template`.
- [x] Implement `feed/stage.rs::ListingFeedStageTransform`.
      Includes typed `FeedChannel`/`FeedItem` →
      `TemplateContext` lifters, item-truncation logic
      (default 20; `feed.items: 0` treated as missing),
      a `most_recent_item_date` helper for `lastBuildDate`,
      and Q-12-15 emission when `website.site-url` is
      missing (once per transform invocation).
- [x] Register the transform in the Pass-2 transform list,
      after `CategoriesSidebarTransform` in
      `pipeline.rs:build_html_pipeline_stages_with_apply_config`.
      Native-only registration (gated with
      `#[cfg(not(target_arch = "wasm32"))]` at the push
      site, mirroring the cfg gate in `feed/mod.rs`).
- [x] Tests pass: `cargo nextest run -p quarto-core
      'project::listing::feed'` → 42 passed
      (32 binding + 10 stage). Full workspace
      `cargo nextest run --workspace` → 8755 passed.
      `cargo xtask lint` clean. `npm run build:wasm`
      (hub-client) clean.

### TDD phase 5 — link injection transform

- [x] Write tests #25–29 plus an extra
      `link_inject_multi_listing_emits_qualified_hrefs` for D7.
- [x] Implement `feed/link_inject.rs::ListingFeedLinkTransform`.
      Appends to `rendered.includes.header` (the slot
      `WebsiteFaviconTransform::apply_favicon` writes to
      via `append_to_rendered_header` —
      `crates/quarto-core/src/transforms/website_favicon.rs:74`).
      The helper is duplicated locally for now; a follow-up
      bd at close-out hoists it to a shared util once a
      third caller appears.
- [x] Register in the Pass-2 transform list. Sits unconditionally
      in `build_transform_pipeline` after the stage-transform
      registration; both native and WASM pipelines reach it
      through `AstTransformsStage::new()` (which JIT-builds
      via `build_transform_pipeline`). Verified
      `npm run build:wasm` clean.
- [x] Tests pass: `cargo nextest run -p quarto-core
      'project::listing::feed::link_inject'` → 6 passed.
      Wider `cargo nextest run -p quarto-core` → 1869 passed
      (was 1863 before phase 4; +6 new).

### TDD phase 6 — reader extension

- [x] Write tests #30–36 plus a handful of helper unit tests
      (`collapse_relative_strips_dotdot`,
      `parent_href_string_handles_root_and_nested`,
      `visible_text_drops_tags_and_decodes_entities`,
      `extract_first_para_html_strips_anchors_with_inline_children`,
      `extract_first_para_html_returns_none_when_no_main`,
      `extract_first_para_html_skips_empty_p`,
      `extract_first_para_html_no_truncate_when_max_zero`,
      `extract_full_contents_rewrites_image_src`,
      `extract_full_contents_passes_external_url_through`,
      `extract_full_contents_resolves_site_rooted_path`,
      `extract_full_contents_keeps_external_anchors_intact`).
- [x] Implement `feed/reader_ext.rs::extract_first_para_html`.
      HTML-preserving when the para fits under `max_length`;
      degrades to plain-text + word-boundary truncation
      otherwise (see file-level "Limitations" note —
      truncation under `max_length` is a v1 follow-up). Anchor
      tags are unwrapped (Q1 partial-mode behavior).
- [x] Implement `feed/reader_ext.rs::extract_full_contents`
      with `urls-to-absolute` (a/link href + img/source/video/audio
      src), `<header id="title-block-header">` removal, and
      `a[href^="#"]` unwrap. External / data / mailto /
      javascript / scheme-relative URLs pass through unchanged.
      Site-rooted paths (`/about.html`) resolve against the
      site URL.
- [x] Tests pass: `cargo nextest run -p quarto-core
      'project::listing::feed::reader_ext'` → 18 passed.
      Wider `cargo nextest run -p quarto-core` → 1887 passed
      (was 1869 before phase 5; +18 new).

### TDD phase 7 — post-render completion

- [x] Write tests #37–43 plus an extra
      `complete_walks_nested_directories` (recursive walk
      catches `_site/posts/index.feed-...`) and a
      `staged_type_from_filename` unit test.
- [x] Implement `feed/complete.rs::complete_staged_feeds`.
      Per-call HashMap cache (`HashMap<PathBuf, Option<String>>`)
      avoids re-reading siblings shared across multiple
      feeds (e.g. the main feed and per-category sub-feeds
      on the same host). Recursive `std::fs::read_dir` walk
      filters strictly on the three staged extensions.
      Errors during one feed are reported as warnings and
      don't abort the whole step.
- [x] Wire the call into `WebsiteProjectType::post_render`
      after L7's `substitute_listing_placeholders`. The
      L9 reader extractors then see fully-finalized
      sibling HTML.
- [x] Tests pass: `cargo nextest run -p quarto-core
      'project::listing::feed::complete'` → 9 passed.
      Wider `cargo nextest run -p quarto-core` → 1896
      passed (was 1887 before phase 6).
- [x] WASM build still clean (`npm run build:wasm`) —
      orchestrator.rs's `complete_staged_feeds` call sits
      inside the existing `cfg(not(target_arch = "wasm32"))`
      block in `WebsiteProjectType::post_render`.

### TDD phase 8 — End-to-end CLI

- [ ] Build three real-binary fixtures
      (`/tmp/l9-fixture-metadata`,
      `/tmp/l9-fixture-partial`,
      `/tmp/l9-fixture-full-categories`). Render each
      via `cargo run --bin q2 --quiet -- render`.
      Inspect output by hand. Validate the resulting
      `.xml` against an offline RSS schema (e.g. a
      saved copy of the RSS 2.0 DTD or a hand-rolled
      sanity check: well-formed XML, has `<rss>`,
      `<channel>`, ≥ 1 `<item>`).
- [ ] Record the verification in §"End-to-end CLI
      verification record" above.

### Verification and close-out

- [ ] `cargo build --workspace` clean (no warnings).
- [ ] `cargo nextest run --workspace` — count delta
      ≥ +35 new tests added.
- [ ] `cargo xtask lint` clean.
- [ ] `cargo xtask verify` (full, including hub-client
      + WASM build).
- [ ] Hub-client browser smoke recorded in close-out
      (per CLAUDE.md §"End-to-end verification" — the
      smoke is a confirmation, not a blocker).
- [ ] Stop and request user permission before any push
      (per CLAUDE.md §"GIT PUSH POLICY").
- [ ] After user approval: `br update bd-o90m
      --status closed`.
- [ ] `br sync --flush-only && git add .beads/ &&
      git commit` from the **main repo**.
- [ ] Update the listings epic table to mark L9 closed
      with the merge commit hash.

### Filed follow-up bd issues

To be filed at L9 close-out:

1. **Math handling in `full` feeds** — port Q1's
   KaTeX/MathJax preservation. Reader extension; affects
   only `RssReaderOptions.math_handling`.
2. **Inline-code-style syntax-highlight class maps** —
   Q1 maps highlight classes to inline styles for feed
   readers without Quarto's CSS. Reader extension;
   affects only `RssReaderOptions.inline_code_style`.
3. **W3C feed-validator-grade output** — investigate any
   parse warnings raised by canonical RSS validators
   on L9's output and fix.
4. **`format.metadata.description` as channel description
   fallback** — third level in the cascade between
   `feed.description` and `website.description`.
5. **Title placeholder substitution** — engine-rendered
   title with math etc. for feed items. v1 uses
   metadata title.
6. **`Q-12-15` deduplication** — emit once per project,
   not once per host. v1 emits per-host
   (user-confirmed 2026-05-08); follow-up consolidates.
7. **Generator version string** — when version story
   stabilizes, emit `quarto-<version>` instead of
   `quarto-2`.
8. **Custom feed templates (`feed.template:`)** — author-
   supplied XML; new feature epic.
9. **Atom 1.0 emission** — currently RSS 2.0 only.
10. **`date_format` doctemplate pipe** — deferred at
    impl-start 2026-05-08 (D8). Implementation requires
    a tree-sitter grammar change in
    `crates/tree-sitter-doctemplate/grammar/grammar.js`
    for the new pipe-with-arg shape, plus a match arm
    in `crates/quarto-doctemplate/src/pipes.rs`. The L9
    binding pre-computes `pub_date_rfc822` server-side,
    so the pipe is not on L9's critical path.

## Filing reminder

This sub-plan corresponds to **one** bd issue:

- `bd-o90m` — L9, RSS feeds.

After impl, close with a reason that references the
landed commit. Update the issue description with a
one-line link to this file.

### Resolved at plan time (2026-05-08)

The four items the plan author originally flagged for
impl-start confirmation were all resolved by the user
in the planning session:

1. **D7** — qualified filename (`<stem>-<listing-id>.feed-...`)
   when multiple listings on a page; no `Q-12-17` code
   needed. Confirmed.
2. **`Q-12-15` per-host warnings in v1** — acceptable;
   per-project consolidation deferred to follow-up bd.
   Confirmed.
3. **D12 generator string `quarto-2`** — acceptable for
   v1; real-version follow-up bd files at close-out.
   Confirmed.
4. **`time` crate format-string surface** — moot once
   the `date_format` pipe was deferred. The L9 binding
   uses `time::format_description::well_known::Rfc2822`
   directly; no strftime-style translation needed.

### Resolved at impl-start (2026-05-08)

Additional items resolved in the implementation
hand-off session:

5. **Transform names.** Plan was written using the
   older L3-plan name `ListingResolveTransform`; the
   actual code uses `ListingGenerateTransform` +
   `ListingRenderTransform`. Plan updated throughout.
6. **`date_format` pipe deferred (D8 revised).**
   See §"Out of scope for L9" entry. Filed as a
   close-out follow-up bd.
7. **Link-injection slot.** `ListingFeedLinkTransform`
   appends to `rendered.includes.header` (the slot
   `WebsiteFaviconTransform` writes to via
   `apply_favicon` in
   `crates/quarto-core/src/transforms/website_favicon.rs:74`).
8. **Module gate granularity.** `feed/link_inject.rs`
   sits outside the `cfg(not(target_arch = "wasm32"))`
   gate; everything else under `feed/` is native-only.
9. **L7 reader vs L9 reader.** Strict duplication per
   the bracketing rule (D11 confirmed). L7's reader
   stays listings-display-only; L9's
   `feed/reader_ext.rs` is a sibling reader with its
   own `RssReaderOptions`. Shared helpers may emerge
   over time but are not introduced speculatively in
   v1.
