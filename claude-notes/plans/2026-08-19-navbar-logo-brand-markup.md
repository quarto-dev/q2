# Navbar logo unstyled: theme ships no `.navbar-logo` rule and brand markup drops Q1's `navbar-brand-logo` structure (bd-navbar-logo-unstyled-gbzd8vcu)

**Date:** 2026-08-19
**Braid:** bd-navbar-logo-unstyled-gbzd8vcu
**Checkout:** main @ `f387bd68` (investigation ran in the main checkout; no worktree created)
**Status:** Plan finalized after design alignment with Carlos (2026-08-19). Awaiting go-ahead to implement.

## Design decisions (settled with Carlos, 2026-08-19)

1. **Companion CSS:** port `.navbar-logo` sizing, the `.navbar-brand-container` layout rules (`min-width: 0; display: flex; align-items: center` + the `lg` `margin-right: 1em`), `.navbar-brand.navbar-brand-logo { margin-right: 4px; display: inline-flex }`, `.navbar-brand { overflow: hidden; text-overflow: ellipsis }`, and `.navbar-container { width: 100% }`. **Omit** the `max-width: calc(100% - 115px)` clamp for now — it reserves ~115px for the hamburger toggler + search icon so an over-wide brand truncates instead of pushing them off-screen on narrow viewports; it only bites in narrow-viewport layouts q2 hasn't fully ported, so it moves to the follow-up strand below.
2. **Q1's navbar ordering system** (`$navbar-title-order` / `$navbar-toggler-order` `order:` rules + the `margin !important` pair): **skip here**. q2 already parses `navbar.toggle-position` into `Navbar::toggle_position` but nothing consumes it in SCSS. Follow-up strand filed: wire toggle-position into the ordering variables, port the `order:`/margin rules, and pick up the width clamp from (1). → **bd-navbar-ordering follow-up** (see § Strands).
3. **`navbar-container` class:** add it — `navbar_to_html`'s wrapper (render_html.rs:84) becomes `class="navbar-container container-fluid"`. Only that site: the footer (line 183) and secondary-nav (line 315) `container-fluid` wrappers are different components, untouched.
4. **Href fallback:** keep q2's relative `logo_href || home_url` fallback for **both** anchors (consistent with the root-relative-paths design, bd-root-relative-paths-design-fc5pvkcv). We adopt Q1's class hooks, not its absolute `/index.html` fallback.
5. **Light/dark logo variants: in scope.** The light/dark epic (PR #537, bd-0pic6 phases A–E) already landed the machinery this needs: `body.quarto-light .dark-content { display: none !important }` / `body.quarto-dark .light-content { … }` ship in `resources/scss/bootstrap/dist/scss/_light-dark.scss`, the toggle syncs the body class, and `Navbar::dark_mode_toggle` already tells the navbar transform whether the format has a dark variant. So the logo work is config parsing + markup + asset copying — no new CSS machinery.

## Issue context

Filed 2026-08-19 by Carlos, type `bug`, priority 2, label `navigation`. Origin strand `br-navbar-logo-unstyled-varhly4s` in the connect-docs porting skein. Real-world hit: Posit Connect docs render the Connect mark at full 512px SVG size in every page's navbar.

Two defects, confirmed at HEAD via the committed repro (`claude-notes/plans/navbar-logo-brand-markup-investigation/`, NOTES.md has the evidence):

1. **No default sizing rule** — Q1 ships `.navbar-logo { max-height: 24px; width: auto; padding-right: 4px }` (`quarto-nav.scss:196`); q2's compiled theme CSS has zero `navbar-logo` occurrences.
2. **Brand markup shape** — q2 inlines the img into the single title anchor; Q1's `navbar-brand-container` / `navbar-brand-logo` / `navbar-title` hooks (which user CSS keys on) are absent.

## Q1 reference (captured from external-sources/quarto-cli)

### Markup (`navbrand.ejs`)

```html
<div class="navbar-brand-container mx-auto">
  <a href="{logo-href || '/index.html'}" class="navbar-brand navbar-brand-logo">
    <img src="…" alt="…" class="navbar-logo light-content" />
    <img src="…" alt="…" class="navbar-logo dark-content" />   <!-- only with dark variant -->
  </a>
  <a class="navbar-brand" href="{logo-href || '/index.html'}">
    <span class="navbar-title">{title}</span>
  </a>
</div>
```

### CSS (`quarto-nav.scss:116-170, 196`)

```scss
.navbar-container { width: 100%; }
.navbar-brand { overflow: hidden; text-overflow: ellipsis; }
.navbar-brand-container {
  max-width: calc(100% - 115px);      // OMITTED here → follow-up strand
  min-width: 0;
  display: flex;
  align-items: center;
  @include media-breakpoint-up(lg) { margin-right: 1em; }
}
.navbar-brand.navbar-brand-logo { margin-right: 4px; display: inline-flex; }
// order:/margin-!important rules → follow-up strand
.navbar-logo { max-height: 24px; width: auto; padding-right: 4px; }
```

### Logo-spec semantics (`definitions.yml` `logo-light-dark-specifier`, `brand.ts::resolveLogo`, `website-shared.ts:136-150`)

Q1 accepts `logo:` as `false` | string | `{path, alt}` | `{light: <string|{path,alt}>, dark: <string|{path,alt}>}`; a bare string can also name a brand.yml logo resource, and an absent `logo:` falls back to brand logos (`small`→`medium`→`large`). Normalization always produces a `{light, dark}` pair: string/`{path,alt}` fill both halves; with `{light, dark}`, a missing light falls back to dark unconditionally, and a missing dark falls back to light when a dark brand exists. Top-level `logo-alt` is folded into the string form as `{path, alt}`.

**q2 scope:** the `false` / string / `{path, alt}` / `{light, dark}` shapes with cross-fallback between variants. Brand.yml logo-name indirection and the no-logo→brand fallback are **out of scope** (related: bd-v5z8w, which tracks wiring brand light/dark generally).

## What the code looks like today

- `crates/quarto-navigation/src/navbar.rs:106-117` — `logo: Option<String>` + `logo_alt` + `logo_href`, each path paired with a `SourceInfo` (`logo_source`, `logo_href_source`) so the path resolver knows the authoring file. Parsing at lines 181-193 (`as_plain_text()`); config round-trip serialization at ~275-310.
- `crates/quarto-navigation/src/render_html.rs:497-543` — `render_brand` builds the single-anchor markup; called from `navbar_to_html` at line 87 inside the line-84 `container-fluid` div.
- Consumers of `navbar.logo` downstream: `copy_navbar_logo` (`quarto-core/src/project/website_post_render.rs:128`) copies the logo file into `_site`; `NavbarRenderTransform` (`quarto-core/src/transforms/navbar_render.rs`) rebases the logo src per page; `metadata_path_resolution` tests pin the authoring-dir resolution behavior via `logo_source`.
- Light/dark infra (from the epic): `_light-dark.scss` content rules; `compile_theme_css.rs` dual-variant compile + `quarto-color-scheme` link attributes; `template.rs:955-975` appends `quarto-light`/`quarto-dark` body class; `Navbar::dark_mode_toggle` set by `NavbarGenerateTransform` when the format has a dark variant.

## Phases

### Phase 0 — Test plan (TDD: failing tests first)

Parsing (`navbar.rs` unit tests) — all landed, verified red (30 compile errors pre-model) then green:
- [x] `logo: path.svg` → single variant pair with identical halves; `logo_alt` fills alt (`logo_string_parses_as_single_pair`).
- [x] `logo: {path: p, alt: a}` → both halves get `p`/`a` (`logo_path_alt_object_parses_as_single_pair`; `logo_object_alt_wins_over_logo_alt_key`).
- [x] `logo: {light: l.svg, dark: d.svg}` → distinct halves; per-variant `{path, alt}` objects honored; per-variant alt wins over top-level `logo-alt` (`logo_light_dark_distinct_variants`).
- [x] `logo: {light: l.svg}` → dark falls back to light (and the mirror case) (`logo_light_only_falls_back_to_dark` / `logo_dark_only_falls_back_to_light`).
- [x] `logo: false` → no logo, no error (`logo_false_is_none`; plus `logo_empty_map_is_none`).
- [x] Round-trip serialization re-emits the authored shape (`logo_single_roundtrips_as_string_wire_shape`, `logo_variants_roundtrip_as_light_dark_map`).
- [x] Each variant path carries its own `SourceInfo` (`logo_variant_sources_captured_and_round_tripped`).

Markup (`render_html.rs` unit tests) — all landed, verified red (7 failing pre-implementation) then green:
- [x] Logo + title → container + dual anchors + `navbar-title` span; shared href (`brand_emits_container_with_logo_and_title_anchors`, `brand_both_anchors_share_the_logo_href`).
- [x] Identical variants → one unclassed `<img class="navbar-logo">` (`brand_single_logo_img_has_no_variant_class`).
- [x] Distinct variants → `light-content`/`dark-content` img pair (`brand_distinct_variants_emit_light_and_dark_imgs`).
- [x] Title-only / logo-only / neither (`brand_title_only_omits_logo_anchor`, `brand_logo_only_omits_title_anchor`; hidden-title test pre-existing).
- [x] Wrapper carries `navbar-container container-fluid` (`navbar_wrapper_carries_navbar_container_class` + updated `navbar_wraps_body_in_container_fluid`).

Theme CSS + end-to-end (drive the real render path per repo policy):
- [x] Website render test asserting the compiled `quarto-theme-*.css` contains `.navbar-logo` with `max-height` (and `.navbar-brand-container`) — `pipeline_theme_css_ships_navbar_brand_rules` in `navbar_footer_pipeline.rs`; verified failing before the SCSS change, passing after.
- [ ] Existing-assertion inventory to update: `render_html.rs:1236-1294`, `navbar_render.rs:872-984`, `shortcode_config_pipeline.rs` (4 sites), `navbar_footer_pipeline.rs`, `metadata_path_resolution.rs:212-248`, plus a workspace-wide grep for `navbar-brand` in `.snap` files after the change.

### Phase 1 — SCSS default rules

- [x] Add the decided rule set (§ decisions 1) to the quarto-nav port region of `resources/scss/bootstrap/_bootstrap-rules.scss`, each with a Q1 source-line comment per house style; note the clamp + order rules as deliberately deferred (follow-up strand id in the comment). Placed next to the existing `.navbar` block; `quarto-sass` theme-compile suite green.

### Phase 2 — Logo variant model

- [x] Replace `Navbar::logo: Option<String>` (+ `logo_source`) with a normalized variant pair: `logo: Option<NavbarLogo>`, `NavbarLogo { light, dark }`, `LogoVariant { path, alt, source }`; `logo_alt` folds into variants that lack alt (the separate `logo_alt` field is gone). Exported from `quarto_navigation`.
- [x] Parsing per Phase 0 specs; round-trip: single logo re-emits the historical `logo: <path>` + `logo-alt:` wire shape, distinct variants a `{light, dark}` map (string when no alt, `{path, alt}` otherwise), per-variant path `SourceInfo` preserved.
- [x] Consumers adapted per-variant (pulled forward from Phase 4 by the compile): `copy_navbar_logo` copies each distinct variant; `navbar_generate` resolves each path against its own source; `navbar_render` rebases each path per page. `render_brand` got a minimal single-img shim (light variant) pending Phase 3.

### Phase 3 — Markup restructure

- [x] Rework `render_brand` to the Q1 container/dual-anchor shape, single img when halves are identical, two variant-classed imgs otherwise.
- [x] Add `navbar-container` to the navbar wrapper div (navbar only; footer/secondary-nav untouched).
- [x] Existing-assertion churn: `navbar_with_title_and_left_items` (span shape), `navbar_wraps_body_in_container_fluid` (wrapper class), `shortcode_config_pipeline::plain_config_strings_unchanged` (`</span></a>` suffix). No `.snap` files reference the brand markup; full workspace suite green (12,915).

### Phase 4 — Downstream consumers

- [ ] `copy_navbar_logo`: copy each distinct variant file (dedupe identical paths).
- [ ] `NavbarRenderTransform` src rebasing + authoring-dir resolution: operate per-variant using each variant's `SourceInfo`.
- [ ] Sweep remaining `navbar.logo` consumers (`grep -rn '\.logo' crates/quarto-core/src crates/quarto-navigation/src`).

### Phase 5 — End-to-end verification

- [ ] Extend the investigation repro with a dark variant (`logo: {light: logo.svg, dark: logo-dark.svg}` + a dark theme) and re-render via `cargo run --bin q2 -- render …`.
- [ ] Inspect: theme CSS has the rules; HTML has the new structure; both variant imgs present with correct classes; logo files copied to `_site`.
- [ ] Browser eyeball (24px logo beside title; toggle flips variants). Record invocation + output snippet per repo policy.
- [ ] Full workspace suite + `cargo xtask verify` (WASM leg — quarto-navigation feeds the preview path).

### Phase 6 — Docs

- [ ] Update the docs/ website's navbar documentation: logo shapes (string / `{path,alt}` / `{light,dark}` / `false`), default 24px sizing, and the CSS override hooks (`.navbar-brand-logo .navbar-logo { … }`) now matching Q1. Render docs/ with q2 to verify.

## Strands

- This plan: bd-navbar-logo-unstyled-gbzd8vcu (in_progress).
- Follow-up **bd-y5y10oir** (filed at plan finalization, discovered-from this strand): navbar ordering system — wire `toggle-position` into `$navbar-title-order`/`$navbar-toggler-order` SCSS, port the `order:`/`margin !important` rules and the `.navbar-brand-container` width clamp.
- Related, not touched: bd-v5z8w (brand light/dark wiring — brand-logo indirection/fallback lands there or after it), bd-l1rx9yzh (light/dark-content CSS port — appears already delivered by the epic's `_light-dark.scss`; flagged to Carlos for close).

## Risks / tradeoffs

- **Test/snapshot churn** across every `navbar-brand` assertion — inventory in Phase 0; workspace-wide grep mandatory before declaring done.
- **`mx-auto` on the container** centers the brand when the navbar has free space; without Q1's ordering rules the flex layout could visibly shift the title. Phase 5's browser eyeball is the guard; the ordering follow-up is the durable fix.
- **Model change ripples**: `Navbar::logo` type change touches quarto-core transforms and the WASM/preview path — full `cargo xtask verify` (not just `--skip-hub-build`) required before commit.
- **Single-img deviation** from Q1's always-two-imgs markup: user CSS keying on `.light-content` specifically won't match single-logo sites; keying on `.navbar-logo`/`.navbar-brand-logo` (the documented hooks) works. Accepted.
