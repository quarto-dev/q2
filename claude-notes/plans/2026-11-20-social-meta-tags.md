---
title: 'Open Graph / Twitter card social meta tags (bd-tyvt)'
date: 2026-11-20
description: 'Emit og:* and twitter:* meta tags for website pages from an AST transform, matching Quarto 1''s website-meta.ts.'
status: in-progress
braid:
  strand: bd-tyvt
---

**Checkout:** main checkout, branch `braid/bd-tyvt-social-meta-tags`, based on
`main` @ `4ca24c110`.
**GitHub:** [quarto-dev/q2#1259](https://github.com/quarto-dev/q2/issues/1259)
**Status:** design settled with the user on 2026-11-20 (see Decisions);
implementing.

## Triage verdict

**Ready to design.** The Quarto 1 behaviour is fully specified in one file, and
q2 already has most of the building blocks (canonical-url transform, body-derived
description/image, `imagesize`, a Q1-parity preview-image selector chain). What
remains open is where the logic lives (AST transform vs. post-render HTML pass)
and how much of Q1's surface ships in the first cut.

## Issue context

- Strand bd-tyvt (feature, P1, open), filed 2026-04-27 by cscheid during
  websites Phase 7 as explicitly out of the Phase 7 MVP.
- GH #1259 (cderv, 2026-11-18): `website.open-graph: true` /
  `website.twitter-card: true` are accepted silently (even under `--strict`)
  and produce no tags, so shared links on LinkedIn/Bluesky show no title,
  description or image. Reporter works around it with a post-render script.

## Dependency graph

- **parent-child**: bd-0tr6 (website projects epic, open). Its MVP list does
  not mention social tags; this is follow-up work on the epic.
- **discovered-from**: bd-b9mz (websites Phase 7, closed). Phase 7 added the
  per-page `website-title-prefix`, `website-favicon` and
  `website-canonical-url` transforms
  (`claude-notes/plans/2026-04-27-websites-phase-7.md`). The strand
  description says this was meant to be "a separate transform alongside Phase
  7's metadata transforms", i.e. the same shape as `website-canonical-url`.
- No `blocks` / `related` edges.

## What Quarto 1 does

Reference: `external-sources/quarto-cli/src/project/types/website/website-meta.ts`
(685 lines) and `util/discover-meta.ts`. An HTML post-processor, per page:

1. **Enable gate.** A provider runs when the page sets `open-graph` /
   `twitter-card` truthy, or the page doesn't set it `false` and
   `website.<key>` is truthy. Skipped when the page has no title.
2. **Merge** (`mergedSiteAndDocumentData`): site object and page object are
   deep-merged; a page `image:` overrides the site object's image; a site
   image string is treated as project-relative when `site-url` is set.
3. **Base values** (`pageMetadata`): `title` = computed page title;
   `description` = `description || abstract || subtitle`; `image` = page
   `image`.
4. **Provider keys.** OG: `title, description, image, image-alt,
   image-height, image-width, locale, site-name` (`site_name` defaults to
   `website.title`; `image-height` → `og:image:height`, etc.). Twitter:
   `title, description, image, image-alt, creator, site, card-style`
   (`card-style` → `twitter:card`, default `summary_large_image` when an
   image exists else `summary`). Twitter `image-width/height` warns once and
   is ignored.
5. **Image discovery** when no image (and page `image` isn't `false`):
   `img.preview-image` → `div.preview-image div.cell-output-display img` →
   any img whose src matches `preview|feature|cover|thumbnail` + image ext →
   first local img in `#quarto-document-content` → `website.image`. Data URIs
   are skipped with a warn-once.
6. **Description discovery** when none: first non-empty `<p>` in
   `main.content` (or a top-level section).
7. **Image resolution** (only when `site-url` is set): relative src →
   absolute `site-url` URL (`/`-prefixed = project-relative, otherwise
   document-relative); width/height read from the file (OG only), preferring
   file size over declared size. The image is registered as a resource.
   Without `site-url` the image href is emitted as written.
8. **Text processing.** Description truncated to 200 chars at punctuation
   (`truncateText(..., "punctuation")`); titles/descriptions go through a
   markdown pipeline so the `content=` is plain text of rendered markdown.

## What the code looks like today

Reproduced at HEAD (`4ca24c110`) with `social-meta-tags-investigation/repro/`
(website, `site-url`, `open-graph: true`, `twitter-card: true`, one page with
a description and a `preview.png`): `q2 render --strict` is clean and the
head has `description` and `canonical` but no `og:*` / `twitter:*` tags.

Pre-flight `cargo xtask verify --skip-hub-build`: steps 1-7 green (lints,
fmt, workspace build, tree-sitter, all Rust tests, ts-packages build). Step 8
failed 4 hub-client `*.wasm.test.ts` tests (highlight `ini` grammar,
smoke-all `columns-width` / `12-builtin-config-languages`, Q-5-53/54). The
cause is a stale hub WASM bundle (built 2026-11-17 12:49, before #1253 added
Q-5-53), which `--skip-hub-build` does not rebuild, not a HEAD regression.

Building blocks already present:

- `crates/quarto-core/src/transforms/website_canonical_url.rs`: a
  Normalization-phase, Bucket B2 transform that writes `canonical-url` into
  meta from `website.site-url` + the page's `output_href`; the full HTML
  template (`template.rs` around line 280) emits it. The natural model for
  this work.
- `DocumentProfile::derived_listing` (`document_profile.rs:188`): plain text
  of the first non-empty paragraph and the first body image src, computed at
  the AST level by `listing_item_info.rs`. Covers step 6 and the last-resort
  part of step 5, but not the `.preview-image` / named-pattern parts.
- `MetadataNormalizeTransform` derives plain-text `description-meta` from
  `description` for `<meta name="description">`; the same mechanism covers
  step 8's markdown-to-plain-text.
- `project/listing/post_render_upgrade/reader.rs::extract_preview_image`: the
  Q1 selector chain over rendered HTML (`scraper`, native-only). Note it
  *accepts* data URIs (correct for listings), whereas social discovery must
  skip them.
- `imagesize` (native-only dep, used by RSS feeds in
  `listing/feed/binding.rs`) for step 7's dimensions.
- `project/book/config.rs:63` already forwards `open-graph` /
  `twitter-card` / `image` / `image-alt` from `book:` to `website:`, so books
  get this for free once websites do.

## Decisions (user, 2026-11-20)

1. **AST transform**, not a post-render HTML pass ("we want to avoid
   reading HTML in general").
2. **Discovery over the AST** (`Image` classes, `.preview-image` cell
   divs); images only raw HTML or late filters produce are out of reach,
   which is accepted.
3. **Ship everything in one PR** (core tags, discovery, image sizes,
   diagnostics, docs). One commit per phase.
4. **No `site-url` → diagnostic**, and the image tags are left out (a
   relative image URL is useless to crawlers).
5. **Validate the types of the fields we read; open schema.** Unknown keys
   are ignored silently; a wrong type (an array where a string belongs)
   warns.

## Quarto 1 reference output

Rendered with the local Quarto 1 dev build (`quarto` 99.9.9) on two
throwaway sites. Observations that pin behaviour beyond what the source
says:

- **Title** is the composed page title, `Home – Site`. `open-graph.title`
  is markdown, emitted as plain text (`OG *title*` → `OG title`).
- **Description precedence, in effect:** provider `description` > page
  `description` / `abstract` / `subtitle` > `website.description` >
  (nothing). Q1's markdown pipeline overwrites the tag after the
  first-paragraph discovery, so a page with no page or site description
  gets `content=""`. **q2 diverges deliberately:** it falls back to the
  discovered first paragraph (truncated to 200 characters at sentence
  punctuation), which is what Q1's code intends.
- **Image precedence:** provider `image` (site map image, replaced by page
  `image:`) > discovery (`.preview-image` > named file > first local image)
  > `website.image` (with `website.image-alt`). Site-map and
  `website.image` paths are project-relative; page paths are
  document-relative. `image: false` turns off discovery and the
  `website.image` fallback only; a site-map image is still emitted.
- **Sizes** (OG only): read from the file, overriding a declared
  `image-width` / `image-height`; a declared size is used only when the
  file can't be read (or for remote images).
- **Alt text:** `image-alt` from the provider, then page `image-alt`; a
  discovered image contributes its alt only when it is an inline image
  (a figure's caption is not alt text).
- **`twitter:card`** defaults to `summary_large_image` with an image,
  `summary` without. Twitter `image-width` / `image-height` warn once and
  are ignored.
- **`open-graph: true` on a page drops the site-level options** in Q1
  (`mergedSiteAndDocumentData` returns the bare `true`). **q2 diverges:** a
  page `true` enables the provider with the site options.
- Tag order: OG `title, description, image, site_name, image:height,
  image:width, locale, image:alt`; Twitter `title, description, image,
  creator, site, image:alt, card`. `property=` for `og:*`, `name=` for
  `twitter:*`.

## Design

New Normalization-phase, Bucket B2 transform
`crates/quarto-core/src/transforms/website_social_meta.rs`
(`website-social-meta`), pushed right after `website-canonical-url`.

- **Gate:** website or book project, HTML-family format (decks included, as
  in Q1's website `formatExtras`), and the provider enabled: page
  `open-graph` / `twitter-card` truthy (or a map), or absent on the page and
  truthy under `website.`. Page `false` disables.
- **Values** are computed by a pure function from the merged meta plus
  discovered values, so the precedence rules are unit-testable without a
  render.
- **Discovery** walks the AST: `Image` with class `preview-image`; a `Div`
  with `preview-image` containing a `cell-output-display` `Div`'s image; an
  image whose src matches Q1's named-file regex; the first local image.
  Data URIs are skipped. Description: first non-empty top-level `Para`.
- **Image URLs:** metadata paths resolve through
  `config_paths::resolve_config_path` (provenance-aware: `_quarto.yml`
  values are project-relative, page values document-relative, leading `/` is
  the project root). Body images resolve against the document dir. The
  project-relative path is joined onto `site-url` (Q1's `joinUrl`). Sizes
  come from `imagesize::blob_size` over bytes read through
  `ctx.runtime` (works over the VFS); the crate dependency moves out of
  the native-only block.
- **Output:** `<meta>` tags appended to `rendered.includes.header` (the
  same slot `draft-alert` and `website-favicon` use), values
  attribute-escaped.
- **Resources:** explicit and site images are registered as
  `ResourceCopyIntent`s so the file reaches `_site` even when no body
  image references it.

New diagnostics (`project` subsystem):

| Code | When |
| --- | --- |
| `Q-5-58` | A social image was found but `website.site-url` is not set; image tags omitted. |
| `Q-5-59` | An `open-graph` / `twitter-card` value or option has the wrong type; ignored. |
| `Q-5-60` | A social image is a data URI, which crawlers can't fetch; ignored. |
| `Q-5-61` | `twitter-card` `image-width` / `image-height` set; ignored (Twitter has no size tags). |

## Work items

The transform landed in one piece rather than one commit per phase: the
value rules (explicit values, discovery, sizes) share one precedence
chain, and splitting them would have meant committing knowingly wrong
intermediate tag sets.

- [x] Phase 0: failing tests first. Integration tests in
  `crates/quarto-core/tests/integration/website_social_meta.rs` modelled
  on the two Quarto 1 reference sites (all failed with no tags before
  the transform); unit tests in the module for enabling, option types,
  discovery order, alt text, truncation, URL joining and escaping.
- [x] Phase 1: transform, gate, explicit values, URL resolution, head
  emission, pipeline registration (builder, B2 bucket table, the HTML /
  preview / Pandoc-exclusion name lists).
- [x] Phase 2: image and description discovery.
- [x] Phase 3: image sizes (native only; the WASM build returns no size)
  and resource copies for metadata images.
- [x] Phase 4: type validation and the four diagnostics (catalog,
  `docs/errors/project/Q-5-58..61`, docs sidebar).
- [x] Phase 5: user guide `docs/guides/projects/social-metadata.qmd`
  (with a "Differences from Quarto 1" section). The repo keeps no
  changelog file.
- [ ] PR for GH #1259; CI green.

## Verification log

- The q2 CLI on a copy of reference site 1 writes exactly Quarto 1's
  tags on all six pages; on site 2 the only differences are the two
  recorded divergences.
- A Jupyter cell with `#| classes: preview-image` beats a named
  `thumbnail.png` and gets its size tags, end to end.
- Windows cross-clippy can't build `quarto-core` from macOS (C build
  scripts for wasmtime, tree-sitter, aws-lc need MSVC headers); the new
  code uses no platform-specific API. The WASM hub-client crate
  type-checks with the change.
