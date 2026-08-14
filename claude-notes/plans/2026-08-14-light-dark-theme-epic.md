# Light/dark theme support epic (bd-0pic6)

**Created**: 2026-08-14
**Status**: DESIGN SETTLED (2026-08-14) — all open questions resolved with
Carlos; epic structure created in braid; awaiting go-ahead to start execution
**Umbrella strand**: bd-0pic6 (epic; children bd-… created 2026-08-14, see
"Proposed epic structure")
**Supersedes/absorbs**: the deferred "Phase 6b.6 Light/Dark Support" placeholder in
`2026-01-23-phase6b-custom-scss.md`; the `SassBundle.dark` scaffolding sketched in
`2026-01-13-sass-compilation.md` §7.3.
**Builds on**: `2026-08-08-theme-light-dark-interim.md` (bd-o76p01wb, PR #475 — map
parses, light half applied, Q-14-3 warning on `dark:`).

## Goal

Full Q1-parity dark mode for Q2, prioritized so that `format: html` documents and
website projects (concretely: `external-sources/quarto-web`) work first:

```yaml
format:
  html:
    respect-user-color-scheme: true
    theme:
      light: [cosmo, theme.scss]
      dark: [cosmo, theme-dark.scss]
highlight-style: a11y
```

renders a site with both CSS variants compiled, a working navbar toggle,
`prefers-color-scheme` respected, persistence via localStorage, correct body
classes (`quarto-light`/`quarto-dark`), and light/dark-aware syntax highlighting.
Later phases extend the same seam to brand pairs, `q2 preview`/hub-client, and
revealjs.

## Reference: how Q1 (1.11) does it

(From a close read of `external-sources/quarto-cli` at 1.11.1. File refs below are
into that tree.)

### Compilation model

- `theme: {light, dark}` normalizes to `Themes {light: string[], dark?: string[]}`
  (`src/format/html/format-html-scss.ts:375-455`). **YAML key order is semantic**:
  `defaultDark = keys[0] === "dark"` (also `format-html-info.ts:46-70`, which
  falls back to the `brand:` map's key order).
- One `SassBundle` per dependency with a nested `dark?: {user, quarto?, framework?,
  default: bool}` variant. At compile time (`src/command/render/pandoc-html.ts:140-198`)
  the bundle list is split into **full, separate CSS compiles**:
  - author-default-dark → emit order: `light, dark`
  - author-default-light → emit order: `light, dark, light-copy` — the trailing
    copy (class `quarto-color-scheme-extra`) exists so that first paint and
    JS-disabled browsers land on the default variant ("last enabled sheet wins").
    It is a cache hit, not a real second compile.
  - Per-layer fallback: a bundle with no dark opinion contributes its light
    layers to the dark compile (`bundle.dark?.user || bundle.user`).
- Link attributes: all variants are emitted `rel="stylesheet"` with classes
  `quarto-color-scheme` (light) / `quarto-color-scheme quarto-color-alternate`
  (dark) / `quarto-color-scheme-extra` (trailing copy), `data-mode="light|dark"`,
  and a shared `id="quarto-bootstrap"` (dupe ids are intentional; JS queries
  "first not-disabled"). **There is no `rel="alternate stylesheet"` anywhere** —
  that mechanism was never used.

### Runtime model

- A **synchronous inline script placed as the first child of `<body>`**
  (`quarto-html-before-body.ejs`, moved there by `format-html-bootstrap.ts:521-527`)
  flips `link.rel` between `"stylesheet"` and `"disabled-stylesheet"`, toggles
  `body.quarto-light`/`body.quarto-dark` from the active sheet's `data-mode`,
  and immediately disables the trailing light copies when the author default is
  light. Running before first paint is what prevents FOUC (Q1 #1325).
- Dark is **layered on top of light** — enabling dark never disables the light
  sheet; the dark CSS only needs to override.
- Persistence: localStorage key `quarto-color-scheme` with values
  `"default"`/`"alternate"` (**alternate ≠ dark**; it means "not the author's
  default variant"). `file://` falls back to a JS variable.
- `respect-user-color-scheme: true` (default false): initial state comes from
  `matchMedia('(prefers-color-scheme: dark)')`, with a `change` listener; an
  explicit localStorage choice always wins over the media query.
- Toggle widget: navbar/sidebar tool (`navdarktoggle.ejs` — an `<a
  class="quarto-color-scheme-toggle"><i class="bi"></i></a>` calling
  `window.quartoToggleColorScheme()`), floating `top-right` fallback for plain
  documents. Icon on/off state is pure CSS keyed on `.alternate`.
- Component adaptation: (a) CSS custom properties recompiled per variant (mermaid
  `--mermaid-*`, Bootstrap `--bs-*`); (b) `body.quarto-light .dark-content
  {display:none}` content-swap rules (`_quarto-rules.scss:766-774`); (c) giscus
  gets an explicit postMessage; (d) a `resize` event is dispatched on toggle.

### highlight-style

- `highlight-style` accepts scalar, `{light, dark}` map, and **adaptive** single
  names (`a11y`, `arrow`, `atom-one`, `ayu`, `breeze`, `github`, `gruvbox`,
  `monochrome`) that resolve `<name>-light.theme` / `<name>-dark.theme`
  (`src/quarto-core/text-highlighting.ts:34-118`).
- Two highlighting CSS files are emitted with the same class/ordering scheme as
  the theme sheets (`id="quarto-text-highlighting-styles"`).
- A theme-darkness sentinel comment `/*! dark */` (emitted by SCSS when
  `blackness($body-bg) > threshold`) auto-selects the dark highlight variant for
  single dark themes and drives `data-mode`.
- The variant's highlight theme also feeds `$code-block-bg`/`$code-block-color`
  etc. *into* the theme SCSS compile (`resolveTextHighlightingLayer`).

### brand

- `brand:` resolves to a light/dark `Brand` pair; a unified `_brand.yml` with any
  `{light:, dark:}`-valued field is split into two `Brand`s
  (`core/brand/brand.ts:635-805`). Brand layers ride as a `key: "brand"` bundle
  spliced at the `"brand"` marker position in each variant's theme list.

## Q2 current state (what exists, what's missing)

### Exists and is directly reusable

| Piece | Where | Notes |
|---|---|---|
| Map parsing (light half) | `quarto-sass/src/config.rs` `light_dark_pair`/`LightDarkPair` | Interim: drops the dark value, keeps its `SourceInfo` for Q-14-3. The fork point. |
| Pure compile path | `process_theme_specs` → `assemble_theme_scss` → `compile_with_doc_vars` | `&ThemeConfig + &ThemeContext → String`; can simply run twice. |
| Path rebasing for map leaves | `quarto-core/src/project/mod.rs` `FRAGMENT_PATH_PATTERNS` | Already rebases every string leaf under `theme`, incl. map form. |
| Body-class seam | `quarto-core/src/template.rs:759` `append_color_mode_class()` | Hardcodes `quarto-light`; bd-mtzry comment says it grows a `mode` arg. |
| Toggle CSS | `resources/scss/bootstrap/_bootstrap-rules.scss:2087-2224` | Full `.quarto-color-scheme-toggle` styling (navbar/sidebar/top-right/`.alternate`) already ported. |
| Content-swap CSS | `resources/scss/bootstrap/dist/scss/_light-dark.scss` | Vendored; only light half live (bd-l1rx9yzh). |
| `BuiltInTheme::is_dark()` | `quarto-sass/src/themes.rs:133` | Unused oracle for the darkness sentinel. |
| Dead scaffolding | `quarto-sass/src/types.rs` `SassBundle{,Dark}` | Ported from TS, wired nowhere. Either wire or delete. |
| Brand light/dark data | `quarto-brand/src/types.rs:153-178, 472-504` | `{light, dark}` fields and `LogoEntry::LightDark` parse; picking a side deferred (bd-v5z8w). |
| Editor color scheme | `hub-client/src/components/ThemeContext.tsx` | auto/dark/light provider for hub-client chrome; obvious signal source for preview iframe. |
| Map key order | `quarto-pandoc-types/src/config_value.rs:218` `Map(Vec<ConfigMapEntry>)` | Insertion order preserved ⇒ Q1's default-dark rule is implementable. (Verify merge doesn't reorder.) |

### Gaps (the actual work)

1. **`ThemeConfig` has no dark variant** — `LightDarkPair.dark_ignored` keeps only
   a location. Same for `extract_brand_ref` (silent TODO).
2. **Single-artifact assumption**: exactly one `css:theme:<fp>` artifact, assumed by
   `pass2_renderer.rs:869,1135`, `wasm-quarto-hub-client/src/lib.rs:1579`
   (`extract_theme_fingerprint`), pipeline tests, `preview_render_css_parity.rs`.
3. **Template can't emit per-link attributes**: `$for(css)$<link rel="stylesheet"
   href="$css$">$endfor$` — no way to add class/id/data-mode. (`SassBundle.attribs`
   was TS's answer; unused here.)
4. **No toggle JS**, no before-body injection point, no localStorage/persistence,
   no `respect-user-color-scheme` reader (zero hits in the workspace).
5. **No `highlight-style` reader at all**; highlight colors are one static SCSS
   layer (`resources/scss/html/templates/highlight.scss`, solarized-ish `hl-*`
   classes) loaded unconditionally into every compile. Q1's `.theme` JSONs target
   Pandoc's short classes (`.kw`, `.st`), not Q2's tree-sitter `hl-*` classes —
   not directly reusable.
6. **Navbar model has no `tools:`** (`quarto-navigation/src/navbar.rs:104`) — no
   place to render the toggle; `navbar_to_html`'s right-hand `<ul>`/search slot is
   the insertion point. (bd-fod3 tracks `tools:` generally.)
7. **`DEFAULT_CSS_CACHE` is a single-slot `OnceLock`**; `cache_key` has no variant
   discriminator (bd-8oqw wants a structured `CompileInputs` anyway).
8. **`ThemeContext` holds exactly one brand** — a dark compile with a dark brand
   needs a second context (or a `brand_dark` field).
9. **Preview transport is single-slot**: renderer writes one `styles.css` to the
   VFS; `Q2PreviewIframe.tsx` posts one `UPDATE_THEME` cssUrl; iframe `applyTheme`
   maintains exactly one `<link data-q2-theme>`.
10. **`BootstrapJsStage` is native-only** — hub-client preview never gets
    Bootstrap JS; the toggle JS must be dependency-free plain DOM JS.
11. **Dark half of content-swap rules not emitted** (bd-l1rx9yzh) — quarto-web's
    footer logo and `include-dark.lua` filter depend on it.

### What quarto-web specifically needs (acceptance target for the first phase)

- `theme: {light: [cosmo, theme.scss], dark: [cosmo, theme-dark.scss]}` — same
  Bootswatch base both sides; dark achieved purely by the user layer. The primary
  case for dual compile.
- `respect-user-color-scheme: true`.
- `highlight-style: a11y` (top-level, adaptive name → a11y-light/a11y-dark).
- `body.quarto-light` / `body.quarto-dark` rules in its own `styles.css`/`index.css`
  and `.light-content`/`.dark-content` content swapping (Posit logo, include-dark
  filter output).
- Its `theme-dark.scss` uses Bootstrap functions (`shade-color`) and vars
  (`$mono-background-color`), i.e. the dark compile must be a full
  framework+quarto+user assembly, not a bolt-on.
- Prerelease profile uses a light-only map (no `dark:` key) — must keep rendering
  without warning (already covered by interim D6 tests).
- Adjacent but separate strand: `css: styles.css` files are not copied into
  `_site/` by website projects (bd-r1y48cx0) — quarto-web's `body.quarto-dark`
  rules live there, so that bug masks part of this feature's effect.

## Proposed design

### D1 — adopt Q1's runtime mechanism essentially verbatim

Compile full separate CSS variants; emit ordered `<link rel="stylesheet">` tags
with Q1's exact classes/ids/`data-mode` (including the trailing default-copy
trick); port the before-body toggle script (de-EJS'd); localStorage sentinel with
the same key and `default`/`alternate` semantics; `body.quarto-light/quarto-dark`
classes.

Rationale: quarto-web's own CSS (and the broader Q1 ecosystem's custom CSS)
targets `body.quarto-light`/`body.quarto-dark` and the toggle classes; Q1 spent
several 1.7 iterations converging on the no-FOUC/no-JS-safe ordering, and the
toggle CSS is already ported. A "modern" alternative (CSS `light-dark()`,
`color-scheme`, media-query-only) cannot express arbitrary author-supplied theme
pairs compiled from different SCSS, and breaks localStorage override semantics.
Divergences we deliberately keep from Q2's cleaner architecture: links come from
the artifact system rather than DOM postprocessing (no DOM postprocessor rule),
and the script is a static asset + tiny inline config rather than an EJS
template.

**Constraint hierarchy (Carlos, 2026-08-14).** Q1's odd-looking triple-link
ordering exists because Q1 tried to serve *two* goals at once: some
light-vs-dark support with JS disabled (degrading to a single stylesheet — the
author default) *and* no FOUC. For Q2 the priorities are explicit: **avoiding
FOUC is a hard constraint; no-JS degradation is a nice-to-have** in this first
pass. We keep the emit-default-last ordering (it serves both goals at zero
cost), but when a future trade-off pits no-JS support against simplicity or
FOUC avoidance, no-JS loses. We are not obligated to mimic every idiosyncrasy
of the Q1 design.

### D1a — improvement over Q1: set `color-scheme` (approved direction 2026-08-14)

Q1 never sets the CSS `color-scheme` property, which is why dark Q1 pages have
light scrollbars/native popups and why its toggle JS carries the Safari
scrollbar-recolor hack (quarto-cli #1455). Q2 does better, cheaply:

- **Each compiled variant emits its own scheme**: `:root { color-scheme: light }`
  / `dark`, driven by the same darkness determination D3 computes for
  `data-mode`. The rel-swap toggle then flips the scheme automatically — no JS
  bookkeeping (optionally one belt-and-braces `documentElement.style.colorScheme`
  sync in the toggle).
- **`<meta name="color-scheme">` in the template head**, baked at render time:
  author-default value normally; `light dark` when
  `respect-user-color-scheme: true` (correct pre-CSS canvas paint, reduces
  flash-into-dark). Must NOT be `light dark` when the author default is fixed —
  otherwise author-light + OS-dark flashes dark before CSS loads.
- **Free bonus independent of the pair feature**: a single dark theme
  (`theme: darkly`) gets `color-scheme: dark` from its compile via the darkness
  sentinel — correct scrollbars/controls for existing dark-theme users.
- **Enables `light-dark()`** in user CSS as the documented Q2 idiom (one rule
  instead of a `body.quarto-light`/`body.quarto-dark` pair; body classes stay
  for Q1 compat). We do not port the Safari scrollbar hack.
- Scope: lands inside A2 (SCSS emission) + A3 (meta tag) + A4 (JS sync line);
  verification includes eyeballing Bootstrap form controls vs UA defaults.

### D2 — data model: `ThemeConfig` grows a dark variant

`LightDarkPair` carries `dark: Option<&ConfigValue>` instead of dropping it.
`ThemeConfig` gains:

```rust
pub struct DarkTheme {
    pub themes: Vec<ThemeSpec>,
    pub theme_locations: Vec<Option<SourceInfo>>,
    pub suppress_bootstrap: bool,     // {dark: none} — mirrors light-half handling
    pub is_default: bool,             // YAML key order: dark listed first
}
pub struct ThemeConfig {
    /* existing light fields unchanged */
    pub dark: Option<DarkTheme>,      // replaces dark_theme_ignored
}
```

- `from_config_value` parses both halves through the shared `from_theme_value`
  helper; brand auto-injection applies per-variant (dark brand once D7 lands;
  until then the light brand feeds both, matching Q1's per-layer fallback).
- Q-14-3 ("dark not yet supported") is **removed** when the dark half takes
  effect; the interim tests in `theme_light_dark.rs` invert (assert dark marker
  present in the dark artifact, no warning).
- `bootstrap_js.rs` predicate updates trivially (any variant non-suppressed ⇒
  ship JS).
- Semantics table (extends interim D3): `{light: none, dark: darkly}` — light
  variant is unstyled default? Q1 gates dark mode on `formatHasBootstrap`; we
  mirror: `none` on either half of the pair is an edge to define in tests
  (proposal: `theme: none`-style suppression applies per-variant; toggle emitted
  only when both variants produce stylesheets).

### D3 — dual compile in `CompileThemeCssStage`

Call the existing pure pipeline twice. Dark compile = same built-in layers +
dark spec list (+ dark doc-vars where variant-dependent, e.g. future
`$code-block-bg` from highlight style). Artifacts:

- keys `css:theme:<fp>` (light) and `css:theme-dark:<fp>` (dark) — the dark key
  sorts after the light key, matching the required link order for
  author-default-light; for author-default-dark we need explicit order control
  (see D4).
- paths: websites `quarto/quarto-theme-<fp>.css` + `quarto/quarto-theme-dark-<fp>.css`;
  single-doc `styles.css` + `styles-dark.css`.
- cache: add a variant discriminator to `cache_key` (fold into bd-8oqw's
  `CompileInputs` refactor if convenient); `DEFAULT_CSS_CACHE` becomes two-slot
  or keyed.
- update every "exactly one `css:theme:*`" consumer (gap #2 list).
- darkness sentinel: instead of grepping compiled CSS like Q1, compute
  `is_dark` from `BuiltInTheme::is_dark()` + (later) the highlight-style
  variant; if that proves insufficient for custom SCSS (e.g. `[cosmo,
  theme-dark.scss]` is "dark" only by its `$body-bg`), fall back to porting the
  `/*! dark */` SCSS sentinel — the vendored `_bootstrap-rules.scss` may already
  contain it (verify).

### D4 — link emission with attributes

Extend the artifact→template channel to structured entries: `Artifact` gains an
optional `attribs: Vec<(String, String)>` (the revival of the dead
`SassBundle.attribs` idea, but on the artifact); `collect_artifact_urls` returns
`{href, attrs}` objects; templates render `<link rel="stylesheet" href="$css.href$"$css.attrs$>`
(doctemplate supports map access; fall back to a pre-rendered attribute string
if not). When no dark variant exists, attrs are empty and output is
byte-identical to today (no snapshot churn outside the feature).

The author-default-light "trailing light copy" is a third artifact entry
referencing the same CSS path with the `quarto-color-scheme-extra` class — no
recompile, just a second link. Explicit ordering: give theme links an explicit
sort-stable key scheme (`css:theme:0:<fp>`, `css:theme:1:<fp>-dark`, …) or an
`order` field, rather than relying on lexicographic accident.

### D5 — toggle runtime

- **Static JS asset** `quarto-color-mode.js` (port of `quarto-html-before-body.ejs`
  logic, no EJS): rel-swap enable/disable, body-class sync, localStorage
  sentinel, `respect-user-color-scheme` media-query path, giscus hook omitted
  until Q2 has comments, `resize` dispatch. Config (authorPrefersDark,
  respectUserColorScheme) passed via `data-*` attributes on its own script tag.
- **Placement**: must run before first paint ⇒ injected at the top of `<body>`
  via the template (new template slot or `include_before`), not via the sorted
  `$for(scripts)$` head loop. Plain DOM JS, no Bootstrap dependency ⇒ works in
  hub-client preview later.
- **Body class at render time**: `append_color_mode_class(mode)` grows its mode
  argument (bd-mtzry); default class = author default (respecting
  `respect-user-color-scheme` means the JS may flip it before paint, same as Q1).
- **Toggle widget**: website navbar/sidebar — render into `navbar_to_html`'s
  right-hand slot when the format has a dark variant (interim: hardcoded
  emission, folded into bd-fod3's `tools:` support when that lands); plain
  documents get the floating `top-right` fallback (small DOMContentLoaded block
  in the same JS asset). CSS already shipped.
- **`respect-user-color-scheme`** reader added to the html format config.
- **Dark half of `_light-dark.scss`** content-swap rules activated
  (bd-l1rx9yzh joins this epic).

### D6 — highlight-style reader + variant palettes

Two-stage scope:

1. **Epic phase (needed for quarto-web)**: add a `highlight-style` config reader
   (scalar + `{light, dark}` map + adaptive-name resolution). Represent each
   style as an SCSS layer in `resources/scss/html/highlight-styles/<name>.scss`
   targeting Q2's `hl-*` classes. Selection replaces the currently-unconditional
   `load_highlight_layer` per compile variant. Ship a small curated set first:
   the current default (solarized) + `a11y` light/dark (hand-translated from
   Q1's `a11y.theme`/`a11y-dark.theme` JSON via a Pandoc-token → tree-sitter
   capture mapping table). Unknown style names ⇒ structured warning + default.
2. **Follow-up strand**: a general `.theme`-JSON → `hl-*` SCSS translator (build
   time or xtask codegen) to cover Q1's full style catalog; plus Q1's
   feedback loop of highlight-derived `$code-block-bg` into the theme compile.

This keeps bd-0pic6 from swallowing a full highlighting-theme subsystem while
still making `highlight-style: a11y` work on quarto-web.

### D7 — brand light/dark seam (bd-v5z8w)

After the theme seam exists: `extract_brand_ref` carries both halves
(`BrandRef` pair); `ThemeConfig::resolve` resolves two `Brand`s; dark compile
gets a `ThemeContext` with the dark brand (add `with_brand` on a second context
— cheaper than widening `ThemeContext`). Unified `_brand.yml` splitting
(Q1's `splitUnifiedBrand`) ports into `quarto-brand`. Default-dark falls back to
the `brand:` map's key order when `theme:` doesn't decide (Q1 rule). Logo
`LightDarkPair` consumers (favicon/navbar) pick per-variant via the
content-swap classes.

### D8 — preview / hub-client

- Second theme slot end-to-end: renderer writes `styles-dark.css` next to
  `styles.css` in the VFS; `UPDATE_THEME` message and `applyTheme` grow a
  variant field (two `<link data-q2-theme="light|dark">`); fingerprint plumbing
  carries a pair.
- The iframe's initial mode follows the same toggle JS if the rendered document
  ships it; additionally, hub-client can post its editor `ColorScheme`
  (ThemeContext.tsx) into the iframe so preview follows the editor chrome.
  Exact policy (document toggle vs editor scheme precedence) is a design point
  for that phase, informed by phase-1 lessons.
- `q2 preview` native serves rendered output unchanged — it inherits phase-1
  behavior for free, but the embedded SPA path needs the D8 transport work.
- Grass-vs-dart-sass divergence exposure doubles (bd-izs62xci) — parity test
  extends to the dark artifact.

### D9 — explicitly out of this epic (tracked separately)

- revealjs light/dark (bd-904h9kmt) — waits for this seam, then Stage-D design.
- mermaid `$mermaid-*`/`--mermaid-*` bridge (bd-sehm2rha/bd-nj25kgbu) — dual
  compile makes the vars per-variant automatically once that lands.
- giscus/comments (no comments support in Q2 yet).
- `website.tools:` general support (bd-fod3) — we hardcode only the dark toggle.
- Typst/`brand-mode` for non-HTML formats.
- bd-r1y48cx0 (`css:` files not copied into `_site/`) — independent bug, but
  quarto-web verification depends on it; schedule alongside phase A.

## Proposed epic structure

**Created in braid 2026-08-14.** bd-0pic6 is the epic parent (retitled). The
A-lane is a `blocks` chain (A1→A2→A3→A4→A5) and is the time-sensitive
`format: html` + website lane; B and C block on A2; D and E block on A5.
Integration branch: `feature/light-dark-theme` (created off `main`).

- [x] **A1 — data model** (D2): `bd-ld-a1-data-model-a12bhj1g`. **Done
  2026-08-14.** `DarkThemeConfig {themes, theme_locations, suppress_bootstrap,
  is_default, key_location}` on `ThemeConfig::dark` (replacing
  `dark_theme_ignored`); both halves parsed via `from_theme_value`; per-variant
  `none` semantics; brand token auto-injected into both halves (explicit token
  position honored per-variant; brand-token-without-brand errors for either
  half); key-order `is_default` rule + two quarto-config materialize tests
  guarding key-order preservation through the merge; `ResolvedThemeConfig`
  carries `dark`. TDD: 10 new/extended unit tests confirmed red first.
  **Deliberate deferrals to A2** (so A1 has zero behavior change): Q-14-3
  still fires (now keyed off `dark.key_location`; retire when the dark half
  actually compiles), `bootstrap_js` predicate unchanged (updates when dark
  CSS ships), interim integration tests in `theme_light_dark.rs` unchanged
  (they invert in A2).
- [x] **A2 — dual compile + artifacts** (D3, D1a): `bd-ld-a2-dual-compile-ds10l5wa`.
  **Done 2026-08-14.** `CompileThemeCssStage` refactored to a per-variant
  `variant_css()` helper (suppress → fast path → themed path, identical
  behavior per variant); dark half compiles via `ThemeConfig::dark_variant()`
  projection into `css:theme-dark:<fp>` / `quarto/quarto-theme-dark-<fp>.css`
  (single-doc: `styles-dark.css`). Key prefix `css:theme-dark:` deliberately
  does NOT match the `css:theme:` prefix, so every existing light-only
  consumer (preview transport, wasm `extract_theme_fingerprint`, tests)
  needed **zero changes**. D1a landed as one SCSS change: the existing
  darkness-sentinel block in `_bootstrap-rules.scss` now also emits
  `:root{color-scheme:light|dark}` — verified surviving grass minification;
  single dark themes (darkly) get it for free; quarto-web's cosmo-based
  dark half gets `dark` via its `$body-bg`. Q-14-3 fully retired (emission,
  catalog entry, docs page, tests). `bootstrap_js` predicate now
  `!ships_bootstrap()` (per-variant). **Discoveries vs the original plan**:
  no cache-key variant discriminator needed (the key hashes spec identities;
  identical inputs correctly share output); no `DEFAULT_CSS_CACHE` two-slot
  needed yet (default compile is variant-independent until highlight-style
  doc-vars differ per variant — phase B may revisit); interim link order
  (dark sorts before light → light wins cascade) keeps pages visually
  unchanged until A3. Golden-hash baseline re-captured (documented delta:
  the one color-scheme rule). E2E: real `q2 render` of a quarto-web-shaped
  project inspected. 12,135 workspace tests green.
- [x] **A3 — link emission** (D4, D1a): `bd-ld-a3-link-emission-ruw9kw4v`.
  **Done 2026-08-14.** `Artifact` gained typed `link_attribs:
  Vec<(String,String)>` + `link_order: i32`; `collect_artifact_urls` returns
  `LinkedResource` sorted by `(link_order, key)` (all order-0 ⇒ pre-existing
  order preserved byte-identically, guarded by the golden-hash test);
  attributed entries render as `TemplateValue::Map` with `$if(css.href)$`
  single-line branches in both built-in templates (plain-string entries keep
  custom-template compat — matches Q1, whose `$css$` never carried theme
  links). Theme trio: light (`quarto-color-scheme`, order 10), dark
  (`quarto-color-scheme quarto-color-alternate`, order 20), and for
  author-default-light a trailing re-link of the SAME light file
  (`quarto-color-scheme-extra`, order 30 — class replaces so toggle
  selectors skip it; needed for the FOUC hard constraint). `data-mode` from
  each sheet's compiled `/*! dark */` sentinel (`css_is_dark`), not its
  slot. `<meta name="color-scheme">` emitted in the full template head when
  a dark variant exists — author default first, both schemes under
  `respect-user-color-scheme: true` (reader introduced here, reused by A4).
  E2E: real render inspected (trio + meta exactly Q1-shaped). 12,139 tests
  green.
- [x] **A4 — toggle runtime** (D5): `bd-ld-a4-toggle-runtime-0t9i2rvs`.
  **Done 2026-08-14.** `quarto-color-mode.js` (de-EJS'd port of Q1's
  before-body script + after-body floating-toggle fallback, config via
  `data-*` attrs on its own tag; divergences documented in the file header:
  no Safari scrollbar hack — `color-scheme` supersedes it — and no giscus)
  injected INLINE as the first child of `<body>` via the
  `color-mode-script` template variable. Same localStorage key/values as Q1
  (`quarto-color-scheme` = `default`/`alternate`) so preferences carry over.
  `append_color_mode_class` grew its `default_dark` arg (bd-mtzry resolved);
  `respect-user-color-scheme` wired into the runtime. Navbar toggle:
  `Navbar.dark_mode_toggle` (set by `NavbarGenerateTransform` from the theme
  config, round-trips via `dark-mode-toggle` in the stored config map),
  rendered as Q1's `quarto-navbar-tools` slot markup. bd-l1rx9yzh resolved
  as a side effect: both content-swap halves were already compiled; the body
  class flip makes them live. **Bug found by browser verification, fixed
  with a regression test**: `colorToRGBA()` was never ported to
  `_bootstrap-functions.scss`, so the toggle icons' SVG fills contained the
  literal call text (silently invalid — string interpolation doesn't error
  on unknown functions) and the icon was invisible. Golden hash re-captured
  for that fix. **Browser-verified end-to-end** (chrome-devtools MCP against
  a served website fixture): initial light state with dark+extra sheets
  disabled pre-paint; toggle → dark (rel-swap, body class, root
  color-scheme, localStorage `alternate`, icon `.alternate` state,
  `.light-content`/`.dark-content` swap); reload restores dark before
  paint; toggle back to light restores everything (`default` stored); no
  console errors. 12,145 workspace tests green.
- [x] **A5 — large-project end-to-end**: `bd-ld-a5-quarto-web-e2e-bzg4o5lc`.
  **Done 2026-08-14.** Rendered the connect-docs testbed
  (`~/repos/github/cscheid/q2-connect-docs/docs-quarto-2`, posit-docs
  extension theme map) with the real binary: **352 of 352 files, zero
  errors, zero Q-14-3** (the interim run printed one warning; now the dark
  half compiles). Output verified: exactly one shared light + one dark
  fingerprinted artifact in `site_libs/quarto/` (deduped across all 352
  pages), Q1-shaped link trio + `<meta name="color-scheme" content="light">`
  + inline runtime + navbar toggle on every page. Browser-verified with
  chrome-devtools MCP: toggle → full posit-docs dark palette (body
  `#181c25`), root color-scheme dark, localStorage persistence, toggle back
  restores light. (Testbed's unrelated known caveat unchanged: the
  quarto-openapi Deno-style pre-render was temporarily disabled during the
  render — bd-wch2dotq — and restored after.)
  **Target corrected 2026-08-14 (Carlos):** quarto-web is currently a Quarto 1
  project and a full q2 render of it is out of scope; the intended large
  testbed is `~/repos/github/cscheid/q2-connect-docs/docs-quarto-2` (the
  posit-docs extension ships `theme: {light: [theme.scss], dark:
  [theme-dark.scss]}` — the same shape, 351 files). quarto-web remains the
  *config-shape* reference only.
  **quarto-web findings recorded en route** (scratch checkout probed, then
  fully restored): (1) two Q-5-24 alias conflicts (`quarto-ast.qmd` aliases
  collide with prerelease pages that render there; `placeholder.qmd` claims
  `/docs/prerelease/1.5/lipsum.html` which `lipsum.qmd` also claims — the
  latter looks like an upstream copy-paste bug); (2) the `_quarto.yml`
  page-footer logo images write attrs Pandoc-style (`{fig-alt="…" width=65px
  .light-content}`), violating qmd's classes-before-key-values rule (Q-2-3)
  and failing every page's profile pass; (3) beyond those, wide Q1-content
  gaps (grid tables Q-2-39, shortcode/attr strictness Q-2-9/Q-2-35, missing
  relative filter paths) — confirming quarto-web is not a Q2 render target
  today.
- [ ] **B — highlight-style** (D6 stage 1): `bd-ld-b-highlight-style-jnb036fz`.
  Reader + a11y light/dark + variant selection; follow-up strand to be filed
  for the general `.theme` translator.
- [ ] **C — brand seam** (D7): `bd-ld-c-brand-seam-wef8ww3n`. Absorbs bd-v5z8w;
  unified-brand split.
- [ ] **D — preview/hub-client** (D8): `bd-ld-d-preview-hub-t4oxv0hf`.
  VFS/iframe dual transport, editor-scheme integration (related: bd-nxe8).
  Uses lessons from A.
- [ ] **E — cleanup**: `bd-ld-e-cleanup-qxidnkng`. Delete-or-wire
  `SassBundle{,Dark}` scaffolding, docs (`docs/` user-facing dark-mode page,
  `light-dark()` idiom, migration notes), audit `bd-36vmz7nk`/`bd-qmpygp02`
  fallback posture for the dark compile path.

Follow-up already filed: `bd-ld-toggle-into-tools-hpae7m9r` — fold the
hardcoded toggle into `tools:` when bd-fod3 lands.

Each child follows TDD (tests-first, red confirmed) per CLAUDE.md; A2/A3
particularly need end-to-end CLI tests (the CodeHighlightStage incident pattern:
in-process tests can pass while the real pipeline bypasses a stage).

## Open questions (for iteration with Carlos)

1. ~~**Q1-verbatim runtime** (D1) — divergences?~~ **Resolved 2026-08-14**:
   Carlos wants the feasibility headroom spent on improvements; `color-scheme`
   adopted as D1a. Core rel-swap/triple-link/localStorage mechanics stay
   Q1-compatible. (Other candidate improvements can still be raised during
   iteration.)
2. ~~**highlight-style scope** (D6)~~ **Resolved 2026-08-14**: curated-set-first
   confirmed; `a11y` + default is enough for phase B. The full theme set may be
   needed soon but is a follow-up strand, not this epic.
3. ~~**Artifact attribs vs header-includes** (D4)~~ **Resolved 2026-08-14**:
   extend `Artifact` + template as proposed.
4. ~~**Toggle placement** (D5)~~ **Resolved 2026-08-14**: hardcode the navbar
   toggle emission now; file a follow-up strand (linked to bd-fod3) to fold it
   into general `tools:` support.
5. ~~**Single-doc dark artifact naming**~~ **Resolved 2026-08-14**:
   `styles-dark.css` alongside `styles.css`.
6. ~~**Epic mechanics**~~ **Resolved 2026-08-14**: retitle bd-0pic6 as the epic
   parent, create A1–E children with parent-child deps, integration branch
   `feature/light-dark-theme`.

## References

- Interim: `claude-notes/plans/2026-08-08-theme-light-dark-interim.md` (PR #475)
- Brand: `claude-notes/plans/2026-05-20-brand-yml-support.md`
- Sass port: `claude-notes/plans/2026-01-13-sass-compilation.md` §6.2/§7.3
- Custom SCSS: `claude-notes/plans/2026-01-23-phase6b-custom-scss.md` (6b.6)
- Highlighting: `claude-notes/plans/2026-04-19-syntax-highlighting-design.md`
- Strands: bd-0pic6 (umbrella), bd-v5z8w (brand pairs), bd-904h9kmt (reveal),
  bd-l1rx9yzh (content-swap CSS), bd-fod3 (`tools:`), bd-nxe8 (hub-client chrome
  scheme), bd-r1y48cx0 (css copy bug), bd-8oqw (CompileInputs), bd-mtzry (body
  class seam), bd-izs62xci (sass compiler split)
- Q1 key files (external-sources/quarto-cli @1.11.1):
  `src/format/html/format-html-scss.ts`, `format-html-info.ts`,
  `src/command/render/pandoc-html.ts`,
  `src/resources/formats/html/templates/quarto-html-before-body.ejs`,
  `src/quarto-core/text-highlighting.ts`, `src/core/brand/brand.ts`,
  `src/core/sass/brand.ts`
- quarto-web config: `external-sources/quarto-web/_quarto.yml:682-707`,
  `theme.scss`, `theme-dark.scss`, `filters/include-dark.lua`
