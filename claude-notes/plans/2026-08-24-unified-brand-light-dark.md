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
consumer single-mode.**

1. **Type surgery (`quarto-brand`).** New untagged value type:

   ```rust
   #[derive(Debug, Clone, Deserialize, Serialize)]
   #[serde(untagged, deny_unknown_fields)]
   pub enum BrandColorValue {
       Single(String),
       LightDark { light: Option<String>, dark: Option<String> },
   }
   ```

   - The 13 named slots in `BrandColor` become `Option<BrandColorValue>`.
   - `palette` stays `BTreeMap<String, String>` (documented limitation,
     matches Q1's schema).
   - `BrandTypographyOptions.color` / `background_color` become
     `Option<BrandColorValue>`.
   - `Brand::has_dark_mode()` — port of `brandHasDarkMode` (colors,
     typography colors, logos).
   - `Brand::split() -> SplitBrand { light: Brand, dark: Brand }` — port of
     `splitUnifiedBrand` semantics (string → both; missing half → slot
     omitted in that half; logos via existing `LogoEntry::LightDark`).
     Post-split brands hold only `Single` values.
   - Single-mode accessors (`named()`, the typography layer, `resolve_color`)
     read the `Single` variant; a `LightDark` reaching them is a logic error
     (they only ever see split brands). Keep the accessor signatures
     returning `Option<&str>` so `brand_layer.rs` / `resolve.rs` change
     minimally.

2. **Split + dark-synthesis seam (`quarto-sass` / stage).** Restructure brand
   resolution so the brand file is **read and parsed once**, split, and each
   variant handed its half:

   - New entry point on `ThemeConfig` (working name
     `resolve_variants(runtime, base_dir)`) that:
     1. resolves the light `brand_ref` → unified `Brand`;
     2. splits it → light/dark halves;
     3. if the brand `has_dark_mode()` and `self.dark.is_none()`, synthesizes
        the dark theme variant (same construction as the existing config-time
        synthesis at config.rs:348 — clone light themes, inject brand token,
        `is_default: false` since a unified brand has no `dark_first`
        ordering signal);
     4. returns per-variant resolved configs for the stage to compile.
   - The existing config-time synthesis for the **two-file** form stays where
     it is (it needs no file contents); the new seam only adds the
     content-driven case. `from_config_value` stays pure.
   - `compile_theme_css.rs` switches from its two `resolve()` calls to the
     new entry point. Because the stage is shared, WASM/live-recompile gets
     the behavior for free.
   - **Two-file form + unified values** (a file named by
     `brand: {light: f1, dark: f2}` that itself uses `{light:, dark:}`
     values): Q1 rejects this (those files validate against the closed
     single-brand schema). Recommendation: split each file and keep the
     matching half (light file → light half), which is strictly more
     permissive than Q1 and avoids a second type family; note the divergence
     in the docs audit (bd-qnylgu69). — **Open decision, flag at review.**

3. **No caching changes.** Each variant still feeds `variant_css` a resolved
   single-mode brand; fingerprints already incorporate the brand content the
   same way they do today.

## Work items

### Phase 0 — tests first (TDD)

- [ ] `quarto-brand` unit tests: parse `BrandColorValue` map form for named
      colors and typography `color`/`background-color`; reject unknown keys
      inside the pair; palette map value still errors.
- [ ] `quarto-brand` unit tests: `split()` semantics (string → both halves;
      light-only → dark half omits slot; typography colors specialize;
      logos); `has_dark_mode()` (true only when a `dark:` key exists
      somewhere; false for all-plain-string brands).
- [ ] `quarto-sass` config tests: unified brand with dark values + no
      `theme:` dark half → dark variant synthesized (brand token injected,
      `is_default: false`); unified brand with no dark values → no dark
      variant; unified brand + explicit `theme: {light:, dark:}` → theme's
      `is_default` wins.
- [ ] End-to-end test (through `render_document_to_file`-level helper, per
      CLAUDE.md): fixture = issue #580's `_brand.yml` + `index.qmd`; assert
      `styles.css` has the light value and `styles-dark.css` the dark value,
      and the toggle/link pair is emitted. Also the typography case
      (`typography.headings.color: {light:, dark:}`).
- [ ] Run all new tests, verify they fail for the expected reason.

### Phase 1 — quarto-brand type surgery

- [ ] `BrandColorValue` enum + field type changes + accessors.
- [ ] `Brand::has_dark_mode()`, `Brand::split()`.
- [ ] Update `brand_layer.rs` / `resolve.rs` call sites to the single-mode
      accessors; quarto-brand + quarto-sass tests green.

### Phase 2 — pipeline wiring

- [ ] `ThemeConfig::resolve_variants` (read once, split, synthesize dark).
- [ ] Switch `compile_theme_css.rs` to it; remove the per-variant re-read.
- [ ] Full workspace: `cargo build --workspace`, `cargo nextest run
      --workspace`, `cargo xtask verify` (full — quarto-core/pampa are in the
      WASM closure).

### Phase 3 — end-to-end verification + docs + bookkeeping

- [ ] `cargo run --bin q2 -- render` on the #580 repro; inspect
      `styles.css` / `styles-dark.css`; record invocation + output snippet
      here.
- [ ] Verify the two controls still behave (plain string; two-file form).
- [ ] Check `docs/guides/authoring/brand.qmd` §"Light and Dark Colors" now
      matches reality (it should — the docs were written for this feature);
      note the palette limitation stays.
- [ ] Close bd-unified-brand-split-ep49amad; comment on GH #580.

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
