# Phase 7 — Post-render (sitemap, favicon, site-url / title-prefix)

**Date:** 2026-04-27
**Beads:** `bd-b9mz` (parent `bd-0tr6`).
**Parent plan:** `claude-notes/plans/2026-04-23-website-project-epic.md`
**Previous phase:** `claude-notes/plans/2026-04-24-websites-phase-6.md`
**Status:** Draft — pending user review.

## Goal of this phase

Phase 7 closes the **site-shape surface** for websites — the four
features that turn a folder of rendered pages into a site that knows
its own identity:

1. **Title prefix.** A page with `title: "Getting Started"` in a
   project where `website.title: "Quarto Docs"` should render as
   `<title>Getting Started – Quarto Docs</title>`.
2. **Favicon.** `website.favicon: favicon.ico` results in (a) the file
   copied to `_site/favicon.ico` and (b) `<link rel="icon"
   href="..." type="...">` injected into every page's `<head>`, with
   the href page-relative.
3. **Sitemap.** When `website.site-url` is set, emit
   `_site/sitemap.xml` listing every rendered page with its `<loc>`
   (absolute URL based on `site-url`) and `<lastmod>` (input file
   mtime).
4. **robots.txt.** If `<project>/robots.txt` exists, copy it. Else, if
   `website.site-url` is set, emit a one-line robots.txt pointing at
   the sitemap.

Three of the four (favicon link, title prefix) are **per-page**
contributions made during Pass 2; favicon copy, sitemap, and
robots.txt are **project-level** writes done in
`WebsiteProjectType::post_render` after Pass 2 finishes.

This phase does **not** implement:

- **Open Graph / Twitter card / social meta tags.** Q1 has these via
  `metadataHtmlPostProcessor`. Out of MVP — file as a follow-up bead
  when there's a real consumer.
- **Brand-aware favicon fallback.** Q1's favicon falls back to
  `brand.light.favicon`. Q2 doesn't have brand support yet; defer.
- **Multi-format favicon variants** (apple-touch-icon, multi-size).
  Single `<link rel="icon">` with a single href is the MVP. Q1 also
  emits one entry.
- **Empty-`index.html` filtering in the sitemap.** Q1 skips
  `index.html` files whose source `qmd` has no body. Q2 emits all
  pages; defer the filter once `DocumentProfile` carries an
  is-empty signal.
- **Draft-mode interaction with the sitemap.** Q1 omits drafts (or
  writes them with `<draft>` markers depending on `draft-mode`). Q2
  has `DocumentProfile.draft` but no draft-mode YAML config — defer
  the visibility logic, and for Phase 7 just emit every profiled
  page (drafts included). The eventual draft-mode bead will gate
  this in the same place Phase 6's body-link rewriter does.
- **Incremental sitemap merge.** Q1 reads the existing `sitemap.xml`,
  patches changed entries, writes back. Q2 today renders every page
  every time, so a fresh-write each run produces the same on-disk
  result as a read-merge-write. Phase 8's incremental rebuild will
  add the merge logic; Phase 7 ships fresh-write only. **The epic
  plan calls out "Incremental-aware: read existing, update, write" —
  we propose deferring that to Phase 8 because without incremental
  rebuilds there is no behavioral difference. User sign-off needed
  on this scope cut.**
- **`canonical-url`.** The full template already has a
  `$canonical-url$` slot (`template.rs:146-148`); Phase 7 could
  populate it from `website.site-url + output_href`. Recommend
  including this — it's a 5-line addition once site-url is read.
- **`<meta name="generator">`.** Already populated by the template
  engine (`template.rs:133`). No work.
- **Repo-actions / source links / `edit-on-github` `<head>`
  contributions.** Out of epic scope per parent plan.
- **Browser tab detection of HTML changes via meta refresh / cache
  headers.** Out of scope.
- **Per-page `<head>` overrides for favicon.** A document that sets
  its own `favicon` field doesn't override the website's. Confirmed
  with user 2026-04-27 to leave out of Phase 7 *but* track as an
  explicit follow-up bead — the user expects this to come up
  sooner rather than later. Filed as `bd-<phase7-favicon-override>`
  at close-out (see §Follow-up beads).

## Reference material

- **Parent epic plan** §"Phase 7 — Post-render".
- **Phase 1 sub-plan** §"`ProjectType` trait shape" (the trait
  signature; `post_render` already accepts everything Phase 7
  needs).
- **Phase 5 sub-plan** §"`ResourceResolverContext`" (page_url_for
  used for the favicon `<link>` href).
- **Phase 6 sub-plan** §Decision 4 (`page_url_for` works for any
  project-relative output href, not just page hrefs — favicon path
  qualifies).
- **Q2 current code:**
  - `crates/quarto-core/src/project/orchestrator.rs:170-229` —
    `WebsiteProjectType::post_render`. Today flushes `site_libs/`;
    Phase 7 grows it.
  - `crates/quarto-core/src/transforms/metadata_normalize.rs` —
    `MetadataNormalizeTransform`. Phase 7's title-prefix transform
    sits adjacent and is the closest analogue.
  - `crates/quarto-core/src/transforms/navbar_render.rs:134-142` —
    `brand_title_fallback`: reads `meta.get_path(&["website",
    "title"])`. Phase 7's transforms reuse this access pattern.
  - `crates/quarto-core/src/template.rs:128-235` —
    `FULL_HTML_TEMPLATE`. Slots Phase 7 writes into:
    `$pagetitle$` (line 149), `$header-includes$` (line 158),
    `$canonical-url$` (line 146).
  - `crates/quarto-core/src/template.rs:330-365` —
    `set_includes_list`: how `header-includes` is currently
    populated from metadata. Phase 7's favicon transform appends to
    the same list.
  - `crates/quarto-core/src/document_profile.rs:46-94` —
    `DocumentProfile`. Phase 7 reads `output_href`, `source_path`,
    `title`. No new profile fields needed.
  - `crates/quarto-core/src/project/index.rs` — `ProjectIndex`.
    Phase 7 reads `profiles()` for the sitemap walk.
  - `crates/quarto-core/src/resource_resolver.rs:176-186` —
    `page_url_for`. Phase 7 calls it for the favicon `<link>` href.
  - `crates/quarto-core/src/project/mod.rs:294-336` —
    `ProjectConfig`. `metadata: Option<ConfigValue>` is where the
    project-level `website.*` keys live; readable from
    `post_render` via `project.config.metadata`.
  - `crates/quarto-system-runtime/src/traits.rs` — `SystemRuntime`
    methods Phase 7 uses: `file_write`, `file_copy`, `path_exists`,
    `path_metadata` (for mtime).
- **Q1 reference:**
  - `external-sources/quarto-cli/src/project/types/website/website-sitemap.ts`
    lines 32–193 — full sitemap + robots.txt logic. Phase 7 ports
    to Rust in idiomatic style.
  - `external-sources/quarto-cli/src/project/types/website/website.ts`
    lines 175–216 — title-prefix and favicon plumbing.
  - `external-sources/quarto-cli/src/project/types/website/website-shared.ts`
    lines 90–115 — `computePageTitle` semantics. Phase 7 mirrors.
  - `external-sources/quarto-cli/src/project/types/website/website-config.ts`
    lines 170–188 — `websiteTitle`, `websiteBaseurl`, `websiteImage`.
  - `external-sources/quarto-cli/src/resources/projects/website/templates/sitemap.ejs.xml`
    — sitemap XML shape (urlset / url / loc / lastmod).
  - `external-sources/quarto-cli/src/project/types/website/website-constants.ts`
    — canonical key names: `kSiteUrl = "site-url"`,
    `kSiteFavicon = "favicon"`, `kSiteTitle = "title"`.

## Key decisions (to confirm with user)

These are proposed — please push back on anything that looks wrong
before we start.

### Decision 1 — Phase 7 splits cleanly into per-page Pass-2 transforms and post-render writes

| Concern | Where it runs | Reads | Writes |
|---|---|---|---|
| Title prefix | Pass 2, AST transform | `ast.meta.website.title`, `ast.meta.title`, `ast.meta.pagetitle` | `ast.meta.pagetitle` |
| Favicon `<link>` | Pass 2, AST transform | `ast.meta.website.favicon`, `RenderContext.resource_resolver` | `ast.meta.header-includes` (append) |
| Canonical URL (proposed inclusion) | Pass 2, AST transform | `ast.meta.website.site-url`, `RenderContext` (current page's output_href via `RenderContext.document.output`) | `ast.meta.canonical-url` |
| Favicon file copy | post_render | `project.config.metadata.website.favicon` | `<output_dir>/<favicon-path>` |
| Sitemap | post_render | `project.config.metadata.website.site-url`, `ProjectIndex.profiles()`, input mtimes | `<output_dir>/sitemap.xml` |
| robots.txt | post_render | `project.dir/robots.txt` (if exists), `website.site-url` | `<output_dir>/robots.txt` |

**Rationale.** The two halves consume different state. Per-page
transforms have access to `ast.meta` and the per-page resolver but
don't see the full `ProjectIndex` walk; post_render has the index and
direct `SystemRuntime` access but doesn't touch any individual page's
AST. The split mirrors Phase 5 (per-page artifact emission +
post-render flush).

### Decision 2 — Three small AST transforms, not one omnibus

Three separate transforms in `crates/quarto-core/src/transforms/`:

1. `WebsiteTitlePrefixTransform` — modifies `pagetitle`.
2. `WebsiteFaviconTransform` — appends a `<link>` to `header-includes`.
3. `WebsiteCanonicalUrlTransform` — sets `canonical-url`.

**Rationale.** Each is ~30–50 lines, has a single responsibility,
and tests independently. Bundling them would force "if any
website.* key is set" branching for shared work that doesn't exist
— each transform reads a different config key. Also keeps Phase 7
easy to extend in follow-ups (Open Graph tags would be a fourth
transform of identical shape; analytics a fifth).

**Naming.** `Website*Transform` is the prefix; matches existing
`WebsiteProjectType` and is searchable. Alternative `Site*` was
rejected because Q1 uses `kSite*` for keys and `website*` for
helpers — Q1's naming is the authoritative reference.

**Trade-off.** Three name slots in `transforms/mod.rs` instead of
one. Acceptable — `transforms/` already has 25+ entries and search
discovers `Website*` cleanly.

### Decision 3 — Pipeline placement: right after `MetadataNormalizeTransform`

`MetadataNormalizeTransform` produces `pagetitle` from `title` if
absent. The website transforms read `pagetitle` *after* that
derivation, so they must run later. Placement:

```
MetadataNormalizeTransform
WebsiteTitlePrefixTransform        ← NEW
WebsiteFaviconTransform            ← NEW
WebsiteCanonicalUrlTransform       ← NEW
… (existing pre-engine and engine stages)
```

These are pure metadata transforms — they touch only `ast.meta`,
not blocks/inlines — so their relative ordering with respect to
later AST transforms (`Sugar`, `Engine`, navigation generate/render,
link-rewrite, crossref) is irrelevant. The latest-binding
constraint is just "after `pagetitle` is derived, before
`ApplyTemplate` reads it".

**Standalone-render no-op contract.** Each transform reads its
config key under `meta.website.*`. If no `website.*` namespace
exists (single-doc render with no project), all three are no-ops.
Same shape as Phases 2–6's "no `project_index` → no-op" pattern.

### Decision 4 — Title prefix algorithm mirrors Q1 `computePageTitle`

Pseudocode:

```
let website_title = meta.website.title.as_plain_text()?;
let title         = meta.title.as_plain_text();
let pagetitle     = meta.pagetitle.as_plain_text();

// Already explicit? Don't touch.
if pagetitle.is_some() && pagetitle != Some(title.unwrap_or("")) {
    return; // user / earlier transform set it
}

let new_pagetitle = match (title, website_title) {
    (Some(t), wt) if t == wt => t,                      // page == site
    (Some(t), wt)             => format!("{t} – {wt}"), // both, distinct
    (None,    wt)             => wt,                    // home page fallback
};
meta.pagetitle = new_pagetitle;
```

Notes:
- Uses an **en-dash** (`–`, U+2013) to match Q1 line 108.
- The "pagetitle already set" check distinguishes
  *MetadataNormalize-derived* `pagetitle` (always equals `title`)
  from *user-or-earlier-transform-set* `pagetitle` (which we
  preserve). Concretely: if `pagetitle == title`, treat it as
  derived and rewrite; otherwise leave it. This is a small heuristic
  that's robust because both Q1 and Q2 only auto-derive
  `pagetitle = title`, never `pagetitle = something-else`.
- Home page handling: Q1 has a dedicated "if `stem == 'index'` and
  no title at all, use `website.title` as pagetitle". Our
  algorithm subsumes this via the `(None, wt) => wt` branch — any
  page with no title uses the website title. The Q1 `stem == index`
  guard exists to avoid clobbering pages that *intentionally* lack
  a title with the website title; Q2 v1 takes the simpler
  always-use-website-title-as-fallback rule. **Recommend going with
  the simple rule and revisiting if a real fixture surfaces a
  problem.**

### Decision 5 — Favicon `<link>` is appended to `header-includes`

The full HTML template already loops over `header-includes` (lines
158–160 of `template.rs`). Phase 7's favicon transform produces a
`<link rel="icon" ...>` string and appends it to that list.

**Why not a dedicated `$favicon$` template slot?** Two reasons:
1. The template would gain a Phase-7-specific slot for a
   one-line contribution. `header-includes` is the existing
   "additional `<head>` content" channel; using it scales as we
   add more head contributions (canonical, OG, analytics, …)
   without churning the template.
2. The template engine renders `header-includes` as raw HTML
   already (see `template.rs:88-91, 158-160`), so no new template
   plumbing needed.

**MIME-type detection.** Trivial extension map:

| extension | type |
|---|---|
| `.ico` | `image/x-icon` |
| `.png` | `image/png` |
| `.svg` | `image/svg+xml` |
| `.gif` | `image/gif` |
| `.jpg` / `.jpeg` | `image/jpeg` |
| else | `omit type="..." attribute` |

~10-line helper in the favicon transform module. No external dep.

**HTML emitted.**

```html
<link rel="icon" href="<page-relative-href>" type="<mime>">
```

When `mime` is unknown, omit the `type` attribute. Q1 emits `type`
unconditionally via `contentType(favicon)`; we degrade gracefully.

**`href` computation.** Use
`ResourceResolverContext::page_url_for(favicon_path)`. The favicon's
project-relative path (e.g. `favicon.ico`, `assets/favicon.png`) is
the same in source and output dirs because Phase 7 copies it
verbatim — so `page_url_for` (which expects a project-relative
output href) gives the right relative URL from any page.

**No-resolver fallback.** If `ctx.resource_resolver` is `None` (a
scenario that shouldn't happen in Pass 2 of a website project but
is possible in tests), emit the favicon path verbatim. Same
defensive shape as Phase 6.

### Decision 6 — Canonical URL transform (proposed inclusion)

Phase 7 also populates the existing `$canonical-url$` slot
(`template.rs:146-148`):

```html
$if(canonical-url)$
<link rel="canonical" href="$canonical-url$">
$endif$
```

Algorithm:

```
let site_url = meta.website.site-url.as_plain_text()?;
let output_href = ctx.document.output_href()?;  // doc's own project-rel output
let canonical = format!("{}/{}", site_url.trim_end_matches('/'), output_href);
meta.canonical-url = canonical;
```

**Why include this?** Marginal cost (~30 lines + 3 tests), and
without it `site-url` is only useful for the sitemap — a half-done
feature. Q1 does not (yet) emit canonical-url either, so this is a
small Q2-only win. **User decides: include in Phase 7 or defer to
its own follow-up bead?** Recommend include.

If the user defers it, drop §Decision 6, drop the `WebsiteCanonicalUrlTransform`
from §Module shape, drop tests 16–19, and remove the
`canonical-url` row from §Decision 1's table.

### Decision 7 — Reading website.* config

Add a small helper module
`crates/quarto-core/src/project/website_config.rs`:

```rust
/// Read `website.title` from a merged metadata value.
pub fn website_title(meta: &ConfigValue) -> Option<String> { … }
/// Read `website.site-url`. Trailing slash NOT stripped — callers
/// strip if they need to (sitemap does, link emission can leave it).
pub fn website_site_url(meta: &ConfigValue) -> Option<String> { … }
/// Read `website.favicon`. Forward-slash, project-relative path.
pub fn website_favicon(meta: &ConfigValue) -> Option<String> { … }
```

All three accept a `&ConfigValue` so they work for both
`ast.meta` (per-page transforms) and
`project.config.metadata.as_ref()?` (post_render).

**Why a dedicated module?** Three reads, three transforms, one
post_render hook → six call sites. Centralizing avoids drift if
Q2 ever moves the keys (the epic-wide nav-config-placement
follow-up `bd-n9dr` could rename `website.title` to e.g.
`title-prefix` at top-level; a single helper is one edit).

**Module placement.** `quarto-core/src/project/website_config.rs`,
re-exported from `project/mod.rs`. Adjacent to `ProjectContext`
which owns the on-disk `_quarto.yml`. Other crates can import
`quarto_core::project::website_config::{…}`.

### Decision 8 — Favicon copy in post_render

Algorithm:

```
let Some(favicon_path) = website_favicon(&project.config.metadata?) else { return };
let src = project.dir.join(&favicon_path);
if !runtime.path_exists(&src) {
    diagnose("website.favicon refers to missing file '{}'", favicon_path);
    return;
}
let dst = project.output_dir.join(&favicon_path);
if let Some(parent) = dst.parent() { runtime.dir_create(parent, true)?; }
runtime.file_copy(&src, &dst)?;
```

Notes:
- **Copy, don't symlink** — `_site/` is meant to be standalone.
- **Missing source.** Diagnose, do not error. `<link>` tag is still
  emitted; if the user packages `_site/` with a missing favicon,
  browsers fall back to no icon. Q1 silently skips; Q2's
  diagnostic is mildly more helpful.
- **Idempotent.** `file_copy` overwrites. No staleness check —
  Phase 8 incremental will gate this.

### Decision 9 — Sitemap algorithm (fresh-write only in Phase 7)

```
let Some(site_url) = website_site_url(&project.config.metadata?) else {
    return; // no site-url → no sitemap
};
let base = site_url.trim_end_matches('/');
let mut entries = Vec::with_capacity(index.profiles().len());
for profile in index.profiles() {
    let loc = format!("{}/{}", base, profile.output_href);
    let lastmod = file_mtime_iso8601(&profile.source_path, runtime);
    entries.push(SitemapEntry { loc, lastmod });
}
let xml = render_sitemap_xml(&entries);
runtime.file_write(&project.output_dir.join("sitemap.xml"), xml.as_bytes())?;
```

Notes:
- **Encoding.** XML-escape `loc` (`&`, `<`, `>`, `"`, `'`). Q1 uses
  `lodash.escape`. Inline our own ~10-line escaper — same tactic
  Phase 6 used for path normalization (no new crate dep).
- **lastmod.** ISO-8601 timestamp from input file mtime.
  `runtime.path_metadata(&profile.source_path)?.mtime()`. If
  unreadable, omit `<lastmod>` (Q1 falls back to "1970"; we omit,
  cleaner XML).
- **Drafts.** Phase 7 includes drafts. The "skip drafts unless
  visible" filter is the same as Phase 6's deferred draft-mode
  handling — file as the same follow-up bead, don't fork the
  decision.
- **Output path.** `<project.output_dir>/sitemap.xml`. Always at
  the site root.
- **Deterministic order.** Iterate `index.profiles()` directly —
  `ProjectIndex` preserves Pass-1 insertion order. Phase 8's
  incremental rebuild will need to preserve that ordering after
  partial updates; for Phase 7 it's just-in-time correct.

**Sitemap XML shape.**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
  <url>
    <loc>https://example.com/index.html</loc>
    <lastmod>2026-04-27T14:32:11Z</lastmod>
  </url>
  …
</urlset>
```

Hand-written formatter, ~25 lines. No XML library dep.

**Open question (§Open questions): trailing-slash policy on
`<loc>`.** Q1 emits `https://example.com/index.html` (file URL).
Some sites prefer `https://example.com/` for the home page. We
follow Q1.

### Decision 10 — robots.txt

```
let dst = project.output_dir.join("robots.txt");
let src = project.dir.join("robots.txt");
if runtime.path_exists(&src) {
    runtime.file_copy(&src, &dst)?;
    return;
}
let Some(site_url) = website_site_url(&project.config.metadata?) else {
    return;
};
let base = site_url.trim_end_matches('/');
let body = format!("Sitemap: {base}/sitemap.xml\n");
runtime.file_write(&dst, body.as_bytes())?;
```

Notes:
- User's `robots.txt` wins (Q1 parity).
- Auto-generated robots.txt only when `site-url` is set —
  otherwise it'd reference a non-existent sitemap.
- No idempotence check — overwrite each render. Phase 8
  incremental will skip if unchanged.

### Decision 11 — `post_render` ordering

```rust
async fn post_render(...) -> Result<()> {
    flush_site_libs(...)?;          // existing (Phase 5)
    copy_favicon(...)?;             // new (Phase 7)
    write_sitemap(...)?;            // new (Phase 7)
    write_robots_txt(...)?;         // new (Phase 7)
    Ok(())
}
```

**Why this order.** No real dependencies between the four — each
writes to a different output file. The grouping is "shared assets
first (site_libs, favicon), discovery files last (sitemap,
robots)" for readability. Failures in any one short-circuit the
rest (matches Phase 1's `?` propagation; Phase 5 already does
this).

**Refactor proposed.** The current `post_render` body in
`orchestrator.rs:186-228` is inlined site_libs flushing. Phase 7
extracts it to a private fn `flush_site_libs(...)` and adds three
sibling fns `copy_favicon`, `write_sitemap`, `write_robots_txt` —
all in `orchestrator.rs` (or in a new
`crates/quarto-core/src/project/website_post_render.rs` if the
file gets too long). Recommend **extract into
`website_post_render.rs`** to keep `orchestrator.rs` focused on
orchestration mechanics; `WebsiteProjectType::post_render` becomes
a four-line composition.

### Decision 12 — DocumentProfile change

**None.** Phase 7's reads (`output_href`, `source_path`, `title`)
are all profile-version-1 fields. No bump.

## Architecture sketch

### Module shape

```
crates/quarto-core/src/project/
    website_config.rs              # NEW — website_title / website_site_url / website_favicon
    website_post_render.rs         # NEW — flush_site_libs / copy_favicon / write_sitemap / write_robots_txt
    orchestrator.rs                # WebsiteProjectType::post_render → 4-line composition
    mod.rs                         # re-export website_config

crates/quarto-core/src/transforms/
    website_title_prefix.rs        # NEW — WebsiteTitlePrefixTransform
    website_favicon.rs             # NEW — WebsiteFaviconTransform
    website_canonical_url.rs       # NEW — WebsiteCanonicalUrlTransform (if Decision 6 confirmed)
    mod.rs                         # re-exports

crates/quarto-core/src/pipeline.rs # insert the three new transforms after MetadataNormalizeTransform
```

### Data flow

**Per-page (Pass 2):**

```
ast.meta (post-MetadataMergeStage, has website.*)
        │
        ▼
MetadataNormalizeTransform (sets pagetitle = title if absent)
        │
        ▼
WebsiteTitlePrefixTransform (rewrites pagetitle if website.title)
        │
        ▼
WebsiteFaviconTransform (appends <link> to header-includes)
        │
        ▼
WebsiteCanonicalUrlTransform (sets canonical-url)
        │
        ▼ (existing engine + transforms)
        ▼
ApplyTemplate reads pagetitle, header-includes, canonical-url
```

**Project-level (after Pass 2):**

```
ProjectIndex.profiles() ──┐
project.config.metadata ──┼──> WebsiteProjectType::post_render
project.dir, output_dir ──┤        ├── flush_site_libs (Phase 5)
SystemRuntime ────────────┘        ├── copy_favicon
                                   ├── write_sitemap
                                   └── write_robots_txt
```

### Single-doc behavior (regression)

For a default project (no `_quarto.yml`, or `project.type:
default`):
- Per-page transforms read `meta.get_path(&["website", "title"])`
  etc., which return `None`. Each transform returns early — no AST
  mutation, no diagnostics.
- post_render is `DefaultProjectType`'s no-op default. Nothing
  written.

A standalone `.qmd` file with no website context produces
byte-identical output to pre-Phase-7. Locked by regression tests.

## Tests (TDD: write and fail first)

Every test authored before the code that makes it pass.

### Unit tests — `website_config` helpers

1. `website_title_reads_string` — `website: { title: "Site" }` →
   `Some("Site")`.
2. `website_title_reads_inlines_as_plain_text` —
   `website: { title: [Str "S", Space, Str "T"] }` → `Some("S T")`.
3. `website_title_missing_returns_none` — no `website` key.
4. `website_site_url_reads_string` — `website: { site-url: "https://example.com/" }`.
5. `website_favicon_reads_string` — `website: { favicon: "favicon.ico" }`.
6. `website_helpers_handle_non_map_meta` — `meta` is a scalar:
   all three return `None` without panic.

### Unit tests — `WebsiteTitlePrefixTransform`

7. `title_prefix_no_op_without_website_title` — no `website.title`
   → `pagetitle` unchanged.
8. `title_prefix_combines_doc_and_site_titles` —
   doc title `"Getting Started"`, website title `"Quarto Docs"` →
   `pagetitle = "Getting Started – Quarto Docs"`.
9. `title_prefix_skips_when_titles_equal` —
   doc title == website title → `pagetitle = "Quarto Docs"` (no
   `– Quarto Docs`).
10. `title_prefix_uses_website_title_for_untitled_page` —
    no doc title, website title `"Quarto Docs"` →
    `pagetitle = "Quarto Docs"`.
11. `title_prefix_preserves_explicit_pagetitle` —
    `pagetitle: "Explicit"`, doc title `"Doc"`, website
    title `"Site"` → `pagetitle = "Explicit"` (untouched, because
    `pagetitle != title`).
12. `title_prefix_overrides_normalize_derived_pagetitle` —
    `MetadataNormalize` set `pagetitle = title = "Doc"`,
    website title `"Site"` → `pagetitle = "Doc – Site"`.

### Unit tests — `WebsiteFaviconTransform`

13. `favicon_no_op_without_website_favicon` —
    `header-includes` unchanged.
14. `favicon_appends_link_with_resolved_href` —
    `website.favicon = "favicon.ico"`, resolver returns
    `"../favicon.ico"` → `header-includes` ends with
    `<link rel="icon" href="../favicon.ico" type="image/x-icon">`.
15. `favicon_appends_without_type_for_unknown_extension` —
    `website.favicon = "favicon.foo"` → `<link>` omits `type`
    attribute.
16. `favicon_falls_back_to_path_verbatim_without_resolver` —
    no `ctx.resource_resolver` → href is `"favicon.ico"`.
17. `favicon_handles_subdirectory_path` —
    `website.favicon = "assets/favicon.svg"`, resolver returns
    `"../assets/favicon.svg"` → `<link>` has that href and
    `type="image/svg+xml"`.
18. `favicon_appends_to_existing_header_includes` — existing
    `header-includes: ["<meta name='foo'>"]` is preserved; new
    `<link>` appended.

### Unit tests — `WebsiteCanonicalUrlTransform` (if Decision 6 confirmed)

19. `canonical_url_no_op_without_site_url` —
    `canonical-url` unchanged.
20. `canonical_url_composes_site_url_and_output_href` —
    `website.site-url: "https://example.com/"`,
    `output_href: "docs/api.html"` →
    `canonical-url = "https://example.com/docs/api.html"`.
21. `canonical_url_handles_trailing_slash_on_site_url` —
    same with no trailing slash on site-url.
22. `canonical_url_skips_when_no_output_href` — defensive: an AST
    used in tests without a populated render context.

### Unit tests — sitemap generator

23. `sitemap_xml_empty_urlset` — zero entries → valid empty
    `<urlset/>`.
24. `sitemap_xml_single_entry` — one entry → conformant XML with
    `<loc>` + `<lastmod>`.
25. `sitemap_xml_escapes_special_chars` — entry with `loc`
    containing `&` → `&amp;` in output.
26. `sitemap_xml_omits_lastmod_when_unknown` — entry with no mtime
    → no `<lastmod>` element.
27. `sitemap_url_join_strips_trailing_slash` —
    site-url `"https://example.com/"` + `output_href "x.html"` →
    `"https://example.com/x.html"`.

### Unit tests — robots.txt generator

28. `robots_txt_default_body` — site-url `"https://example.com"`
    → `"Sitemap: https://example.com/sitemap.xml\n"`.
29. `robots_txt_strips_trailing_slash_in_sitemap_url` —
    site-url `"https://example.com/"` → same as test 28.

### Integration tests — `crates/quarto-core/tests/website_post_render.rs` (new)

30. `pipeline_title_prefix_combines_titles` — two-page website
    with `website.title: "Site"`. After render,
    `_site/index.html` has `<title>Index – Site</title>`,
    `_site/about.html` has `<title>About – Site</title>`.
31. `pipeline_favicon_link_emitted_per_page` — website with
    `website.favicon: "favicon.ico"`. After render, every page's
    `<head>` contains a `<link rel="icon">`. Nested page's href
    starts with `../`; root page's does not.
32. `pipeline_favicon_file_copied_to_output_dir` — same fixture:
    `_site/favicon.ico` exists.
33. `pipeline_canonical_url_per_page` (if Decision 6) —
    `website.site-url: "https://example.com"`. `_site/index.html`
    has `<link rel="canonical" href="https://example.com/index.html">`.
34. `pipeline_sitemap_emitted_with_site_url` — sitemap has both
    pages' URLs based on site-url.
35. `pipeline_sitemap_omitted_without_site_url` — fixture with
    `website.title` set but no site-url → no `_site/sitemap.xml`.
36. `pipeline_robots_txt_emitted_when_site_url_set` —
    `_site/robots.txt` written with `Sitemap:` line.
37. `pipeline_robots_txt_user_file_takes_precedence` — fixture
    with hand-written `robots.txt` in the project root → that
    file copied verbatim, not the auto-generated one.
38. `pipeline_favicon_missing_diagnoses_continues` — fixture
    `website.favicon: "missing.ico"` (file not present) →
    rendered HTML still has `<link rel="icon" href="missing.ico">`,
    `_site/missing.ico` does NOT exist, diagnostic warning
    surfaced in summary.
39. `pipeline_default_project_no_phase_7_outputs` — single-doc
    fixture (no `_quarto.yml`, no `website.*`) → no sitemap, no
    robots.txt, no favicon copy, byte-identical output to
    pre-Phase-7 (regression guard for the cross-cutting invariant).

### CLI end-to-end (per CLAUDE.md §End-to-end verification)

40. **Full-stack smoke** at `/tmp/q2-phase7-smoke/`:
    ```
    _quarto.yml:
      project: { type: website, output-dir: _site }
      website:
        title: "Phase 7 Test Site"
        site-url: "https://example.com/site"
        favicon: "favicon.ico"
    favicon.ico:  # 1×1 transparent PNG renamed
    index.qmd:    "---\ntitle: Home\n---\nWelcome."
    about.qmd:    "---\ntitle: About\n---\nAbout us."
    docs/api.qmd: "---\ntitle: API\n---\n# API"
    ```
    Run `cargo run --bin q2 -- render /tmp/q2-phase7-smoke/` and inspect:
    - `_site/sitemap.xml` exists and lists all three URLs with
      `https://example.com/site/...` prefix.
    - `_site/robots.txt` exists and contains
      `Sitemap: https://example.com/site/sitemap.xml`.
    - `_site/favicon.ico` exists (binary copy).
    - `_site/index.html` contains
      `<title>Home – Phase 7 Test Site</title>`,
      `<link rel="icon" href="favicon.ico" type="image/x-icon">`,
      `<link rel="canonical" href="https://example.com/site/index.html">`.
    - `_site/docs/api.html` contains
      `<title>API – Phase 7 Test Site</title>`,
      `<link rel="icon" href="../favicon.ico" type="image/x-icon">`,
      `<link rel="canonical" href="https://example.com/site/docs/api.html">`.
    Record observed snippets in close-out.

41. **Regression smokes**: re-run `/tmp/q2-phase2-smoke/`,
    `/tmp/q2-phase3-smoke/`, `/tmp/q2-phase4-smoke/`,
    `/tmp/q2-phase5-website-test/`, `/tmp/q2-phase6-smoke/`. None
    of these set `website.site-url` or `website.favicon`, so:
    - No sitemap, no robots.txt, no favicon copy.
    - `<title>` includes the website-title prefix where
      `website.title` is set in those fixtures.
    - All other behavior unchanged.

### Snapshot tests

None — inline asserts over emitted HTML, sitemap XML, and
robots.txt cover the vocabulary. Consistent with Phases 2–6.

## Work items (checklist)

### Preparation
- [ ] Re-read `claude-notes/instructions/testing.md`,
      `coding.md`, `review.md`.
- [ ] Confirm user agreement with Decisions 1–12.
- [ ] Resolve open questions §"Open questions" below.
- [x] File `bd` issue under parent `bd-0tr6`. (`bd-b9mz`)
- [ ] Commit directly on `feature/websites` (Phase 1–6 precedent).

### `website_config` helper (`quarto-core/src/project/website_config.rs`)
- [x] New module: `website_title`, `website_site_url`, `website_favicon`,
      plus `normalize_favicon_path` (Open Question 4).
- [x] Re-export from `project/mod.rs`.
- [x] Tests 1–6 + 2 normalization tests (8 total, all passing).

### `WebsiteTitlePrefixTransform` (`transforms/website_title_prefix.rs`)
- [x] New module per Decision 4. En-dash separator (U+2013).
- [x] `mod.rs` re-export.
- [x] Tests 7–12 + 2 extras (idempotency, non-map meta defensive). 8
      total, all passing.

### `WebsiteFaviconTransform` (`transforms/website_favicon.rs`)
- [x] New module per Decision 5. Promotes scalar `header-includes`
      to array on append; preserves Pandoc-inline / unknown shapes
      defensively.
- [x] Inline MIME-type helper + HTML-attr escape.
- [x] Tests 13–18 + 4 extras (leading-slash normalize, ampersand
      escape, MIME table, mod re-export). 10 total, all passing.

### `WebsiteCanonicalUrlTransform` (`transforms/website_canonical_url.rs`) — Decision 6 confirmed
- [x] New module per Decision 6. Pure helper `apply_canonical_url`
      decouples site-url + output-href composition from
      `RenderContext` lookup so the pure-helper unit tests cover
      the no-op branches.
- [x] `mod.rs` re-export.
- [x] Tests 19–22 + 5 helper/setter extras (sub-path site URL,
      leading-slash normalization, insert-vs-replace, non-map
      defensive). 9 total, all passing.

### Pipeline wiring (`pipeline.rs`)
- [x] Insert the three new transforms after
      `MetadataNormalizeTransform` per Decision 3 (steps 4a, 4b, 4c).
- [x] Doc-block update enumerating the new pre-engine transforms.
- [x] Full quarto-core test suite green (1275 tests pass) — no
      regressions in existing transforms or integration tests.

### `website_post_render.rs` (`quarto-core/src/project/website_post_render.rs`)
- [x] New module containing `flush_site_libs` (extracted from
      orchestrator.rs:186-228), `copy_favicon`, `write_sitemap`,
      `write_robots_txt` per Decision 11. Module is
      `cfg(not(target_arch = "wasm32"))`-gated.
- [x] Inline `escape_xml_text` helper for sitemap.
- [x] Inline `format_iso8601_utc` helper (Howard Hinnant
      civil-date arithmetic, no `chrono` dependency).
- [x] Tests 23–29 + 5 extras (XML escape table, ISO-8601 unit
      tests for epoch / known timestamp / end-of-year / leap day).
      12 total, all passing.

### Orchestrator wiring (`project/orchestrator.rs`)
- [x] Refactor `WebsiteProjectType::post_render` to call
      `flush_site_libs`, `copy_favicon`, `write_sitemap`,
      `write_robots_txt` in order. Body is now a four-line
      composition.
- [x] **Trait signature extension:** `post_render` gained a
      `&mut Vec<DiagnosticMessage>` parameter so non-fatal
      warnings (missing favicon source) reach the user.
      `DefaultProjectType` still uses the default no-op impl.
      `ProjectRenderSummary` gained
      `pub project_diagnostics: Vec<DiagnosticMessage>`.
- [x] CLI surface: `quarto/src/commands/render.rs` prints
      `summary.project_diagnostics` after the per-doc
      diagnostics.
- [x] Updated the in-test `CountingProjectType` /
      `CountingProjectTypeWrapper` in
      `crates/quarto-core/tests/project_pipeline.rs` to the new
      signature.
- [x] Full workspace nextest green (7922 tests pass) — no
      regressions.

### Integration tests (`quarto-core/tests/website_post_render.rs`)
- [x] Tests 30–39 (all 10 passing).
- [x] Use `NativeRuntime` and a temp project directory.
- [x] Test 39 reframed to use `output-dir: _out` so a default
      project actually renders files (the existing default-kind
      output-dir-equals-project-dir overlap collides with file
      discovery; not a Phase 7 concern but worth a note for the
      epic close-out).

### Regression check
- [x] Re-ran `/tmp/q2-phase2-smoke/`, `/tmp/q2-phase3-smoke/`,
      `/tmp/q2-phase4-smoke/`, `/tmp/q2-phase5-website-test/`,
      `/tmp/q2-phase6-smoke/`. All render cleanly with no
      errors or warnings. Phase 3's fixture sets
      `website.title: "Phase 3 smoke"`, so Phase 7's
      title-prefix transform activated there: `<title>Home –
      Phase 3 smoke</title>` and `<title>About – Phase 3
      smoke</title>` are the new title strings. No favicon /
      canonical-url / sitemap / robots.txt emitted in any
      regression fixture (none set the relevant keys), as
      intended.
- [x] Test 39 in the integration suite (default-project
      no-Phase-7-outputs) locks in the no-op contract.

### CLI end-to-end + verification
- [x] Smoke fixture at `/tmp/q2-phase7-smoke/` (test 40):
      3 pages (`index`, `about`, `docs/api`), `website.title`,
      `website.site-url`, `website.favicon`. Inspection results:
      * `_site/sitemap.xml` lists all 3 URLs prefixed with
        `https://example.com/site/...` and per-page
        ISO-8601 lastmods.
      * `_site/robots.txt`: `Sitemap: https://example.com/site/sitemap.xml`.
      * `_site/favicon.ico` exists (4-byte placeholder copied
        verbatim).
      * `_site/index.html`: `<title>Home – Phase 7 Test
        Site</title>`, `<link rel="icon" href="favicon.ico"
        type="image/x-icon">`, `<link rel="canonical"
        href="https://example.com/site/index.html">`.
      * `_site/docs/api.html`: `<title>API – Phase 7 Test
        Site</title>`, `<link rel="icon" href="../favicon.ico"
        type="image/x-icon">`, `<link rel="canonical"
        href="https://example.com/site/docs/api.html">`.
      Matches the plan example table 1:1.
- [x] Broken-favicon smoke at `/tmp/q2-phase7-broken-smoke/`:
      stderr printed `Warning: website.favicon refers to missing
      file 'nope.ico'`, `_site/index.html` still has
      `<link rel="icon" href="nope.ico">`, `_site/nope.ico`
      does not exist.
- [ ] `cargo build --workspace`.
- [ ] `cargo nextest run --workspace`.
- [ ] `cargo xtask lint`.
- [ ] `cargo fmt --check`.
- [ ] `cargo xtask verify` (full, including WASM build) — Phase 7
      touches `quarto-core` types accessible from
      `wasm-quarto-hub-client` indirectly; full verify is the
      safety net.

### Hub-client / WASM impact check
- [x] Audited `crates/wasm-quarto-hub-client/src/`: no references
      to `post_render`, `WebsiteProjectType`, `ProjectPipeline`,
      or `website_post_render`. The WASM path goes through
      `render_qmd_to_html` for single-doc renders only.
      Phase 7's `post_render` is `cfg(not(target_arch =
      "wasm32"))` — it never compiles into WASM. Phase 9 adds the
      multi-doc orchestration flow.
- [x] Per-page transforms (title prefix, favicon, canonical URL)
      compile under WASM — they're pure `quarto-core` metadata
      transforms with no platform-specific code. Confirmed by
      successful `npm run build:wasm` (release build of
      `wasm-quarto-hub-client` target wasm32-unknown-unknown
      includes Phase 7's modules). In single-doc WASM renders,
      the title-prefix and favicon transforms still activate
      when the user's qmd has `website.*` keys at the top level;
      the canonical URL transform short-circuits because there's
      no `project_index`. Behavior is correct for the Phase 9
      project-aware path; today's single-doc preview is
      unaffected unless the user explicitly sets `website.*`.

### Verification and close-out
- [x] `cargo build --workspace` clean.
- [x] `cargo nextest run --workspace` — **7922 tests pass** (up
      from 7876 pre-Phase-7; net +46 tests across the four new
      modules, the integration suite, and the orchestrator
      diagnostic-channel test).
- [x] `cargo xtask lint` passes (638 files checked).
- [x] `cargo fmt --all -- --check` clean.
- [x] `cargo xtask verify` (full, including WASM build,
      hub-client `npm run build:all`, hub-client tests, and
      trace-viewer build/tests) — all 9 steps green.
- [x] No snapshot drift.
- [x] Follow-ups filed (each `discovered-from:bd-b9mz`,
      parent-child to `bd-0tr6`, with extra `related` links
      where noted):
      * **`bd-7h6a`** — Per-page favicon override
        (`meta.favicon` beats `website.favicon`). User flagged
        2026-04-27 as expected-soon. P3.
      * **`bd-pphv`** — Sitemap incremental merge
        (read-existing/update/write). Loops with Phase 8. P3.
      * **`bd-tyvt`** — Open Graph / Twitter card / social meta
        tags (Q1 `metadataHtmlPostProcessor` parity). P3.
      * **`bd-ochm`** — Brand-aware favicon fallback (once Q2
        brand support lands). P4.
      * **`bd-4zdf`** — Multi-format favicon variants
        (apple-touch-icon, sizes). P4.
      * **`bd-1hdz`** — Draft-mode interaction with sitemap.
        Coordinate with `bd-p4sc` from Phase 6. P3.
      * **`bd-97yc`** — Title-prefix home-page carve-out
        (Q1 `stem == "index"` parity). P4.
      * **`bd-82dn`** — Empty-`index.html` filter in sitemap.
        Coordinate with `bd-r82e` (`DocumentProfile.includes`
        enrichment is the natural place to add `is_empty`). P4.
- [x] Updated epic plan §"Work items" — Phase 7 marked done with
      sub-plan link, `bd-b9mz` reference, and full follow-up
      list.
- [x] Updated §"Follow-up beads report (running log)" with the
      eight filed bd issues.
- [x] `br close bd-b9mz` (reason cites commit `78aa80cc`).
- [x] All Phase-7 changes committed in commit `78aa80cc`
      ("Phase 7: post-render (sitemap, favicon, site-url/title
      prefix)") on `feature/websites`. The single commit
      includes the .beads/issues.jsonl flush (br auto-flushed).
- [ ] Ask user permission before pushing.

## Risks and mitigations

- **Risk:** Title prefix corrupts pages that already set
  `pagetitle` deliberately (e.g. a Lua filter that sets
  `pagetitle = "Custom"` and expects it preserved).
  *Mitigation:* Decision 4's "if `pagetitle != title`, leave alone"
  branch. Test 11 locks it in.

- **Risk:** Favicon `<link>` href is wrong for nested pages —
  page-relative math fails.
  *Mitigation:* `page_url_for` is shared with Phase 5 (assets) and
  Phase 6 (body links); both have integration tests for
  nested-page hrefs. Test 31 explicitly checks the `../`
  prefix.

- **Risk:** Sitemap XML has invalid characters (URLs with `&`).
  *Mitigation:* dedicated escape helper (test 25); golden-file
  shape test (tests 23–24).

- **Risk:** `robots.txt` overwrites user's manually-tuned file.
  *Mitigation:* Decision 10's user-file-precedence rule. Test 37
  locks it.

- **Risk:** Sitemap uses input file mtime, but mtime is unstable
  across CI runs (git checkout doesn't preserve it).
  *Mitigation:* this is a real CI concern but Phase 7 ships fresh
  sitemap on every render — the lastmod is "when did this run". For
  Phase 8, when incremental rebuilds make stale lastmods possible,
  we may need a deterministic-source-of-truth (input content hash,
  or `_quarto.yml`-pinned date). Leave as a Phase-8 concern.

- **Risk:** Phase 7 affects Phase 6's link-rewrite by adding
  `<link rel="icon">` tags whose hrefs would also be candidates
  for rewriting.
  *Mitigation:* Phase 6's `LinkRewriteTransform` walks
  `Inline::Link` only (body content). `<link rel="icon">` lives
  in `header-includes` as a raw HTML string, never an
  `Inline::Link` node. No interaction.

- **Risk:** `WebsiteFaviconTransform` runs before
  `header-includes` reaches the template — but the template engine
  reads `header-includes` from `meta` at apply time, not at
  transform time. Verified by reading `template.rs:330-365`.

- **Risk:** Per-page transform runs even when `is_single_file`
  (e.g. a `.qmd` opened with `--meta website.title=…` somehow).
  *Mitigation:* the transform is keyed entirely on whether
  `meta.website.title` is set; if a single-doc render carries that
  metadata, applying the prefix is correct behavior, not a bug.

- **Risk:** `post_render` failure aborts the whole render
  (Phase 1's hook-failure rule). A misspelled favicon path
  shouldn't fail the render.
  *Mitigation:* Decision 8 — diagnose, do not error. The hook
  returns `Ok` even if the favicon is missing. Same for sitemap
  if `path_metadata` fails on one input.

- **Risk:** `cargo xtask verify`'s WASM leg fails because
  `quarto-system-runtime`'s `path_metadata` trait method isn't
  WASM-implemented.
  *Mitigation:* Phase 7's post_render is `cfg(not(target_arch =
  "wasm32"))` (Phase 5 already gated it). Per-page transforms
  don't call `path_metadata`. Verify the cfg is preserved.

- **Risk:** En-dash (U+2013) in `pagetitle` is mojibake'd
  somewhere.
  *Mitigation:* en-dash is plain UTF-8; the entire pipeline is
  UTF-8-clean already (Phase 6 used `–` in diagnostics; Q1 uses
  it). Tests 8, 9, 10, 12 all assert on the en-dash — any
  encoding regression surfaces.

## Explicit non-goals for this phase

- No Open Graph / social meta tags (`<meta property="og:…">`).
- No `<meta name="twitter:…">` cards.
- No Schema.org / JSON-LD.
- No analytics (Google Analytics, Plausible, etc.).
- No alias / redirect support.
- No 404 page.
- No reader-mode toggle.
- No repo-actions (`edit-on-github`).
- No incremental sitemap merge (Phase 8).
- No draft-mode visibility filtering on sitemap (deferred).
- No empty-page filter on sitemap.
- No brand-aware favicon fallback.
- No multi-variant favicon (apple-touch-icon, sizes).
- No per-page favicon override.
- No `<base href>` emission.
- No `<meta http-equiv="refresh">`.

## Follow-up beads (to file at close-out)

- **Open Graph / social meta tags** — full
  `metadataHtmlPostProcessor` Q1 parity. Prior art:
  `external-sources/quarto-cli/src/project/types/website/website-meta.ts`.
- **Sitemap incremental merge** — read existing
  `sitemap.xml`, patch entries for re-rendered files, preserve
  others. Couples with Phase 8.
- **Empty-`index.html` filter** — once `DocumentProfile`
  carries an `is_empty` signal (placeholder field tracked by
  `bd-r82e`-adjacent), filter the sitemap.
- **Brand-aware favicon fallback** — when brand support lands,
  fall back to `brand.light.favicon`.
- **Multi-format favicon** — apple-touch-icon, sizes.
- **Per-page favicon override** — `meta.favicon` at doc level
  beats `website.favicon`.
- **Draft-mode interaction with sitemap** — once draft-mode
  YAML config exists (`bd-p4sc` and friends), gate
  draft-page sitemap entries.
- **Title-prefix home-page special-case** — match Q1's
  `stem == "index" && offset === "."` carve-out if a real
  fixture surfaces a problem.

## Open questions (resolved 2026-04-27)

1. **Include `WebsiteCanonicalUrlTransform` in Phase 7?**
   *Resolved: yes.* Decision 6 confirmed; tests 19–22 included;
   `WebsiteCanonicalUrlTransform` ships with Phase 7.

2. **Defer incremental sitemap merge to Phase 8?**
   *Resolved: yes.* Phase 7 ships fresh-write only. Phase 8's
   incremental rebuild will add the read-existing/update/write
   path. Filed as a follow-up bead at close-out.

3. **Title-prefix home-page special-case?**
   *Resolved: simpler rule.* Decision 4's `(None, wt) => wt`
   branch — any untitled page falls back to `website.title`.
   Q1's narrower `stem == "index" && offset === "."` carve-out
   is filed as a follow-up to revisit if a real fixture surfaces.

4. **Favicon path normalization?**
   *Resolved: normalize.* The favicon helper strips a leading
   `/` and treats the path as project-relative. Test 17 (or a
   new test 17b) covers it.

5. **Sitemap `<lastmod>` precision.**
   *Resolved: second precision, UTC.* Format
   `YYYY-MM-DDThh:mm:ssZ`.

6. **En-dash vs hyphen separator.**
   *Resolved: en-dash (U+2013).* Matches Q1 and typographic
   convention. Locked in Decision 4.

7. **`website.site-url` trailing-slash handling.**
   *Resolved: strip on join.* Decisions 6 and 9 both call
   `trim_end_matches('/')` before composing absolute URLs.

## Decisions log (confirmed 2026-04-27)

1. Phase 7 splits into per-page Pass-2 transforms (title prefix,
   favicon link, optional canonical URL) and post_render writes
   (favicon copy, sitemap, robots.txt).
2. Three small AST transforms instead of one omnibus.
3. Per-page transforms run right after `MetadataNormalizeTransform`.
4. Title prefix algorithm matches Q1 `computePageTitle`, with
   en-dash separator and untitled-page fallback to website title.
5. Favicon `<link>` appended to `header-includes`; href via
   `page_url_for`.
6. `WebsiteCanonicalUrlTransform` populates the existing
   `$canonical-url$` slot (confirmed in Phase 7).
7. `website_config` helper module centralizes the three reads.
8. Favicon copy in post_render; missing file → diagnose, don't
   error.
9. Sitemap is fresh-write only in Phase 7 (no incremental merge);
   `<lastmod>` from input file mtime; XML escaping inline.
10. robots.txt: user file wins, otherwise auto-generate when
    site-url is set.
11. post_render orchestrator extracted to
    `website_post_render.rs`; refactor site_libs flushing alongside.
12. No `DocumentProfile` change.

## Epic-level impact

Phase 7 closes the **site-identity surface** for websites:

- Site-relative navigation links — Phases 2–4
- Site-shared resource paths — Phase 5
- Cross-document body links — Phase 6
- **Site-level title / favicon / sitemap / robots.txt — Phase 7**
- Incremental rebuild caching — Phase 8
- Hub-client live preview — Phase 9

After Phase 7, a Q2 website project has feature parity with the
**static-output portion** of a Q1 minimal website (everything Q1
does *without* search, listings, or analytics). The Q2 docs site
(`bd-tr81`) can begin authoring against Phases 0–7 and produce a
renderable, navigable, well-titled, indexable site.

The `website_config` helper module added in Phase 7 will be the
natural home for any future site-level config readers
(`website.image`, `website.repo-url`, `website.draft-mode`, etc.).
The follow-up `bd-n9dr` (epic-wide nav config placement) might
rename keys; keeping reads behind named functions makes that a
single-file edit.

The `website_post_render.rs` extraction keeps `orchestrator.rs`
focused on the two-pass mechanics. Phase 8's incremental rebuild
will likely add `incremental` parameters to several of these
post_render functions; having them as named functions in a
dedicated module makes that a localized change.
