---
title: 'Open Graph / Twitter card social meta tags (bd-tyvt)'
date: 2026-11-20
description: 'Emit og:* and twitter:* meta tags for website pages, matching Quarto 1''s website-meta.ts; investigation skeleton pending design alignment.'
status: draft  # Investigation — pending design alignment with user; do not implement before the go-ahead
braid:
  strand: bd-tyvt
---

**Checkout:** main checkout, branch `main` @ `4ca24c110` (no worktree created).
**GitHub:** [quarto-dev/q2#1259](https://github.com/quarto-dev/q2/issues/1259)
**Do not start implementation until the user gives the go-ahead.**

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

## Proposed phases (draft)

- Phase 0: Tests first. Unit tests on a pure "compute social tags from
  merged meta + derived values" helper (enable gate, merge, defaults, key
  mapping, card-style default, truncation); one integration test rendering
  the repro site and asserting the head tags.
- Phase 1: Core tags. New `website-social-meta` transform next to
  `website-canonical-url`: gate, merge, base values, provider keys, absolute
  image URL via `site-url`, template slot(s) in the full HTML head.
- Phase 2: Discovery. Preview image (the Q1 chain, data URIs skipped,
  `website.image` fallback) and first-paragraph description.
- Phase 3: Image dimensions via `imagesize` (native only; OG only), and
  registering the image as a page resource so it is copied to `_site`.
- Phase 4: Docs (website guide section on social metadata) and a
  changelog entry; update GH #1259.

## Open design questions for the user

1. **Where does it live?** (a) An AST transform + template slot, like
   `website-canonical-url` (works in the hub-client preview too, discovery
   reimplemented over the AST), or (b) a native post-render pass over the
   written HTML reusing `reader.rs` (closest to Q1, discovery reuses the
   existing selector chain, but adds another read/rewrite of every page and
   never runs in preview). Leaning (a).
2. **Image discovery over the AST.** If (a): is it acceptable for
   `.preview-image` detection to work on `Image` classes and on cell-output
   divs with `.preview-image` in the AST rather than on rendered HTML? Edge
   cases (images produced by raw HTML or by filters after the transform)
   would be missed where Q1 finds them.
3. **First cut scope.** Ship Phases 1-3 together for full parity, or ship
   Phase 1 (explicit title/description/image) first to unblock #1259 and
   follow with discovery and dimensions?
4. **Without `site-url`.** Q1 emits the image href unresolved (relative),
   which crawlers can't use. Match Q1, or skip the image tag and warn (a new
   diagnostic code)?
5. **Schema.** Should q2 validate the `open-graph` / `twitter-card`
   object keys (unknown key → warning), or keep accepting them as free-form
   maps like today?

## Risks / tradeoffs (draft)

- Template slot design: a `$for(social-meta)$` list of `name`/`content`
  pairs keeps the template small; per-tag `$if$`s match the existing head
  style. Either way the values must be attribute-escaped.
- Attribute name: Q1's `writeMetaTag` (`format-html-shared.ts:536`) writes
  `property=` for `og:*` and `name=` for everything else; match it. Q1
  appends at the end of `<head>`, after any include-in-header tags.
- Description truncation needs a port of Q1's `truncateText` "punctuation"
  mode; `listing/helpers.rs::truncate_text_at_space` only does the space
  mode.
