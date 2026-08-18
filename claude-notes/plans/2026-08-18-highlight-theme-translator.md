# Highlight styles: general `.theme` translator + full Q1 palette catalog

**Strand:** bd-hl-theme-translator-2mdgh4k6 (open, feature, P3 — field evidence
argues for higher; see below)
**Status:** design settled 2026-08-18 (all six questions answered — see
"Design decisions" below); ready for implementation planning/TDD
**Investigated:** 2026-08-18, on branch `main`
**Discovered-from:** bd-ld-b-highlight-style-jnb036fz (closed — light/dark
phase B, stage 1 of highlight-style support)
**Related:** bd-0pic6 (light/dark theme epic, open)
**Epic plan:** `claude-notes/plans/2026-08-14-light-dark-theme-epic.md` § D6

## Overview

Stage 1 (phase B) shipped a working `highlight-style:` reader with a
**three-palette catalog**: `default`, `a11y-light`, `a11y-dark`
(hand-translated from Q1's `.theme` JSON onto q2's tree-sitter `hl-*` class
vocabulary). Quarto 1 ships ~26 palette names, 8 of them **adaptive pairs**
(`a11y`, `arrow`, `atom-one`, `ayu`, `breeze`, `github`, `gruvbox`,
`monochrome`). Any ported project naming one of the other ~23 gets a Q-14-5
warning and the default palette.

This strand builds the **general translator** so the full catalog ships, plus
the two follow-up semantics items from stage 1:

1. a general Q1-`.theme`-JSON → `hl-*` translator (the catalog);
2. Q1's feedback of highlight-derived `$code-block-bg` / `$code-block-color` /
   `$btn-code-copy-color` / `$btn-code-copy-color-active` into the theme
   compile (`resolveTextHighlightingLayer`);
3. revisiting single-variant adaptive resolution for custom-SCSS themes
   (currently `BuiltInTheme::is_dark` approximation; Q1 greps the compiled
   CSS's darkness sentinel).

### Concrete driver: Posit Connect docs (352 pages)

The strand comment (2026-08-18) documents that this is the **only remaining
diagnostic** in the entire Connect-docs port render: their extension sets

```yaml
highlight-style:
  light: github
  dark: arrow
```

and q2 0.23.0 warns `Q-14-5` twice (summary: "704 warnings" = 2 × 352 pages)
and renders every code sample with the default palette in both variants.
Repro: `~/repos/github/cscheid/q2-connect-docs/llms-info/repros/highlight-style-palettes/`
(external evidence only — **the q2 implementation must not reference that
repo**; we vendor Q1 `.theme` files from `external-sources/quarto-cli` into
`resources/` per the external-sources policy).

Concrete acceptance bar for this strand: `light: github / dark: arrow`
renders warning-free with genuinely different light/dark token colors, via
generic catalog machinery (no Connect-specific anything).

## Current state (q2, at f7cf8322)

- **Config reader** — `crates/quarto-sass/src/config.rs`:
  - `parse_highlight_style` (~line 871): scalar + `{light, dark}` map forms;
    each slot goes through `resolve_adaptive_highlight(name, dark)`.
  - `ADAPTIVE_HIGHLIGHT_STYLES: &[&str] = &["a11y"]` (~line 832) — the only
    adaptive name so far. Growing this list to Q1's 8 makes the map form
    resolve `github` → `github-light` / `arrow` → `arrow-dark` per slot,
    matching Q1's `textHighlightThemePath` (try `<name>-<style>.theme`
    first, then `<name>.theme`).
  - `builtin_darkness` (~line 850): the item-(3) approximation for
    single-variant configs.
- **Catalog + layer loader** — `crates/quarto-sass/src/bundle.rs`:
  - `KNOWN_HIGHLIGHT_PALETTES = ["default", "a11y-light", "a11y-dark"]`
    (line 214); `load_highlight_layer(palette)` (line 240) composes
    structural `highlight.scss` + `highlight-<palette>.scss`, unknown →
    `default`.
- **Palette files** — `resources/scss/html/templates/highlight-{default,a11y-light,a11y-dark}.scss`.
  Each is `scss:defaults` (`$code-block-bg`/`$code-block-color` `!default`)
  + `scss:rules` (grouped `.hl-*` color rules). The a11y file headers carry
  the hand-derived Pandoc-token → capture-group mapping table — the seed of
  the general translator's table.
- **Warning** — `crates/quarto-core/src/stage/stages/compile_theme_css.rs`
  ~line 399: one Q-14-5 per distinct unknown name, "Available palettes"
  listed from `KNOWN_HIGHLIGHT_PALETTES`.
- **Class emission** — `crates/pampa/src/writers/html.rs`
  `capture_to_class` (line 735): tree-sitter capture name, dots → hyphens,
  `hl-` prefix (`function.builtin` → `hl-function-builtin`). Capture
  universe = union of `highlights.scm` captures across
  `crates/quarto-highlight/src/langs` grammars + user grammars.

## Q1 model (what "full complexity" means)

From `external-sources/quarto-cli/src`:

- **`.theme` files**: `src/resources/pandoc/highlight-styles/*.theme` — 38
  files ≈ 26 user-facing names (8 adaptive `-light`/`-dark` pairs + singles
  like `dracula`, `monokai`, `nord`, `zenburn`, `pygments`, `tango`,
  `espresso`, `kate`, `haddock`, `oblivion`, `printing`, `radical`,
  `solarized`, `vim-dark`, `breezedark`, `none`, plus plain `monochrome`
  alongside its pair, `ayu-mirage`). KDE-syntax-highlighting JSON:
  `text-styles` (≈30 Pandoc token names → `text-color`, `bold`, `italic`,
  `underline`, `background-color`), `editor-colors` (incl.
  `BackgroundColor`), optional `custom-styles` (per-language overrides — Q1
  **ignores** these in HTML output; skylighting's CSS emitter only reads
  `text-styles`).
- **Resolution** (`src/quarto-core/text-highlighting.ts`): adaptive-name
  list (the 8 above); `textHighlightThemePath` tries
  `<name>-{light|dark}.theme` then `<name>.theme` then a **user-supplied
  path relative to the input** (`highlight-style: custom.theme` is a
  supported Q1 feature); map form `{light, dark}` counts as adaptive.
- **Translation is runtime, not codegen**: `generateThemeCssVars` /
  `generateThemeCssClasses` (`src/command/render/pandoc-html.ts` ~line 380+)
  turn the JSON into CSS at render time via the skylighting abbreviation
  table `kAbbrevs` (`Keyword` → `.kw`, etc.).
- **Item (2) exactly** (`resolveTextHighlightingLayer`,
  `src/format/html/format-html-scss.ts` line 270): a defaults-band SCSS
  layer, unshifted ahead of user layers, containing:
  - `$code-block-bg` from `background-color` or
    `editor-colors.BackgroundColor`, and `$code-block-color` from
    `text-color` — **only when the theme is NOT adaptive** and the user
    didn't set `code-block-bg` in metadata;
  - `$btn-code-copy-color` from `text-styles.Comment.text-color` and
    `$btn-code-copy-color-active` from `text-styles.Function.text-color` —
    **always** (adaptive or not).
- **Item (3)**: for a single dark theme Q1 decides highlight darkness by a
  sentinel grepped from the compiled CSS, not by built-in-theme lookup.

### Why a translator, not a rename (confirmed by field evidence)

Q1 emits skylighting classes (`.kw`, `.st`, `.co`); q2 emits its own
strictly finer-grained tree-sitter vocabulary (`.hl-keyword`,
`.hl-function`, `.hl-variable`, `.hl-type`...). A Pandoc-token →
capture-class-set mapping table does real work: q2 distinguishes tokens Q1
cannot (e.g. function name / parameter / type in a Python signature). The
three hand-written palettes established the group structure; the translator
formalizes it once.

## Proposed shape (settled)

**A runtime translator in Rust**, mirroring Q1's own
architecture (Q1 translates `.theme` JSON at render time too):

- Vendor Q1's `.theme` files into `resources/` (e.g.
  `resources/pandoc/highlight-styles/`), per external-sources policy.
- New module (likely in `quarto-sass`, or a small `quarto-highlight-theme`
  helper): parse `.theme` JSON → emit an SCSS layer string (defaults band:
  `$code-block-bg`/`-color`/copy-button vars per the item-(2) semantics;
  rules band: grouped `.hl-*` rules from the mapping table, honoring
  `bold`/`italic`/`underline`, not just `text-color`).
- One canonical **mapping table** (Pandoc token → `.hl-*` selector list),
  seeded from the a11y file headers + `highlight-default.scss` groups.
- `load_highlight_layer` consults: `default` (q2's own SCSS) → vendored
  `.theme` catalog (translated on demand) → fallback + Q-14-5.
  `KNOWN_HIGHLIGHT_PALETTES` / the Q-14-5 "available" list derive from the
  vendored catalog instead of a hand-list.
- `ADAPTIVE_HIGHLIGHT_STYLES` grows to Q1's 8 names.
- The runtime path leaves a natural seam for **user-supplied `.theme`
  paths** later (same translator, file read instead of embedded resource) —
  that's the "robust enough for full complexity" part; implementation can
  be deferred.

Alternative considered: xtask codegen of checked-in `highlight-<name>.scss`
files. Reviewable diffs and zero runtime JSON parsing, but user `.theme`
support would then need the runtime translator anyway (two
implementations), and 20+ generated SCSS files churn the tree. Q1 itself is
runtime. (Question 1 below.)

## Draft phases

### Phase 0 — tests first (TDD)

- [x] Unit tests for the translator: given a small `.theme` JSON fixture,
      assert emitted SCSS contains expected `.hl-*` rules, defaults vars,
      bold/italic handling, and stable group ordering. (17 tests in
      `highlight_theme.rs`, written first and observed failing.)
- [x] Config tests: `light: github / dark: arrow` resolves to
      `github-light` / `arrow-dark` (`test_highlight_style_map_form_github_arrow_pair`,
      observed failing before growing the adaptive list); bare adaptive on a
      dark single-variant already covered by existing a11y tests; unknown
      name still warns Q-14-5 once (existing coverage).
- [x] Smoke-all fixture `highlighting/07-adaptive-pair-github-arrow.qmd`:
      light/dark pair + `{light: github, dark: arrow}`,
      `noErrorsOrWarnings`, both palettes' keyword colors present, default
      palette's solarized green absent.
- [x] Tests for the YAML `code-block-bg` / `code-block-color` keys: unit
      tests on `derive_doc_scss_layer` (written first, observed failing) +
      smoke fixture `highlighting/08-code-block-bg-override.qmd` proving the
      metadata value beats github-light's `!default` end-to-end.
- [x] Snapshot/existing-test audit: existing a11y integration tests
      (`theme_light_dark.rs`) and `compile.rs` palette tests pass unchanged
      through the translator (colors come from the same `.theme` sources).

### Phase 1 — vendored catalog + translator

- [x] 34 `.theme` files vendored to `resources/pandoc/highlight-styles/`
      (+ README with provenance quarto-cli `2e6695811` and update
      procedure); `build.rs` hashes them into `SCSS_RESOURCES_HASH` so
      palette edits bust the CSS cache.
- [x] `highlight_theme.rs`: serde model, canonical `CAPTURE_TOKENS` table
      with dotted-name fallback (`capture_token`), 66-capture cover set,
      bold/italic/underline + per-token `background-color`, color-value
      validation (`is_safe_css_value`), empty-`text-styles` palettes (e.g.
      `none`) translate to a true no-op layer.
- [x] `load_highlight_layer` translation path; `known_highlight_palettes()`
      (26 user-facing names, adaptive variants folded) replaces the
      `KNOWN_HIGHLIGHT_PALETTES` const; `is_known_highlight_palette`
      accepts catalog stems; `ADAPTIVE_HIGHLIGHT_STYLES` grown to Q1's 8;
      `highlight-a11y-{light,dark}.scss` deleted.
- [x] Q-14-5 hint lists the 26 names sorted + a sentence noting adaptive
      resolution and explicit `-light`/`-dark` forms.

### Phase 2 — item (2) semantics + override keys

- [x] `$btn-code-copy-color` (Comment) / `$btn-code-copy-color-active`
      (Function) defaults emitted by the translator.
- [x] `$code-block-bg` / `$code-block-color` `!default` emitted from every
      translated palette that defines canvas colors (top-level
      `background-color`/`text-color` first, then
      `editor-colors.BackgroundColor` / `Normal`).
- [x] YAML `code-block-bg` / `code-block-color` metadata keys in
      `derive_doc_scss_layer` (unconditional assignments, matching the
      `$sidebar-border` doc-vars pattern; `code-block-bg` also accepts
      booleans; unsafe values dropped).

### Phase 3 — verification + docs

- [x] End-to-end (2026-08-18): a scratchpad website project with
      `theme: {light: cosmo, dark: darkly}` +
      `highlight-style: {light: github, dark: arrow}` and a Python block,
      rendered via `cargo run --bin q2 -- render <dir>`. Output inspected:
      - render summary: `Rendered 1 of 1 files` with **zero warnings**
        (previously two Q-14-5);
      - light CSS (`quarto-theme-*.css`):
        `.hl-keyword,.hl-keyword-operator,…,.hl-tag{color:#d73a49}` and
        `div.sourceCode{background-color:#fff;…;color:#24292e}` —
        github-light's token colors and translated canvas;
      - dark CSS (`quarto-theme-dark-*.css`):
        `.hl-keyword,…{color:#ffa07a;font-weight:bold}` — arrow-dark's
        color **and** its `bold` flag;
      - `#6a737d` (github Comment → `$btn-code-copy-color`) present in the
        light CSS.
- [x] Full workspace verify: `cargo nextest run --workspace` green and
      full `cargo xtask verify` (14/14 steps, including the
      WASM/hub-client leg) passed 2026-08-18.
- [x] Docs: `docs/guides/formats/html/themes.qmd` (full catalog, adaptive
      list, `code-block-bg`/`code-block-color` override) and
      `docs/errors/theme/Q-14-5.qmd` (catalog list, custom-`.theme`-path
      not-yet-supported note). Both pages render clean under Q2 (the 4
      warnings in a single-page docs render are pre-existing llms.txt
      companion noise, reproduced on an untouched page).
- [x] Spot-check of translated palettes end-to-end (`q2 render`, output
      inspected): github-light `#d73a49`, arrow-dark `#ffa07a`+bold, a11y
      pair (via existing integration tests), dracula `#ff79c6`, nord
      `#81a1c1`+bold — each matching its `.theme` source exactly,
      warning-free, correct per-variant selection in light/dark slots.

### Deferred (design leaves room; not in this strand's concrete scope)

- User-supplied `.theme` file paths (`highlight-style: custom.theme`) —
  filed as bd-ag5n55ca.
- Item (3): compiled-CSS darkness sentinel for custom-SCSS single
  variants — filed as bd-o20jxpfc.
- `syntax-highlighting:` key (Q1's new name for `highlight-style`).
- Per-language `custom-styles` overrides (Q1 ignores them in HTML output
  too).

## Design decisions (settled with user, 2026-08-18)

1. **Translator architecture: runtime Rust translator** over vendored
   `.theme` JSON. One implementation, natural seam for user `.theme` files
   later, mirrors Q1's own runtime translation.
2. **Route everything through the translator**, including a11y — the three
   hand-written palette SCSS files are replaced by translated output (q2's
   own `default` stays hand-written SCSS). Pixel-stability of stage-1 output
   is explicitly NOT a constraint at this stage of Quarto 2.
3. **`$code-block-bg`/`$code-block-color`: palette wins.** Translated
   palettes emit both as `!default` (from `editor-colors.BackgroundColor` /
   `text-styles.Normal` or top-level `background-color`/`text-color`),
   adaptive or not — the palette's bg was designed to match its highlights.
   Escape hatches, verified against the layer machinery:
   - **Custom SCSS (works today):** user theme layers' defaults land above
     built-in layers' defaults in the merged band (`assemble_with_user_layers`
     ordering in `crates/quarto-sass/src/compile.rs`), so one
     `$code-block-bg: …;` line in a user theme file wins.
   - **YAML keys (added by this strand):** support Q1's `code-block-bg` /
     `code-block-color` metadata keys via the existing `doc_vars` seam
     (`derive_doc_scss_layer` in
     `crates/quarto-core/src/stage/stages/compile_theme_css.rs:72`, which
     already lands at the top of the defaults band and wins the `!default`
     race — currently carries only `$sidebar-border`). ~15 lines + tests.
     This also reproduces Q1's "user metadata suppresses injection" guard
     for free via `!default` semantics.
   Accepted caveat: ported sites diverge from Q1's look (Q1 skips bg/fg
   injection for adaptive/map-form styles — e.g. Connect docs get github's
   designed `#ffffff` where Q1 showed the theme-default gray); restoring Q1
   parity is one YAML/SCSS line.
4. **One canonical capture→token mapping table** for all translated
   palettes, with **dotted-name fallback** (`function.builtin` inherits
   `function`'s bucket unless specifically mapped, so new upstream grammar
   captures degrade gracefully). Rationale: `.theme` files carry nothing
   finer than Pandoc's ~30 token names, so a single table hits the quality
   ceiling (Q1 parity) by construction — there is no per-palette information
   to lose. The judgment lives in bucket assignment (which of the ~67 known
   `hl-*` classes counts as Function-like, etc.), which is
   palette-independent; the stage-1 hand translations already applied
   exactly such a table (documented in their file headers). Better-than-Q1
   refinements exploiting q2's finer captures remain possible later via
   hand-written per-palette overlay SCSS layered after the translated output
   (the layering mechanism already exists) — opt-in, deferred.
5. **Full Q1 catalog** in one pass (~26 names, 8 adaptive pairs).
6. **Copy-button colors in scope** (`$btn-code-copy-color` from Comment,
   `$btn-code-copy-color-active` from Function, per Q1's
   `resolveTextHighlightingLayer`); **item (3)** (compiled-CSS darkness
   sentinel) deferred to a follow-up strand.

## Verdict

**Design settled (2026-08-18); ready to implement.** The stage-1 machinery
is sound and well-seamed (`load_highlight_layer`,
`resolve_adaptive_highlight`, `KNOWN_HIGHLIGHT_PALETTES` are exactly the
touch points); the Q1 reference implementation is small and fully
understood; the concrete acceptance case (github/arrow pair) is pinned by
field evidence in the strand. All six design questions were answered by the
user on 2026-08-18 (see "Design decisions" above).

Priority note: filed P3, but the strand comment argues this is now the sole
diagnostic standing between the 352-page Connect docs port and a clean
render — consider P2.
