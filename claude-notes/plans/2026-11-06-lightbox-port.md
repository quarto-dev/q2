---
title: "Port lightbox (GLightbox image galleries) from Quarto 1"
---

**Braid:** bd-kcpkgftw
**Status:** approved 2026-11-06; implemented on `braid/bd-kcpkgftw-port-lightbox-glightbox-image`.

## Overview

Quarto 1 ships a built-in "lightbox" feature for HTML output. It wraps
images in `<a class="lightbox">` links, and the bundled
[GLightbox](https://biati-digital.github.io/glightbox/) library opens those
links as a zoomed overlay or gallery. Q2 has no lightbox support today. The
only trace is `transforms/book_cover_image.rs`, which already emits
`{.quarto-cover-image .nolightbox}` for the book cover (a Q1 #11520
regression guard). Nothing in Q2 consumes that class yet.

The goal is Q1 parity for the user-facing surface, implemented natively in
Rust as an AST transform, following the `anchor-sections` precedent.
Q1's Lua filter is already vendored at
`resources/pandoc-filters/filters/layout/lightbox.lua`, but it runs only on
the Pandoc-writer path, where it is a no-op because the format is not
`html:js`. It is a reference, not something to reuse.

## Quarto 1 behaviour (the spec)

Sources:

- `external-sources/quarto-cli/src/resources/filters/layout/lightbox.lua` (filter, 430 lines)
- `filters/modules/lightbox.lua` (`automatic(meta)`, `el_has_lightbox`)
- `filters/normalize/flags.lua` (`has_lightbox` gating)
- `schema/document-lightbox.yml`
- `formats/html/glightbox/{glightbox.min.js, glightbox.min.css, lightbox.css}`
- Docs: `external-sources/quarto-web/docs/output-formats/html-lightbox-figures.qmd`
- Tests: `quarto-cli/tests/docs/smoke-all/lightbox/*.qmd`, plus `2023/11/15/7516-b`,
  `2023/12/08/7619`, `2024/04/25/lightbox-no-caption`, `2024/12/12/11520`

### Document option `lightbox`

The option applies only to formats that are `html:js`: HTML, revealjs and
dashboard, but not epub.

| value | meaning |
|---|---|
| omitted | Only images with the `.lightbox` class get the treatment ("explicit" mode). |
| `true` / `auto` / `{match: auto}` | Automatic mode: every *figure or block image* is lightboxed unless it has `.nolightbox`. |
| `false` | Off. The filter is skipped entirely, even for `.lightbox` images. See note 1. |
| object | `match: auto`, `effect: fade\|zoom\|none` (default `zoom`), `desc-position: top\|bottom\|left\|right` (default `bottom`), `loop: bool` (code default `false`; schema/docs say `true`, see note 2), `css-class: <str>` (GLightbox `skin`). |

Note 1: in Q1, `lightbox: false` sets `flags.has_lightbox = false`, so the
whole filter is skipped. That is why `.lightbox` images are ignored when the
option is off.

Note 2: Q1's docs and schema say `loop` defaults to `true`, but the code
initialises `loop = false`, and GLightbox's own default is also `false`.
Behaviour wins: we keep `false` and fix the docs. *(Open question Q4.)*

The manuscript project type defaults `lightbox: auto`. Q2 has no manuscript
type yet, so this is out of scope; it is recorded here for when one lands.

### Which images are lightboxed

The filter runs top-down with these handlers:

- **`Link`:** stop descending. Images already inside a link, such as
  `[![](ferry.png)](https://…)`, are never lightboxed.
- **`Para` containing only one image** (a captionless block image, which
  Pandoc does not turn into a Figure): automatic.
- **`Figure`** (implicit figure with caption): automatic. The caption is
  `figEl.caption.long`. A `column-margin` class on the image is re-added to
  the figure (#8697).
- **`FloatRefTarget`** (crossref figure): automatic. The title is the full
  caption prefix plus caption ("Figure 1: …").
- **Sub-`FloatRefTarget`s:** gallery = parent id, so all subfigures of
  `#fig-plots` page together. The title is "Figure 1 (a): …".
- **Inline `Image`** anywhere else: only when it has an explicit `.lightbox`
  class.
- **Code cell `Div.cell` with a `lightbox` attribute** (a JSON-encoded chunk
  option forwarded by knitr's "unknown option" path). It rewrites each
  descendant image:
  - `false` or `{nolightbox: true}` adds `.nolightbox`;
  - `true` or an object (in non-auto mode) adds `.lightbox`;
  - `group` and the forwarded attributes are copied on. A list value is
    indexed by plot ordinal, with a warning when there are more plots than
    entries.

### Transformation per image

Image counter `n` (1-based, document order):

1. Remove `.lightbox` from the image.
2. Wrap it in `Link(href = img.src, title = caption-or-alt-text, class="lightbox", data-gallery=…)`.
   - Gallery is `group` attribute (moved off the image) → parent float id
     (for subfloats) → `quarto-lightbox-gallery-<n>`.
3. If the image has a `description="…markdown…"` attribute:
   - parse it as markdown;
   - emit `<div class="glightbox-desc lightbox-desc-<n>">…</div>`, collected
     into one `<div class="hidden" aria-hidden="true">` appended at the end
     of the body;
   - set `data-glightbox="description: .lightbox-desc-<n>;title:<title>;"` on
     the link.
4. Forward `title`, `desc-position`, `type`, `effect`, `zoomable`, `draggable`
   from the image to the link. Pandoc renders unknown link attributes as
   `data-*`, which is what GLightbox reads. (Q1 also renders a forwarded
   `description` as HTML, a path that is effectively unreachable after step
   3.)

### Page-level injection (only if ≥1 image was lightboxed)

- An HTML dependency `glightbox` with `glightbox.min.js`,
  `glightbox.min.css` and `lightbox.css` (theme-variable overrides for the
  slide and description colours).
- An after-body `<script>`:
  - `var lightboxQuarto = GLightbox({selector:".lightbox", closeEffect, openEffect, descPosition, loop[, skin]})`;
  - a chained `window.onload` (#9727, don't clobber other handlers) that
    registers:
    - `slide_before_load`: swaps in `data:` URI `src` for embedded images;
    - `slide_after_load`: calls `window.Quarto?.typesetMath(slideNode)`.
  - Option key order is stable (#11124), so output is deterministic.

### Interactions

- **Figure alignment** (`fig-align`) must keep working: the wrapper divs
  survive (7516-b, 7619).
- **Built-in extension:** Q1 drops `quarto-ext/lightbox` from a document's
  extensions (with a warning) because the feature is now built in
  (`kBuiltInExtNames`). Old documents with `filters: [lightbox]` must not
  double-wrap.
- **`html.lua` cell-output fixup** has to match `Para/Link/Image` as well
  as `Para/Image`. Any Q2 transform that pattern-matches cell-output images
  and runs after lightbox needs the same tolerance.

## Q2 situation (where it plugs in)

- **Transform slot:** Finalization phase in `build_transform_pipeline`
  (`crates/quarto-core/src/pipeline.rs:2311+`). Put it after
  `crossref-render`, so floats are already rendered to
  `div.quarto-float > figure` with captions, and before
  `resource-collector`. That is next to `anchor-sections` (`pipeline.rs:2779`).
  - **Ordering decision:** after `crossref-render`, the caption prefix
    ("Figure 1:") is already materialised, so the title can be read off
    the rendered `figcaption` content.
  - The alternative is running before `crossref-render` and walking
    `FloatRefTarget` custom nodes (`caption_long` slot), as Q1 does. That
    would let the preview (which never runs `crossref-render`) share the
    exact same logic. See open question Q2.
- **Precedent:** `transforms/anchor_sections.rs`.
  - Gates on `PipelineProfile` and `is_minimal_html`, and reads its option
    from `ast.meta`.
  - Render-only: stores a `js:` artifact (Project scope → `site_libs/`) and
    an after-body init script via `append_with_sentinel`.
  - The CSS precedent is `website_bootstrap_icons.rs` (a `css:` artifact).
  - `ApplyTemplateStage` turns `css:`/`js:` artifacts into tags
    (`stage/stages/apply_template.rs:173,390-420`).
- **Reading tri-state options:** follow
  `code_block_generate.rs:114-140` (`resolve_default_copy_mode`). A bare
  `auto` arrives as `PandocInlines`, so use `as_plain_text()`, not
  `as_str()` (bd-y89ihf0i). There is no option schema to extend in Q2.
- **Image walking:** `transforms/image_walk.rs`.
  - `for_each_image_with_context_mut` gives the enclosing float and the
    standalone-ness, but it can't replace an `Image` with a `Link`.
  - Wrapping needs an inline-list walker (`ast_walk.rs:388`
    `for_each_inline_list_mut`) or a dedicated recursive walk that tracks
    "inside Link", "inside float (id, parent id)", and "inside
    `Div.cell[lightbox]`".
- **HTML writer:** pampa `writers/html.rs:967/978` (Link/Image). Figure
  single-image detection already accepts `Link[Image]` (`:1458`), and
  `crossref_render.rs:527-570` already looks through `Link[Image]`.
  Wrapping should therefore not break figure rendering; this needs
  verification.
- **Subfloats / layout panels: not in Q2 yet.**
  - Subfloats are tracked by bd-plcqhfcn; Jupyter `fig-subcap` synthesis by
    bd-gbaykhth.
  - `layout-ncol` divs pass through as `<div data-layout-ncol>`, with no
    panel engine (see `claude-notes/designs/float-layout-class-taxonomy.md`).
  - So the "subfloats share a gallery titled 'Figure 1 (a)'" behaviour can't
    be implemented or tested now. The plan builds the hook (a gallery
    parameter from the parent float id) and defers the test until
    bd-plcqhfcn lands.
  - `group=` galleries inside a plain `layout` div work regardless, because
    they are attribute-driven.
- **Preview** (`q2 preview` / hub-client) runs the same transforms minus
  `Q2_PREVIEW_TRANSFORM_EXCLUDED`.
  - It excludes `crossref-render` and `apply-template`, so `js:` artifacts
    never become script tags.
  - Figures render through React (`ts-packages/preview-renderer/src/q2-preview/custom/FloatRefTarget.tsx`,
    `inlines/Link.tsx`, `inlines/Image.tsx`).
- **Pandoc formats:** add the transform name to `PANDOC_TRANSFORM_EXCLUDED`
  (`pipeline.rs:3022`).
- **Cell options:**
  - knitr forwards unknown chunk options as JSON-encoded attributes on
    the cell div (`engine/knitr/resources/rmd/hooks.R` ~`:466+`), as in Q1.
  - The Jupyter path needs checking. Also add `lightbox` to the
    presentation-only allowlist in `engine/jupyter/mod.rs:245-285`, so that
    changing it does not invalidate execution records.
- **Vendoring:**
  - Files go to `resources/js/glightbox/` (byte-identical to Q1's copy).
  - Add an `[[asset]]` in `resources/vendored.toml` (copy the AnchorJS
    entry at `:578-589`) and a section in `resources/js/README.md`.
  - Otherwise `cargo xtask lint` (`vendored-registry`) fails.
  - Check GLightbox's version string in the minified header, and its
    license (MIT).

## Plan

### Phase 0 — Assets

- [x] Copy `glightbox.min.js`, `glightbox.min.css` and `lightbox.css` from
      `external-sources/quarto-cli/src/resources/formats/html/glightbox/`
      into `resources/js/glightbox/`.
      *only the two GLightbox files; `lightbox.css` was ported into SCSS instead, see Implementation notes*
- [x] Register them in `resources/vendored.toml` and `resources/js/README.md`.
- [x] `lightbox.css` uses `rgba(0,0,0,.7)` for the `.goverlay` scrim. That
      is fine under the hub-client theme rule, which allows scrims.
- [x] Confirm `--quarto-body-color` / `--quarto-body-bg` exist in Q2's
      Bootstrap SCSS output. If not, map them to the Q2 equivalents.
      *they don't; ported to `--bs-body-*`.*

### Phase 1 — Option parsing (unit-tested)

- [x] `LightboxConfig { mode: Off | Explicit | Auto, effect, desc_position, loop_, css_class }`
      parsed from `ast.meta["lightbox"]`:
      - accept `true`, `false`, `"auto"` (as plain text) and the object form;
      - return a diagnostic (warning, falling back to the default) for
        invalid `effect` / `desc-position`. Q1 errors here, and its error
        call is itself buggy (`"…" + val` in Lua).
- [x] Tests for each form, including `format: html: lightbox: …` flattening.

### Phase 2 — `LightboxTransform` (`transforms/lightbox.rs`)

- [x] Gating:
      - Runs for `HtmlRender` (and see Q3 for revealjs/dashboard).
      - Skipped when `is_minimal_html` or mode is `Off`.
      - Explicit mode still walks the document, looking only for
        `.lightbox`.
- [x] Recursive block/inline walk with context:
      - `in_link: bool`
      - `float: Option<(id, caption_title)>`
      - `gallery_override: Option<String>` (for subfloats, later)
      - `auto_eligible: bool`: true for standalone `Para[Image]`, `Figure`
        and float content; false for inline images.
- [x] **Cell pre-pass:**
      - find `Div.cell` with a `lightbox` attribute;
      - decode it as JSON (or the bare string `true`/`false`);
      - push classes and attributes down to the descendant images, with
        per-plot list indexing and a warning on overflow;
      - remove the attribute.
- [x] **Per image:**
      - apply the Q1 transformation above (counter, gallery, title,
        description div collection, forwarded attributes, `.lightbox`
        removal);
      - replace `Image` with `Link[Image]` in its parent inline list.
- [x] **Title:** for a float, `"<prefix><delim><caption>"` stringified (see
      the Q2 ordering question); for a plain Figure, its caption; otherwise
      the image's alt text.
- [x] **Margin:** if an image carries `column-margin`, keep the class on
      the figure wrapper (#8697). Check whether Q2's figure rendering
      already hoists it; it may already be handled.
- [x] **`.nolightbox`:** respected in auto mode. Remove it from the output?
      Q1 leaves it on (the cover-image test `11520` asserts on it), so keep
      it.
- [x] **Body tail:** if any descriptions were collected, append
      `Div.hidden[aria-hidden=true]` with the `glightbox-desc` divs.
- [x] **If ≥1 image was wrapped (render profile only):**
      - store `js:glightbox`, `css:glightbox`, `css:glightbox-quarto`
        artifacts (Project scope, path `glightbox/…`);
      - `append_with_sentinel(after-body, …)` the init script with
        deterministic JSON option order (`selector, closeEffect, openEffect,
        descPosition, loop, skin`), keeping Q1's onload chaining and
        `data:`/`typesetMath` hooks verbatim.
- [x] Register in `pipeline.rs` (slot per Q2 below), add to
      `PANDOC_TRANSFORM_EXCLUDED`, and decide on preview exclusion (Q1
      below).

### Phase 3 — Interop and cleanup

- [x] Book cover: confirm the `.nolightbox` cover image is skipped
      (strengthen `book_multifile_html.rs:559` with a `lightbox: true`
      case).
- [x] Built-in extension supersession:
      - if a document resolves `quarto-ext/lightbox` (by filter name or
        extension id), drop it with a warning, like Q1's
        `kBuiltInExtNames`;
      - look at `extension/discover.rs` / `filter_resolve.rs`, where
        "lightbox" already appears in tests as a sample extension name. Make
        sure those tests still mean what they say.
- [ ] Jupyter: add `lightbox` to the presentation-only cell-option
      allowlist, and check that the attribute reaches the cell div.
      *deferred to bd-cm1mwyuq (Q2's Jupyter presenter does not forward cell options at all).*
- [x] Run the cell-output fixups (if Q2 has an equivalent of Q1's
      `html.lua` `.cell-output-display` matcher) after lightbox, and verify
      they tolerate `Link[Image]`.
      *no Q2 equivalent of Q1's `html.lua` cell-output matcher exists; nothing to adjust.*

### Phase 4 — Tests

- [x] Rust unit tests in `transforms/lightbox.rs` covering:
      - explicit, auto, off;
      - inline-image exclusion in auto mode;
      - link-wrapped image skip;
      - `.nolightbox`;
      - `group` galleries;
      - description collection plus `data-glightbox`;
      - forwarded attributes;
      - cell attribute decode (bool, object, per-plot lists);
      - determinism of counter-based ids.
- [x] `crates/quarto-core/tests/integration/lightbox_pipeline.rs` (register
      in `main.rs`, alphabetized), modelled on `anchor_sections_pipeline.rs`:
      - artifacts are shipped byte-identical;
      - the script is injected once;
      - nothing is injected when no image qualifies;
      - no artifacts are shipped for `lightbox: false`.
- [x] `crates/quarto/tests/smoke-all/lightbox/` fixtures ported from Q1 with
      Q2-adjusted selectors (`site_libs`/libs paths differ):
      - `pandoc-figure`, `example` (minus R cells, or with them if knitr is
        available in CI), `lightbox-off`, `options`, `8697`, `7619`,
        `7516-b` (non-R variant), `lightbox-no-caption`;
      - a `nolightbox` case.
      - Images come from Q1's `images/` (small JPGs). Check their sizes, or
        substitute tiny generated PNGs.
- [ ] Deferred until bd-plcqhfcn: the subfloat gallery test (`plot-layout`).
      Leave a strand note or `discovered-from` dependency.
      *filed as bd-lnb5x22l (blocked on bd-plcqhfcn).*

### Phase 5 — Preview and docs

- [x] Preview: see Q1 below. Minimum: the DOM-parity fixture passes, either
      because the transform runs in preview too, producing the same
      `<a class="lightbox">`, or because it is excluded and the parity
      fixture opts out.
- [x] Docs:
      - add `docs/guides/formats/html/lightbox.qmd`, modelled on
        `anchor-sections.qmd`: overview, enabling, disabling, per-image
        opt-out, galleries, options table (with the corrected `loop`
        default), per-image attributes, computational cells, and
        "Differences from Quarto 1";
      - link it from `docs/_quarto.yml` and from
        `docs/guides/authoring/figures.qmd`.
- [ ] Optionally dogfood with `lightbox: auto` on the docs site figures
      guide.
      *not done; the new docs page itself uses `lightbox: true`.*

### Verification

- [x] `cargo xtask verify` (includes lint/vendored-registry).
- [ ] Windows cross-clippy for `quarto-core`
      (`cargo clippy -p quarto-core --all-targets --target x86_64-pc-windows-msvc -- -D warnings`).
      *not possible from macOS: the C build scripts of `quarto-core` (tree-sitter, aws-lc-sys, wasmtime) fail to cross-compile. No platform APIs are used; flagged in the PR.*
- [x] Render the Q1 `example.qmd` with `q2 render` and click through in a
      browser:
      - zoom works;
      - galleries page;
      - description position;
      - theme colours (light and dark);
      - math in captions typesets.

## Decisions (decided 2026-11-06)

The user accepted every recommendation:

1. **Preview:** the AST rewrite runs in q2-preview too, for DOM parity;
   no GLightbox there. The preview `Link` swallows clicks on `a.lightbox`
   (otherwise the raw image href navigates the iframe away). A clickable
   preview overlay is a follow-up strand.
2. **Ordering:** right before `crossref-render`, walking `FloatRefTarget`
   nodes; the title is built from `plain_data.kind`/`order` + caption,
   the same way `prefix_caption` builds the figcaption.
3. **Formats:** `HtmlRender` + `HtmlPreview` only; revealjs and dashboards
   are follow-up strands.
4. **`loop`:** defaults to `false` (Q1's behaviour); docs corrected.
5. **Invalid values:** warning `Q-19-5` at the value's location, default
   used.
6. **GLightbox:** 3.1.0 vendored byte-identical.

## Implementation notes (deviations found while building)

- Q1's `lightbox.css` uses `--quarto-body-bg/-color`, which Q2 does not
  define. Instead of vendoring it, its rules are ported into
  `_bootstrap-rules.scss` with `--bs-body-bg/-color` (which follow dark
  mode). Only `glightbox.min.{js,css}` are vendored.
- Link attributes are emitted with the `data-` prefix in the AST
  (`data-gallery`, `data-desc-position`, …). Same HTML as Q1 (whose bare
  names get the prefix from Pandoc's writer), and the preview `Link`
  (which passes only `data-*`) renders them too.
- Q1's forwarding of per-image `desc-position`/`type`/… reads
  `imgEl[attrName]`, which is the Image *field*, not the attribute — so
  it appears to forward nil. The docs promise per-image `desc-position`,
  so Q2 forwards the attribute value (documented behaviour wins).
- A kv `title` on the image overrides the slide title (Q1's `title`
  forwarding read the image's target title).
- `quarto-ext/lightbox` in `filters` is dropped with warning `Q-16-13`
  (Q1 `kBuiltInExtNames`), via `ResolvedFilters::superseded`.
- `hephaestus-render` rewrites `.hep` sources after lightbox runs; it
  calls `sync_lightbox_hrefs` so the link follows the SVG.
- An image alone in a footnote paragraph is lightboxed in auto mode
  (Q1's `Para` handler visits footnotes too).
- Q2's Jupyter presenter does not forward unknown cell options to the
  cell `Div` (`presenter/cell.rs` emits a bare `::: {.cell}`), so the
  cell-level `lightbox:` option works with knitr only. Follow-up strand.

## Open questions (resolved — see Decisions)

- **Q1 — Preview behaviour.** Options:
  - (a) Run the AST rewrite in preview too, for DOM parity (anchor-sections
    style), with no click behaviour in preview.
  - (b) Exclude it from preview.
  - (c) Also make the preview clickable via a React overlay. That needs
    either GLightbox in the preview bundle or a light custom overlay. File
    it as a follow-up strand.

  Recommendation: (a) now, (c) as a follow-up strand. The catch with (a) is
  that the preview has no `crossref-render`, so float captions/titles must
  come from the `FloatRefTarget` node. That favours Q2's ordering option
  "before crossref-render".
- **Q2 — Ordering relative to `crossref-render`.** Before: walk
  `FloatRefTarget` nodes as Q1 does, and compute the title from the crossref
  index (prefix + number). This works identically in preview. After: read
  the rendered figcaption. That is simpler for render but breaks preview
  parity. Recommendation: before. This needs `crossref-resolve` numbering
  available, which happens earlier in the pipeline; to be confirmed.
- **Q3 — Formats beyond plain HTML.** Q1 enables lightbox for every
  `html:js` format, including revealjs and dashboards, and Q1's own
  dashboard docs use `lightbox: auto`. Do we include `RevealjsRender` and
  `DashboardRender` in this strand, or file follow-ups? Recommendation:
  HTML now; dashboard and revealjs as follow-up strands, since their
  dependency injection and figure structure differ and both are still
  evolving (bd-dashboard-format-k21hd4vy).
- **Q4 — `loop` default.** Keep Q1's actual behaviour (`false`) and document
  it correctly, or match Q1's docs (`true`)? Recommendation: behaviour
  (`false`).
- **Q5 — Invalid option values.** Q1 hard-errors on a bad `effect`.
  Recommendation: a Q2 diagnostic warning, falling back to the default.
  This needs a new error code in the catalog.
- **Q6 — Upgrade GLightbox?** Q1 pins 3.1.0. Recommendation: vendor
  byte-identical now (`policy = "track"`) and let the vendored-deps survey
  flag upgrades.

## Verification log (2026-11-06)

- `SMOKE_FILTER=lightbox cargo nextest run -p quarto --test integration smoke_all`:
  15 fixtures pass (14 ported/contract fixtures + `preview-parity.qmd`).
- `cargo nextest run -p quarto-core --lib transforms::lightbox filter_resolve`:
  lightbox unit tests and the `quarto-ext/lightbox` supersession tests pass.
- `cargo nextest run -p quarto-core --test integration lightbox_pipeline`: 4
  pass (site_libs assets byte-identical, init once, nothing shipped when
  unused, book cover escapes lightbox — Q1 #11520).
- `cd hub-client && SMOKE_FILTER=lightbox npm run test:wasm`: WASM smoke-all
  passes; DOM parity `lightbox/preview-parity.qmd` compared, 0 failed.
- Browser click-through: rendered `description.qmd` (plus a real
  photo) with `target/debug/q2 render`, served over HTTP, and in Chrome:
  `typeof GLightbox === "function"`; clicking an `elsewhere` image opened
  the overlay with 3 slides; the Chilmark slide showed title "Beach in
  Chilmark" and the rendered markdown description (`<em>credit:</em>` plus
  link) to the left of the image (per-image `desc-position=left` honoured);
  description background followed `--bs-body-bg`.
- `q2 render docs/guides/formats/html/lightbox.qmd`: page renders, example
  image wrapped, `../../../site_libs/glightbox/glightbox.min.js` linked.
