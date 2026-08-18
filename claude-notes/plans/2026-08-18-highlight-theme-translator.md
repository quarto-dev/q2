# Highlight styles: general `.theme` translator + full Q1 palette catalog

**Strand:** bd-hl-theme-translator-2mdgh4k6 (open, feature, P3 — field evidence
argues for higher; see below)
**Status:** plan skeleton — design questions pending user alignment
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

## Proposed shape (draft — pending design questions)

**Recommendation: a runtime translator in Rust**, mirroring Q1's own
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

- [ ] Unit tests for the translator: given a small `.theme` JSON fixture,
      assert emitted SCSS contains expected `.hl-*` rules, defaults vars,
      bold/italic handling, and stable group ordering.
- [ ] Config tests: `light: github / dark: arrow` resolves to
      `github-light` / `arrow-dark`; bare `github` on a dark single-variant
      resolves `github-dark`; unknown name still warns Q-14-5 once.
- [ ] Smoke-all test(s) under `crates/quarto/tests/smoke-all/highlighting/`:
      a light/dark theme pair + `{light: github, dark: arrow}` asserting the
      two variant stylesheets carry different `.hl-keyword` colors and no
      warnings (regression for the Connect shape, expressed generically).
- [ ] Snapshot/existing-test audit: a11y palettes' rendered CSS must not
      regress (or changes documented if we regenerate them via translator).

### Phase 1 — vendored catalog + translator

- [ ] Copy Q1 `.theme` files from
      `external-sources/quarto-cli/src/resources/pandoc/highlight-styles/`
      into `resources/` (+ README noting provenance/update procedure).
- [ ] Implement `.theme` JSON parse + SCSS emission with the canonical
      mapping table.
- [ ] Wire into `load_highlight_layer` + derive the known-palette list;
      grow `ADAPTIVE_HIGHLIGHT_STYLES`.
- [ ] Q-14-5 "available palettes" message: now ~26 names — decide message
      format (sorted, wrapped).

### Phase 2 — item (2) semantics

- [ ] Emit `$btn-code-copy-color` (Comment) / `$btn-code-copy-color-active`
      (Function) defaults from the selected palette.
- [ ] Settle adaptive-vs-not `$code-block-bg` injection semantics
      (question 3) and implement; reconcile with the stage-1 a11y files.

### Phase 3 — verification + docs

- [ ] End-to-end: `cargo run --bin q2 -- render` on a local fixture with the
      github/arrow pair; inspect both variant stylesheets (record snippet in
      plan per CLAUDE.md policy).
- [ ] Full workspace verify (`cargo xtask verify` — WASM leg affected via
      quarto-sass/quarto-core).
- [ ] `docs/guides/formats/html/themes.qmd` (and wherever highlight-style is
      documented): list the full catalog.
- [ ] Manual visual spot-check of a handful of translated palettes
      (dracula, github, nord) against Q1 renders.

### Deferred (design leaves room; not in this strand's concrete scope)

- User-supplied `.theme` file paths (`highlight-style: custom.theme`).
- Item (3): compiled-CSS darkness sentinel for custom-SCSS single variants.
- `syntax-highlighting:` key (Q1's new name for `highlight-style`).
- Per-language `custom-styles` overrides (Q1 ignores them in HTML output
  too).

## Design questions (need user input)

1. **Translator architecture**: runtime Rust translator over vendored
   `.theme` JSON (my recommendation — one implementation, natural seam for
   user `.theme` files, mirrors Q1) vs. xtask codegen of checked-in SCSS
   files (reviewable, zero runtime parsing)?
2. **Existing a11y palettes**: keep the three hand-written SCSS files as-is
   (default stays q2's own either way), or route a11y through the translator
   too so there's exactly one code path? Routing through the translator may
   change their emitted CSS slightly (the hand translation made judgment
   calls, e.g. `.hl-function-call`, `.hl-variable-parameter` refinements) —
   is pixel-stability of stage-1 output a constraint?
3. **`$code-block-bg` injection semantics**: Q1 injects bg/fg **only for
   non-adaptive** themes (adaptive/map-form → theme's own bg wins; that's
   what Q1 does on the Connect docs, whose theme.scss sets no
   code-block-bg). Stage 1's a11y files set bg unconditionally. Follow Q1
   (visual fidelity for ported sites) or keep stage-1's palette-always-sets-bg
   (arguably better contrast guarantees, e.g. a11y's #fefefe)?
4. **Mapping-table fidelity policy**: q2's finer-grained captures
   (`hl-function-call`, `hl-variable-parameter`, `hl-constructor`, ...) must
   derive colors from coarser Pandoc tokens. OK to fix one canonical
   derivation table for all palettes (my recommendation), accepting that
   translated palettes won't exploit q2's extra granularity? (Palette-specific
   refinements would stay possible via hand-written override files layered
   after the translated one.)
5. **Catalog scope**: full Q1 catalog in one pass (my recommendation — the
   translator makes each additional palette free, and it retires the whole
   class of Q-14-5 ports) vs. just the 8 adaptive pairs, vs. just
   github+arrow?
6. **Item (2)/(3) scoping**: include copy-button colors (cheap once the
   translator exists — my recommendation) and defer item (3) sentinel work
   to a follow-up strand?

## Verdict

**Ready to design.** The stage-1 machinery is sound and well-seamed
(`load_highlight_layer`, `resolve_adaptive_highlight`,
`KNOWN_HIGHLIGHT_PALETTES` are exactly the touch points); the Q1 reference
implementation is small and fully understood; the concrete acceptance case
(github/arrow pair) is pinned by field evidence in the strand. The open
questions are genuine design choices (architecture, fidelity semantics), not
missing information.

Priority note: filed P3, but the strand comment argues this is now the sole
diagnostic standing between the 352-page Connect docs port and a clean
render — consider P2.
