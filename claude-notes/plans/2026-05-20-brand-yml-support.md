---
title: '`_brand.yml` support in Quarto 2'
date: 2026-05-20
description: 'Adds `_brand.yml` theming to Quarto 2 by porting Quarto 1''s brand model into a new crate and feeding its colors, typography and fonts into the SCSS layer stack.'
---

**Created**: 2026-05-20
**Status**: DRAFT — design questions resolved 2026-05-20, awaiting
implementation approval
**Related**: `2026-01-13-sass-compilation.md` (Phase 8 stub supersedes itself with this plan)

## Goal

Allow Q2 projects to be themed via the same three-axis configuration that
Q1 supports today:

```yaml
# _quarto.yml
format:
  html:
    theme:
      - cosmo            # built-in Bootswatch theme
      - brand            # the brand.yml layer (or a `brand: <path>` key)
      - custom.scss      # user overrides
```

At the end of this work, a project that ships a `_brand.yml` alongside
`_quarto.yml` should produce HTML themed according to brand colors,
typography, and logo, with the brand layer landing in the correct
position in the SCSS layer stack.

Out of scope for this milestone (tracked as follow-ups):

- **Light/dark mode pairs** — Q2 doesn't have light/dark plumbing yet
  (see `bundle.rs::assemble_themes`). Brand will produce a single light
  variant first; the dark-variant work piggybacks on whatever produces
  the light/dark seam in Q2.
- **Logo wiring** (favicon, navbar brand-image) — `bd-97yc` tracks the
  favicon hookup; navbar brand wiring lives in the website plan. The
  brand crate exposes the data; consumers wire it in their own change.
- **Typst `_brand.yml` integration** — Q1 has a `typst-brand-yaml.lua`
  filter; Q2 typst format is too immature. Defer.
- **`quarto use brand` scaffolding command** — Q1 has it
  (`src/command/use/commands/brand.ts`); Q2's `quarto-project-create`
  crate is the analogous home but adding scaffolding is a separate
  command-surface change.

## What exists today

### In Q2 (`crates/quarto-sass/`)

- `ThemeSpec::parse(s)` resolves `"cosmo"` → `BuiltInTheme::Cosmo` or
  `"foo.scss"` → custom file path. Adding a third variant `Brand` is
  the natural extension point.
- `ThemeConfig::from_config_value(&config)` extracts `theme:` from the
  format-flattened metadata as a `Vec<ThemeSpec>`. Order-preserving.
- `process_theme_specs(&[ThemeSpec], &ThemeContext)` returns
  `ThemeLayerResult { layers, load_paths }`. The brand layer needs to
  slot into the layer stream at the position the user wrote it.
- `assemble_with_user_layers(&[SassLayer])` performs the final
  defaults-reversed assembly; brand-generated `SassLayer`s just need
  to flow through this unmodified.
- `compile_theme_css` / `compile_with_doc_vars` are the two
  compilation entry points; they consume `ThemeConfig` and a
  `ThemeContext`. The brand data has to arrive *via* `ThemeConfig`
  (because the pipeline stage that calls these is
  `CompileThemeCssStage`, which only sees `doc.ast.meta`).

### In Q1 reference (`external-sources/quarto-cli/`)

- `src/core/brand/brand.ts` (798 LoC) — `Brand` class: data model,
  color resolution (with palette aliasing + cycle detection),
  font/logo resolution. Uses Zod schemas generated from
  `definitions.yml` (the `brand-*` ids).
- `src/core/sass/brand.ts` (674 LoC) — produces three SCSS layers from
  a `Brand`: `brandColorLayer`, `brandDefaultsBootstrapLayer`,
  `brandTypographyLayer`. The output is `SassBundle` with brand
  layers in the `user` slot, plus a `dark` variant.
- `src/resources/schema/project.yml` — the `brand:` project key that
  resolves to `_brand.yml` (or a `{light, dark}` pair).
- `tests/docs/brand-yaml/kitchen-sink/_brand.yml` — comprehensive
  fixture exercising color palette, named theme colors, all font
  slots, logo references.

### Beads context

- `bd-97yc` (open, P4): Brand-aware favicon fallback — explicitly
  blocked on "Q2 doesn't have brand support yet". This plan unblocks
  it.
- No other beads issues touch brand.

## Design

### Crate layout

Two new pieces of code:

1. **`crates/quarto-brand/`** — new crate. Holds the data model and
   color/font/logo resolution logic. Stateless aside from the parsed
   `Brand` value. Depends on `quarto-yaml` (for parsing with source
   locations), `quarto-yaml-validation` (for schema enforcement), and
   `quarto-pandoc-types` only if needed for `ConfigValue` interop.
2. **`crates/quarto-sass/src/brand_layer.rs`** — new module *inside*
   the existing sass crate. Holds the `Brand → Vec<SassLayer>`
   translation (port of `core/sass/brand.ts`). Depends on
   `quarto-brand` for the input type.

Splitting along the data-vs-rendering seam matches Q1's split
(`brand/brand.ts` vs `sass/brand.ts`) and keeps `quarto-sass` from
having to grow YAML-parsing dependencies.

### Schema work — light-touch

`quarto-yaml-validation` is still bare; a comprehensive Q2 YAML
validation system is a separate, future task. We will not over-invest
here.

**Validation lives in serde for the first cut.** Strongly typed Rust
structs with `serde(deny_unknown_fields)` give us the same end-result
the user cares about — typos produce errors — without standing up a
full schema framework. Source locations are sacrificed for now; the
later validation task can layer schemas on top.

What this implies for the plan:

- No `crates/quarto-yaml-validation/` schema additions in this
  milestone.
- Brand types in `quarto-brand` derive `serde::Deserialize` with
  `deny_unknown_fields`, custom `untagged` enums where Q1's schema
  accepts multiple shapes (e.g. `BrandFont` source variants,
  `BrandLogoResource` string-or-object).
- Hand-ported fixtures from Q1's `tests/docs/brand-yaml/` are copied
  into the new crate's `tests/fixtures/`. They double as the
  acceptance suite — if they deserialize cleanly and produce the
  expected SCSS, we're good.
- Filing a follow-up beads issue: "Wire `_brand.yml` into the future
  schema validator" once that system exists.

### Configuration surface

Two YAML keys, matching Q1:

```yaml
# Project-level (preferred): _quarto.yml resolves a sibling _brand.yml
brand: _brand.yml             # or { light: _brand.yml, dark: _brand-dark.yml }

# Document-level: an inline brand block (Q1 also accepts this)
brand:
  color:
    primary: "#007bff"
```

`brand:` is processed *before* `theme:` resolution: the merged config's
`brand` key turns into a parsed `Brand` value, then `theme: [..., brand,
...]` references it. This is the same model Q1 uses: in
`external-sources/quarto-cli/src/format/html/format-html-scss.ts:216`,
the string `"brand"` in the theme array is preserved as a position
marker through `layerTheme`, then `brandBootstrapSassBundles`
substitutes the brand layers at that marker's position.

**Q2 ordering rule** (matches Q1):

- The literal token `brand` in `theme: [...]` expands to the
  brand-derived layers at that position.
- A bare `brand:` key with no `brand` token in `theme:` still
  works — the brand marker is auto-injected at the end of the theme
  array (Q1's default placement).
- `brand:` set but no `_brand.yml` resolvable → hard error (matches
  Q1).
- `theme: [brand, ...]` but no `brand:` configured → hard error (the
  user named a thing that doesn't exist).

### `ThemeSpec` extension

```rust
pub enum ThemeSpec {
    BuiltIn(BuiltInTheme),
    Custom(PathBuf),
    Brand,  // NEW
}
```

`ThemeSpec::parse("brand")` returns `ThemeSpec::Brand`. (Note: this
shadows a hypothetical theme named `brand.scss` — but `brand.scss` as
a path will still parse to `Custom("brand.scss")` because it has the
`.scss` extension.)

### `ThemeConfig` extension — pure parsing + separate brand resolution

Decision (was Open Question #3): keep `ThemeConfig::from_config_value`
pure; do the file I/O in a separate resolution step. The split:

```rust
// Stays pure. No I/O. Tests stay trivial.
pub struct ThemeConfig {
    pub themes: Vec<ThemeSpec>,
    pub minified: bool,
    pub suppress_bootstrap: bool,
    pub brand_ref: Option<BrandRef>,  // NEW: unresolved
}

pub enum BrandRef {
    Path(PathBuf),              // brand: _brand.yml
    Inline(serde_yaml::Value),  // brand: { color: { primary: ... } }
    // light/dark pair deferred — emit warning for now
}

// Resolved form: brand_ref → Brand. Lives in quarto-brand.
pub struct ResolvedThemeConfig {
    pub themes: Vec<ThemeSpec>,
    pub minified: bool,
    pub suppress_bootstrap: bool,
    pub brand: Option<Brand>,
}

impl ThemeConfig {
    // I/O lives here, takes the runtime. Symmetric with how
    // `process_theme_specs` already takes a runtime via ThemeContext.
    pub async fn resolve(
        self,
        runtime: &dyn SystemRuntime,
        base_dir: &Path,
    ) -> Result<ResolvedThemeConfig, SassError>;
}
```

Why this shape:

- **Hub-client parity.** `SystemRuntime` is the single abstraction
  that lets the native CLI and WASM hub-client share code; both can
  resolve a `BrandRef` against their respective filesystems. No
  divergent paths.
- **Testability.** `ThemeConfig::from_config_value` keeps its
  \~30 existing pure-function tests untouched; brand resolution gets
  its own test surface with a `MockRuntime`.
- **Cache fingerprinting.** `theme_fingerprint` can hash the
  `BrandRef` (or the resolved `Brand`\'s YAML serialization) without
  needing the runtime to look up files during fingerprinting.

`from_config_value` reads `brand:` from the merged config:
1. String → `BrandRef::Path`.
2. Map → `BrandRef::Inline`.
3. `{light, dark}` → log soft warning, take `light` half as
   `BrandRef::Path` or `BrandRef::Inline`.
4. Auto-injects `ThemeSpec::Brand` at end of `themes` if `brand_ref`
   is `Some` but `themes` doesn't already contain `Brand`.

`resolve` loads any `BrandRef::Path` via `runtime.file_read`, parses
inline data via serde, and constructs the typed `Brand`.

### Layer expansion in `process_theme_specs`

`process_theme_specs` already returns `ThemeLayerResult { layers,
load_paths }` in source order. The change:

```rust
for spec in specs {
    match spec {
        ThemeSpec::BuiltIn(t) => layers.push(load_theme(t)?),
        ThemeSpec::Custom(p) => layers.push(load_custom_theme(p, ctx)?),
        ThemeSpec::Brand    => layers.extend(brand_to_layers(ctx.brand()?)?),
    }
}
```

`brand_to_layers(&Brand)` is the port of Q1's `brandColorLayer`,
`brandDefaultsBootstrapLayer`, and `brandTypographyLayer` and returns
3 layers (or 2 if no typography/defaults configured).

### Brand → SassLayer translation

Lifted from `core/sass/brand.ts`. Three sub-layers:

1. **Color layer**
   - `$brand-<name>: <value> !default;` for each `color.palette` entry
     plus `--brand-<name>: <value>;` CSS custom properties.
   - `$<theme-color>: <resolved>;` for each named theme color
     (`primary`, `secondary`, `foreground`, `background`, ...).
   - Q1's `defaultColorNameMap` (`link-color → link`, `body-bg →
     background`, ...) ported verbatim.
2. **Bootstrap defaults layer** (only if `defaults.bootstrap.*` is set)
   - Variable injection from `defaults.bootstrap.defaults` (dict or
     raw SCSS string).
   - Bootstrap color palette (`$blue`, `$red`, ... from
     `color.palette` keys that match Bootstrap's named colors).
   - Passes through `uses` / `functions` / `mixins` / `rules` blocks
     from `defaults.bootstrap.*` to the corresponding SassLayer
     section.
3. **Typography layer**
   - `@import url(...)` directives for Google Fonts / Bunny Fonts.
   - `@font-face` blocks for `source: file` fonts (resolves relative
     paths against the brand file's directory).
   - SCSS variable assignments via Q1's `variableTranslations` map
     (`family → font-family-base`, `family → mainFont`, etc. — note
     this map is large; port it as-is).

Skip Q1's `quarto-scss-analysis-annotation` push/pop comments in the
first pass — they're for an analysis tool we don't have yet. Track as
a follow-up.

### Render-pipeline integration

Three touch points in `quarto-core`:

1. `CompileThemeCssStage::execute` already calls
   `ThemeConfig::from_config_value`. The `ThemeConfig` now carries the
   `Brand`; the stage continues to call `compile_theme_css` /
   `compile_with_doc_vars` unchanged. The brand expands inside
   `process_theme_specs`.
2. `bootstrap_js.rs` reads `ThemeConfig` to decide whether to ship
   Bootstrap JS. Brand-only-but-no-theme projects still need
   Bootstrap, so the existing `has_themes()`/`suppress_bootstrap`
   logic gets a third condition: also-not-brand-only-empty.
3. `theme_fingerprint` (the cache key) must include the brand
   contents — otherwise the cache lies. Easiest: hash the parsed
   `Brand`\'s YAML serialization into the existing fingerprint.

### CLI surface

No new CLI flags. `quarto render` and `quarto preview` already accept
`_quarto.yml`, and `brand:` is just a new key. The hub-client preview
path (WASM) consumes the same `ConfigValue`, so it inherits brand
support for free if the brand layer translation is WASM-compatible.

**WASM compatibility note**: the `quarto-brand` crate should build on
`wasm32-unknown-unknown` without feature gates. The only thing native-
only is the schema validator's regex (already cross-platform in
`quarto-yaml-validation`). Verify in CI.

## Phased breakdown (TDD throughout)

### Phase 0: fixtures (no schema yet — see "Schema work — light-touch")

- [x] One-shot copy `external-sources/quarto-cli/tests/docs/brand-yaml/`
  into `crates/quarto-brand/tests/fixtures/brand-yaml/`
  (kitchen-sink, monospace-colors, palette-colors at minimum).
  These become tracked test data; the original copy in
  external-sources is reference-only and never touched at build
  or test time.
- [x] One-shot copy
  `external-sources/quarto-cli/tests/smoke/use-brand/{basic,nested,multi-file}-brand/`
  → `crates/quarto-brand/tests/fixtures/use-brand/`.
- [ ] (Deferred to Phase 4) Generate Q1 reference SCSS for each
  fixture. Approach: rather than running TS Quarto once, port
  Q1's `brandColorLayer` etc. by hand and write expected SCSS
  by-construction. Q1 logic is deterministic and small enough to
  port without a reference oracle; Phase 4 will commit expected
  outputs derived from a careful read of Q1's `brand.ts`.

### Phase 1: `quarto-brand` crate scaffolding (data model only)

- [x] `cargo new --lib crates/quarto-brand`; add to workspace.
- [x] Define types: `Brand`, `BrandColor`, `BrandTypography`,
  `BrandLogo`, `BrandFont` (Google/Bunny/File/System variants),
  `BrandRef`.
- [x] Tests for serde deserialization of all 6 committed fixtures
  (kitchen-sink, monospace-colors, palette-colors, basic-brand,
  multi-file-brand, nested-brand).
- [x] **Tests first**: wrote `parse_kitchen_sink` against an empty
  `Brand` type before filling in fields; iterated until green.
- [x] Bonus: `unknown_top_level_key_is_rejected` regression test
  confirms `deny_unknown_fields` is wired.

### Phase 2: color resolution

- [x] Port `Brand::getColor` — palette aliasing, named-theme-color
  lookup, cycle detection (Q1's seen-set with 100-step cap).
- [x] Tests: palette alias, theme color → palette, cycle detection,
  raw CSS color passthrough, multi-step aliasing, empty config,
  quiet variant.

### Phase 3: typography + logo resolution

- [x] Port `getFont` as `Brand::font_slot(name)`,
  `effective_monospace_inline`, `effective_monospace_block` with
  Q1's `{ ...monospace, ...monospace-inline }` spread semantics.
- [x] Port `getLogo` (`Brand::logo`), `logo_image`, `resolvePath`
  (`BrandLogoResource::with_path_relative_to`),
  `getFavicon` (`Brand::favicon`).
- [x] Tests: font slot lookup (base/headings/link/monospace/\{inline,block\}),
  monospace merge semantics, logo path resolution relative to
  brand file dir, external-URL passthrough, alt preservation.

### Phase 4: SCSS layer generation in `quarto-sass`

- [x] New module `crates/quarto-sass/src/brand_layer.rs`.
- [x] Port `brandColorLayer`, `brandDefaultsBootstrapLayer`,
  `brandTypographyLayer` as
  `brand_to_layers(&Brand, font_path_prefix) -> Vec<SassLayer>`.
- [x] **Tests first**: 14 string-search tests in
  `tests/brand_layer_test.rs` verifying each layer's content.
- [x] Bonus: 6 compile-time parity tests in
  `tests/brand_compile_test.rs` that flatten the layers and run
  them through grass — guarantees the generated SCSS is
  syntactically valid for every committed fixture.
- [x] One intentional improvement over Q1: font-family values are
  double-quoted (Q1 emits them bare, which is fragile for
  multi-word names like "EB Garamond"). Documented in the module
  docstring.
- [x] Q1's `quarto-scss-analysis-annotation` comments are omitted (no
  Q2 analyzer reads them); tracked as Phase 8 follow-up.

### Phase 5: `ThemeSpec` and `ThemeConfig` extensions

- [x] Add `ThemeSpec::Brand` variant in `crates/quarto-sass/src/themes.rs`;
  update `parse`, `is_*`, `as_*`, `Display`. `"brand"` parses to
  `ThemeSpec::Brand`; preserve the path interpretation for
  `"brand.scss"`.
- [x] Add `BrandRef` enum to `quarto-brand` (`Path(PathBuf)`,
  `Inline(serde_yaml::Value)`).
- [x] Extend `ThemeConfig` with `brand_ref: Option<BrandRef>` (pure,
  no I/O).
- [x] Extend `ThemeConfig::from_config_value` to read the `brand:`
      key:
  - String → `BrandRef::Path`.
  - Map → `BrandRef::Inline`.
  - `{light, dark}` → soft handling, take `light` half.
  - Auto-inject `ThemeSpec::Brand` at end of `themes` when
    `brand_ref` is `Some` and theme list doesn't already mention it.
- [x] Hard error if `theme: [brand, ...]` but no `brand_ref`.
- [x] Add `ResolvedThemeConfig` + sync `ThemeConfig::resolve(&dyn
  SystemRuntime, base_dir)` that reads the brand file and
  constructs the typed `Brand`. (Sync — `SystemRuntime::file_read`
  is sync; matches the design's parity-with-hub-client
  goal.)
- [x] Tests: theme array with `brand` token, bare `brand:` key, both
  together, neither, error cases — 12 tests in
  `tests/brand_config_test.rs`.

### Phase 6: layer expansion + pipeline wiring

- [x] Update `process_theme_specs` to expand `ThemeSpec::Brand` via
  `brand_to_layers`, reading the brand from `ThemeContext`. The
  brand-not-set case errors with a clear message.
- [x] `compile_theme_css` and `compile_with_doc_vars` keep their
  signature (taking `&ThemeConfig`). The resolved brand flows
  via `ThemeContext::with_brand`, so the existing entry points
  get brand support for free.
- [x] Update `CompileThemeCssStage::run` to call
  `theme_config.resolve(runtime, project_dir)` and attach the
  resulting `Brand` to `ThemeContext` via `with_brand`. All
  brand I/O lives at this one site.
- [x] Update `cache_key` in
  `crates/quarto-core/src/stage/stages/compile_theme_css.rs` to
  hash the brand's YAML serialization (via
  `theme_context.brand()`) so brand changes invalidate the
  cache.
- [x] `bootstrap_js.rs` `suppress_bootstrap` logic still works
  correctly for brand-only projects: `from_config_value`
  auto-injects `ThemeSpec::Brand` into the theme list, so
  `suppress_bootstrap` stays `false` and Bootstrap JS ships.
- [x] Full workspace tests pass (9290/9290).

### Phase 7: end-to-end testing

- [x] Manual smoke tests with the real `q2` binary against
      tempdir-based projects:
  - `theme: [cosmo, brand]` + `brand: _brand.yml` ✓ — palette
    `brand-blue: "#0066cc"` shows up as `--brand-brand-blue`;
    `primary: brand-blue` resolves through palette to
    `--bs-primary-rgb: 0,102,204`; `foreground: "#222"` maps to
    `--bs-body-color`.
  - `brand: _brand.yml` alone (no `theme:`) ✓ — brand is
    auto-injected, primary + background apply.
  - `theme: [..., brand]` without `brand:` ✓ — stage logs warning
    and falls back to `DEFAULT_CSS`.
  - Inline brand block in document frontmatter — **does not apply**
    via the single-file render path. Filed as a follow-up; the
    project-level mode is the supported path for v1.
- [x] Committed regression test at
  `crates/quarto-core/tests/brand_render.rs` that drives
  `render_to_file` against the first two cases above (project +
  brand, brand-only). Catches the
  `CompileThemeCssStage`-doesn't-fire incident pattern for
  brand-yml.
- [x] `cargo xtask verify --skip-hub-build` passes all steps
  (9292/9292 tests + lint + scss-build).
- [ ] Browser verification in hub-client `q2 preview` — deferred
  to a follow-up beads issue; the WASM build chain still needs
  one final native-WASM resync, which is out of scope here.

### Phase 8: docs + follow-ups

- [x] User-facing docs page: `docs/guide/themes/brand.qmd`,
  linked from `docs/guide/index.qmd`.
- [x] File follow-up beads issues:
  - `bd-v5z8w` — Light/dark brand variant pairs.
  - `bd-1elkd` — Brand-aware favicon (related to `bd-97yc`).
  - `bd-hp3tx` — Navbar brand-image wiring.
  - `bd-dsco4` — Typst `_brand.yml` integration.
  - `bd-1vlw8` — `quarto use brand` scaffolding command.
  - `bd-q1fyw` — Wire `_brand.yml` into future YAML validator.
  - `bd-u67gw` — Port `quarto-scss-analysis-annotation` markers.
  - `bd-rwxa0` — Inline brand in document frontmatter (single-file
    path).
  - `bd-wjg4h` — Browser-verify in hub-client `q2 preview`.

## Decisions (resolved 2026-05-20)

1. **Schema porting strategy** — hand-port, but **light-touch**:
   serde-driven validation with `deny_unknown_fields` is enough for
   this milestone. No new `quarto-yaml-validation` schemas. A
   follow-up beads issue is filed in Phase 8 to wire brand into the
   future comprehensive YAML validator. _(User: "quarto-yaml-validation
   is relatively bare; we shouldn't overindex on schema validation
   right now.")_
2. **`brand` token in theme array** — controls position. Same as
   Q1, confirmed via
   `external-sources/quarto-cli/src/format/html/format-html-scss.ts:216`.
   When `brand:` is configured but no `brand` token appears in
   `theme:`, the marker is auto-injected at the end (Q1's default).
3. **`ThemeConfig` parsing and file I/O** — split: keep
   `from_config_value` pure (produces `BrandRef`); add
   `ThemeConfig::resolve(runtime, base_dir)` that does the file I/O
   and produces a `ResolvedThemeConfig` with a typed `Brand`.
   `SystemRuntime` is the abstraction layer, so the native CLI and
   WASM hub-client share the same code path — parity is automatic.
4. **Crate name** — `quarto-brand` (new crate, confirmed).
5. **Light/dark deferral** — defer. `{light, dark}` brand pairs emit
   a soft warning and use the `light` half. Follow-up beads issue
   tracks the light/dark seam work.
6. **Q1-reference fixtures** — commit Q1-generated SCSS as static
   fixtures. **`external-sources/quarto-cli/` is reading material
   only** and must not be referenced at build, test, or runtime —
   consistent with the existing External Sources Policy in
   `CLAUDE.md`. Fixture copy happens once during Phase 0; from that
   point on, the Q2 repo owns the assets.

## Risks

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| Brand SCSS output diverges from Q1 in subtle ways | Medium | Medium | Phase 4 golden-test parity against Q1-generated reference |
| `defaultColorNameMap` typo in port | Low | Low | Direct copy from Q1; unit-test each entry |
| Light/dark deferral surprises users | Medium | Low | Soft warning when `brand: {light, dark}` form is detected; doc it |
| Font `@import` URLs leak through to caches incorrectly | Low | Medium | Include `Brand` hash in `theme_fingerprint` |
| Project-vs-document `brand:` precedence | Medium | Medium | Mirror Q1: project `brand:` resolved first, document `brand:` merges/overrides via existing `ConfigValue::merge` |

## References

### Internal
- `claude-notes/plans/2026-01-13-sass-compilation.md` — Phase 8 stub
  this plan supersedes
- `crates/quarto-sass/src/{config,compile,themes,bundle}.rs` —
  integration surface
- `crates/quarto-core/src/stage/stages/compile_theme_css.rs` —
  pipeline stage that consumes `ThemeConfig`

### Q1 reference (external-sources/quarto-cli/)
- `src/core/brand/brand.ts` — data model
- `src/core/sass/brand.ts` — SCSS layer generation
- `src/resources/schema/definitions.yml` — `brand-*` schema entries
- `src/resources/schema/project.yml:35-39` — project-level `brand:`
  key shape
- `tests/docs/brand-yaml/kitchen-sink/_brand.yml` — comprehensive
  fixture

### External
- [\_brand.yml spec on posit-dev/brand-yml](https://posit-dev.github.io/brand-yml/) —
  cross-product spec (Quarto / Shiny / Posit Connect)
