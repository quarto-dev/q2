---
title: 'Reveal render/preview theme parity'
date: 2026-06-16
---

**Strand:** bd-y259zb57
**Found:** 2026-06-16, during e2e testing on `main` (post-#297).

## Problem

Two render/preview divergences, **one root cause**: `q2 render` uses the
*compiled Quarto reveal theme*; `q2 preview` uses *stock vendored `white.css`*.
They diverge in opposite directions:

| | `q2 render` | `q2 preview` |
|---|---|---|
| Theme | compiled Quarto theme | stock `white.css` + `quarto-reveal.css` |
| Text align | left ✓ | center ✗ |
| Headings | none ✓ | uppercase ✗ |
| Font | (was) Helvetica ✗ → now SSP via @import ✓ | SSP (white.css embeds it) ✓ |

## Bug 1 — render fonts — DONE (commit 5b46b50e)

The compiled theme named `Source Sans Pro` but loaded nothing → Helvetica.
**Fixed**: the Quarto reveal layer (`resources/scss/revealjs/quarto-revealjs.scss`
`scss:defaults`) now `@import`s Source Sans Pro from Google Fonts (foundry
import, no bundling — user decision, matches Q1's `_brand.yml` font model).
Sass hoists the CSS `@import` to the top. Test:
`default_theme_imports_source_sans_pro_from_google`. E2E: Chrome confirmed the
SSP 400 face loads + applies on render.

## Bug 2 — preview centered + uppercase (Level 2: full per-document parity) — TODO

The q2-slides preview **never applies the Quarto reveal layer**, for *any*
theme/brand. So preview shows reveal's stock defaults (centered, uppercase) and,
once Level 2 lands, must instead compile and apply the document's actual reveal
theme (default / named / custom `.scss` / `_brand.yml`).

### Corrected architecture map (the 2026-06-16 Explore agent got two key things wrong)

- **WRONG (agent):** "the reveal branch in `CompileThemeCssStage` fires for
  q2-slides." **Reality:** that branch is gated on
  `ctx.format.identifier == FormatIdentifier::Revealjs`
  (`compile_theme_css.rs:267`); `q2-slides` → `("html", preview)`
  (`format.rs:125`), so `identifier == Html`. **The preview never compiles the
  reveal theme at all.** (The agent confused the `is_revealjs_target()` helper —
  true for `q2-slides` — with the identifier check.)
- **WRONG (agent):** "reveal registers `css:theme:<fp>` like HTML." **Reality:**
  `register_reveal_assets` uses `css:revealjs:*` keys (`assemble.rs:153`); the
  WASM `extract_theme_fingerprint` looks for `css:theme:` (`wasm-…/lib.rs:1463`).
  They do not match.

### HTML theme-delivery model to mirror (verified parts)

1. `CompileThemeCssStage` compiles the HTML theme → `store_css` registers
   `css:theme:<fingerprint>` (`compile_theme_css.rs`).
2. WASM `render_page_for_preview` (`wasm-quarto-hub-client/src/lib.rs:1188`) →
   `extract_theme_fingerprint` (1461) → `RenderResponse.theme_fingerprint`.
3. SPA reads VFS bytes at `DEFAULT_CSS_ARTIFACT_PATH`
   (`/.quarto/project-artifacts/styles.css`), mints a blob URL, posts
   `UPDATE_THEME` to the iframe (`Q2PreviewIframe.tsx:281`).
4. Iframe applies it.

### WASM reveal compile — AVAILABLE

`compile_reveal_theme_css` is cfg-split: native `grass`
(`quarto-sass/src/compile.rs:373`), WASM dart-sass via the JS bridge (`:575`,
async). So the preview can compile the reveal theme on WASM.

### Work (each TDD; verify through real `q2 preview` + Chrome)

- [x] **L2.1 (Rust).** DONE. Gate changed from `identifier == Revealjs` to
      `is_revealjs_target(&ctx.format.target_format)` in `CompileThemeCssStage`
      (`compile_theme_css.rs`), so the reveal branch fires for the `q2-slides`
      preview pseudo-format (identifier `Html`) as well as `revealjs` render.
      **Delivery split**: render (`revealjs`) still calls
      `register_reveal_assets` (linkable `css:revealjs:*` set via site_libs);
      preview (`q2-slides`) calls `store_css` so the compiled reveal theme rides
      the *existing* `css:theme:<fp>` → styles.css transport the SPA already
      consumes. Compile-failure fallback → `stock_reveal_theme_css()` (vendored
      white). Tests: `reveal_preview_delivers_compiled_theme_via_css_theme_artifact`
      (compile_theme_css.rs), `q2_preview_pipeline_compiles_reveal_theme_for_slides`
      (pipeline.rs, exercises the full preview pipeline). Render output unchanged
      (all 2372 quarto-core tests pass).
- [x] **L2.2 (WASM).** DONE — **no WASM code change required** for the single-doc
      path (the `q2 preview` target use case). Because L2.1 routes the reveal
      theme through `css:theme:<fp>`, `extract_theme_fingerprint` (looks for
      `css:theme:`) and `flush_artifacts_to_vfs` (writes styles.css) already
      surface it via `render_page_for_preview` → `RenderResponse.theme_fingerprint`.
      (Project-of-decks path uses the multi-doc `quarto/quarto-theme-<fp>.css`
      artifact path rather than styles.css — out of scope for the single-deck
      "done" criteria; tracked as a follow-up if needed.)
- [x] **L2.3 (hub-client).** DONE. `entry.tsx`: removed the slides theme-link
      suppression (`currentDocIsSlides`/`setDocIsSlides`) — `reconcileThemeLink`
      now always applies `lastThemeCssUrl` (for slides that URL is the compiled
      reveal theme, not Bootstrap). `RevealDeck.tsx`: dropped the static
      `theme/white.css` import (kept reset + reveal + quarto-reveal); the
      per-document theme arrives via the `UPDATE_THEME` → `<link data-q2-theme>`
      transport. quarto-reveal.css is theme-independent/additive, so the runtime
      theme link landing after it does not change the cascade.
      **hub-client change → two-commit changelog; WASM/SPA rebuild required.**
- [x] **L2.4.** DONE. Tests + E2E parity check across **default, named (`dark`),
      and `_brand.yml`** — all three match `q2 render`.
  - **Rust tests** (`cargo nextest`, all green; full workspace 10147 pass):
    - `compile_theme_css.rs`: `reveal_preview_delivers_compiled_theme_via_css_theme_artifact`.
    - `pipeline.rs`: `q2_preview_pipeline_compiles_reveal_theme_for_slides`,
      `…_compiles_named_reveal_theme_for_slides` (guards the metadata-flattening
      fix), `…_compiles_brand_reveal_theme_for_slides` (real tempdir `_brand.yml`).
    - `quarto-hub/discovery.rs`: `test_discover_brand_file`.
  - **Two extra fixes surfaced during E2E** (both were silent before because the
    default theme needs no `theme:`/brand lookup):
    1. **Metadata flattening** (`metadata_merge.rs`): `q2-slides` flattened
       `format.html.*`, burying `format.revealjs.{theme,brand,…}`. Now maps the
       reveal preview to the `revealjs` base format. Without this, *named* themes
       and brand silently fell back to the default theme in preview.
    2. **`_brand.yml` VFS sync** (`quarto-hub/discovery.rs`): the preview server
       never synced `_brand.yml` into the VFS (only `_quarto.yml`/`_metadata.yml`
       were recognized as config), so brand resolution died with "Path not found:
       /project/\_brand.yml" — for HTML brand decks too, not just reveal. Added
       `_brand.yml`/`_brand.yaml` to config-file discovery.
  - **E2E evidence** (`cargo run --bin q2 -- preview <deck>`, computed styles read
    from the live iframe via Chrome DevTools; compared to `q2 render`\'s compiled
    `theme-*.css`):
    - **default** (`.e2e-reveal/default.qmd`): heading `text-transform: none`,
      content `text-align: left`, `font-family: "Source Sans Pro"`, color `#222` —
      matches render. `white.css` no longer statically imported; theme arrives via
      `<link data-q2-theme>`.
    - **dark** (`theme: dark`): `--r-background-color: #191919`, viewport bg
      `rgb(25,25,25)`, `--r-main-color: #fff` — matches render `#191919`.
    - **brand** (`_brand.yml`, run as a project): `--r-background-color: #fdf6ff`,
      `--r-main-color: #2a1a3a`, `--r-heading-color: #6f42c1`, `--r-main-font:
      Georgia`; h1 computed `rgb(111,66,193)` — all match render exactly.
  - **Known limitations (out of scope / follow-ups):**
    - **Single-file `q2 preview deck.qmd` + sibling `_brand.yml`**: the `bd-tnm3k`
      single-file watcher deliberately does NOT pull in siblings, so brand only
      resolves when the deck lives in a *project* (`_quarto.yml` present). Same
      for HTML brand decks. Tracked separately.
    - **`[Q-1-20] "Failed to parse metadata value as markdown"`** on `brand:
      \_brand.yml` — pre-existing, appears identically in render *and* preview (so
      itself a parity success); unrelated to theming.

### Notes / gotchas

- Per-document theme means the SPA can no longer statically import one theme;
  it must inject the per-document compiled CSS at runtime (blob/style), like the
  HTML path does.
- `q2 preview` embeds the SPA bundle; after hub-client changes, rebuild WASM +
  SPA + re-embed (`hub-client npm run build:wasm`, `cargo xtask
  build-q2-preview-spa`, `cargo build --bin q2`) — see CLAUDE.md "Verifying Rust
  changes in q2 preview".
- Existing related strands: bd-v053sk3s (preview revealjs Tier-1 parity GA
  gate), bd-qn8yi1su (golden render↔preview parity test).
