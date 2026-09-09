# `source: file` brand fonts: never copied, `@font-face` URLs resolve against the theme CSS's directory (bd-ve916wr8)

**Date:** 2026-09-08
**Braid:** bd-ve916wr8
**Checkout:** main @ `b7e7c96a` (investigation ran in the main checkout)
**Status:** Design agreed with user 2026-09-08 (decisions below). Ready to implement on a topic branch.

## Overview

`source: file` fonts in `_brand.yml` never reach the output, and the
`@font-face` URL the theme CSS carries is document-relative while the CSS
itself is relocated (`site_libs/quarto/` for websites, `{stem}_files/` for
single documents). Every local brand font 404s. Evidence and committed
repro fixtures: `claude-notes/plans/brand-file-fonts-website-investigation/`
(NOTES.md has the exact invocations and observed output).

Fix: publish font files as **project-scope artifacts beside the theme CSS**
and emit a constant `fonts/<name>` URL. One mechanism serves website,
single-doc, and the WASM preview; the prefix no longer depends on the
document, which also removes the per-depth bundle split and the cache-key
aliasing.

## Decisions (agreed 2026-09-08)

1. **Fonts as artifacts.** Font bytes are stored as `ArtifactScope::Project`
   artifacts at `quarto/fonts/<basename>` (website → `site_libs/quarto/fonts/`)
   or `fonts/<basename>` (single-doc → `{stem}_files/fonts/`), i.e. always
   in a `fonts/` dir beside the theme CSS, so the `@font-face` URL is the
   constant `fonts/<basename>`. Not a source-mirrored copy.
2. **Name collisions are an error, never a silent overwrite.** Two distinct
   byte contents claiming the same `fonts/<basename>` (across brands,
   light/dark variants, or documents) fail the render with a diagnostic
   naming both source files. Identical bytes dedupe (existing
   `merge_into_project` semantics).
3. **Leading `/` in a font path means project root** (path contract), for
   locating the file. The emitted URL is the constant form regardless. The
   `/assets/… + resources:` workaround keeps working (the file is now also
   found via project root and published as an artifact; the explicit
   `resources:` copy becomes redundant but harmless).
4. **`format()` hint** derived from the extension (`woff2`, `woff`, `ttf` →
   `truetype`, `otf` → `opentype`); unknown extension → no hint.
5. **Unknown keys on a file entry (`format:`, `display:`) warn and are
   ignored**, not rejected — the brand.yml spec has not settled them.
6. **bd-5fseopxy** (weight ranges) edits the same emitter concurrently;
   expect a textual conflict in `file_font_face_block` and resolve at rebase.
7. **On merge**, comment on **bd-r1y48cx0** (`css:` files never copied)
   pointing at the artifact mechanism as the one to reuse.

Why not the resource channel for the copy: the user's answer to Q2 chose
the resource channel *if* a copy was needed; with fonts as artifacts there
is no copy step, and artifacts already flush through both the native sink
and the preview VFS. The render-manifest gap (flushed `site_libs` artifacts
are not listed in `.quarto/render-manifest.json`) is pre-existing for the
theme CSS and out of scope here.

## Phases

### Phase 0 — Tests (TDD: written first, verified failing)

- [x] `quarto-sass` unit (`brand_layer_test.rs`): `file_font_face_block`
      emits `src: url('fonts/<basename>') format('woff2')`; `/`-rooted and
      `../`-style source paths all collapse to `fonts/<basename>`;
      external URLs pass through unchanged, no `format()`.
- [x] `quarto-sass` unit: `font_path_prefix` no longer influences the
      URL — remove/replace the `"brand/regular.woff2"` assertion in
      `typography_layer_emits_file_font_face`.
- [x] `quarto-brand` unit: an explicit file entry with `format:` /
      `display:` parses (not an error) and exposes the unknown keys for
      the warning.
- [x] `quarto-core` integration, driving the real render path (project
      render / `render_document_to_file`, fixture promoted from
      `brand-file-fonts-website-investigation/repro`):
  - [x] website with pages at two depths → exactly one theme bundle in
        `site_libs/quarto/`, both pages link it, and the font exists at
        `site_libs/quarto/fonts/<basename>` — i.e. the `@font-face` URL
        resolves from the bundle's directory to a real file.
  - [x] single-doc → `{stem}_files/styles.css` + `{stem}_files/fonts/<basename>`.
  - [x] cache-aliasing regression: render the nested page alone, then the
        whole project (same runtime cache); the root page's bundle has the
        correct URL. (Passes structurally once the prefix is constant; keep
        the test so a future document-dependent input can't regress it.)
  - [x] missing font file → warning naming the brand file, family and path;
        render continues (matches `copy_navbar_logo` warn-and-continue).
  - [x] collision: two brands / two files with the same basename and
        different bytes → render error naming both sources.
  - [x] unknown key on a file entry → one warning, render continues.
  - [x] light/dark brands with distinct font files → both published, no
        conflict.

### Phase 1 — Constant font URL in the SCSS layer

- [x] `brand_layer.rs`: `file_font_face_block` emits `fonts/<basename>` +
      `format()`; drop `join_url_path` and the `font_path_prefix`
      parameter from `brand_to_layers` / `typography_layer` (update
      `config.rs:695` caller and its doc comment).
- [x] `themes.rs:777-783`: remove the `pathdiff(document_dir, brand_dir)`
      prefix; `brand_dir` stays on `ThemeContext` (Phase 2 reads it).
- [x] `compile_theme_css.rs` `cache_key()`: document in the comment why
      `document_dir` is deliberately absent (URL is constant) — and hash
      the font *basenames* so a renamed file invalidates the CSS.

### Phase 2 — Publish font artifacts

- [x] In `CompileThemeCssStage::run`, after brand resolution, walk each
      resolved brand's `typography.fonts` `source: file` entries (light and
      dark): resolve the path (leading `/` → `ctx.project.dir`, else
      `brand_dir.join(path)`; external URLs skipped), read bytes via
      `ctx.runtime`, store `Artifact::from_bytes(bytes, <mime by ext>)`
      with key `font:<basename>` and path `quarto/fonts/<basename>` /
      `fonts/<basename>` (mirror `theme_artifact_key_and_path`'s
      single-doc switch), scope `Project`.
- [x] Collision check before `ctx.artifacts.store`: same key already
      present with different bytes → structured error naming both source
      paths. Cross-document collisions surface via
      `ArtifactStore::merge_into_project`'s existing conflict — wrap that
      error so it names the font sources too (today it prints key + byte
      lengths only).
- [x] Missing file → warning diagnostic; render continues without the
      artifact (the CSS still references it, matching logo behavior).
- [x] Confirm nothing emits a `<link>` for `font:*` keys (link emission
      selects by `css:` / `js:` prefixes — verify) and that
      `flush_artifacts_to_vfs` carries binary content to the preview.
- [x] New `Q-14-*` catalog entries for the collision error and the
      missing-file / unknown-key warnings, each with its
      `docs/errors/theme/<code>.qmd` page **and** sidebar entry in the same
      commit (`cargo xtask lint`).

### Phase 3 — Unknown-key warning on file entries

- [x] `quarto-brand` `BrandFontFileEntry::Explicit`: capture unrecognised
      keys (`#[serde(flatten)]` map) so `format:`/`display:` survive parse.
- [x] Emit one warning per entry from the stage (has the diagnostics
      sink); ignore the keys.

### Phase 4 — Docs, contract, verification

- [x] `docs/guides/authoring/brand.qmd`: `source: file` paths resolve
      relative to the brand file (leading `/` = project root), files are
      published automatically, `format:`/`display:` are not yet supported.
      Hand the wording to bd-qnylgu69.
- [x] `claude-notes/designs/path-resolution-model.md`: add a row for
      brand-file font paths (brand-dir base, `/` = project root, emitted
      URL is artifact-relative).
- [x] End-to-end: `cargo run --bin q2 -- render` on both fixtures; inspect
      `site_libs/quarto/fonts/`, the `@font-face` line, and a browser
      load; record invocation + output snippet here. → see
      "End-to-end verification" below.
- [x] `cargo xtask verify` (full — `quarto-core` changes affect the WASM
      leg; hub-client vitest is currently red on `main` for an unrelated
      `localStorage` environment issue, see NOTES.md).
- [x] Rebase over bd-5fseopxy if it has landed; resolve the
      `file_font_face_block` conflict. Done 2026-09-09 onto `main` @
      `9d236060` (after #661, #663, #664–#667): kept #663's weight-range
      emitter (`font_weight_to_css` + `RangeOk::FontFace`, YAML-path
      error locations, `Brand::validate`) and #661's structured
      `sass_error_to_parse_error` mapping in the reveal branch; layered
      the constant `fonts/<name>` URL, the `BrandFontFileEntry`
      accessors, and the brand-file tracking on top. `validate.rs` now
      reads `entry.weight()`. Catalog carries `Q-14-6`..`Q-14-11`.

### On merge

- [ ] Comment on bd-r1y48cx0 pointing at the font-artifact mechanism as
      the one for `css:` files to reuse.
- [ ] Close bd-ve916wr8.

## Code coordination (2026-09-08)

`Q-14-6`/`Q-14-7` are claimed by PR #661 (bd-jsvetdea, theme-compile
hard errors) and `Q-14-8` by PR #663 (bd-5fseopxy, weight ranges), both
open. This work therefore uses **`Q-14-9`** (font file missing),
**`Q-14-10`** (same-name collision), **`Q-14-11`** (unsupported
file-entry key). Re-check the catalog at rebase time.

## Risks

- **Artifact-store size.** Variable fonts are ~100–500 KB each; a brand
  with several files puts a few MB through the artifact map per render.
  Images already take this route; measure once on a real brand.
- **Collision granularity.** Basename-keyed names mean two *different*
  brands (per-document `brand:` front matter) with `regular.woff2` each
  collide by design (decision 2). If that bites in practice, the escape is
  a content-hash suffix — not silent overwrite.
- **Preview transport.** Binary project-scope artifacts should flow to the
  hub-client VFS like images do; verify rather than assume before claiming
  preview support.
- **Concurrent edits** with bd-5fseopxy in `brand_layer.rs:553-584`.

## End-to-end verification (2026-09-08, branch `braid/bd-ve916wr8-brand-file-fonts`)

Output inspected by hand after each invocation; fixture outputs are
gitignored and were removed afterwards.

```
cargo run --bin q2 -- render claude-notes/plans/brand-file-fonts-website-investigation/repro
```

- `find _site -name '*.woff2'` → `_site/site_libs/quarto/fonts/EBGaramond-VariableFont_wght.woff2`
  (bytes identical to the source, `cmp` clean).
- `ls _site/site_libs/quarto/ | grep theme` → **one** bundle,
  `quarto-theme-80c9169f3165fd8b.css`; `index.html` and `posts/one.html`
  both link it.
- The bundle's rule:

  ```
  @font-face{font-family:"EB Garamond";src:url("fonts/EBGaramond-VariableFont_wght.woff2") format("woff2");font-weight:400;font-style:normal}
  ```

  `fonts/…` resolves from `site_libs/quarto/` to the published file.

```
cargo run --bin q2 -- render claude-notes/plans/brand-file-fonts-website-investigation/repro-single/doc.qmd
```

- `doc_files/fonts/EBGaramond-VariableFont_wght.woff2` published;
  `doc_files/styles.css` carries the same rule; `doc.html` links
  `doc_files/styles.css`.

Twelve integration tests in
`crates/quarto-core/tests/integration/brand_fonts.rs` drive the same
paths (plus reveal, light/dark, collisions, and the three diagnostics)
through `render_to_file` / `ProjectPipeline`.

