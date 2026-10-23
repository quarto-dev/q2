---
title: 'link-external-icon / -newwindow / -filter for HTML output (bd-t3n1nghb)'
date: 2026-10-22
description: 'The three link-external options are accepted into the merged config but nothing consumes them; this plan scopes a render-time AST transform plus a gated SCSS layer to match Quarto 1.'
status: done
braid:
  strand: bd-t3n1nghb
---

**Branch:** `braid/bd-t3n1nghb-link-external-icon-newwindow` (topic branch in the main checkout, based on `main` @ `03da98869`)

## Triage verdict

**Ready to design.** The gap is real and reproducible at HEAD (see the
repro below), the Quarto 1 behaviour is small and fully understood, and
q2 has a clear home for each piece (a finalization-phase AST transform
next to `ResponsiveImageTransform`, and a gated SCSS layer modelled on
`title_block_layer`). The user answered all design questions on
2026-10-22 (see § Decisions), and the work is implemented (see
§ Implementation).

## Issue context

Filed 2026-10-22 by Claude Fable 5.1 (P2 feature, label
`websites parity`) while assessing `posit-dev/product-doc-theme` on q2.
The `posit-docs` project type sets `link-external-icon: true` and
`link-external-newwindow: true`. q2 accepts both keys into the merged
config (`q2 get-config` prints them), but no code in `crates/*/src` or
`resources/` reads them, so those sites get plain links.

Acceptance criteria (from the strand): with `newwindow`, external body
links get `target="_blank" rel="noopener"`; with `icon: true`, they get
class `external` and a visible icon; a string icon value renders the
named Bootstrap icon; `link-external-filter` (default derived from
`site-url`) exempts matching links; internal links are left alone. The
work ships with a smoke-all fixture and a docs entry.

## Dependency graph

- **discovered-from**: bd-0i6fii0k (in progress): the product-doc-theme
  (posit-docs) exploratory assessment. This strand is finding F3 of that
  assessment. Sibling findings were filed as bd-74ktvzv7 (F1, logo
  doubling), bd-rfz4e3bw (F4, `q2 add`), bd-7u2xu4gp, bd-hfghrqvh,
  bd-3z5bubrf, bd-f5l0n6oa and bd-2pmjmjmj. None of them blocks this
  strand, and this strand blocks none of them. The assessment plan
  (`claude-notes/plans/2026-10-21-product-doc-theme-q2.md`) lives on
  `braid/bd-0i6fii0k-product-doc-theme-q2`, not on `main`.
- **blocks / related**: none.

Nothing else depends on this strand. Its urgency comes from the
posit-docs adoption path alone.

## What the code looks like today

### q2

- No consumer: `grep -rn link-external crates/*/src resources/` matches
  only an unrelated Typst octique icon. The only matches elsewhere are
  in the `ts-packages/quarto-api` config key list, which classifies
  these keys as render options.
- No schema entries either: the keys pass through as unvalidated
  metadata.
- No `a.external` rule exists in q2's SCSS (`resources/scss/`).
- Bootstrap Icons CSS is shipped **only for websites**
  (`crates/quarto-core/src/transforms/website_bootstrap_icons.rs`). The
  string-icon form (`<i class="bi-…">`) on a standalone document would
  therefore render nothing unless the icons are also shipped there.
- Natural home for the transform: the finalization phase in
  `crates/quarto-core/src/pipeline.rs` (around line 2699), next to
  `TableBootstrapClassTransform` and `ResponsiveImageTransform`. Both
  are late, idempotent, HTML-gated passes over body nodes.
- SCSS gating precedent: `ThemeConfig::title_block_layer`
  (`crates/quarto-sass/src/config.rs`) together with
  `load_title_block_layer()` (`crates/quarto-sass/src/bundle.rs`). A
  `link_external_layer: bool` flag would follow the same shape and go
  into the CSS cache key.
- `site-url` is already read by `project/website_config.rs`.

Repro: `link-external-options-investigation/repro.qmd` (one relative
link, one external link, one filter-matched link and one `mailto:`
link). Result at HEAD: `q2 get-config repro.qmd link-external-icon`
prints `true`, but every link renders as a bare `<a href="…">`. No
`external` class, no `target`/`rel`, no `a.external` CSS rule in the
compiled stylesheet, and no client-side script. The bug reproduces as
the strand describes it.

### Quarto 1 (reference behaviour)

From the quarto-cli checkout at `~/repos/github/quarto-dev/quarto-cli`:

- `src/format/html/format-html.ts:355-368` passes the three options to
  the template. When `link-external-filter` is unset and
  `website.site-url` is set, the filter defaults to `site-url` with `.`
  and `/` escaped.
- `src/resources/formats/html/templates/quarto-html-after-body.ejs:~220-260`
  holds the client-side logic, which runs on `DOMContentLoaded`:
  - A link counts as **internal** when `filterRegex` matches its
    resolved `href`, when it is `localhost`, or when it is `mailto:`.
    Without a filter, `filterRegex` is `/<window.location.host>/`. Links
    are tested after resolution, so relative links are always internal.
  - Candidate links are `a[href]` minus the navigation classes
    (`.nav-link`, `.navbar-brand`, `.toc-action`, `.sidebar-link`,
    `.sidebar-item-toggle`, `.pagination-link`, `.no-external`,
    `[aria-hidden]`, `.dropdown-item`, `.quarto-navigation-tool`,
    `.about-link`). The test covers every link in the DOM, including
    links from raw HTML, listings and the title block.
  - `newwindow` sets `target="_blank"`, and sets `rel="noopener"` only
    when the link has no `rel` already.
  - `icon: true` adds the class `external`. A string `icon: <name>`
    inserts `<i class="bi-<name> quarto-ext-icon">` as a **sibling
    after** the `<a>`, not inside it. The value is the bare icon name
    (for example `box-arrow-up-right`) without the `bi-` prefix. The
    strand text says `bi-<name>`, which is slightly off.
  - The filter string is used as a JS regex. If it contains `\\`, it is
    emitted as a regex literal; otherwise it goes through
    `new RegExp("…")`.
- `src/resources/formats/html/_quarto-rules-link-external.scss` is
  included only when `link-external-icon` is truthy
  (`format-html-shared.ts:281`). It holds `a.external:after` (an inline
  SVG box-arrow tinted with `$link-color`), a `none` override for
  `div.sourceCode code a.external:after`, and the `.quarto-ext-icon`
  sizing.
- Default: `formats-shared.ts:259-260` sets both booleans to `false`.
- **Quarto 1's schema rejects the string form.**
  `src/resources/schema/document-links.yml` declares
  `link-external-icon` as `schema: boolean` (tagged
  `[$html-doc, revealjs]`). Rendering the repro with
  `link-external-icon: box-arrow-up-right` fails with "Render failed due
  to invalid YAML". With `validate-yaml: false` the template's string
  branch works and emits `quarto-ext-icon` /
  `bi-box-arrow-up-right`. So the string form is an undocumented,
  validation-blocked feature in Quarto 1. The TypeScript type
  (`string | boolean`, `config/types.ts:513`) and the template support
  it; the schema and quarto-web docs do not.
- **Bootstrap Icons ship with every Bootstrap HTML document**, not only
  websites: `bootstrapFormatDependency()`
  (`src/format/html/format-html-bootstrap.ts:93-115`) lists
  `bootstrap-icons.css` and `.woff`. The rendered single file has them
  under `icon_files/libs/bootstrap/`.
- **The `\\` filter quirk has no semantic reason.** It was added in
  `af851f8d9` ("Don't allow invalid regex to result in JS error", fixes
  quarto-cli#4765, 2023-03-11). The filter string is spliced into
  generated JS source, and the branch only picks between a regex literal
  and `new RegExp("…")` to survive JS string escaping. Rust compiles the
  pattern directly, so the quirk does not carry over.
- **quarto-web docs** (`docs/output-formats/html-basics.qmd:347-375`)
  describe the filter only as "a regular expression", with the example
  `^(?:http:|https:)\/\/www\.quarto\.org\/custom`. They say nothing
  about the dialect and do not mention the string icon form. They also
  show manual `{.external target="_blank"}` on a single link. The
  example compiles under Rust's `regex` crate: since 1.8 the crate
  accepts superfluous escapes such as `\/`, and q2 uses 1.12.

## Decisions (user, 2026-10-22)

1. **Classification happens in a render-time AST transform**, not in the
   browser. A transform can later move around the pipeline and be
   replaced or augmented by user filters. q2 treats as external every
   absolute `http(s)` URL that does not match the filter, and as
   internal every relative link and every `localhost` and `mailto:`
   link. The filter defaults to the escaped `site-url`. This diverges
   from Quarto 1 in one accepted case: an absolute link to the page's
   own host on a site with no `site-url` counts as external. A custom
   filter can fix that case.
2. **Only body AST links.** Raw-HTML `<a>` tags, listing chrome and
   title-block metadata links are out of scope, and no follow-up strand
   is needed. Quarto 2 steers users away from raw HTML.
3. **Ship Bootstrap Icons for the string icon form on standalone
   documents too**, which is option (a). The investigation shows this is
   cheap: `WebsiteBootstrapIconsTransform` gates on
   `ProjectKind::Website` alone, and Project-scope artifacts already
   resolve on single-file renders (mermaid and the theme CSS use them).
   See open question A for how wide the new gate should be.
4. **All HTML-family formats** (html, revealjs, …), deliberately wider
   than strict Quarto 1 parity. Quarto 1's schema tags revealjs too. The
   **q2-preview shows adorned links**, so the transform is *not* added
   to `Q2_PREVIEW_TRANSFORM_EXCLUDED`.
5. **The filter uses the Rust `regex` dialect.** A pattern that fails to
   compile produces a diagnostic. The feature docs must say plainly that
   the dialect differs from JavaScript (no lookaround, no
   backreferences). Quarto 1's `\\` quirk is **not** replicated; it has
   no legitimate reason (see above). The default filter is built with
   `regex::escape(site_url)`.

6. **(Follow-up A) Bootstrap Icons gate: the narrow option.** Ship the
   icons for websites, or when `link-external-icon` is a string. Do not
   ship them for every Bootstrap HTML page.
7. **(Follow-up B) The `.external` styling is unconditional.** The
   layer is compiled for all HTML output, so a hand-written or
   filter-set `.external` always draws its icon. No `ThemeConfig` flag.

## Implementation (2026-10-22)

All phases are done on this branch. What landed, and where it departs
from the draft:

- **Transform.** `crates/quarto-core/src/transforms/link_external.rs`
  (`LinkExternalTransform`, name `link-external`). It runs in the
  finalization phase **after `llms-capture`**, not next to
  `responsive-image` as drafted, so the `llms.txt` markdown companion
  keeps clean links. It is classified B4, is listed in
  `PANDOC_TRANSFORM_EXCLUDED`, and is not in the preview exclusion list.
  It walks with `ast_walk::for_each_inline_list_mut` so the named icon
  can be spliced in as a sibling.
  - The named icon is an empty `Span` (`bi bi-<name> quarto-ext-icon`,
    `aria-hidden="true"`) rather than a raw `<i>`, so filters and the
    React preview see a real node. A leading `bi-` on the name is
    dropped.
  - Only `http(s)` and `//host` URLs can be external. An author's
    existing `target` and `rel` are kept; Quarto 1 overwrote `target`.
  - The default filter is `^` plus `regex::escape(site-url)`. It is
    anchored, where Quarto 1's was not.
- **Front-matter regex bug.** In document front matter q2 parsed
  `link-external-filter` as markdown, which consumed its backslash
  escapes: `\/\/www\.quarto\.org` came back as `//www.quarto.org`. The
  key is now listed in `ANNOTATIONS`
  (`crates/pampa/src/pandoc/meta_annotations.rs`) as `PlainString`,
  both at top level and as `format.*.link-external-filter`.
- **Diagnostic.** A new catalog code, `Q-13-14` "Invalid
  link-external-filter" (navigation subsystem, alongside the other
  link diagnostics), with a page at
  `docs/errors/navigation/Q-13-14.qmd` and an entry in the error
  sidebar. When the filter does not compile, the code falls back to the
  `site-url` default.
- **SCSS.** `resources/scss/html/templates/link-external.scss` is
  bundled unconditionally in all five HTML compile paths and in
  `assemble_reveal_scss`, and registered in `resources/vendored.toml`
  as `native-scss`. The SVG fill uses inline
  `rgb(#{red()}…)` because `colorToRGB()` is undefined on the HTML
  path. That gap is a pre-existing bug, filed as bd-na72nf6u.
- **Icons.** `WebsiteBootstrapIconsTransform` gates on
  `needs_bootstrap_icons`: the project is a website, or the icon
  option names an icon.
- **Schema (dropped).** q2 has no document schema for format options
  yet, so there was nothing to add. The string form is accepted by the
  transform.
- **Docs.** `docs/guides/formats/html/external-links.qmd`, listed in
  the Guides sidebar. It covers the Rust regex dialect, body-only
  scope, `.no-external`, manual `.external`, and the differences from
  Quarto 1.
- **Tests.**
  - 18 unit tests in `link_external.rs`.
  - A gate test in `website_bootstrap_icons.rs` and an `ANNOTATIONS`
    test.
  - Two quarto-sass compile tests (HTML and reveal).
  - Updated exact-name lists in `pipeline.rs`.
  - Smoke-all fixtures under `crates/quarto/tests/smoke-all/link-external/`
    (icon class, named icon, newwindow only, manual class, invalid
    filter, revealjs) and `link-external-website/`, which covers the
    `site-url` default.

## Follow-ups filed

- bd-na72nf6u: HTML theme CSS emits a literal `colorToRGB(...)` in SVG
  fills, so the code-tools icons get an invalid fill. This predates the
  branch.
- bd-oq6tbfr3: in the q2-preview, a named `link-external-icon` likely
  renders as an empty span, because the preview iframe loads only the
  theme CSS and not Bootstrap Icons. The boolean form is unaffected.

## Risks / tradeoffs

- Every HTML and reveal stylesheet grows by the roughly 1.6 KB layer.
  No existing CSS assertion broke.
- The divergence from Quarto 1 described in decision 1 is documented
  in the user guide.
