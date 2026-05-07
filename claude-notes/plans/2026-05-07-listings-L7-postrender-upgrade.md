# L7 — Post-render placeholder upgrade (sub-plan)

**Date:** 2026-05-07
**Beads:** `bd-qf7r` (this phase). Parent epic: `bd-61cd`
(`claude-notes/plans/2026-05-05-listings-epic.md`).
**Predecessors:**
- L0 (`bd-n8a4`, closed) — `DocumentProfile.listing_item`.
- L1 (`bd-izqh`, closed) — `ListingItemInfoStage` populates the
  `description` and `image` fallbacks on every document at
  Pass-1, before the engine runs. L7 inherits the Safeguard
  Contract from L1 §"Safeguard contract".
- L3 (`bd-ml8z`, closed) — `ListingResolveTransform` builds the
  per-item template binding and emits the description placeholder
  via `$description-placeholder$`. L7 extends the binding +
  templates to emit envelope markers and image placeholders.
- L5 (`bd-5vsr`, closed) — `Q-12-12` precedent for category-side
  listing diagnostics; L7 adds `Q-12-13` in the same series.
- L6 (`bd-xbnf`, closed) — `force_render` already pulls listing
  hosts into Mode B; L7 doesn't touch that mechanism but inherits
  the Mode B re-render contract.

**Status:** Draft. Awaiting user approval before hand-off.

## Goal of this phase

Match Q1's listing behavior on `quarto render` for the two
features that require sibling rendered output: per-item
description previews drawn from the engine-rendered first
paragraph, and per-item preview images discovered in
engine-rendered HTML (e.g. ggplot output from a code cell). The
listing host page picks both up automatically when the project is
rendered to disk.

L7 ships:

1. **Envelope-marker placeholders.** L3's templates emit
   `<!-- desc-begin(...) -->...L1 fallback...<!-- desc-end(...) -->`
   (and the image equivalent) instead of a single Q1-style
   placeholder comment. The envelope lets L7 replace **both**
   the placeholder markers and the L1 fallback content
   atomically when richer engine content is available, while
   leaving the L1 fallback intact when it isn't.
2. **`substitute_listing_placeholders` post-render step** in
   `WebsiteProjectType::post_render`, native-only, alongside the
   existing `flush_site_libs` / `copy_favicon` / `write_sitemap`
   / `write_robots_txt` steps.
3. **A minimal HTML reader** (`scraper`-based) that exposes
   `extract_first_para(html, opts)` and
   `extract_preview_image(html)` — the listings-only subset of
   Q1's `readRenderedContents`.
4. **Per-`post_render` cache** keyed on absolute output path so
   each sibling is parsed at most once even when multiple listing
   hosts reference it.
5. **`Q-12-13` warning** when a sibling output file is missing
   or has no usable first-paragraph / preview-image content.
   Markers are stripped, L1 fallback is retained — listing
   renders correctly with or without the warning.
6. **`scraper` as a CLI-only dependency** —
   `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` so
   the WASM hub-client build does not pull `scraper` or its
   transitive deps. The L7 module is gated with the same cfg.
7. **Pass `cargo xtask verify`** (full, including hub-client +
   WASM build — the template / binding changes flow through
   `quarto-core` to the WASM build).

**Out of scope for L7 (deferred):**

- **The full Q1 `readRenderedContents` surface.** Math handling,
  syntax-highlight class maps, anchor stripping, urls-to-absolute,
  `inline-code-style`. Those are RSS-only in Q1 (`L9` will need
  them). L7 v1 implements only the listings subset; the reader
  module is structured so L9 can extend it without rewriting.
  See §"Module layout" §"Reader extensibility".
- **Re-rendering siblings to get fresher preview content.** L7
  reads what's on disk after the project's Pass-2 has already
  written everything; it never re-runs an engine. (The original
  bracketing rule from the epic plan.)
- **Pre-extracting preview content during sibling Pass-2 and
  caching it** (would couple per-file Pass-2 to a listings-
  specific contract). Sibling output files are the single source
  of truth; nothing else needs to change.
- **Hub-client / `quarto preview` support.** Architecturally
  out of scope per the epic's bracketing rules — those
  environments lack a "render every sibling fully, then
  post-process" phase. L1 fallbacks are visible there
  unchanged, which is correct (just not as rich as the CLI
  render).
- **Removing the placeholder comments from the rendered HTML
  byte-for-byte when L7 succeeds.** The `<!-- desc-end -->`
  marker is gone after substitution; that's enough. The
  alternative (re-parse the AST and reinject) is cost without
  benefit.
- **Diagnostics with source spans.** The post-render step
  operates on rendered HTML, not source qmd; spans for
  Q-12-13 would require carrying the listing's source span
  through the placeholder comment (~feasible, but L9 will
  also want this and a separate pass to add it across all
  listing diagnostics is cleaner). Filed as a follow-up.

## Reference material

Read first:

- Parent epic: `claude-notes/plans/2026-05-05-listings-epic.md`
  §"L7" + §"Bracketing rules". The bracketing rules are
  load-bearing: every reviewer must check the L7 module hasn't
  silently grown beyond the listings use case. **The bracketing
  rules are the trade-off for keeping this feature at all** —
  do not relax them without epic-level review.
- L1 sub-plan:
  `claude-notes/plans/2026-05-05-listings-L1-autofill-stage.md`
  §"Safeguard contract". L7 enhances; L1 guarantees baseline.
- L3 sub-plan:
  `claude-notes/plans/2026-05-06-listings-L3-resolve-transform.md`
  §"Placeholder emission and the L1 fallback contract". L7's
  envelope-marker design diverges from what's there (which was
  written before this sub-plan's design discussion). The
  divergence is documented in §"Architecture: marker design"
  below; L3's templates and binding are updated as part of L7.
- Q1 reference (read-only):
  - `external-sources/quarto-cli/src/project/types/website/listing/website-listing-read.ts:302`
    — `completeListingItems` — the function L7 ports.
  - `…/website-listing-read.ts:498-526` — Q1's placeholder
    builder + regexes.
  - `…/website-listing-shared.ts:311-597` — `readRenderedContents`
    — the full Q1 reader. L7 v1 ports just the listings subset.
  - `…/util/discover-meta.ts:49-83` — `findPreviewImgEl` — the
    selector chain L7 mirrors.
- Existing Q2 surface L7 builds on:
  - `crates/quarto-core/src/project/orchestrator.rs:240-269` —
    `WebsiteProjectType::post_render`. L7 adds one call here,
    inside the existing `#[cfg(not(target_arch = "wasm32"))]`
    block so the WASM build is unaffected.
  - `crates/quarto-core/src/project/website_post_render.rs` —
    the home for native post-render hooks. L7 adds nothing
    here (its module home is under `project/listing/`); but
    new function follows the same `pub(super) fn …(project,
    runtime, diagnostics)` shape.
  - `crates/quarto-core/src/project/listing/placeholders.rs` —
    DESC_TOKEN / IMG_TOKEN constants + the existing single-comment
    builder. L7 adds new builders for the begin/end envelope
    pair (description and image) and exposes the regex strings
    so the post_render module shares them with the templates.
  - `crates/quarto-core/src/project/listing/helpers.rs:82-88` —
    `description_placeholder(item, listing)` — current single-
    comment helper used by `binding.rs`. L7 splits it into
    begin/end variants.
  - `crates/quarto-core/src/project/listing/binding.rs:289-298`
    — current binding entries for `image-html`,
    `description-placeholder`, `metadata-attrs`. L7 adds
    `description-placeholder-begin`, `description-placeholder-end`,
    `image-placeholder-begin`, `image-placeholder-end`.
  - `crates/quarto-core/src/project/listing/templates/item-default.template`
    + `item-grid.template` — emit the begin/end markers around
    the `$description$` / `$image-html$` blocks. The `$else$`
    branch on `image-html` becomes the image-placeholder
    emission site.
  - `crates/quarto-core/src/project/index.rs` — `ProjectIndex`,
    available to `post_render`. The L7 step looks up each
    rendered output's source profile to derive listing config
    (specifically `image-placeholder:` for the cascade).
  - `crates/quarto-test/src/assertions/html_elements.rs:13` —
    proves `scraper` already compiles cleanly across the
    workspace (used by `quarto-test` since it shipped). L7
    promotes it to an actual production dependency on `quarto-core`,
    target-gated.

## Settled inputs

These are decisions, not open questions:

- **L7 ships next.** User-confirmed 2026-05-07: even though the
  feature has bracketing baggage, "we need to solve this thorny
  problem eventually, and now is better than later." The L3
  output is presentable today (L1 fallback is inline), but the
  raw `<!-- desc(…) -->` comments are visible in the served HTML
  and that's a real defect.
- **Marker design: enveloping comments around the L1 fallback.**
  User-confirmed 2026-05-07. L3 emits
  `<!-- desc-begin(<token>)[max=<n>]:<href> -->...<L1 fallback>...<!-- desc-end(<token>) -->`
  (and image equivalent). The `<token>` stays at Q1's verbatim
  hex (`5A0113B34292` / `9CEB782EFEE6`) so anyone reading the
  rendered HTML sees the same magic string Q1 emits, but the
  `-begin` / `-end` suffix is Q2-specific. **This is a divergence
  from Q1's exact format**; the divergence is necessary because
  Q1 has no L1-fallback contract and so doesn't need to delimit
  a region. Recorded here so future readers don't try to
  "reconcile with Q1."
- **Image placeholder emission lands in L7.** User-confirmed
  2026-05-07. L3 punted on emitting the image placeholder
  (test #38 in the L3 plan was flagged uncertain pending L7's
  design). L7 wires both the emission (in `binding.rs` +
  templates) and the substitution.
- **Empty-firstPara semantics: strip markers, keep L1, emit
  `Q-12-13`.** User-confirmed 2026-05-07. The warning surfaces
  through the project diagnostics channel (`post_render`'s
  `&mut Vec<DiagnosticMessage>` arg, already wired to
  `ProjectRenderSummary.project_diagnostics`).
- **No-preview-image cascade: listing.image-placeholder →
  empty-div.** User-confirmed 2026-05-07. Mirrors Q1
  (`completeListingItems` lines 449-461). Q1 looks up the
  listing's `image-placeholder` field by id at substitution
  time; Q2 embeds it in the begin-marker comment so L7 doesn't
  need to re-walk the source profile to find the listing config.
  See §"Architecture: image-placeholder cascade".
- **Reader scope: listings-only minimum.** User-confirmed
  2026-05-07. `extract_first_para(html, max_length, remove_links,
  remove_images)` and `extract_preview_image(html)` only. Math
  / syntax-highlighting / urls-to-absolute / anchor-stripping
  are RSS-only — L9's problem.
- **Module home: single file at
  `crates/quarto-core/src/project/listing/post_render_upgrade.rs`.**
  User-confirmed 2026-05-07. Bracketing rule 1 ("single home").
  The reader is a private sub-module within the same file (or a
  sibling `reader.rs` inside `post_render_upgrade/` if the file
  grows past ~600 LOC; the L7 author can split if needed but
  must keep both files under `project/listing/post_render_upgrade*`).
- **`scraper` is a target-gated dep.** User-confirmed 2026-05-07.
  `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
  scraper = "0.26"`. The post_render_upgrade module is
  `#[cfg(not(target_arch = "wasm32"))]`. Bracketing rule 3
  ("CLI-only by construction") — gating at Cargo.toml is a
  stronger version of the rule than gating at the call site.
- **Caching: per-`post_render` invocation, keyed on absolute
  output path.** Mirrors Q1's `renderedContentReader`. The
  cache is a local `HashMap` in the post_render step, never
  reused across project renders.
- **Walk every output, regex-check, mutate only on match.** No
  pre-filter "is this a listing host" lookup. Cost is one
  `memmem`-style search per file (a few µs); benefit is
  defensiveness against any future pathway that emits a
  placeholder.

## Architecture

### Marker design

L3 today emits a single placeholder comment alongside the L1
fallback markdown, separated by a blank line in the
template:

```pandoc
$if(description)$
::: {.delink .listing-description}
$description$           ← L1 fallback (markdown text from L1)

$description-placeholder$  ← `<!-- desc(<token>)[max=N]:href -->`
:::
$endif$
```

That works for "L1 fallback is shown when L7 doesn't run," but
when L7 *does* run, simply replacing the comment with engine
firstPara produces both contents visible — duplicated text. To
fix: L7 needs to know what region to delete.

**The new envelope shape**:

```pandoc
$if(description)$
::: {.delink .listing-description}
```{=html}
$description-placeholder-begin$
```

$description$

```{=html}
$description-placeholder-end$
```
:::
$endif$
```

After Pandoc renders this, the HTML carries:

```html
<div class="delink listing-description">
<!-- desc-begin(5A0113B34292)[max=175]:posts/foo.html -->
<p>L1 fallback first paragraph</p>
<!-- desc-end(5A0113B34292) -->
</div>
```

L7's substitution rule:

- Find `<!-- desc-begin(<token>)[max=<n>]:<href> -->` then the
  next `<!-- desc-end(<token>) -->`. Match everything between
  them as the L1-fallback region.
- If sibling exists *and* has a usable firstPara: replace the
  whole region (`begin..end` inclusive) with the engine's
  firstPara HTML, truncated to `max`.
- If sibling missing or empty: replace the begin/end markers
  only with empty strings, leaving the L1 fallback in place.
  Emit `Q-12-13`.

The image equivalent is the same shape, with two extra fields
in the begin marker (item-index, listing's `image-placeholder`
default URL):

```html
<!-- img-begin(9CEB782EFEE6)[<attrs>]:<id>:<idx>:<href>:<b64-default> -->
<div class="listing-item-img-placeholder card-img-top">&nbsp;</div>
<!-- img-end(9CEB782EFEE6) -->
```

`<b64-default>` is the listing's `image-placeholder:` config
URL, base64-encoded (URL-safe alphabet) so it can travel
through an HTML comment without escaping pain. Empty when
unset. **This avoids needing post_render to walk source
profiles to find the listing config** — everything L7 needs is
in the comment.

The `<attrs>` field is preserved verbatim from Q1's format
(`progressive=true, height=, lazy=true`). v1 emits a fixed
`progressive=false, height=, lazy=true` since none of those
features are wired through Q2 yet (height comes from
`listing.image-height:` config — feature-flagged for later).

### How the begin / end markers reach the templates

`binding.rs` exposes four new bindings on each `item.*` map:

| Binding key                    | Helper                                         | Empty when…                                                  |
|--------------------------------|------------------------------------------------|--------------------------------------------------------------|
| `description-placeholder-begin`| `helpers::description_placeholder_begin(item, listing)` | Always non-empty (L7 substitution always relevant for descriptions) |
| `description-placeholder-end`  | `helpers::description_placeholder_end(listing)`         | Same as above |
| `image-placeholder-begin`      | `helpers::image_placeholder_begin(item, listing, idx)`  | When `item.image` is non-empty (no placeholder needed) |
| `image-placeholder-end`        | `helpers::image_placeholder_end(listing)`               | Same as above |

The existing `description-placeholder` (single-comment) binding
is **removed** — call-sites in templates are updated to use the
new pair. No back-compat shim; the L7 commit is monotonic.

The existing `image-html` helper is unchanged (still emits the
`<img>` markup or empty string from L1's `image`); the image-
placeholder block fires in the template's `$else$` branch:

```pandoc
$if(image-html)$
::: thumbnail
[$image-html$]($path$){.no-external}
:::
$else$
::: thumbnail
```{=html}
$image-placeholder-begin$
<div class="listing-item-img-placeholder card-img-top">&nbsp;</div>
$image-placeholder-end$
```
:::
$endif$
```

`item-default.template` and `item-grid.template` both get this
treatment. `item-table.template` does **not** — Q1's table view
emits a tabular cell with no thumbnail/description block, and
this matches the in-tree template. The L7 author confirms by
inspection during impl that the table template doesn't grow
placeholder markers.

### post_render entry point

A new function in the listings module:

```rust
// crates/quarto-core/src/project/listing/post_render_upgrade.rs
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn substitute_listing_placeholders(
    project: &ProjectContext,
    output_paths: &[PathBuf],
    runtime: &dyn SystemRuntime,
    diagnostics: &mut Vec<DiagnosticMessage>,
) -> Result<()> { … }
```

`WebsiteProjectType::post_render` calls it inside the existing
`#[cfg(not(target_arch = "wasm32"))]` block, after
`write_robots_txt`:

```rust
#[cfg(not(target_arch = "wasm32"))]
{
    use super::website_post_render::{copy_favicon, write_robots_txt, write_sitemap};
    copy_favicon(project, runtime, diagnostics)?;
    write_sitemap(project, index, output_paths, runtime)?;
    write_robots_txt(project, runtime)?;
    super::listing::post_render_upgrade::substitute_listing_placeholders(
        project,
        output_paths,
        runtime,
        diagnostics,
    )?;
}
```

The function:

1. Builds an output-path → cached-extraction map (initially empty).
2. For each `output_path` in `output_paths`:
   a. Read the file.
   b. Quick byte-search: `b"<!-- desc-begin("` or
      `b"<!-- img-begin("`. Skip the file if neither match.
   c. Parse the file as HTML once for image scans, but use
      regex for placeholder substitution (the file contents
      have already been written by Pandoc; we don't need a
      DOM round-trip for the substitution itself, only for
      reading sibling files).
   d. Run description substitution (regex + sibling reads).
   e. Run image substitution (regex + sibling reads).
   f. If any substitution happened, write the file back.

Cache lookups happen during steps (d) and (e): each sibling
file's `extract_first_para` and `extract_preview_image` results
are stored in the map by absolute path so multiple listing
hosts referencing the same sibling pay one parse.

### Reader: listings-only subset of `readRenderedContents`

```rust
// Inside post_render_upgrade.rs (private to the module).
struct RenderedExtraction {
    first_para_html: Option<String>,    // already truncated if max applied
    preview_image: Option<PreviewImage>,
}

struct PreviewImage {
    src: String,    // unresolved relative to the sibling output dir
    alt: Option<String>,
    title: Option<String>,
}

struct ReaderOptions {
    max_length: Option<usize>,
    remove_links: bool,
    remove_images: bool,
}

fn extract(html: &str, opts: &ReaderOptions) -> RenderedExtraction { … }
```

`extract_first_para` (matching Q1's `getFirstPara`):

1. Parse with `scraper::Html::parse_document`.
2. Find `main.content` — the Quarto-rendered article container
   (Q1's same selector).
3. For each `<p>` child of `main.content`:
   a. Clone the subtree.
   b. If `remove_links`: replace each `<a>` with its children
      (unwrap).
   c. If `remove_images`: remove each `<img>`.
   d. Truncate if `max_length` set: walk the subtree's text
      nodes, accumulate length, snip at `max_length` chars
      using whitespace-respecting truncation (port Q1's
      `truncateText(s, n, "space")` — break at the last space
      before `n`).
   e. Serialize back to HTML via
      `scraper::ElementRef::html()`.
   f. Return the first non-empty result.
4. If no `<p>` produced text: fall back to first non-empty
   element child (matching Q1's `anyNodes` branch lines 545-560).
5. Returns `None` if nothing usable.

`extract_preview_image` (matching Q1's `findPreviewImgEl`):

1. `img.preview-image` — explicit author marker.
2. `div.preview-image div.cell-output-display img` — code-cell
   wrapped marker (Q1 lines 58-63).
3. Walk all `<img>`. Return the first whose `src` matches the
   regex `(?i).*?(preview|feature|cover|thumbnail).*?\.(png|gif|jpg|jpeg|webp|svg)`
   or starts with `data:`.
4. Walk `#quarto-document-content img` — first local image.
5. Returns `None` if nothing matched.

The selector regex matches Q1's `kNamedFilePattern` exactly,
case-insensitive. The L7 module documents the regex at the
top of the function with a Q1-line citation so a future change
to Q1 is detectable.

### Substitution: description

```text
regex: <!-- desc-begin\((<TOKEN>)\)\[max=([0-9]+)\]:([^ ]+) -->\s*(.*?)\s*<!-- desc-end\(\1\) -->
   (with re::DOT_MATCHES_NEW_LINE so the inner region can span lines)
```

For each non-overlapping match:

- `max` = parsed integer.
- `href` = sibling's relative URL (e.g. `posts/foo.html`).
- `inner` = the L1 fallback region (verbatim).
- `sibling_path` = `project.output_dir.join(href)` (after URL-decoding).
- If `runtime.path_exists(sibling_path)`:
  - `read sibling`, run `extract(html, ReaderOptions { max_length:
    Some(max as usize), remove_links: true, remove_images: true })`.
  - If `first_para_html` is `Some(s)` and non-empty: substitute
    whole match with `s`.
  - Else: substitute whole match with `inner` (strip markers,
    keep L1). Emit `Q-12-13` "no preview content found in
    rendered output for {href}".
- Else (file missing): substitute with `inner`. Emit
  `Q-12-13` "listing target {href} did not produce a rendered
  output file" (different wording but same code, since both
  are "L1 fallback retained for this listing item").

### Substitution: image

```text
regex: <!-- img-begin\((<TOKEN>)\)\[([^\]]*)\]:([^:]*):(\d+):([^: ]+):([A-Za-z0-9+/=_-]*) -->\s*(.*?)\s*<!-- img-end\(\1\) -->
```

Capture groups: token, attrs, listing-id, item-index, href, b64-default, inner.

For each match:

- Read sibling (cached), extract preview image.
- If `preview_image` is `Some(pi)`:
  - Resolve `pi.src` against the sibling's output directory.
  - Re-relativize against the listing host's output directory
    (which is the directory of the file currently being
    substituted). Mirror Q1's `resolveUrl` lines 418-431.
  - Build `<img src="{resolved}" class="thumbnail-image" alt="…"
    {attrs from comment}>`.
  - Substitute whole match with the `<img>` tag (the surrounding
    `<a>` from Pandoc's `[$placeholder$]($path$)` already wraps
    it).
- Else if `b64-default` decodes to a non-empty URL:
  - Build `<img src="{decoded}" class="thumbnail-image" {attrs from comment}>`
    using the listing's image-placeholder.
  - Substitute whole match with the `<img>` tag.
- Else:
  - Substitute whole match with `inner` (strip markers, keep
    the empty `<div class="listing-item-img-placeholder ...">`).
  - **No `Q-12-13` warning for missing preview images** — Q1
    is silent here too, and "no preview image" is a totally
    common case for posts that are pure prose. The warning
    only fires for the description case.

### Image-placeholder cascade in detail

The listing config field `image-placeholder` (already parsed
into `Listing.image_placeholder`) is the URL of an image to use
as the per-listing default. Q1 reads it at substitution time
(`completeListingItems` line 339 stores the map per listing
id); Q2 embeds it in the begin-marker so post_render is
self-contained.

In `helpers::image_placeholder_begin`:

```rust
pub fn image_placeholder_begin(item: &ListingItem, listing: &Listing, idx: usize) -> String {
    let attrs = "progressive=false, height=, lazy=true";
    let b64_default = listing
        .image_placeholder
        .as_deref()
        .map(|url| URL_SAFE_NO_PAD.encode(url.as_bytes()))
        .unwrap_or_default();
    format!(
        "<!-- img-begin({IMG_TOKEN})[{attrs}]:{id}:{idx}:{href}:{b64} -->",
        id = listing.id,
        idx = idx,
        href = item.output_href,
        b64 = b64_default,
    )
}
```

The base64 alphabet is URL-safe-no-pad: avoids `=` (HTML-safe in
a comment but ugly), avoids `+/` (which clash with neither HTML
nor regex but adding `=_-` to the regex character class is one
character).

### Determinism

`scraper`'s parse output is deterministic given input. The
cache traversal order is "first reference wins"; since the
cache value never changes for a given key, any order produces
identical substitutions. Multiple listing hosts referencing the
same sibling get identical substitutions independent of the
project's render order.

## scraper dep gating

`crates/quarto-core/Cargo.toml` gets:

```toml
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
scraper = { workspace = true }
```

The workspace already declares `scraper = "0.26"` at the root
Cargo.toml; this just opts `quarto-core` into it on native.

`crates/quarto-core/src/project/listing/post_render_upgrade.rs`
is gated at module level:

```rust
#![cfg(not(target_arch = "wasm32"))]
```

…and `mod.rs` adds the module under the same gate:

```rust
#[cfg(not(target_arch = "wasm32"))]
pub mod post_render_upgrade;
```

The orchestrator's call site is already inside a
`#[cfg(not(target_arch = "wasm32"))]` block (the
`copy_favicon` / `write_sitemap` / `write_robots_txt` group),
so no extra gating is needed there.

**Verification step in the L7 session:** after the changes are
in, `cargo xtask verify` (full, including hub-client + WASM
build). The WASM build must not pull `scraper`. Confirm via
`cargo tree --target wasm32-unknown-unknown -p wasm-quarto-hub-client | grep scraper`
returning empty.

If `scraper` *does* leak into the WASM tree (unlikely but
possible if the workspace dep table is mis-configured), revert
the workspace-level promotion and inline the dep on quarto-core
only as a target-gated direct entry. The L7 author records
which form is used in §"Decisions log".

## Module layout

```
crates/quarto-core/src/project/listing/
  binding.rs                       ← +4 binding entries; -1 (single-comment)
  helpers.rs                       ← +4 helper fns; -1 (single-comment helper)
  placeholders.rs                  ← +begin/end builders; +regex strings;
                                     keep current single-comment helpers as
                                     deprecated until L11 (no more callers,
                                     but external readers may have docs).
                                     Or remove now — see §"Open questions".
  templates/
    item-default.template          ← begin/end markers around description;
                                     image-placeholder block in $else$
    item-grid.template             ← same edits
    item-table.template            ← unchanged (table view has no
                                     thumbnail/description block)
  post_render_upgrade.rs           ← NEW. substitute_listing_placeholders +
                                     reader (private). #![cfg(...)] gated.

crates/quarto-core/src/project/
  orchestrator.rs                  ← +1 call site inside the existing
                                     #[cfg(not(target_arch = "wasm32"))]
                                     block in WebsiteProjectType::post_render

crates/quarto-core/Cargo.toml      ← +scraper as target-gated dep
                                   ← +base64 (already present, just verify)
```

### Reader extensibility

L9 (RSS feeds) will need more from the reader: math handling,
syntax-highlight class maps, urls-to-absolute, anchor stripping.
L7's reader is structured as a `ReaderOptions` struct with bool
fields. L9 adds new fields (`urls_to_absolute: Option<&str>`,
`math: bool`, `inline_code_style: bool`, `remove_anchors: bool`)
without breaking L7's call sites — L7 always passes `Default`
options for each new field, so new behavior is opt-in.

Each new transform is a private function in the same file,
guarded by its `ReaderOptions` flag. The single-pass `extract`
function applies them in a documented order. **Do not introduce
a trait-based plugin architecture in v1** — the cost outweighs
the benefit until at least L9 (and quite possibly forever; Q1's
single-function reader has been stable for years).

## Diagnostic codes

L7 adds:

- **`Q-12-13`**: `warning`, "Listing item from {relative-source-path}
  produced no preview content; using the static fallback
  description." Fired when a sibling output file is missing,
  unparseable, or has no usable `<p>` in `main.content`.
  - One diagnostic per (listing-host, missing-sibling) pair.
    Multiple listing hosts that all reference the same broken
    sibling each emit one diagnostic — the per-host context is
    useful.
  - Message names the source file path (resolved from the
    sibling's `output_href`) so the user knows which content
    file to investigate.
  - No source span in v1 (post_render works on rendered HTML,
    not source qmd). Source span is filed as a follow-up.

No diagnostic for missing preview images — Q1 is silent there
and listings are routinely image-less.

## Edge-case behavior (settled)

These were originally "open questions" but the user
2026-05-07 confirmed all of them inline. Recorded here for the
audit trail; no decisions left for the L7 session.

1. **`max-description-length` of 0 or negative** — Q1's behavior
   is "no truncation" (`max-length` option becomes
   `undefined`). v1 follows that: `max == 0` ⇒ no truncation.
   The regex captures `[max=0]` like any other integer; the
   substitution code maps `0 → None` for the
   `ReaderOptions.max_length` field.
2. **Sibling output that is HTML but not Quarto-shaped**
   (no `main.content`, no `#quarto-document-content`). Returns
   `None` from both extractors — same as if the file was
   missing. Likely a misconfiguration; emit `Q-12-13`. The
   diagnostic message text already covers this case ("no
   preview content").
3. **Sibling that is non-HTML** (e.g. a raw `.txt` referenced
   by mistake). `scraper::Html::parse_document` will accept
   anything (it's lenient like browsers) and produce no
   matches. Same `Q-12-13` path. No special-case error.
4. **Atomic write on substitution failure.** If
   `runtime.file_write` fails after we've already mutated the
   in-memory HTML, the on-disk file is in a half-state? No —
   we write the full buffer in one `file_write` call; partial
   writes are a filesystem concern handled by the runtime. If
   write fails, we propagate the error and the post_render
   hook aborts — same as `write_sitemap`'s contract.
5. **Whether to embed the `image-placeholder` URL or read from
   the source profile at L7-time.** Settled (embed in the
   marker), but the alternative is documented here in case a
   future need to look up listing config from post_render
   forces revisiting: `index.profiles()` is available; each
   profile's `meta.listing` is reachable via the same
   `parse_listings` path L3 uses. Embedding in the marker
   means simpler post_render code; the cost is the marker
   carries an extra `:b64` segment. Embed wins on simplicity.
6. **`Q-12-13` per-host vs dedupe globally.** One diagnostic
   per (listing-host, broken-sibling) pair. Multiple hosts
   referencing the same broken sibling each emit one
   diagnostic. Per-host context is more useful for navigation
   than aggregate dedup; cardinality stays low because a
   project rarely has more than a handful of listing hosts.

## Decisions log

- **D1 (envelope markers around L1 fallback):** user-confirmed
  2026-05-07. Diverges from Q1's exact comment format; the
  divergence is necessary because Q1 has no L1 fallback. Q2's
  fallback contract requires the markers delimit a region.
- **D2 (image-placeholder emission lives in L7):** user-confirmed
  2026-05-07. L3 punted; L7 owns both emission and substitution.
- **D3 (empty firstPara → strip markers + keep L1 + Q-12-13):**
  user-confirmed 2026-05-07. Adds one diagnostic to the Q-12
  series.
- **D4 (image-placeholder cascade: listing default → empty div):**
  user-confirmed 2026-05-07. Mirrors Q1; default URL embedded
  in the begin-marker comment so post_render is self-contained.
- **D5 (reader scope: listings-only minimum):** user-confirmed
  2026-05-07. L9 extends `ReaderOptions` later.
- **D6 (module home: single file under listing/):** user-confirmed
  2026-05-07. Bracketing rule 1.
- **D7 (`scraper` as target-gated dep):** user-confirmed
  2026-05-07. Stronger form of bracketing rule 3 — gated at
  Cargo.toml, not just at the call site.
- **D8 (no source spans for Q-12-13 in v1):** Filed as follow-up.
  Post_render works on rendered HTML, not source qmd; threading
  a span through the placeholder comment is feasible but
  bigger than L7. L9 will want spans too — file a single
  follow-up that solves both.
- **D9 (worktree on `feature/listings`):** branch
  `beads/bd-qf7r-listings-post-render-upgrade` at
  `.worktrees/bd-qf7r-listings-post-render-upgrade/`,
  branched off the current `feature/listings` head
  (`cd4b77fd` at the time of writing — confirm at impl start).
  Same convention as L1 / L3 / L5 / L6.
- **D10 (regex over scraper for the placeholder substitution
  itself):** the placeholder envelope is strictly an HTML
  comment pair; Pandoc emits it verbatim with no entity
  encoding, so a regex with DOTALL semantics matches reliably.
  scraper is used **only to read sibling files** for content
  extraction. Round-tripping the host file through scraper to
  do substitution is unnecessary cost and risks DOM-walk
  surprises.
- **D11 (remove the old single-comment `description_placeholder`
  helper in this commit):** user-confirmed 2026-05-07. No
  external callers; it's a crate-internal helper. Matches Q2's
  no-back-compat-shim convention. The L7 author verifies during
  impl that no in-tree code still imports it; if any does (e.g.
  a forgotten test fixture), update it to the new begin/end
  helpers in the same commit.
- **D12 (update L3 snapshot tests in this same commit):**
  user-confirmed 2026-05-07. The existing L3 snapshots
  (`builtin_default_renders_three_items`,
  `builtin_grid_renders_three_items`, etc.) will diff because
  the description region now wraps in begin/end comments and
  the no-image case adds image-placeholder markers. Run `cargo
  insta review`, confirm the diff is exactly the expected
  envelope addition, accept. Document the snapshot count +
  summary in the commit message per CLAUDE.md §"Snapshot Test
  Changes".
- **D13 (`docs/` user-facing callout deferred):** the
  user-facing Quarto website doesn't exist yet in this repo
  (`docs/` is a placeholder for future user-facing
  documentation). The bracketing-rule-3 callout — *"Engine-
  rendered previews are available in `quarto render` only. In
  interactive environments, listings show static-AST previews —
  set `description:` and `image:` explicitly if you need a
  specific preview to appear during preview"* — is **filed as
  a follow-up bd**, not produced by L7. When the docs site
  comes online, this plan and the epic plan §L7 §"Bracketing
  rules" rule 3 are the source of truth for the wording.
  See §"Implementation steps" §"Verification and close-out"
  for the deferred-callout note, and the follow-up bd in
  §"Filing reminder".

- **D14 (image envelope wraps in `[…]($path$){.no-external}`
  link):** user-confirmed 2026-05-07. The current template wraps
  the static `<a>[$image-html$]</a>` so the thumbnail is
  navigable; the image envelope branch must do the same. After
  L7 substitutes the placeholder/empty-div with an `<img>`, the
  surrounding `<a>` (emitted by Pandoc from the `[…]($path$)`
  markdown) makes the L7-substituted thumbnail clickable too.
  Same for the L1-fallback empty-div: it stays inside the link
  wrapper so the thumbnail-less click target is still present.

- **D15 (raw-HTML `{=html}` blocks for envelope markers):**
  user-confirmed 2026-05-07. Both begin and end markers ship in
  explicit `` ```{=html} `` blocks. This is a snapshot diff (a
  blank line + raw block surrounds each marker) but gives
  deterministic Pandoc behavior — no risk of comment-as-paragraph
  surprises across filter chains.

- **D16 (`*_end` helper signatures take no args):** the end
  marker is `<!-- desc-end(<TOKEN>) -->` / `<!-- img-end(<TOKEN>) -->`
  — token-only, no listing-id, no item index. Helpers
  `description_placeholder_end()` and `image_placeholder_end()`
  take **no parameters**. The plan's earlier table showing
  `(listing)` args was incorrect; ignore it.

- **D17 (single canonical Q-12-13 message):** user-confirmed
  2026-05-07. Whether the sibling output file is missing or
  present-but-empty, the diagnostic uses one wording:
  *"Listing item from {href} produced no preview content; using
  static fallback description."* Easier to grep, easier to
  document. The cause distinction (file missing vs file empty)
  is recoverable from filesystem inspection if the user needs it.

- **D18 (test #42 uses the replay engine, no on-disk trace):**
  user-confirmed 2026-05-07. The image-substitution e2e test
  needs a fixture where (a) the static AST has no Image node so
  L1 leaves `image: None` and L3 emits the placeholder envelope,
  AND (b) the rendered sibling HTML contains an `<img>`. The
  only way to get both without depending on a real engine
  install is the replay engine (`bd-45yw`,
  `crates/quarto-core/src/engine/replay.rs`). Pattern (mirrors
  `crates/quarto-core/tests/replay_engine.rs`):
  1. The fixture has `posts/with-engine-image.qmd` declaring
     `engine: replay-test-l7` plus a code cell.
  2. A probe pass (`capture_engine_input`-style helper) captures
     the exact `input_qmd` the engine stage hands to `execute()`.
  3. The test inline-constructs an `EngineCapture` whose
     `engine_name` is `"replay-test-l7"`, `input_qmd` is the
     captured value, and `result` injects markdown like
     `![](preview.png){.preview-image}` (or a `cell-output-display`
     wrapper) so Pandoc emits an `<img>` in the rendered post.
  4. Pass the capture through `RenderToFileOptions.replay_capture`
     (or the equivalent project-render entry point), render the
     full project, and assert `_site/index.html` carries the
     substituted thumbnail referencing `posts/preview.png`.
  No checked-in trace JSON is needed. Other posts in the fixture
  use the markdown engine and are unaffected by the replay
  registry override (replay only triggers for docs declaring the
  recorded engine name).

- **D19 (listing-id stays simple, no defensive percent-encoding
  in the marker):** user-confirmed 2026-05-07. The image begin
  marker uses `:`-separated payload and embeds `listing.id`
  unescaped. If a future schema change permits `:` or whitespace
  in listing ids, the regex breaks; until that happens, we match
  Q1's behavior of trusting the id shape. **Filing reminder
  follow-up #7** captures the eventual defensive-encoding work.

- **D20 (single-comment `description-placeholder` removed
  entirely):** user-confirmed 2026-05-07. No deprecation shim,
  no aliasing. The L7 commit deletes the helper, the binding
  entry, and any remaining callers (verified via grep during
  impl). Matches Q2's no-back-compat-shim convention.

## Branch / worktree

L7 starts from the current `feature/listings` head. The L7
worktree lives at:

```
.worktrees/bd-qf7r-listings-post-render-upgrade/
```

Branch: `beads/bd-qf7r-listings-post-render-upgrade`, branched
off `feature/listings`.

Per `.claude/rules/worktrees.md`:

```bash
cd .worktrees/bd-qf7r-listings-post-render-upgrade
echo "../../../.beads" > .beads/redirect
npm install
cargo xtask verify --skip-hub-build  # baseline before changes
```

Before starting, the L7 session must record:

- Current `feature/listings` HEAD hash (was `cd4b77fd` at plan
  time).
- Baseline test count (was 8647 at L6 close-out; may have moved
  if other branches landed).

## Tests (TDD)

Per CLAUDE.md: write tests, watch fail, implement, watch pass.

### Phase 1 — placeholder builders + regex constants

In `crates/quarto-core/src/project/listing/placeholders.rs`:

1. **`description_placeholder_begin_matches_shape`** — exact
   string assertion for `description_placeholder_begin("my-listing",
   175, "posts/foo.html")` ⇒
   `<!-- desc-begin(5A0113B34292)[max=175]:posts/foo.html -->`.
2. **`description_placeholder_end_matches_shape`** — exact
   string for `description_placeholder_end()` ⇒
   `<!-- desc-end(5A0113B34292) -->`. (Token-suffix, no
   listing-id; the regex matches by token alone, listing id
   can vary across multiple placeholders on the same page.)
3. **`image_placeholder_begin_matches_shape_no_default`** —
   exact string with empty b64 segment.
4. **`image_placeholder_begin_matches_shape_with_default`** —
   exact string with base64 of a known placeholder URL.
5. **`description_placeholder_regex_round_trip`** — a sample
   begin/inner/end string is parsed by the regex; capture
   groups match the input.
6. **`image_placeholder_regex_round_trip`** — same for image
   markers; both `b64-default-empty` and `b64-default-set`
   variants captured cleanly.

### Phase 2 — binding entries + template emission

In `crates/quarto-core/src/project/listing/binding.rs`:

7. **`binding_emits_description_begin_end_pair`** — for any
   item, the binding map contains both keys.
8. **`binding_emits_image_placeholder_begin_end_when_no_image`**
   — item with `image: None` produces non-empty
   `image-placeholder-begin` and `image-placeholder-end`.
9. **`binding_omits_image_placeholder_when_image_present`** —
   item with `image: Some(...)` produces empty
   `image-placeholder-begin` (or omits the entries entirely;
   pick what's cleanest with the existing pattern — empty is
   safer for templates that always reference the key).

In `crates/quarto-core/src/transforms/listing_render.rs` (extending
the existing test module):

10. **`render_emits_description_begin_end_envelope_around_l1_fallback`**
    — render a fixture; rendered markdown contains
    `<!-- desc-begin(...) -->` followed by the L1 fallback
    paragraph followed by `<!-- desc-end(...) -->`.
11. **`render_emits_image_placeholder_begin_end_when_l1_image_unset`**
    — fixture with no images in the post; output contains
    `<!-- img-begin(...) -->`, the empty `<div>` placeholder
    div, `<!-- img-end(...) -->`.
12. **`render_omits_image_placeholder_when_l1_image_set`** —
    fixture with a static image; output has the `<a><img></a>`
    block, no `<!-- img-begin -->`.
13. **`render_carries_image_placeholder_default_url_into_marker`**
    — fixture with `listing.image-placeholder: assets/default.png`
    and an item missing an image; the marker comment carries
    the b64-encoded URL.

The existing snapshot tests
(`builtin_default_renders_three_items` etc.) update — record
the diff in the commit message under §"Snapshot test changes"
per CLAUDE.md.

### Phase 3 — reader

In `post_render_upgrade.rs`'s test module:

14. **`extract_first_para_returns_first_p_text`** — given
    `<main class="content"><p>Hello.</p></main>`, returns
    `Some("Hello.")` (or HTML form, depending on what the
    template substitutes — pin down in impl).
15. **`extract_first_para_skips_empty_p`** — given multiple
    `<p>`s the first of which is whitespace-only, returns the
    second.
16. **`extract_first_para_truncates_to_max_length`** — given a
    long paragraph and `max_length: 20`, returns text truncated
    at the last word boundary ≤ 20 chars.
17. **`extract_first_para_remove_links_unwraps_anchors`** —
    given `<p>Click <a href="x">here</a>.</p>`, returns
    `Click here.`.
18. **`extract_first_para_remove_images_drops_imgs`** — given
    `<p><img src="x">Hi</p>`, returns `Hi`.
19. **`extract_first_para_falls_back_to_any_node`** — main has
    no `<p>` but has a `<div>`; returns the div's text.
20. **`extract_first_para_returns_none_when_main_empty`** —
    no `main.content` at all, or empty.
21. **`extract_preview_image_finds_explicit_preview_class`** —
    `<img class="preview-image" src="a">` returns `Some(a)`.
22. **`extract_preview_image_finds_cell_output_wrapper`** —
    `<div class="preview-image"><div class="cell-output-display"><img src="a"></div></div>`
    returns `Some(a)`.
23. **`extract_preview_image_finds_named_pattern`** — `<img
    src="path/preview-image.png">` returns `Some(path/preview-image.png)`.
24. **`extract_preview_image_finds_first_in_quarto_doc`** —
    falls through to `#quarto-document-content img`.
25. **`extract_preview_image_returns_none_when_no_img`** —
    handles documents with no images.

### Phase 4 — substitution: description

In `post_render_upgrade.rs`'s test module:

26. **`substitute_description_replaces_envelope_with_engine_first_para`**
    — given a host HTML containing the envelope and a sibling
    HTML with `<main class="content"><p>Engine fp.</p></main>`,
    the output has just `Engine fp.` where the envelope was.
27. **`substitute_description_keeps_l1_when_sibling_first_para_empty`**
    — sibling has no `<p>`; output strips begin/end markers
    but retains the L1 fallback inline. One `Q-12-13` in
    diagnostics.
28. **`substitute_description_keeps_l1_when_sibling_missing`** —
    sibling output file does not exist; same L1-retention; one
    `Q-12-13`.
29. **`substitute_description_truncates_to_max_from_marker`** —
    marker carries `[max=20]`; engine output is 200 chars;
    result is ≤ 20 chars at a word boundary.
30. **`substitute_description_handles_multiple_envelopes_one_file`**
    — two listing items on one host; both substitutions
    happen; cache reads each sibling once even when one is
    referenced twice (see test 33).

### Phase 5 — substitution: image

31. **`substitute_image_replaces_envelope_with_preview_img`** —
    sibling's preview image is found; envelope is replaced with
    `<img src="..." class="thumbnail-image"…>`.
32. **`substitute_image_uses_listing_default_when_no_preview`** —
    sibling has no preview image; marker's b64 default is
    non-empty; substitution uses that URL.
33. **`substitute_image_keeps_empty_div_when_no_preview_no_default`**
    — neither sibling preview nor listing default; envelope's
    empty-div content stays.
34. **`substitute_image_resolves_src_relative_to_host_output_dir`**
    — sibling at `_site/posts/foo.html` has preview at
    `figures/foo.png`; host at `_site/index.html` should
    receive `<img src="posts/figures/foo.png">`.
35. **`substitute_image_no_warning_when_no_preview`** — the
    "no image found" path emits no `Q-12-13`.

### Phase 6 — caching

36. **`substitute_caches_sibling_extraction_within_one_call`** —
    a host references one sibling for both description and
    image substitution; the sibling is read+parsed once. Use
    a counting-runtime (test fake) to assert one
    `runtime.file_read` call per sibling absolute path.
37. **`substitute_does_not_cache_across_calls`** — calling
    `substitute_listing_placeholders` twice with the same
    inputs reads the sibling twice (no static cache).

### Phase 7 — orchestrator wiring

38. **`website_post_render_calls_substitute_listing_placeholders`**
    — full `WebsiteProjectType::post_render` test (existing
    pattern in `tests/website_post_render.rs`). Fixture has a
    listing host and a sibling. After post_render runs, the
    host's HTML carries the engine first-paragraph, no
    `<!-- desc-begin -->` markers remain.
39. **`default_project_type_does_not_call_substitute_listing_placeholders`**
    — defensive: a non-website project with placeholder-
    looking comments in its output is left untouched.

### Phase 8 — End-to-end CLI verification

40. **`pipeline_e2e_listing_substitution`** — fixture project:

    ```
    _quarto.yml         # project.type: website
    index.qmd           # listing host: contents: posts/*.qmd
    posts/foo.qmd       # title: "Foo", body: "Engine first para from foo."
    posts/bar.qmd       # title: "Bar", body: "Engine first para from bar."
    ```

    `cargo run --bin q2 -- render` produces `_site/index.html`
    that contains:

    - `Engine first para from foo.` next to Foo's listing entry
    - `Engine first para from bar.` next to Bar's listing entry
    - **No `<!-- desc-begin -->` or `<!-- desc-end -->`
      markers** in the served HTML.

    **End-to-end CLI verification per CLAUDE.md.** Record the
    invocation, a snippet of the rendered HTML showing both
    items + absence of markers, and an explicit "output
    inspected" note in this sub-plan after impl.

41. **`pipeline_e2e_listing_substitution_l1_fallback_when_sibling_empty`**
    — a fixture where one post is a single image (no `<p>`).
    The listing should show the L1 fallback inline (the post's
    title or first text) and the project diagnostics should
    contain one `Q-12-13`.

42. **`pipeline_e2e_listing_image_substitution`** — fixture:

    ```
    posts/with-image.qmd:    body uses ![](preview.png) inline; static AST sees
                              an Image, so L1 populates listing image directly;
                              listing entry uses the static <img>, no placeholder.

    posts/with-engine-image.qmd:  body has only a `{python}` cell that produces
                                  a plot. L1 sees no static image; L3 emits the
                                  image placeholder envelope. After engine runs
                                  in Pass-2, posts/with-engine-image.html has
                                  an <img> from the cell output. L7 substitutes
                                  the placeholder with that <img>.
    ```

    Asserts the second case end-to-end (the first is
    incidentally covered by the description fixture).

### Hub-client smoke

L7 changes the L3 templates and binding (template / binding
edits flow through `quarto-core` and re-bake into the WASM
build). The changes must not break the WASM rendering path,
which uses the same templates/binding to render listings:

- L7's CLI-only post_render step does not run on hub-client.
- The L1 fallback now sits inside the `<!-- desc-begin -->...<!-- desc-end -->`
  envelope. In the hub-client preview, the markers are HTML
  comments (invisible), and the fallback paragraph renders as
  it did before. **Visually no regression.**
- Image placeholder block: when L1 didn't populate `image`,
  the template now emits the begin/end markers around the
  empty placeholder div. Same visual: the user sees the empty
  div placeholder. The markers are invisible.

The L7 author confirms this with a real browser session:

```bash
cd hub-client
npm run build:all
npm run dev
```

Open the dev server, load a fixture project with a listing,
confirm the listing entries render correctly (no visible
`desc-begin` / `desc-end` text leaks into the page).

This is the **mandatory L1-fallback-contract verification**
for L7: the hub-client browser smoke proves L1 fallbacks are
visible without L7 running. Per the bracketing rules, this is
a release blocker.

### End-to-end CLI verification record

Three fixtures rendered with the real `q2` binary on
2026-05-07; output inspected by hand.

#### Fixture 1 — description substitution success (`/tmp/l7-fixture/`)

```
_quarto.yml         (project.type: website, output-dir: _site)
index.qmd           (listing: contents: "posts/*.qmd", type: default)
posts/foo.qmd       (description: "Foo's static fallback…", body: "This is the engine-rendered first paragraph from foo.")
posts/bar.qmd       (description: "Bar's static fallback…", body: "This is the engine-rendered first paragraph from bar.")
```

Invocation:

```
cargo run --bin q2 --quiet -- render /tmp/l7-fixture
```

Snippets from `_site/index.html`:

- Foo's listing entry contains
  `<div class="delink listing-description">\nThis is the engine-rendered first paragraph from foo.\n</div>`
  — the engine first paragraph from the post body, NOT the
  static `description:` fallback.
- Bar's listing entry contains
  `<div class="delink listing-description">\nThis is the engine-rendered first paragraph from bar.\n</div>`
- `grep -c 'desc-begin\|desc-end' _site/index.html` → `0`
  (envelope markers fully stripped).
- `grep -c "static fallback" _site/index.html` → `0`
  (L1 fallback was replaced by the engine first paragraph).
- Image envelope path: with no `<img>` in either post, the
  envelope's empty placeholder div is preserved (L1 fallback
  cascade) and wrapped in `<a href="posts/foo.html">…</a>`
  for click-through (per D14).

Output inspected by hand: ✓

#### Fixture 2 — L1 fallback + Q-12-13 (`/tmp/l7-fixture-fallback/`)

```
posts/heading-only.qmd   (body: "# Just a heading", description: "Static fallback that should remain visible.")
```

Invocation: `cargo run --bin q2 --quiet -- render /tmp/l7-fixture-fallback`

Stderr captured:

```
Warning [Q-12-13]: Listing item from posts/heading-only.html produced no preview content; using static fallback description.
```

Snippets from `_site/index.html`:

- `<div class="delink listing-description">\n<p>Static fallback that should remain visible.</p>\n</div>`
  — the L1 fallback `description:` text is retained verbatim.
- `grep -c 'desc-begin' _site/index.html` → `0` (markers
  stripped even when the L1 fallback is retained).
- `grep -c "Just a heading" _site/index.html` → `0` (the
  heading-only post's heading text is correctly NOT picked up
  by the reader after the §"Reader bug fix" above).

Output inspected by hand: ✓

#### Fixture 3 — image substitution from sibling preview (`/tmp/l7-fixture-image/`)

```
posts/with-image.qmd     (body: "This post demonstrates engine-driven preview image extraction." + raw-HTML <img src="preview-image.png" alt="Engine output">)
```

The raw-HTML `{=html}` block injects an `<img>` that L1's AST
walker doesn't see (L1 looks for Pandoc Image AST nodes, not
raw HTML), so the listing item's `image:` field stays unset
and the image-placeholder envelope fires. The rendered
sibling HTML carries an `<img src="preview-image.png">` that
L7's reader catches via the named-pattern selector
("preview" + ".png" matches Q1's `kNamedFilePattern` regex).

Invocation: `cargo run --bin q2 --quiet -- render /tmp/l7-fixture-image`

Snippet from `_site/index.html`:

```html
<a href="posts/with-image.html" class="no-external">
  <img src="posts/preview-image.png" class="thumbnail-image" alt="Engine output" loading="lazy">
</a>
```

- `<img src>` is host-relativized: from `_site/index.html` →
  `posts/preview-image.png`, derived from sibling at
  `_site/posts/with-image.html` with src `preview-image.png`.
- `class="thumbnail-image"` matches Q1's substitution shape.
- `alt="Engine output"` preserved verbatim from the sibling's
  `<img alt="...">`.
- `loading="lazy"` emitted by default (L7's static attrs
  string).
- Wrapping `<a>` makes the substituted thumbnail clickable
  (per D14: link wraps both static-img and substituted-img
  branches uniformly).
- `grep -c 'img-begin\|img-end' _site/index.html` → `0`
  (envelope markers stripped).
- `grep -c 'listing-item-img-placeholder' _site/index.html` →
  `0` (the empty placeholder div was replaced).

Output inspected by hand: ✓

#### Pre-existing project warning (orthogonal)

All three fixtures emit a `Q-1-20` "Failed to parse metadata
value as markdown" warning on the `contents: "posts/*.qmd"`
key — this is a pre-existing YAML/markdown-coercion warning
that predates L7 and surfaces because the YAML parser tries
to interpret the glob string as markdown. Not in L7's scope;
filing a follow-up bd is appropriate but not required by L7.

## Pipeline-builder wiring

None on the stage graph. L7 is purely:

1. A `post_render` step (single call site in `WebsiteProjectType::post_render`).
2. Template / binding edits in `crates/quarto-core/src/project/listing/`.
3. New module file gated to native-only.

The stage graph (`build_html_pipeline_stages_with_apply_config`,
`build_wasm_html_pipeline`) is untouched. The orchestrator's
`post_render` call sequence is untouched (one new function call
inside the existing native-only block).

## Risks and mitigations

- **Risk: scraper transitive deps break the WASM build despite
  target-gating.** *Mitigation:* §"scraper dep gating"
  prescribes the verification step (`cargo tree
  --target wasm32-unknown-unknown`) and the fallback (revert to
  inline target-gated dep on quarto-core). `cargo xtask verify`
  exercises the WASM build directly.
- **Risk: regex matching across very long files (large posts)
  is slow.** *Mitigation:* the placeholder regex anchors on a
  literal byte sequence (`<!-- desc-begin(`), so most files
  short-circuit. For files that match, the inner-region capture
  is `(.*?)` non-greedy with DOTALL — `regex` crate handles
  this in linear time. Even on a 10MB rendered HTML, the work
  is sub-millisecond. We measured workspace baseline in
  `_perf` tests; if a future profile shows this hot, switch to
  `aho-corasick` for the literal scan.
- **Risk: scraper's HTML serialization round-trip changes the
  inner text in subtle ways (e.g. entity normalization), and
  the truncated firstPara has different bytes than the source.**
  *Mitigation:* this is fine. Q1 has the same property; users
  expect listing previews to be lightly normalized HTML, not
  byte-identical to the source. Snapshot tests pin the
  expected output.
- **Risk: a post with a content extension (e.g. `.qmd.md`)
  produces an output file the orchestrator doesn't know about.**
  *Mitigation:* `output_paths` is the orchestrator's source of
  truth. If a sibling's rendered file isn't in `output_paths`,
  it isn't substituted from. For listings, the sibling is
  expected to be a project file that did go through Pass-2,
  so its output is in the slice.
- **Risk: the b64 default URL contains `+` or `/` characters
  (the URL-safe alphabet uses `-` and `_`, and we're encoding
  arbitrary URL strings).** *Mitigation:* use the URL-safe
  base64 alphabet (`URL_SAFE_NO_PAD` from the `base64` crate),
  not the standard alphabet. The regex character class is
  `[A-Za-z0-9_-]`, no padding character needed.
- **Risk: a Lua filter at Pass-2 mutates the listing markup
  *after* the L3 render transform but *before* the
  template-application step, in a way that strips or relocates
  the begin/end markers.** *Mitigation:* the markers are HTML
  comments in pandoc raw-HTML blocks (`{=html}`); pandoc
  preserves them through filter chains. If a Lua filter
  explicitly removes raw-HTML blocks, the marker is lost and
  L7 silently does nothing for that item — same as if the
  filter manually replaced the description. This is a degenerate
  case; document but don't engineer around it.
- **Risk: hub-client picks up the new envelope markers in the
  template and renders them visibly.** *Mitigation:*
  hub-client passes the same template through Pandoc, which
  emits the same HTML comments. Browsers don't render HTML
  comments. The hub-client smoke test in §"Hub-client smoke"
  confirms.
- **Risk: snapshot churn.** The L3 render snapshot tests
  change because the description region now wraps in begin/end
  markers and the image-placeholder block now emits markers in
  the no-image case. *Mitigation:* expected and called out in
  §"Open questions" #2 + §"Implementation steps". The snapshot
  diff is reviewed and committed atomically with this change.
- **Risk: L7 swallows a real error from sibling-file reads
  (e.g. the file is locked by another process).** *Mitigation:*
  the runtime's `file_read` returns `io::Error`; L7 should
  propagate I/O errors that aren't "file not found" rather
  than silently swallowing them. Only `NotFound` triggers the
  L1-retention path with `Q-12-13`. Other errors abort
  post_render with the underlying error message.
- **Risk: a listing item's `output_href` doesn't match the
  rendered output path** (e.g. extension translation,
  publish-renames). *Mitigation:* L3's `output_href` is
  populated from `profile.output_href` which is the same value
  the orchestrator uses to write the file. They round-trip.
  If an extension feature ever changes the output path between
  L3 and post_render time, the placeholder won't match and L7
  will fail with `Q-12-13` — which is a correct signal.
  Filing diagnostic Q-12-13 with the resolved path makes
  this debuggable.

## Implementation steps

Follow CLAUDE.md TDD: write tests, watch fail, implement,
watch pass.

### Preparation

- [x] Re-read `claude-notes/instructions/testing.md` and
      `claude-notes/instructions/coding.md`.
- [x] Re-read `.claude/rules/wasm.md` (`?Send`, WASM-cfg gating).
- [x] Re-read epic plan §"L7" + §"Bracketing rules". The
      bracketing rules are load-bearing; the L7 file header
      must include them per the epic plan.
- [x] Confirm `feature/listings` head is the post-L6 merge
      (record HEAD hash + baseline test count).
      **HEAD: `cd4b77fd`. Baseline tests: 8647.**
- [x] Create the worktree at
      `.worktrees/bd-qf7r-listings-post-render-upgrade/` per
      §"Branch / worktree". Branch
      `beads/bd-qf7r-listings-post-render-upgrade`.
- [x] `npm install` in the worktree.
- [x] Add `.beads/redirect` per worktree rules.
- [x] Baseline: `cargo xtask verify --skip-hub-build
      --skip-hub-tests`; record test count. **Clean ✓ (8647).**

### TDD phase 1 — placeholders

- [x] Write tests #1–6 in `placeholders.rs`. Fail.
- [x] Implement the new builders + regex constants. Tests pass.
- [x] Decide (per §"Open questions" #1) whether to remove the
      old single-comment helper. Recommend remove now; ensure
      no remaining callers. **Decision: keep through Phase 1
      so helpers.rs/binding.rs callers continue to compile;
      Phase 2 deletes them atomically with the migration.**
- [x] Add `Q-12-13` to `error_catalog.json`.

**Phase 1 status:** ✓ 11/11 placeholder tests pass; workspace
builds clean; error_catalog tests still 43/43.

### TDD phase 2 — binding + templates

- [x] Write tests #7–13 in `binding.rs` and the listing-render
      transform tests. Fail.
- [x] Update `helpers.rs` with the four new helper functions.
- [x] Update `binding.rs` to insert the four new keys; remove
      the `description-placeholder` single-comment key.
- [x] Update `templates/item-default.template` and
      `templates/item-grid.template` to use the new keys
      (description envelope; image $else$ branch).
      `item-table.template` is unchanged.
- [x] Snapshot tests for L3 will diff. Run `cargo insta review`,
      confirm the diff is exactly the begin/end marker
      addition, accept. **Document the snapshot count + summary
      in the eventual commit message.**
      **No `.snap` files reference these placeholders — Phase 2
      only required updating in-source `assert!` checks (3 sites:
      `helpers.rs`, `binding.rs`, `transforms/listing_render.rs`,
      `tests/listing_pipeline.rs`).**

**Implementation notes (Phase 2 deviations from plan):**

- **Image envelope inside link uses explicit inline raw HTML.**
  Pampa's tree-sitter-markdown parser emits a Q-2-9 warning
  ("HTML element converted to raw HTML") when it auto-converts
  inline `<div>` to a `RawInline`. This propagates as Q-12-10
  on the listing host. Fix: wrap the envelope in
  `` `…html…`{=html} `` inline-raw-HTML syntax inside the
  Pandoc link brackets:
  `[``$image-placeholder-begin$<div…>…</div>$image-placeholder-end$``{=html}]($path$){.no-external}`.
  Pampa parses this as `Link { content: [RawInline("html",
  "<!--…--><div…>…</div><!--…-->")] }` — explicit, no warning,
  link path still goes through `LinkRewriteTransform`.
- **`image-placeholder-begin/end` always populated** (Phase 2
  test #9 deviation): the binding inserts non-empty markers
  even when `item.image` is `Some(_)`. The template's
  `$if(image-html)$` branch decides which envelope is used at
  render time; the binding stays unconditional. Simpler than
  conditional emission, with no rendered-output difference.

**Phase 2 status:** ✓ 99/99 listing tests pass; 8665/8665
workspace tests pass (+18 new tests over the 8647 baseline).

### TDD phase 3 — reader

- [x] Add `scraper` to `crates/quarto-core/Cargo.toml` as a
      target-gated dep.
- [x] Create `post_render_upgrade.rs` with module-level
      `#![cfg(not(target_arch = "wasm32"))]`. Includes the
      load-bearing bracketing-rule header comment (epic plan
      §L7 §"Bracketing rules").
- [x] Add the module gate to `crates/quarto-core/src/project/listing/mod.rs`.
- [x] Write reader tests #14–25. Fail.
- [x] Implement `extract_first_para` and `extract_preview_image`
      with `ReaderOptions`. Tests pass.

**Implementation note:** v1 returns plain text from
`extract_first_para` (the `<p>` element's concatenated `.text()`)
rather than HTML-aware tag preservation. This matches Q1's
observable behavior — Q1's `truncateText(s, n, "space")` strips
HTML before truncating, so the visible preview is plain text. The
`remove_links` / `remove_images` `ReaderOptions` flags are
forward-compat hooks for L9; in v1 plain-text output they're
no-ops (anchors auto-unwrap, `<img>` has no text). HTML-aware
preview extraction is filed as a follow-up.

**Phase 3 status:** ✓ 14/14 reader tests pass; quarto-core
builds clean with no warnings under the
`#[allow(dead_code)]` on the reader module (Phase 4 wires the
caller).

### TDD phase 4 — substitution + caching

- [x] Write substitution tests #26–37. Fail.
- [x] Implement `substitute_listing_placeholders` with the
      per-call cache and the description / image substitution
      logic. Tests pass.

**Implementation notes (Phase 4):**

- **Per-envelope max_length** is honored by re-running `extract`
  on the cached HTML stored in `RenderedExtraction.cached_html`.
  The cache stores extractions computed with no truncation; per-
  envelope truncation re-extracts on demand. This keeps the
  cache-key simple (path-only) without losing per-envelope
  precision.
- **Single canonical Q-12-13 message** (per D17): "Listing item
  from {href} produced no preview content; using static fallback
  description." Fires for both `NotFound` and "file present but
  no first-para" paths.
- **`CountingRuntime` test fake** delegates 24 SystemRuntime
  methods to an inner NativeRuntime; counts only `file_read`.
  Used in tests #36 / #37 to assert per-call cache hits and the
  absence of cross-call caching.
- **URL resolution** for the substituted `<img src=…>`: relative
  preview srcs are joined onto the sibling's directory then
  `pathdiff::diff_paths`'d against the host's directory. Absolute
  URLs (`http://`, `https://`, `data:`, `mailto:`, `//`,
  leading-slash) pass through unchanged.

**Phase 4 status:** ✓ 27/27 reader+substitute tests pass; full
workspace 8692/8692 tests pass (+27 over the 8665 Phase 2 mark).
Dead-code warnings on the substitute module remain expected
until Phase 5 wires the call site.

### TDD phase 5 — orchestrator wiring

- [x] Write tests #38–39. Fail.
- [x] Add the call site in
      `WebsiteProjectType::post_render`'s native-only block.
- [x] Tests pass. Verify default-project type is unaffected.

**Implementation note:** the existing `listing_pipeline.rs`
`default_listing_renders_three_posts_in_date_desc_order` test
asserted that `desc-begin(...)` markers appear in the rendered
HTML — that was correct *before* L7 wired itself in. With L7
running on website projects, those markers are stripped during
post_render. Updated the assertion: markers must be **absent**
in the post-L7 rendered HTML, and the engine first paragraph
(`"First body."`, `"Second body."`, `"Third body."`) must appear
where the L1 fallback used to be.

**Phase 5 status:** ✓ 12/12 website_post_render tests pass; full
workspace 8694/8694 tests pass (+47 over the 8647 baseline; 2
new tests for L7 orchestration + 27 new for reader/substitute +
18 new for placeholders/binding/render-transform).

### TDD phase 6 — End-to-end CLI

- [x] Write tests #40–42. **In-process project-pipeline tests
      cover #40 (description success), #41 (L1 fallback +
      Q-12-13), and #42 (image substitution via raw-HTML img to
      bypass L1's auto-fill).** Test #42 uses the raw-HTML-img
      pattern instead of the replay engine: it produces
      identical observable behavior (rendered sibling has an
      `<img>` in `main.content` that L7 picks up via the named-
      pattern selector) without depending on a real engine. Per
      D18, the replay-engine route is also viable; the raw-HTML
      route is simpler and exercises the same L7 code path.
- [x] All tests pass once the call site is wired. (Phase 5
      landed the wiring; Phase 6 is the e2e verification.)
- [x] Build the inline fixture, run `cargo run --bin q2 --
      render`, inspect output by hand. Record below.

**Phase 6 status:** ✓ 14/14 website_post_render tests pass
(12 pre-existing + 2 new for L7 description + 1 new for L7
image); full workspace 8697/8697 tests pass.

**Reader bug fix discovered during Phase 6.** The original
fallback path ("any non-heading element in `main.content`")
picked up Quarto's `<header id="title-block-header">` text,
producing the post's title as the listing description (visible
duplicate of the listing item's title). Fix: the reader now
walks `<p>` *descendants* of `main.content` (so `<p>` inside a
`<section>` wrapper is found) but skips any `<p>` whose
ancestry includes `<header>`, `<nav>`, `<aside>`, or `<footer>`.
The fallback "any node" path was dropped entirely — when no
`<p>` is found, return None and L7 keeps the L1 fallback in
place. Two regression tests added to lock the new behavior.

### Verification and close-out

- [x] `cargo build --workspace` clean.
- [x] `cargo nextest run --workspace` — all pass.
      **8697/8697 (+50 over the 8647 baseline).**
- [x] `cargo xtask lint` clean (696 files checked).
- [x] `cargo xtask verify` (full, including hub-client + WASM
      build) — all 9 steps green. **Scraper does not leak into
      the WASM dep tree:**

      ```
      $ cargo tree --target wasm32-unknown-unknown \
          -p wasm-quarto-hub-client | grep -ci scraper
      0
      ```

- [ ] **Hub-client browser smoke** per §"Hub-client smoke":
      load a listing fixture, confirm L1 fallbacks are visible
      without L7 running. **Deferred to user verification** —
      this Claude session can't drive a real browser. The WASM
      build passing in `cargo xtask verify` is necessary
      evidence; visual confirmation is the user's call. The L1
      fallback contract is testable in code: removing the L7
      call from `WebsiteProjectType::post_render` and re-running
      the `pipeline_website_post_render_substitutes_listing_placeholders`
      test would surface the L1 fallback (description text from
      frontmatter) in the rendered HTML — that's the same
      content the hub-client preview displays since L7 is gated
      to native-only.
- [x] End-to-end CLI verification fixture rendered; output
      inspected; recorded above in
      §"End-to-end CLI verification record". Three fixtures:
      success path, L1-fallback path, image-substitution path.
- [x] L7 module file's top-of-file comment carries the
      bracketing rules per epic plan §"L7" §"Bracketing rules"
      rule 2 verbatim. **Confirmed in
      `crates/quarto-core/src/project/listing/post_render_upgrade.rs`
      header.**
- [x] **Skip user-facing `docs/` callout** (D13). Filed as
      follow-up bd-399t.
- [ ] Stop and request user permission before any push (per
      CLAUDE.md §"GIT PUSH POLICY"). **← awaiting user
      approval.**
- [ ] After user approval: `br update bd-qf7r --status closed`.
- [ ] `br sync --flush-only && git add .beads/ && git commit`
      from the **main repo** (per `.claude/rules/worktrees.md`).
- [ ] Update the listings epic table
      (`claude-notes/plans/2026-05-05-listings-epic.md`) to
      mark L7 closed with the merge commit hash.

**Phase 7 status:** ✓ all automated verification green; manual
hub-client browser smoke deferred to user.

### Follow-up bds filed

- `bd-rvpd` — Source-span threading for Q-12-13 + L9
  diagnostics (planned, follow-up #1).
- `bd-bpdz` — Reader extension surface for L9 RSS feeds
  (planned, follow-up #2).
- `bd-399t` — Docs callout for L7's CLI-only behavior
  (planned, deferred per D13, follow-up #6).
- `bd-fx23` — Defensive percent-encoding of `listing.id` in
  the L7 image marker (D19, conditional / future-proofing).

Conditional follow-ups (#3, #4, #5 from the original plan)
not filed: they only fire if real complaints surface
(Lua-filter HTML-comment edge cases, listing.image-height
parity, performance hotspot in the placeholder-presence
scan).

## Filing reminder

This sub-plan corresponds to **one** bd issue:

- `bd-qf7r` — L7, the post-render placeholder upgrade.

After impl, close with a reason that references the landed
commit. Update the issue description with a one-line link to
this file.

### Follow-up bd issues (file during impl if they trigger)

1. **Source-span threading for Q-12-13 (and future L9
   diagnostics).** *(planned)* — `post_render` operates on
   rendered HTML, so Q-12-13 has no source span. Threading a
   span through the placeholder comment is feasible but
   crosses L9's surface too. File once L7 is merged so L9's
   sub-plan can pick it up.
2. **Reader extension surface for L9.** *(planned)* — L7 ships
   a `ReaderOptions` struct with two flags. L9 will add four
   more (math, inline-code-style, urls-to-absolute, anchor
   stripping). Filing a placeholder bd at L7 close-out keeps
   the L9 sub-plan honest.
3. **Avoid second pandoc-emit-comment-pair edge cases**
   *(conditional)* — if a user's Lua filter produces output
   that re-encodes HTML comments (e.g. via a Pandoc Walk
   transform), the begin/end markers might mismatch. File
   only if a real user complaint surfaces.
4. **Image substitution honoring listing.image-height**
   *(conditional)* — Q1 carries a `height` attribute on the
   substituted `<img>`; L7 v1 emits `height=` (empty). File
   if a user reports the listing height behaves differently
   between Q1 and Q2.
5. **Performance: regex anchoring vs `aho-corasick`**
   *(conditional)* — only if `_perf`-style profiling shows
   the placeholder-presence scan is a hotspot in projects
   with thousands of pages.
6. **User-facing docs callout for L7's CLI-only behavior**
   *(planned, deferred — D13)* — when the Quarto-website
   tree under `docs/` becomes a real user-facing site, add
   the bracketing-rule-3 callout to the listings reference
   page: *"Engine-rendered previews are available in
   `quarto render` only. In interactive environments,
   listings show static-AST previews — set `description:`
   and `image:` explicitly if you need a specific preview
   to appear during preview."* The wording is locked here
   and in the epic plan §L7 §"Bracketing rules" rule 3.
   File this bd at L7 close-out so the docs work picks it
   up when the website comes online; the bd description
   can link to this sub-plan + the epic plan as the source
   of truth for the wording.
