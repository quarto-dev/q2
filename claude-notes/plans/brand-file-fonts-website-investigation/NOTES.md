# Investigation notes — bd-ve916wr8

**Date:** 2026-09-08, main @ `b7e7c96a` (investigation ran in the main checkout; no worktree created).

Fixtures (committed, outputs gitignored):

- `repro/` — website project, `brand: _brand.yml`, `theme: [brand]`, one
  `source: file` font at `assets/EBGaramond-VariableFont_wght.woff2`
  (placeholder bytes — the copy/URL defects don't need a real font),
  pages `index.qmd` and `posts/one.qmd`.
- `repro-single/` — the same brand + font, single document `doc.qmd`
  with `brand: _brand.yml` in front matter.

## Repro confirmation at HEAD

### Website

```
cargo run --bin q2 -- render claude-notes/plans/brand-file-fonts-website-investigation/repro
find repro/_site -name '*.woff2'          # → (empty)
```

Observed (inspected by hand):

- **Defect 1 (never copied):** no `.woff2` anywhere under `_site/`.
- **Defect 2 (wrong base):** the one theme file,
  `_site/site_libs/quarto/quarto-theme-15564923cb73fc48.css`, contains

  ```
  @font-face{font-family:"EB Garamond";src:url("assets/EBGaramond-VariableFont_wght.woff2");font-weight:400;font-style:normal}
  ```

  A browser resolves that against the CSS file's directory →
  `_site/site_libs/quarto/assets/…` — wrong even if the file had been
  copied to `_site/assets/`.

- **Defect 3 (per-document prefix)**, refined — see "cache aliasing" below.
  On a *cold* SASS cache each page compiles with
  `pathdiff(document_dir, brand_dir)` (`quarto-sass/src/themes.rs:781`),
  which is `""` for `index.qmd` and `..` for `posts/one.qmd`, so the two
  pages produce two different theme bundles (the strand's original
  observation). On a *warm* cache they alias instead (below).

### Single document

```
cargo run --bin q2 -- render claude-notes/plans/brand-file-fonts-website-investigation/repro-single/doc.qmd
```

- `doc.html` links `href="doc_files/styles.css"`; that file contains the
  same `src:url("assets/EBGaramond-VariableFont_wght.woff2")`, which
  resolves to `doc_files/assets/…` → **404**. The font file is not copied
  into `doc_files/` either.
- So the strand's "the document dir for single-doc `styles.css`" is
  inaccurate: single-doc theme CSS is *also* relocated (into
  `{stem}_files/`, `resource_resolver.rs:113-124`), and `source: file`
  fonts are broken for `q2 render doc.qmd` as well, not only websites.

### Cache aliasing (new finding, not in the strand)

`cache_key()` in `compile_theme_css.rs:197` hashes the brand YAML but
**not** the font URL prefix derived from `document_dir`. The SASS cache
is persistent across sessions (`cache_get_lru` on the runtime). So the
first document to compile a given brand pins the `@font-face` URL for
every later document — in this render and every future render — until
the brand YAML changes.

Demonstrated by bypassing the cache (temporarily `weight: 400 → 500`),
rendering the nested page alone, then the whole site:

```
q2 render repro/posts/one.qmd   # cold key → compiles with prefix ".."
q2 render repro                 # index.qmd hits the cache
```

Result: **one** theme bundle `quarto-theme-3a2db3789e5edced.css` linked
by both `index.html` and `posts/one.html`, with
`src:url("../assets/EBGaramond-VariableFont_wght.woff2")` — i.e. the
root page now carries the nested page's prefix. (Reverted the fixture to
`weight: 400` afterwards.) Whichever prefix wins, both are wrong because
the CSS lives in `site_libs/quarto/` — but the point is the key is
incomplete: two compiles with different correct outputs share one entry.

## Related code (spot-checked, still as described)

- `crates/quarto-sass/src/brand_layer.rs:553` `file_font_face_block` —
  emits `src: url('…')` with no `format()` hint; `join_url_path` (`:592`)
  is `PathBuf::push`, so a leading `/` discards the prefix (this is why
  the strand's `/assets/…` workaround yields a stable, site-root URL).
- `crates/quarto-sass/src/themes.rs:777-783` — prefix =
  `pathdiff_from_to(context.document_dir(), brand_dir)`.
- `crates/quarto-core/src/stage/stages/compile_theme_css.rs:482-522` —
  `document_dir = doc.path.parent()`; `:823-846` — artifact path is
  `styles.css` (single-doc, lands in `{stem}_files/`) or
  `quarto/quarto-theme-<fp>.css` (project, lands in `site_libs/`).
- `crates/quarto-brand/src/types.rs:593-604` — `BrandFontFileEntry` is
  `#[serde(untagged)]` with no `deny_unknown_fields` on the `Explicit`
  variant, so `format:`/`display:` on a file entry are silently dropped.
- Asset copy precedents: `project/website_post_render.rs:128`
  `copy_navbar_logo` (+ `copy_favicon`, `copy_footer_images`), all native-only
  post-render hooks keyed off `_quarto.yml`; resource channel
  `project_resources.rs:814` `resolve_reported_resources` with
  `ResourceOrigin::ProjectMetadata` (resolved against the project root).
- Q1: `external-sources/quarto-cli/src/core/sass/brand.ts:155-175` uses
  the same `relative(projectDir, brandDir)` prefix; its HTML path does not
  copy font files either (`command/render/pandoc.ts:1536` collects font
  dirs for **typst only**). Q1 got away with it because its theme CSS is
  inlined/adjacent to the page.

## Pre-flight

`cargo xtask verify --skip-hub-build` at `b7e7c96a`: Rust build + nextest
green; the hub-client `test:ci` leg failed with 23 tests all raising
`TypeError: Cannot read properties of undefined (reading 'clear')` on
`localStorage.clear()` — a local vitest/jsdom environment problem on a
clean `main` (Node v26.8.1), unrelated to this Rust-only strand.
