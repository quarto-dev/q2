# Interim support for `theme: {light: [...], dark: [...]}` — use light, warn on dark

**Strand:** bd-o76p01wb (P1, feature) — discovered-from bd-ad7i1pc6 (custom project types, PR #474)
**Full-support strand (out of scope here):** bd-0pic6 — "Support theme: {light, dark} dark-mode config (object form)"
**Branch:** `braid/bd-o76p01wb-light-dark-theme-map`, based on `origin/feature/bd-ad7i1pc6-custom-project-types` (PR #474). The eventual PR waits for #474 to merge, then retargets/rebases onto `main`.

## Overview

Q1 supports a map form for `format.html.theme` (and `highlight-style`):

```yaml
format:
  html:
    theme:
      light: [theme.scss]
      dark: [theme-dark.scss]
    highlight-style:
      light: github
      dark: arrow
```

Q2 rejects the theme map with **Q-14-1** ("theme must be a string or array of
strings"). The posit-dev/posit-docs extension ships exactly this form, so after
PR #474 resolves `project.type: posit-docs` on the Connect docs testbed
(`~/repos/github/cscheid/q2-connect-docs/docs-quarto-2`), all 351 files fail on
the theme map. With the map hand-flattened to a plain list, everything renders
— the map form is the single remaining testbed blocker.

**Scope decision (per Carlos):** full dual-theme support (compile both CSS
variants + toggle wiring) is a separate task — it stays on bd-0pic6. This
strand does the documented degradation: **accept the map, use only the `light`
half, emit a user-visible warning that `dark` is ignored.**

## Assessment (code reading, 2026-08-08)

### Where the error comes from

- `crates/quarto-sass/src/config.rs::extract_theme_specs` handles string and
  array; anything else → `SassError::InvalidThemeConfig` ("theme must be a
  string or array of strings"), carrying the value's `SourceInfo`.
- `ThemeConfig::from_config_value` (same file) is the only entry point that
  calls it; it also handles the `theme: none` sentinel and brand auto-inject.
- `crates/quarto-core/src/theme_diagnostic.rs::sass_error_to_parse_error`
  lifts `InvalidThemeConfig` into the structured **Q-14-1** ariadne diagnostic.

### Consumers of `ThemeConfig::from_config_value`

1. `crates/quarto-core/src/stage/stages/compile_theme_css.rs:368` — the main
   consumer. On `Err`, the render of that document hard-fails with Q-14-1.
2. `crates/quarto-core/src/stage/stages/bootstrap_js.rs:152` — on `Err` it
   logs a trace warning and **silently suppresses Bootstrap JS** (treats the
   parse failure as `suppress_bootstrap = true`). Today the map form both
   fails the render *and* (in any path that tolerates the failure) drops
   Bootstrap JS. Fixing the parse in quarto-sass repairs both consumers at
   once — a point in favor of fixing at the parsing layer rather than
   pre-normalizing metadata in one stage.

### Prior art already in the same file

`extract_brand_ref` (config.rs:415) already implements exactly this shape for
`brand:`: a map whose keys are only `light`/`dark` is treated as a pair, the
`light` half is used, the dark side is deferred ("TODO(brand light/dark): wire
dark variant once Q2 has a light/dark seam"). Caveat: its doc comment promises
a "soft warning" but **no warning is actually emitted** — quarto-sass has no
diagnostics channel. The theme interim should do better (see D2) and is the
occasion to define the pattern the brand path can later adopt.

### Path rebasing (PR #474) already handles the map form

`FRAGMENT_PATH_PATTERNS` in `crates/quarto-core/src/project/mod.rs:681` ends
patterns at `["format", "*", "theme"]` and rebases **every string leaf
underneath** — the doc comment explicitly says this is what makes
`theme: {light: […], dark: […]}` work from one table entry. So
`light: [theme.scss]` arrives at the theme stage already rebased to a
project-relative `ConfigValueKind::Path` (e.g.
`_extensions/posit-dev/posit-docs/theme.scss`). No rebasing work needed here;
the existing array-item text extraction (`config_value_as_text`) already
handles `Path` kinds (the flattened-list workaround renders today through the
same code).

### `highlight-style` needs no interim work

Q2 currently has **no reader for `highlight-style` at all** — code
highlighting is tree-sitter-based (`CodeHighlightStage`), and the only
`highlight_style` mention in the workspace is an inert default in
`crates/pampa/src/options.rs`. The map form therefore passes through silently
today (confirmed by the testbed: with only the theme flattened, all 351 files
render — highlight-style stayed in map form). Wiring `highlight-style` (both
scalar and map forms) into the highlighter is part of the full-support work;
noted on bd-0pic6 rather than handled here.

### Warning plumbing and noise control

- Stages emit user-visible warnings via `StageContext::add_diagnostic`
  (`crates/quarto-core/src/stage/context.rs:339`); per-document diagnostics on
  successful outputs are printed by `print_render_diagnostics_text`
  (`crates/quarto/src/commands/render.rs`) through
  `coalesce_by_source` — diagnostics sharing a source location collapse into
  **one** report with an "Affected files:" tail. Our warning's `SourceInfo`
  points at the theme map in the extension's `_extension.yml` (same location
  for every document), so 351 renders produce one printed warning, not 351.
- Warnings do not affect the exit code unless `--strict`
  (`should_exit_nonzero`: `counts.errors > 0 || (strict && counts.warnings > 0)`).
  So the testbed exits 0 by default with the degradation visible. Under
  `--strict` it would exit non-zero — that is the designed meaning of
  `--strict`, not a problem to engineer around.

### Error catalog

`Q-14-1` / `Q-14-2` live in `crates/quarto-error-catalog/error_catalog.json`
under subsystem `theme`. Next free code: **Q-14-3** for the new warning.

## Design

**D1 — parse the map form in quarto-sass, not in a quarto-core pre-pass.**
`ThemeConfig::from_config_value` gains a branch: when the theme value is a map
whose keys ⊆ {`light`, `dark`} (at least one present), it is a light/dark
pair. Rationale: single source of truth repairs both consumers
(compile_theme_css + bootstrap_js), covers project config *and* document
frontmatter overrides, and sits next to the brand precedent. Any map with
other keys keeps the Q-14-1 error (message updated to mention the accepted
`light:`/`dark:` form).

**D2 — carry the degradation as data; emit the warning in the stage.**
quarto-sass stays diagnostics-free: `ThemeConfig` gains a field, e.g.
`pub dark_theme_ignored: Option<SourceInfo>` (location of the `dark:` entry,
falling back to the map). `CompileThemeCssStage` converts it into a
**Q-14-3 warning** via `ctx.add_diagnostic` after a successful parse.
`BootstrapJsStage` deliberately does not emit (it would double-report every
document); it only reads `suppress_bootstrap` as before.

**D3 — semantics of the pair (interim):**
- `light` present → its value goes through the existing string/array logic
  (`light: cosmo` and `light: [cosmo, custom.scss]` both work, matching Q1's
  accepted shapes). `dark` recorded for the warning, otherwise ignored.
- `dark` only → default Bootstrap themes (empty spec list) + the same Q-14-3
  warning. (Mirrors the brand precedent's "only dark → none", but with the
  warning the brand path lacks.)
- Nested maps inside `light:` (e.g. `light: {light: …}`) → Q-14-1, as today.
  Implementation guard: the pair branch runs only at the top level; the
  recursive extraction for the light half rejects maps.
- `theme: {light: none}` → the `none` sentinel is honored for the light half
  (suppress Bootstrap), same as `theme: none`. Implementation: factor the
  existing `none`/string/array handling into a helper both the top level and
  the light half call.
- Empty map `theme: {}` → Q-14-1 (no light, no dark ⇒ not a pair).

**D4 — Q-14-3 catalog entry.** Subsystem `theme`, title
"Dark theme variant not yet supported", message: the `dark:` half of a
`theme: light:/dark:` map is ignored in this release; the document is styled
with the `light:` themes only. The user-facing message must **not** reference
braid strands (they are not publicly readable); internal tracking of the full
feature stays on bd-0pic6, mentioned only here and in code comments.

**D6 — warning granularity (decided 2026-08-08).** Warn only when `dark:` is
actually present. `theme: {light: [...]}` alone is a fully-honored (if
redundant) spelling and is accepted silently — with explicit test coverage
for both the scalar and list forms of a light-only map.

**D5 — out of scope.** RevealJS light/dark (bd-904h9kmt), the brand
light/dark seam (existing TODO in `extract_brand_ref` — can adopt the same
`SourceInfo`-carrying pattern later), `highlight-style` wiring, and actual
dual-CSS compilation + toggle (bd-0pic6). No new strands needed; everything
is already filed.

## Work items

### Phase 1 — tests first (TDD) ✅ (red state confirmed 2026-08-08)

- [x] quarto-sass unit tests (`crates/quarto-sass/src/config.rs` tests
      module, next to the existing "theme set to a map" rejection test):
  - [x] `{light: [a.scss, cosmo], dark: [b.scss]}` → specs = light list,
        `dark_theme_ignored` is `Some` pointing at the `dark` entry
  - [x] `{light: cosmo}` scalar form → single spec, no warning field
        (per D6: no `dark:` present ⇒ nothing ignored ⇒ no warning)
  - [x] `{light: [a.scss, cosmo]}` list form, no `dark:` → specs = light
        list, no warning field (per D6)
  - [x] `{dark: darkly}` only → empty specs (default Bootstrap),
        `dark_theme_ignored` is `Some`
  - [x] `{light: cosmo, dark: darkly, contrast: high}` → Q-14-1 (unchanged)
  - [x] `{}` → Q-14-1 error (already covered by
        `test_from_config_value_invalid_type` /
        `test_invalid_theme_map_carries_location`, which stay untouched)
  - [x] `{light: none}` → `suppress_bootstrap = true`
  - [x] map form + `brand:` key → brand auto-inject still appends to the
        light specs
  - [x] (extra) `{light: {light: …}}` nested map → Q-14-1; unknown light
        theme → Q-14-2; light half as PandocInlines (frontmatter path)
- [x] quarto-core stage tests:
  - [x] `compile_theme_css`: map-form metadata → stage succeeds, exactly one
        Q-14-3 warning in `ctx.diagnostics`, compiled CSS reflects the light
        theme
  - [x] `bootstrap_js`: map-form metadata no longer suppresses Bootstrap JS
- [x] catalog test: Q-14-3 registered under subsystem `theme`
      (extended `theme_diagnostic_code_is_registered_in_catalog`)
- [x] integration test through the real render path
      (`crates/quarto-core/tests/integration/theme_light_dark.rs`, 3 tests:
      project-config map, frontmatter map, light-only map): render succeeds,
      CSS contains the light marker rule and not the dark one, exactly one
      Q-14-3 warning (none for light-only)
- [x] run new tests, verify they fail for the expected reason —
      quarto-sass: 8 failed (Q-14-1 on map form) / 3 passed (error-shape
      tests that stay valid); quarto-core: 6 failed (Q-14-1 render failures,
      suppressed JS, Q-14-3 not in catalog). Scaffolding note: the
      `dark_theme_ignored` field was added as an inert stub (always `None`)
      so the red tests compile; all behavior is Phase 2.

### Phase 2 — implementation ✅ (2026-08-08)

- [x] `ThemeConfig`: `dark_theme_ignored: Option<SourceInfo>`; pair branch
      (`light_dark_pair` + `LightDarkPair`) and the shared
      `Self::from_theme_value` helper (null / `none` sentinel / string /
      array) used by both the top level and the light half
- [x] Q-14-1 fallback message now reads "theme must be a string or array of
      strings, or a map with only `light:`/`dark:` keys"
- [x] Q-14-3 added to `crates/quarto-error-catalog/error_catalog.json`
      (no braid-strand references in the user-facing text, per review)
- [x] `CompileThemeCssStage`: emits Q-14-3 warning via `ctx.add_diagnostic`,
      located at the ignored `dark:` key, with a remove-or-keep hint;
      emitted after parse and before the `suppress_bootstrap` early return
      so `{light: none, dark: …}` still warns
- [x] All Phase-1 tests green; full workspace suite: 11,125 passed

### Phase 3 — verification

- [x] `cargo nextest run --workspace` — 11,125 passed, 0 failed
- [x] End-to-end minimal fixture (2026-08-08): project in scratchpad with
      `format.html.theme: {light: [cosmo, light-marker.scss], dark: [darkly,
      dark-marker.scss]}`. Invocation: `cargo run --bin q2 -- render
      <fixture-dir>`. Observed: exit 0; stderr shows
      `Warning: [Q-14-3] Dark theme variant not yet supported` with an
      ariadne snippet pointing at `_quarto.yml:7:7` (the `dark:` key) and
      the remove-or-keep hint; compiled `quarto-theme-*.css` contains
      `.q-light-marker{color:#123456}` and zero occurrences of
      `q-dark-marker`. Output inspected directly.
- [x] Testbed (2026-08-08): rendered
      `~/repos/github/cscheid/q2-connect-docs/docs-quarto-2` with the
      posit-docs extension's theme map **unflattened** via
      `target/debug/q2 render <testbed>`. Result: **`Rendered 351 of 351
      files`, exit 0**; Q-14-3 printed **exactly once**, coalesced with
      "Affected files: … (and 348 others)". Compiled site theme CSS contains
      light-half markers (`Open Sans`, posit orange `ee6331`) and **not**
      the dark-only body background `#181c25`. The `highlight-style`
      light/dark map passed through inert as assessed (no reader in Q2).
      Caveat A: the run required temporarily stripping quarto-openapi's
      contributed `pre-render` (its Deno-style `.ts` fails under Node —
      the separate bd-wch2dotq gap; file restored via git afterwards).
      Caveat B: the Q-14-3 warning renders span-less on the testbed — its
      location points into the extension's `_extension.yml`, which per-page
      SourceContexts don't register (`attach_config_source` only repairs
      `_quarto.yml`-anchored FileIds). Message + affected-files list are
      still clear; snippet support for extension-fragment locations noted
      as a possible follow-up.
- [x] `cargo xtask verify` (full, including hub-client/WASM leg) — all 14
      steps passed (2026-08-08)

### Phase 4 — bookkeeping

- [x] `braid dep add bd-o76p01wb bd-0pic6 --type related` (interim ↔ full)
- [x] Comment on bd-0pic6 (c-9gw5c02b): interim degradation landed; full
      work = dual-CSS compilation + toggle + `highlight-style` map + brand
      light/dark seam; Q-14-3 warning to be removed/replaced when it lands
- [x] Work committed as 29ed786d on
      `braid/bd-o76p01wb-light-dark-theme-map`; completion note on
      bd-o76p01wb (c-gfv7ojsc)
- [x] #474 merged; rebased onto `main` (single commit fc422255), re-ran
      the full gates on the rebased tree (11,135 workspace tests + full
      `cargo xtask verify`, both green), pushed as
      `origin/feature/bd-o76p01wb-light-dark-theme-map`, opened
      **PR #475** (https://github.com/quarto-dev/q2/pull/475), and closed
      bd-o76p01wb.

## Resolved questions (review with Carlos, 2026-08-08)

1. **Warning granularity:** warn only when `dark:` is present; a light-only
   map is honored silently. Both scalar and list light-only forms get
   explicit tests. (→ D6)
2. **Q-14-3 wording:** "Dark theme variant not yet supported" is the right
   frame. The warning is expected to be short-lived, and user-facing text
   must not reference braid strands (not publicly readable). (→ D4)
3. **Close timing:** close bd-o76p01wb upon opening the PR; full support
   stays on bd-0pic6.
