# Unified `_brand.yml` per-color light/dark values (GH #580, bd-unified-brand-split-ep49amad)

## Overview

GH issue [#580](https://github.com/quarto-dev/q2/issues/580) reports that
`docs/guides/authoring/brand.qmd` ("Light and Dark Colors", L156–L203)
documents per-color `{light:, dark:}` values inside a single brand, but the
type model cannot deserialize them:

```yaml
color:
  background:
    light: "#b22222"
    dark: "#22b222"
```

fails with `color.background: invalid type: map, expected a string`.

**Reproduced 2026-08-24** at `main` (post-v0.26.0) with
`cargo run --bin q2 -- render index.qmd`; both controls behave as the issue
states (plain string renders; two-file `brand: {light:, dark:}` produces
`--bs-body-bg: #b22222` in `styles.css` and `#22b222` in `styles-dark.css`).

**This work was already tracked** as `bd-unified-brand-split-ep49amad`
(follow-up from light/dark phase C, bd-ld-c-brand-seam-wef8ww3n). This plan
executes that strand. Epic: bd-0pic6.

The issue's secondary report — the error lacks the `Q-14-1` code and a source
snippet — is a separate diagnostics defect, filed as its own strand (see
"Out of scope / follow-ups").

## Q1 reference implementation

`external-sources/quarto-cli/src/core/brand/brand.ts`:

- Schema (`src/resources/schema/definitions.yml`): `brand-color-unified`
  gives every *named* color slot the type `brand-color-light-dark` =
  `string | {light?: string, dark?: string}` (closed). **`palette` entries
  stay plain strings** — matching the limitation callout in our docs.
  Typography `color` / `background-color` likewise accept the pair.
- `splitUnifiedBrand` (brand.ts:629–805): parses the unified brand, then
  produces `{light: Brand, dark: Brand, enablesDarkMode}`:
  - a plain string goes to **both** halves;
  - `{light: X}` with no `dark` puts X in the light half and **omits the
    slot from the dark half** (no fallback to the light value);
  - `specializeTypography` re-applies split colors per mode; all non-color
    typography fields are shared;
  - `splitLogo` does the same for logo slots (Q2's `LogoEntry::LightDark`
    already parses this shape).
- `brandHasDarkMode` (brand.ts:545): dark mode is enabled **iff some slot
  actually carries a `dark:` key** (color, typography color/background-color,
  or logo). A unified brand with only plain strings does not enable dark mode.

## Current Q2 architecture (what has to change)

- `crates/quarto-brand/src/types.rs` — `BrandColor` slots are
  `Option<String>` with `deny_unknown_fields` (L128–164); the map form has
  nowhere to go. `BrandTypographyOptions.color` / `background_color` are the
  same shape (L318–349).
- `crates/quarto-sass/src/config.rs`:
  - `ThemeConfig::from_config_value` (pure, no I/O) parses `theme:` +
    `brand:`; when `brand: {…, dark: …}` exists it **synthesizes the dark
    theme variant at config time** (L348) — possible there only because the
    two-file form announces its dark half in the config itself.
  - `ThemeConfig::resolve(runtime, base_dir)` (L462) does the I/O:
    reads the brand file and `serde_yaml`-parses it, **once per variant**
    (light and dark variants each re-read the same file today).
- `crates/quarto-core/src/stage/stages/compile_theme_css.rs` (L487–541):
  resolves the light variant's brand, compiles; if `dark_variant()` exists,
  resolves the dark variant's brand and compiles that too. This stage runs
  in both native and WASM (hub-client live recompile), so wiring here covers
  both paths.
- `crates/quarto-sass/src/brand_layer.rs` + `crates/quarto-brand/src/resolve.rs`
  consume `Option<String>`-shaped slots (`named_colors()`, `resolve_color`,
  typography layer). Color resolution (palette/theme-color reference chains)
  operates on a single-mode brand.

**The architectural wrinkle:** for a unified brand, "does this brand enable
dark mode" is a property of the brand *file contents*, which are only read in
`resolve()` — after `from_config_value` has already decided whether a dark
variant exists. The synthesis decision has to move to (or be repeated at) a
point where the parsed brand is available.

## Design

Follow Q1's shape: **parse unified, split early, keep every downstream
consumer single-mode.** (Refined 2026-08-24 after reading all consumers;
deltas from the original sketch are marked ⚠.)

1. **Type surgery (`quarto-brand`).** Make the brand types generic over the
   color-value type, with a default that keeps every existing consumer
   compiling unchanged:

   ```rust
   pub struct Brand<V = String> { ... }          // BrandColor<V>, BrandTypography<V>,
   pub type UnifiedBrand = Brand<BrandColorValue>; // BrandTypographyOptions<V> likewise

   #[serde(untagged)]
   pub enum BrandColorValue {
       Single(String),
       LightDark(BrandColorLightDark),           // { light: Option<String>, dark: Option<String> },
   }                                             // deny_unknown_fields on the inner struct
   ```

   - ⚠ Generic-with-default instead of one type holding enum slots: consumers
     keep `Brand` (= `Brand<String>`) and get a *compile-time* guarantee that
     split brands contain only plain strings; `resolve_color`, `named()`,
     `named_colors()`, and the SCSS layer are only defined on `Brand<String>`.
     Parsing (`from_yaml_str`, inline `serde_yaml::from_value`) produces
     `UnifiedBrand` only, so every construction site is forced through the
     split. Manual `Default` impls avoid the spurious `V: Default` bound.
   - The 13 named slots in `BrandColor<V>` become `Option<V>`; `palette`
     stays `BTreeMap<String, String>` (documented limitation, matches Q1's
     schema). `BrandTypographyOptions<V>::color` / `background_color` become
     `Option<V>`.
   - `UnifiedBrand::has_dark_mode()` — port of `brandHasDarkMode`: true iff
     some named color, typography color/background-color, or logo entry
     carries a `dark:` key.
   - `UnifiedBrand::split() -> SplitBrand { light: Brand, dark: Brand,
     enables_dark_mode: bool }` — port of `splitUnifiedBrand` semantics
     (string → both halves; `{light: X}` only → slot omitted from the dark
     half, no fallback; palette/meta/defaults/fonts shared).
   - ⚠ **Logos are carried through unsplit** (divergence from Q1's
     `splitLogo`): q2's logo consumers (favicon bd-97yc, navbar image
     bd-hp3tx) run once per document and need both sides to emit light/dark
     markup — `LogoEntry::LightDark` already models that. Logo dark halves
     still count for `has_dark_mode` (Q1 parity).

2. **Split + dark-synthesis seam (`quarto-sass`).**

   - ⚠ `parse_highlight_style` gains a side product: when no dark variant
     exists at config time, the dark-applicable highlight name (the pair's
     `dark:` value, or the scalar resolved with `dark = true`) is stashed in
     a new `ThemeConfig::deferred_dark_highlight` field instead of being
     dropped. This keeps `from_config_value` pure while letting a
     later-synthesized dark variant get the right palette (e.g.
     `highlight-style: a11y` → `a11y-dark`).
   - New `ThemeConfig::resolve_variants(self, runtime, base_dir) ->
     Result<ResolvedVariants, SassError>`:
     1. resolves the light `brand_ref` → `UnifiedBrand` → `split()`;
     2. if `enables_dark_mode && self.dark.is_none() && !suppress_bootstrap`,
        synthesizes the `DarkThemeConfig` (clone light themes — brand token
        already injected — `is_default: false`, `highlight_style:
        deferred_dark_highlight`, `brand_ref` = light ref);
     3. resolves the dark variant's brand: same ref as light → reuse the
        split's dark half (single file read); different ref (two-file form)
        → read + split that file and take its **dark** half;
     4. returns `ResolvedVariants { config /* with synthesized dark */,
        light_brand: Option<ResolvedBrand>, dark_brand: Option<ResolvedBrand> }`.
   - The existing config-time synthesis for the two-file form stays where it
     is. `ThemeConfig::resolve` / `ResolvedThemeConfig` are subsumed by
     `resolve_variants` and get removed/privatized once callers migrate.
   - `resolve_brand` (favicon/site-level helper) and `resolve_brand_layers`
     parse unified + split and use the **light** half — logo behavior
     unchanged since logos pass through the split intact.
   - Two-file form + unified values inside a file: split and take the
     matching half (light file → light half). More permissive than Q1
     (which rejects); noted for the docs audit (bd-qnylgu69).

3. **Decision flow (`quarto-core`).** ⚠ THREE consumers independently
   re-derive "does a dark variant exist / which is default" from config:
   `CompileThemeCssStage`, `render_with_compiled_template`
   (template.rs:837 — color-scheme meta + color-mode script), and
   `navbar_generate.rs:88` (dark-mode toggle). Content-driven enablement
   breaks the two pure re-derivations. Fix: the stage — which runs at
   position 11, before `AstTransformsStage` (13, navbar) and
   `ApplyTemplateStage` (17) — records the decision in doc metadata as
   `rendered.theme.dark-is-default: bool` (precedent:
   `rendered.includes.*`). Both downstream consumers read that key first
   and fall back to the current pure derivation when absent (direct-call
   and unit-test contexts where the stage never ran → behavior unchanged).

4. **No caching changes.** Each variant still feeds `variant_css` a resolved
   single-mode brand; fingerprints already incorporate brand content the
   same way they do today.

## Work items

### Phase 0 — tests first (TDD)

- [x] `quarto-brand` unit tests: parse `BrandColorValue` map form for named
      colors and typography `color`/`background-color`; reject unknown keys
      inside the pair; palette map value still errors.
- [x] `quarto-brand` unit tests: `split()` semantics (string → both halves;
      light-only → dark half omits slot; typography colors specialize;
      logos); `has_dark_mode()` (true only when a `dark:` key exists
      somewhere; false for all-plain-string brands).
- [x] `quarto-sass` config tests: unified brand with dark values + no
      `theme:` dark half → dark variant synthesized (brand token injected,
      `is_default: false`); unified brand with no dark values → no dark
      variant; unified brand + explicit `theme: {light:, dark:}` → theme's
      `is_default` wins.
- [x] End-to-end test (through `render_document_to_file`-level helper, per
      CLAUDE.md): fixture = issue #580's `_brand.yml` + `index.qmd`; assert
      `styles.css` has the light value and `styles-dark.css` the dark value,
      and the toggle/link pair is emitted. Also the typography case
      (`typography.headings.color: {light:, dark:}`).
- [x] Run all new tests, verify they fail for the expected reason.

### Phase 1 — quarto-brand type surgery

- [x] `BrandColorValue` enum + field type changes + accessors.
- [x] `Brand::has_dark_mode()`, `Brand::split()` (new `split.rs`).
- [x] Update `brand_layer.rs` / `resolve.rs` call sites to the single-mode
      accessors; quarto-brand + quarto-sass tests green.

### Phase 2 — pipeline wiring

- [x] `ThemeConfig::resolve_variants` (read once, split, synthesize dark).
- [x] Switch `compile_theme_css.rs` to it; remove the per-variant re-read
      (plus the `rendered.theme.dark-is-default` meta channel and the
      template/navbar consumers reading it with pure fallback).
- [x] Full workspace: `cargo build --workspace` ✓, `cargo nextest run
      --workspace` ✓ (13,165 pass). `cargo xtask verify`: steps 1–10 green
      (incl. hub-client build:all/WASM and test:ci); step 11 fails only on
      the KNOWN pre-existing bd-s36g9dav katex test (reproduced on a clean
      baseline via stash); the remaining legs (preview-runtime tests, hub
      MCP builds+tests, q2-preview-spa build) run individually — all green.
      Coverage: split.rs 100 %, quarto-brand 91 % of functions.

### Phase 3 — end-to-end verification + docs + bookkeeping

- [x] `cargo run --bin q2 -- render` on the #580 repro; inspected
      `styles.css` / `styles-dark.css`; recorded in the Phase 1–2 log below.
- [x] Verify the two controls still behave (plain string; two-file form) —
      covered by `unified_brand_all_plain_stays_single_variant` (e2e),
      `two_file_brand_resolves_each_variants_file` (sass), and the whole
      pre-existing `theme_light_dark` suite (26/26 green).
- [x] Check `docs/guides/authoring/brand.qmd` §"Light and Dark Colors" now
      matches reality: rendered the section's exact `_brand.yml` example —
      light headings color lands in `styles.css` (as minified `#114`), dark
      `#d0d0ff` in `styles-dark.css`. Palette limitation callout stays true.
      The "directly in the document metadata" example remains blocked by the
      unrelated GH #581 (inline front-matter brand), tracked separately.
- [x] Implementation committed on `braid/bd-unified-brand-split-ep49amad`
      (`f92c85ff6`); strand commented. Closing the strand (plus superseded
      bd-v5z8w) and commenting on GH #580 deferred until the branch is
      pushed/merged — both are outward-facing states that should follow the
      user's review.

## Out of scope / follow-ups

- **Diagnostics quality** (#580's secondary report): brand-file parse errors
  bypass `sass_error_to_parse_error`, so they print without `Q-14-1` and
  without a snippet despite serde_yaml providing line/col
  (`brand_err` sets `location: None`; the stage wraps it as a plain stage
  error). Filed as its own strand — needs the brand file bound via
  `bind_config_source` (mind the `add-file-with-id` lint) and a
  serde_yaml line/col → span mapping.
- **GH #581** (inline `brand:` block always fails: PandocInlines walker) —
  separate defect, separate strand; not addressed here.
- **Palette light/dark** — deliberately unsupported (docs callout + Q1
  schema agree).
- bd-v5z8w is superseded by phase C + this strand (its own comment already
  suggests closing it in favor of bd-unified-brand-split-ep49amad).

## Phase 0 log (2026-08-24)

- Tests written: `crates/quarto-brand/tests/integration/light_dark_test.rs` (20 tests),
  `crates/quarto-sass/tests/integration/brand_light_dark_test.rs` (11 tests),
  4 e2e tests appended to `crates/quarto-core/tests/integration/brand_render.rs`.
- Failure verified: the 3 feature e2e tests fail with exactly the GH #580 error
  (`color.background: invalid type: map, expected a string at line 3 column 5`);
  the all-plain control passes. quarto-brand / quarto-sass test binaries fail to
  compile on the missing `UnifiedBrand` / `resolve_variants` API, as expected
  for type-surgery TDD.

## Phase 1–2 log (2026-08-24)

- **quarto-brand**: `Brand<V = String>` generic (`UnifiedBrand = Brand<BrandColorValue>`);
  explicit `#[serde(bound(...))]` needed on the four generic structs (serde's
  syntactic inference otherwise demands `V: Default` for `#[serde(default)]`
  fields); manual `Default` impls; new `split.rs` with `SplitBrand`,
  `has_dark_mode`, callback-macro field lists shared with `types.rs`.
  All parse entry points now produce `UnifiedBrand`; existing tests migrated
  to `.split().light`. 70/70 tests pass (20 new).
- **quarto-sass**: `resolve()`/`ResolvedThemeConfig` replaced by
  `resolve_variants()`/`ResolvedVariants{config, light_brand, dark_brand}`;
  `load_split_brand` is the single brand-deserialization point;
  `deferred_dark_highlight` reserve slot on `ThemeConfig` filled by
  `parse_highlight_style` when no dark variant exists at parse time.
  277/277 tests pass (11 new). One test expectation fixed during TDD:
  `github` is itself adaptive, so a pair's `dark: github` correctly resolves
  to `github-dark` — the non-adaptive verbatim case now uses `dracula`.
- **quarto-core**: stage reads brand once via `resolve_variants`; records
  `rendered.theme.dark-is-default` in doc meta after storing a variant pair;
  `render_with_compiled_template` and `navbar_generate` read that key first,
  keeping their pure config derivation as fallback for stage-less pipelines.
- **Full workspace**: 13,165 tests pass.
- **Real-binary e2e** (per CLAUDE.md): `cargo run --bin q2 -- render index.qmd`
  on the exact #580 fixture → `styles.css` has `--bs-body-bg: #b22222`,
  `styles-dark.css` has `--bs-body-bg: #22b222`; HTML carries
  `quarto-color-scheme` + `quarto-color-alternate` + `quarto-color-scheme-extra`
  links, `<meta name="color-scheme" content="light">`, and
  `data-author-prefers-dark="false"`. Output inspected directly.
