# RevealJS themes: Quarto 1 → Quarto 2 (reveal.js 6)

**Strand:** bd-yown2ts4
**Branch:** `feature/revealjs-q1-themes` (epic integration line; stages land on sub-branches)
**Status:** Stage A (bd-r9mkybwl) + Stage B (bd-n697m7u7) implemented, verified, and
**merged** to `feature/revealjs-q1-themes` (2026-06-16). Running a Q1/reveal-5
look-and-feel audit (below) before deciding Stage C scope vs implementing
additional parity features.

## Overview

Quarto 2 ships reveal.js 6 but currently exposes only a single precompiled
`white.css` and no SCSS theme-layer system. For Quarto 1 users the output
feels unfamiliar — title-slide placement, bullet-point alignment, and the
general theme set all differ from what they're used to. The goal of this work
is to bring Quarto 1's revealjs themes and theming **defaults** into Quarto 2
so the output feels "at home" for Q1 users — *not* a pixel-perfect port, but
close enough in feel.

### The adaptation diagram

```
"native" reveal.js 5 themes  --adapted into-->  Quarto 1
"native" reveal.js 6 themes  --used directly-->  Quarto 2   (← the problem)
```

We want a theming system for reveal.js 6 that feels native to Quarto 2 — an
arrow **Quarto 1 → Quarto 2**. To draw that arrow well we must also understand
the **reveal.js 5 → 6** relationship, so the "used directly" arrow for Q2 can
become an "adapted into" arrow in the same sense Q1 adapted reveal 5.

---

## Assessment (study phase, 2026-06-16)

Three parallel source studies: Q2's current state, reveal.js 5→6 theming
differences, and how Q1 adapts reveal themes. The findings converge cleanly.

### 0. Versions

| | reveal.js | Theming approach |
|---|---|---|
| **Quarto 1** | **5.1.0** (vendored) | full SCSS layer system + 12 adapted themes + 1116-line `quarto.scss` |
| **Quarto 2 (now)** | **6.0.0** (vendored) | precompiled stock `white.css` only; reveal bypasses SCSS entirely |
| reveal latest 5.x | 5.2.1 | — |
| reveal target | 6.0.1 | — |

### 1. What Quarto 2 has today

- **reveal branch short-circuits before SCSS.** `CompileThemeCssStage`
  (`crates/quarto-core/src/stage/stages/compile_theme_css.rs:267-276`) returns
  immediately for `FormatIdentifier::Revealjs`, calling
  `register_reveal_assets` and skipping the entire Bootstrap/SCSS path.
- **One theme, hardcoded.** `resolve_theme_name`
  (`crates/quarto-core/src/revealjs/assemble.rs:43-48`) collapses *every* theme
  name to `"white"`. Any `theme:` value renders as white.
- **The only Quarto-authored reveal CSS is 1.5 KB.** `quarto-reveal.css` has
  `.columns`/`.column`, `.aside`, and footnote rules — **no title-slide rules,
  no list/bullet rules.** So title placement and bullet alignment come purely
  from reveal core + stock `white.css` defaults. *That is exactly why they feel
  un-Quarto.*
- **Assets are vendored + linked, not compiled.** `assemble.rs` embeds
  `reset.css`, `reveal.css`, `theme/white.css`, `quarto-reveal.css`,
  `reveal.js` via `include_str!`, registered as cascade-ordered artifacts
  (`1-reset → 2-reveal → 3-theme-<name> → 4-quarto-reveal`).
- **Q2 *does* have a full SCSS subsystem** — the `quarto-sass` crate — with the
  exact same 5-region layer machinery as Q1 (`uses`/`functions`/`defaults`/
  `mixins`/`rules`, with `defaults` reversed so higher-priority `!default`
  wins; `crates/quarto-sass/src/layer.rs`, `bundle.rs`). It compiles via
  `grass` natively and dart-sass on WASM. **But it is Bootstrap-centric** —
  `assemble_scss` always loads the Bootstrap framework layer + Quarto layer. It
  is wired for `format: html`, never for reveal.
- **Hard constraint — render/preview parity.** `q2 render` links the vendored
  CSS; `q2 preview` uses `@revealjs/react` importing the npm `reveal.js`. A
  drift-guard test (`assemble.rs:377-404`) asserts the vendored assets are
  byte-identical to the npm package. Any theming mechanism must serve both
  paths.
- **Existing roadmap.** `claude-notes/plans/2026-06-08-revealjs-presentations.md`
  explicitly deferred "Full Quarto-1 `quarto.scss` + 12-theme + brand parity"
  to **Phase 7 — Theming parity**, noting Q1 is on reveal 5.1.0 and the SCSS
  must be ported forward. **This plan is that Phase 7.**

### 2. reveal.js 5 → 6: what changed for theming

**The visual output of stock themes is essentially unchanged.** Same colors,
same layout, same core CSS. No themes added or removed (dracula, black-contrast,
white-contrast were *already* in 5.2.1). Core `reveal.scss`/`layout.scss` diffs
are almost entirely Prettier reformatting + Sass-module compliance — **no layout
drift**. So "feels different" is **not** caused by reveal core changing.

What *did* change is **how themes are authored/compiled** (compile-time only):

| Change | 5.x | 6.x | Impact |
|---|---|---|---|
| Sass variable names | `$backgroundColor`, `$mainColor`, `$headingColor`… (camelCase) | `$background-color`, `$main-color`, `$heading-color`… (**kebab-case**) | ⚠ breaking for any code that sets reveal vars |
| Theme authoring API | `@import "template/settings"` + reassign globals | `@use 'template/settings' with (...)` (module system) | ⚠ breaking |
| `:root` exposer | separate `template/exposer.scss` | folded into `settings.scss` | path reference gone |
| Theme dir layout | `css/theme/source/*.scss` | `css/theme/*.scss` (flattened) | path change |
| Background | single `$backgroundColor` + `bodyBackground()` mixin | split `$background` (shorthand/gradient) + `$background-color` (solid); mixin removed | gradients via var now |
| Gradient mixins | `radial-gradient()` etc. (vendor shims) | removed; use native CSS | minor |
| Build | Gulp + legacy `sass.render` | Vite + modern compiler | Q2 uses `grass`/dart-sass, irrelevant |

**The runtime `--r-*` CSS custom-property contract is STABLE 5↔6** (only net
addition: `--r-background`). reveal 6's `theme.scss` styles everything in terms
of `var(--r-*)`, which `settings.scss` emits into `:root`. A theme that
overrides `--r-*` is forward-compatible across both versions.

> ✅ **Toolchain verified (2026-06-16).** The "⚠ breaking" above is about *Q1's
> hand-written themes* needing migration — **not** about our compiler. We run
> `grass` 0.13.4 natively (dart-sass on WASM). A throwaway probe compiled **all
> 14 reveal-6 stock themes** through `grass::from_string` with the reveal theme
> dir on the load path. Every theme compiled, exercising the full reveal-6
> module API: `@use 'template/settings' with (...)` (configured modules),
> `@use 'sass:color'` + `color.scale()`, namespaced `@use 'template/mixins' as
> mixins` + `@include mixins.dark-bg-text-color()`, and `@import url(...)`
> passthrough. Output was correct (`white` → `--r-main-color:#222`,
> `--r-heading-text-transform:uppercase`, etc.). So `grass`'s module support is
> sufficient for reveal 6; the migration cost is on the *content* of Q1's
> themes, not on the build. (Probe deleted after recording; reproduce by
> compiling `external-sources/reveal.js/css/theme/*.scss` with that dir as a
> load path.)
>
> ⚠ **Implementation note for Stage A:** reveal 6's `@use 'template/settings'
> with (...)` needs configuration values *at the `@use` site*, which fights our
> layered-`!default` merge model (where higher-priority layers override via
> `!default` reversal). Q1 sidestepped this by **not** using reveal's
> `settings.scss` as a framework layer — it hand-ported settings into
> `quarto.scss`. We will likely do the same: port reveal 6's `settings.scss`
> `:root{--r-*}` emission into our Quarto reveal layer (now trivially
> kebab-case), and use reveal's `theme.scss` as the rules framework. This is a
> Stage-A design detail, not a blocker.

### 3. How Quarto 1 adapts reveal themes (the arrow we're copying)

- **5-region layered SCSS** (identical convention to Q2's `quarto-sass`).
- **Q1 invents its own kebab-case vocabulary** (`$body-bg`, `$body-color`,
  `$presentation-heading-color`, `$presentation-slide-text-align`, …) in the
  `scss:defaults` layer, then `quarto.scss` **maps those to reveal 5's
  camelCase** (`$backgroundColor`, `$headingColor`, …). Per-theme files never
  touch reveal's variable names.
- **Adapted themes are minimal.** `default.scss` is one line; `dark.scss` is
  four vars; `league.scss`/`dracula.scss` are the elaborate ones. Theme
  resolution aliases `white→default`, `black→dark`.
- **`quarto.scss` (1116 lines) is the heart**: defaults vocabulary + kebab→
  camelCase mapping + ~850 lines of rules (title slide, code blocks,
  blockquotes, callouts, `.smaller` system, columns, panels, asides, task
  lists, kbd, light/dark sentinel).
- **Split SCSS compilation + `exposer.scss` bridge** — Q1's load-bearing design
  decision. (NB: "split" here = two *separate Sass-compiler invocations*; this
  is unrelated to Q2's "pass 1 / pass 2" project-render phases.) The theme is
  compiled in its **own Sass invocation** *before* Pandoc (full variable scope →
  output `quarto-{hash}.css`); format `sass-bundles` are compiled in a
  **second, separate Sass invocation** *later*, where the theme's Sass variables
  are **not** in scope. The two invocations are bridged only by `exposer.scss`
  writing theme values to `:root --r-*`, with cross-format rules using
  `var(--r-background-color, $body-bg)` (runtime value for reveal, compile-time
  fallback for Bootstrap). **This split is the single most important thing to
  consciously replicate or deliberately avoid in Q2** (see Decision D1).

### 4. Root cause of the two specific complaints

Both are **explicit Q1 departures from reveal defaults** that Q2 simply doesn't
make (because Q2 has no Quarto reveal layer):

1. **Bullet/paragraph alignment.** reveal defaults slides to **center**
   (`.reveal .slides`). Q1 sets `$presentation-slide-text-align: left`
   (`quarto.scss:71, 316`). Without it, Q2 inherits reveal's centered look.
2. **Headings.** reveal defaults `$heading-text-transform: uppercase`. Q1 sets
   `$presentation-heading-text-transform: none` (`quarto.scss:58`). Without it,
   Q2 shows uppercase headings.
3. **Title slide** (bonus). Q1 centers the title slide while body slides are
   left, and shrinks the title `h1` to h2 size (1.6em) except in linear-nav
   mode (`quarto.scss:307-326`). Q2 has none of this.

> ⚠ **To verify on the Q2 side** (not yet confirmed by running the binary):
> render a deck and confirm Q2 currently shows centered bullets + uppercase
> headings. This is the cheap empirical check before designing.

### 5. The lucky convergence

reveal 6 independently moved to **kebab-case Sass variables** and a **stable
`--r-*` runtime contract** — i.e. it adopted exactly the shape Q1 built by hand
for reveal 5. Consequences:

- Q1's kebab→camelCase mapping section in `quarto.scss` **largely collapses** —
  Q2 can map `$presentation-*` straight to reveal 6's kebab `$heading-color`
  etc., or skip the indirection and override `--r-*` directly.
- Because reveal 6 themes read only `--r-*`, **a single unified Sass compilation
  may suffice** for Q2: compile reveal's `settings`+`theme` (emitting
  `:root --r-*`) together with the Quarto layer's rules (which use
  `var(--r-*, fallback)`) in **one** `quarto-sass` invocation. This could let us
  *avoid* Q1's split-compilation complexity (Decision D1).

---

## Proposed approach

A staged port, smallest-feeling-fix first.

### Stage A — Quarto reveal layer (fixes the "feels wrong" defaults)

Port the **defaults + core rules** of Q1's `quarto.scss` into a Quarto reveal
SCSS layer compiled through `quarto-sass`, targeting reveal 6 variable names.
Prioritize the items that close the perceived gap:
- `slide-text-align: left`, `heading-text-transform: none`
- title-slide layout (centered, h1→h2 sizing)
- list/bullet alignment, block margins, font sizing, code-block styling

This alone should make a *white* deck feel like Q1.

### Stage B — Theme set + selection plumbing

- Adapt Q1's 12 themes to reveal-6 kebab-case `@use ... with (...)` form (or
  `--r-*` overrides), as `quarto-sass` `defaults` layers.
- Replace the hardcoded `resolve_theme_name` with real theme resolution: built-in
  names, `white→default`/`black→dark` aliases, and user `theme: [name, custom.scss]`
  arrays. Wire a **reveal variant of `assemble_scss`** (reveal framework layer
  instead of Bootstrap) into the reveal branch of `CompileThemeCssStage`.

### Stage C — Parity + extras

- Render/preview parity (compiled theme served to both paths; keep the drift
  guard meaningful).
- `_brand.yml` integration, callouts/panels/tabsets/`.smaller`, light/dark
  sentinel — as needed for "at home" feel.

### Decisions (resolved 2026-06-16)

- **D1 — RESOLVED: single unified SCSS compilation.** One `quarto-sass`
  invocation assembles reveal framework + Quarto reveal layer + theme, treated
  as a conscious, documented divergence from Q1's split model.
  **Caveat (user):** because the variable surface is now *ours*, we must ship
  **documentation/guidance/examples** covering (a) what the theming variables
  are, and (b) how to adapt an existing Quarto-1 revealjs customization to Q2.
  → tracked as a Stage-C doc deliverable + a doc strand.
- **D2 — RESOLVED: keep Q1's `$presentation-*` / `$body-*` vocabulary**
  (Option 1). Full consequence analysis below; the deciding factors are
  cross-format brand/theme sharing and zero user-migration, and the fact that
  Option 2 unlocks essentially *no* capability that Option 1 forecloses (the
  runtime `--r-*` custom properties are emitted either way, so power users keep
  a runtime escape hatch regardless).
- **D3 — RESOLVED: ship Stage A alone first**, then B, then C — each on its own
  branch off the `feature/revealjs-q1-themes` integration line, so the user can
  check out and experiment with each stage independently.
- **D4 — RESOLVED: accept both naming schemes**, aliasing `white→default` /
  `black→dark` as Q1 does.
- **D5 — RESOLVED: acceptance bar is "feels at home," not pixel-perfect.** Q2 is
  also still missing Q1+reveal features (code annotations, transitions for
  executable-output slides, etc.), so pixel parity is not achievable now anyway.
  Simplify wherever reveal 6 already does the right thing.

#### D2 consequence analysis (why keep `$presentation-*`)

What Q1's `$presentation-*` / `$body-*` vocabulary actually *is*: a curated
Quarto-owned variable layer that (i) **shares names with HTML/Bootstrap theming**
(`$body-bg`, `$body-color`, `$link-color`, `$font-family-sans-serif/-monospace`),
(ii) adds **presentation-specific knobs** reveal has no direct name for
(`$presentation-slide-text-align`, `$presentation-title-slide-text-align`,
`$presentation-font-size-root`, …), and (iii) **insulates users from reveal's own
variable names**. Q1 maps these to reveal's vars in `quarto.scss`.

**Option 1 — keep `$presentation-*` (CHOSEN).**
- *Ongoing maintenance cost in Q2:* a single small mapping layer
  (`$presentation-*`/`$body-*` → reveal-6 kebab vars), ~30–50 lines, that must be
  re-checked when we bump reveal.js (one checklist item on a reveal upgrade) and
  extended when we choose to surface a new reveal knob. This is the *same kind*
  of curation we already do for Bootstrap variables, so it is bounded, familiar,
  and localized to one file. The abstraction is a curated subset: if reveal adds
  a knob we haven't mapped, users reach it via raw `--r-*` overrides or custom
  rules until we add it — completeness of the Quarto vocabulary is on us.
- *Benefits:* **cross-format theme/brand sharing** — set `$body-bg` /
  `$link-color` once and it applies to HTML *and* reveal; `_brand.yml` "just
  works" because brand emits these shared names (this is core to Quarto's value
  prop). **Migration-free for Q1 users** — existing `theme: custom.scss` files
  written against `$presentation-*`/`$body-*` keep working unchanged, which is
  the whole point of this epic. **Future-proof** — when reveal renames its vars
  again (as 5→6 did), only our one mapping file changes, not user themes.

**Option 2 — expose reveal-6 kebab vars directly (NOT chosen).**
- *What it means for themes:* new/edited themes are authored against reveal's own
  `$heading-color`/`$main-color` (essentially vanilla reveal theme format), so
  they are more directly portable to/from upstream reveal, and we invent/maintain
  no Quarto vocabulary.
- *What it costs existing users:* their `$presentation-*`/`$body-*` customizations
  **break** and must be rewritten against reveal's names — i.e. users must learn
  reveal internals, the opposite of "feel at home." It also **re-couples** user
  themes to reveal's naming, so the next reveal rename breaks them again.
- *Does it unlock new functionality?* **Essentially no.** Because reveal 6 emits
  the entire variable surface as runtime `--r-*` custom properties regardless of
  which Sass vocabulary we expose, every reveal knob is reachable at runtime
  under Option 1 too (via `:root { --r-…: … }` overrides). Option 2's only
  genuine upside is *lag-free, uncurated* access to the full reveal Sass surface
  without us maintaining a mapping — but that upside is largely neutralized by
  the `--r-*` runtime escape hatch, while its downside (breaking every Q1 theme)
  is exactly what the epic exists to avoid. We also wouldn't even escape the
  mapping work: `_brand.yml` → reveal still needs a brand→reveal bridge, so the
  mapping cost reappears in the brand layer.

**Net:** the choice is *not* about capability (both can reach everything via
`--r-*`); it is about **who bears migration cost** and **cross-format
consistency**. Option 1 puts a small, bounded maintenance cost on us and keeps
Q1 users (and `_brand.yml`) working unchanged. Decision: **Option 1.** We will
still *document* the underlying reveal `--r-*` properties as the supported
runtime escape hatch for power users.

---

## Work items

_D-series resolved 2026-06-16. Stages land separately on branches off
`feature/revealjs-q1-themes`. TDD: tests first. **Implementation not yet
greenlit** — awaiting user go-ahead._

### Study phase
- [x] Q2 current revealjs rendering + theming state
- [x] reveal.js 5 vs 6 theming differences
- [x] Quarto 1's revealjs theme adaptation + SCSS layers
- [x] **Empirically confirmed** Q2's current centered-bullets + uppercase-headings
      via a real `q2 render` + Chrome computed-style inspection (2026-06-16).
      Deck: two H2 slides with bullet lists. Findings on the first slide:
      `h2` → `text-transform: uppercase`, `text-align: center`, `font-size: 67px`;
      `.reveal .slides` → `text-align: center` (the `ul` is `display:inline-block;
      text-align:left`, so it centers as a block → classic reveal centered look).
      Confirms §4: both differences are reveal defaults Q2 doesn't override.

### Stage A — Quarto reveal layer (branch `beads/bd-r9mkybwl-reveal-scss-layer`)

**Architecture (sound, unified-compilation per D1).** A reveal assembly in
`quarto-sass` mirroring the Bootstrap one, but with a **reveal framework layer**
instead of Bootstrap. Layers, assembled with the existing 5-region order
(`defaults` reversed):

- **reveal framework layer** (vendored locally — External Sources Policy):
  - `defaults` = reveal-6 `settings.scss` `$kebab: … !default` declarations
    (the low-priority fallbacks: `#bbb`, `uppercase`, etc.)
  - `rules` = the `:root{--r-*: #{$kebab}}` emitter (split out of settings so it
    runs *after* defaults collapse) **then** reveal `theme.scss` rules (`var(--r-*)`)
  - `mixins` = reveal `mixins.scss` (`light/dark-bg-text-color`)
  - `uses` = `@use 'sass:color'`, `@use 'sass:meta'` (settings needs `color.scale`)
- **Quarto reveal layer** (ported from Q1 `quarto.scss`, Stage-A subset):
  - `defaults` = `$presentation-*`/`$body-*` vocabulary (D2) **+** the
    Q1 mapping section converted to reveal-6 **kebab** names
    (`$background-color: $body-bg !default;` etc.) so Quarto values flow into `--r-*`
  - `rules` = the look-fixing overrides: `$presentation-slide-text-align: left`,
    `heading-text-transform: none`, title-slide layout (centered, h1→h2 size),
    list/heading/spacing/basic code-block rules
- **theme layer** = none in Stage A (white-equivalent is the Quarto defaults);
  real themes are Stage B.

Sub-steps (TDD — test first each time):
- [x] **A1.** Vendored reveal-6 theme template SCSS locally under
      `resources/scss/revealjs/reveal-template/` (settings split into
      `_settings-vars.scss` + `_expose.scss` `:root` emitter, `_theme.scss`,
      `_mixins.scss`). Provenance in `resources/scss/revealjs/README.md`.
      (Landed under `resources/scss/revealjs/`, alongside the crate's other
      embedded SCSS, rather than the originally-noted `resources/revealjs/scss/`.)
- [x] **A2.** `quarto-sass`: `load_reveal_framework` + `load_quarto_reveal_layer` +
      `assemble_reveal_scss()` (reuses the shared `assemble_scss` ordering).
      7 unit/integration tests in `reveal_theme_test.rs` — all green: layer load;
      grass compile; `--r-main-color:#222`, `--r-background-color:#fff`,
      `--r-link-color:#2a76dd` (collision case); `--r-heading-text-transform:none`
      + no `uppercase`; `text-align:left`; `#title-slide` centered.
- [x] **A3.** Authored `resources/scss/revealjs/quarto-revealjs.scss`. Discovered
      a useful simplification: in reveal 6 `$link-color`/`$link-color-hover`/
      `$selection-color` coincide with reveal's own kebab names, so those need no
      mapping line — set the Quarto default once and it feeds the `:root` emitter.
- [x] **A4.** `compile_reveal_theme_css()` in `quarto-sass/compile.rs`
      (native `grass`, WASM dart-sass). Self-contained: no embedded load paths
      (only built-in `sass:color`/`sass:meta`).
- [x] **A5.** Wired `CompileThemeCssStage` reveal branch via a cfg-split
      `compile_reveal` helper; `register_reveal_assets` now takes
      `compiled_theme_css: Option<&str>` (`Some` = compiled theme in the
      `3-theme-*` slot; `None` = vendored stock fallback on compile failure).
      `RevealAsset.content` is now `Cow<'static, str>`. `reset.css` + core
      `reveal.css` stay vendored; **`quarto-reveal.css` kept separate** (it's
      columns/aside/footnotes, orthogonal to theming). Integration test
      `revealjs_theme_slot_is_compiled_quarto_theme` (reads the flushed theme).
- [x] **A6.** E2E verified through `q2 render` + Chrome computed styles:
      content slide `h2` → `text-transform:none`, `text-align:left`; `.reveal
      .slides` → `text-align:left`; title slide centered, title h1 → 1.6em (h2
      size). Screenshots confirm the Quarto-1 feel (mixed-case left-aligned
      content; centered h2-sized title). Font falls back to system Helvetica
      (Source Sans Pro bundling is the Stage B item).
- [x] **A7.** `q2 preview` parity assessed. **Stage A converges the render path
      ONLY.** The reveal branch keys on `FormatIdentifier::Revealjs`; the preview
      pseudo-format `q2-slides` is `(Html, preview)` and never enters it. The SPA
      (`hub-client/.../RevealjsReactAstSlideRenderer.tsx`, `q2-debug/entry.tsx`)
      **statically imports the vendored stock `resources/revealjs/theme/white.css`**
      at build time, so preview still shows the centered/uppercase reveal look.
      ⚠ **This re-introduces a render/preview divergence that bd-4b7f1hr7
      deliberately removed** by pointing both at the same vendored files. The
      divergence is intentional + temporary for staged delivery (D3). Converging
      preview (Stage C) means feeding the *compiled* Quarto reveal theme to the
      SPA — likely by precompiling the default Quarto reveal theme to a committed
      static CSS asset that both the render default and the SPA import, with the
      runtime compile reserved for non-default themes/brand/user vars. Tracked on
      Stage C (bd-j8qoyc0s). No automated parity test breaks (the hub-client
      parity test mocks the CSS imports; `preview_render_css_parity.rs` covers
      Bootstrap, not reveal).
- [x] **A8.** Vertical alignment parity (found during user experimentation).
      Q1 defaults reveal `center: false` (slides top-align; reveal's own default
      is `true`) and re-centers the title slide via a per-slide `.center` class
      (`format-reveal.ts`). Q2 wrongly defaulted `center: true`. Fixed:
      `reveal_config_json` now defaults `center: false`; `build_title_slide`
      adds the `center` class to the title-slide section. TDD (config test +
      title-slide-class test); E2E + Chrome confirmed body slides top-align
      (`top:18`, no inline offset) while the title slide stays centered (reveal
      applies `top` from the `.center` class). `.reveal` carries no global
      `center` class.

### Stage B — theme set + selection (own branch)

**Per-deck themes in websites WORK like `format: html` (corrected 2026-06-16).**
Each document render has its own `StageContext.artifacts`; `ApplyTemplateStage`
collects `css:revealjs:*` from *that* store only, so a deck links exactly the
theme it registered. Project-scoped artifacts drain into the orchestrator's
`project_artifacts` accumulator (`pass_two`) for one deduped flush to
`site_libs/`. HTML keys its theme by **content fingerprint** (`css:theme:<hash>`)
so different themes get different files and identical themes dedup — reveal will
do the same. (An earlier note here wrongly claimed reveal cross-links decks; that
was a wrong mental model — `ctx.artifacts` is per-document, not project-wide.)

- [x] **Theme artifact keyed by content fingerprint**: `css:revealjs:3-theme-<hash>`
      → `theme-<hash>.css` (replaced the fixed `3-theme-white`, reusing
      `theme_fingerprint`). `3-theme-` prefix keeps cascade order. Same theme →
      one shared file; different themes → distinct files; each deck links its own
      (per-document `ctx.artifacts`). Matches HTML; future-proof for Stage-C
      brand/per-deck vars. Tests: `register_reveal_assets_keys_theme_by_content_fingerprint`,
      and the two-deck website test now asserts exactly one shared `theme-<hash>.css`.
- [x] Adapted the 12 themes → `resources/scss/revealjs/themes/*.scss`: kebab-case
      for direct reveal-var sets (`$overlayElementBgColor` →
      `$overlay-element-bg-color`); `bodyBackground()`/`radial-gradient` →
      `$background: radial-gradient(...)`. Renamed dracula's local `$background`
      palette var → `$drac-background` (collides with reveal-6's `$background`).
- [x] Reveal theme resolution (`crates/quarto-core/src/revealjs/theme.rs`):
      parse `theme:` (string|array|absent→`default`); built-in (12 names +
      `white→default`/`black→dark` via `quarto_sass::resolve_reveal_theme_name`)
      vs user `.scss` (reuse `load_custom_theme`). Unknown/missing → loud stage
      error. `resolve_reveal_theme_name`/`load_reveal_theme_layer` added to
      quarto-sass; `resolve_theme_name`/`theme_css` removed.
- [x] `assemble_reveal_scss(&[SassLayer])` merges theme layers via `merge_layers`;
      `compile_reveal_theme_css(runtime, minified, &[layer], &[load_path])`;
      stage `compile_reveal` helper + reveal branch updated to resolve→compile→
      register.
- [x] Tests: 6 per-theme/alias/unknown tests in `reveal_theme_test.rs`; resolution
      unit tests in `theme.rs`; integration `revealjs_named_theme_is_compiled_and_selected`
      (`theme: dark`). Full quarto-sass+quarto-core suites + `cargo xtask verify`
      (incl. WASM) green.
- [x] **Font handling (B7 decision):** kept Q1's Google-CDN `@import`s; converted
      local `./fonts/league-gothic/…` → League Gothic Google-Fonts CDN; default
      theme stays system-font (Helvetica). Offline/self-contained font bundling
      deferred to Stage C.
- [x] E2E + Chrome: `theme: dark` (#191919 bg, white text, blue links) and
      `theme: dracula` (#282a36 bg, purple headings, cyan bullets, orange bold,
      yellow italic) render faithfully, top-aligned/left-aligned.

### Stage C — config defaults + Sass-helper foundation + SCSS quick-wins (bd-jxyqjf15)

_Inserted before the brand/callouts/docs stage per the 2026-06-16 audit decision
(user chose "config fixes + quick-wins first"). Branch
`beads/bd-jxyqjf15-reveal-config-and-quickwins` off `feature/revealjs-q1-themes`.
TDD; commit per increment._

- [x] **C1. Reveal config defaults.** Ported Q1's opinionated block into
      `reveal_config_json` (transition/backgroundTransition:none, center:false,
      1050×700, margin:0.1, navigationMode:linear, controlsLayout:edges,
      controlsTutorial:false, history:true, fragmentInURL/pdfSeparateFragments/
      hashOneBasedIndex:false) — each front-matter-overridable. `slideNumber`
      true→`c/t`(linear)/`h.v`(vertical); width/height accept `%` strings. Fixed
      preview parity in **both** `RevealDeck.tsx` and `RevealjsReactAstSlideRenderer.tsx`
      (center:false, transition:none, margin:0.1, linear nav, edge controls).
      TDD: `config_defaults_match_quarto1` + vertical-nav/override tests;
      integration `slideNumber` assertion. E2E: `q2 render` + Chrome
      `Reveal.getConfig()` confirmed. Full verify (incl. hub build) green.
      Commits 37061765 (code) + 3cea68ce (hub changelog).
- [x] **C2. Sass-helper foundation.** Ported into the Quarto reveal layer:
      `colorToRGB`/`tint`/`shade`/`shift-color` functions; `shift_to_dark`
      (kebab `$background-color` + `quarto-color.blackness`),
      `make/undo-smaller-font-size` (`quarto-math.pow`). Added the
      `quarto-color`/`quarto-math` `@use`s + supporting defaults
      (`$code-block-theme-dark-threshhold`, border/gray vars). Probed `grass`
      first — it supports `color.blackness`/`color.scale`/`math.pow`/`mix`.
- [x] **C3. SCSS quick-wins** ported from `quarto.scss`: `.has-light/dark-background`
      text+link+code recoloring; code-block border + full-width + scrollable
      `max-height`; `.smaller` system (global + per-slide, headings keep size via
      `undo-smaller-font-size`); blockquote restyle (left accent border, not
      italic-centered); kbd keycaps (`shift_to_dark` background); slide-number
      (muted, bg-aware); figure captions; multi-column gutters; ordered-list
      `type=` + task-list checkboxes; edge nav-control spacing; link
      weight/decoration; `--r-*` code-font custom properties. Tests:
      `quick_wins_compile_and_emit`, `shift_to_dark_picks_dark_value_on_dark_theme`.
      E2E + Chrome: code block, blockquote, kbd, `.smaller`, and dark-background
      legibility all render faithfully. Full `cargo xtask verify` (incl. WASM)
      green. **Deferred to Stage D** (need markup/consumer/plugins): light/dark
      sentinel, footer/logo, panels/tabsets, callouts, code-annotation, tippy.

### Stage D — brand + callouts + title/footer/logo + docs (bd-j8qoyc0s)

**Scope set 2026-06-16** after the audit. Three audit items are **deferred to
their own strands** (not part of Stage D):
- light/dark theme integration → **bd-904h9kmt** (Q2 lacks Q1's light/dark
  story broadly; the `/*! dark */` sentinel waits on that design).
- code-annotation styling → **bd-h176qcgp** (wants dedicated design time).
- reveal plugin foundation (menu/notes/line-highlight) → **bd-buwhvpc2**.

Remaining Stage D scope (concrete decomposition pending the prerequisite
investigation, 2026-06-16):
- [ ] **Callouts** — the highest-value "feels like Quarto" element (helper
      foundation from C2 is in place). Verify whether Q2 emits `.callout` markup
      for reveal first (else AST work; cf. bd-1kor9).
- [ ] **`_brand.yml` integration** — thread a resolved brand layer into
      `assemble_reveal_scss` (quarto-sass `brand_layer.rs` has reveal hooks).
- [ ] **Fancy title slide** — authors/affiliations/ORCID/email (likely needs
      author-metadata normalization).
- [ ] **`footer:` / `logo:`** — markup injection (reveal transform) + SCSS.
- [ ] **`auto-stretch`** (default on in Q1) — single-image slides → `.r-stretch`.
- [ ] **panels/tabsets** SCSS (state TBD by investigation).
- [ ] Render/preview parity hardening; keep the vendored-asset drift guard meaningful.
- [ ] **Docs (D1 caveat):** user-facing guide on the theming variables + a
      "migrating a Quarto-1 revealjs theme to Q2" how-to incl. the `--r-*` runtime
      escape hatch (docs/ site — render with `q2`, not Q1). Open a doc strand.

_(A concrete per-increment decomposition + branch will be added once the
prerequisite investigation lands.)_

---

## Q1/reveal-5 look-and-feel audit (2026-06-16)

Done after Stage B at the user's request, prompted by the `center` bug (A8) —
which showed the study-phase assumption "Q2's reveal config matches Q1" was
false. Full evidence-based audit of Q1's revealjs look-and-feel surface vs Q2
(post-A+B). **Decision pending: implement a subset of these vs proceed to Stage C
as originally scoped.**

### Headline finding (same class as the `center` bug)

**`transition` defaults to `"slide"` in Q2 but `"none"` in Q1**
(`format-reveal.ts:357`; schema `document-reveal-transitions.yml:6`). Q2 decks
animate on every slide change; Q1 decks don't. Immediately visible. Q2 emits only
**8** reveal config keys; Q1 seeds an **opinionated default block** (gated on
`revealjs-config != "default"`, `format-reveal.ts:343-361`) the user effectively
gets. Divergent/missing defaults Q2 should adopt:

| Key | Q1 default | Q2 today |
|---|---|---|
| `transition` | `"none"` | `"slide"` ⚠ |
| `width` / `height` | `1050` / `700` | unset (reveal `960`/`700`) |
| `margin` | `0.1` | unset (reveal `0.04`) |
| `navigationMode` | `"linear"` | unset (`"default"`) — also drives the title-h1 sizing rule |
| `controlsLayout` | `"edges"` | unset (`"bottom-right"`) |
| `controlsTutorial` | `false` | unset (`true` — bouncing arrows appear) |
| `history` | `true` | unset (`false`) |
| `backgroundTransition` | `"none"` | unset (`"fade"`) |
| `fragmentInURL` / `pdfSeparateFragments` | `false` | unset (`true`) |
| `slideNumber` | rewritten to `"c/t"`/`"h.v"` then quoted (`:336-340,516-519`) | raw bool/string passthrough |

⚠ **Preview parity bug:** `RevealDeck.tsx:158-171` hardcodes `center: true` +
`transition: 'slide'` — contradicts render's `center:false` + (corrected)
`transition:"none"`. Fix alongside.

### SCSS systems not yet ported from `quarto.scss` (highest-value)

- **Callouts** (`quarto.scss:873-1116`) — *very high* impact ("the #1 feels-like-Quarto element"), **large**; needs the Sass-helper foundation + revealjs callout AST output (bd-1kor9).
- **`.has-light/dark-background` text switching** (117-123, 269-305) — *high* (legibility on contrasting bg), **small SCSS**.
- **Code-block border/scroll/max-height** (328-373) — *high* (recognizable), **small-medium SCSS**.
- **`.smaller` font-scaling** (240-250, 488-549) — *high* (`smaller:true` ubiquitous), **medium**; needs `make/undo-smaller` mixins.
- **Light/dark sentinel** `/*! dark */` (669-677) — *high (functional)*: renderer greps it to pick the code-highlight theme. Small emit + a Q2 consumer.
- **Blockquote restyle** (385-400), **kbd** (841-856), **slide-number** (594-604), **figure/caption** (606-613), **multi-column gutters** (551-563), **link/brand** (865-871), **code-annotation** (735-824), **ol type/task-lists** (679-727) — mostly *medium/small* pure SCSS.
- **Foundational prerequisite:** the high-value SCSS items depend on porting Q1's Sass helpers (`quarto-color.blackness/.scale`, `quarto-math.pow`, `shift-color`, `shift_to_dark`, `make/undo-smaller-font-size`; `quarto.scss:204-258`). **Port this helper layer once first** — it unblocks the whole SCSS batch.

### Chrome / transform / plugin gaps

- **Fancy title slide** (authors/affiliations/ORCID/email; `title-fancy/`) — *high*, **medium-large**; needs author-metadata normalization (AST) + template + SCSS. Q2 has only single-author plain text.
- **`auto-stretch`** (default **true**) — single-image slides → `.r-stretch`; without it images overflow. *Medium-high*, AST/transform.
- **`logo:` / `footer:`** — markup injection (transform) + per-slide JS placement + SCSS. *Medium(-high)*.
- **Plugins: Q2 ships ZERO reveal plugins** in the render scaffold (no `plugins:` array). Q1 bundles menu (default), notes/search/zoom, line-highlight, pdfexport. Speaker view (S), menu, code-line stepping all absent. *Medium*, JS-plugin foundation. (Notes follow-up: bd-0qaarvzx.)
- **Footnotes/refs trailing-slide treatment** (`.smaller .scrollable` + title; `format-reveal.ts:770-808`) — Q2 coalesces footnotes but lacks this. *Medium*, transform.
- **`output-location: slide`**, slide backgrounds (`data-background-*`), citation→`#/references` rewriting — Tier-3, *medium/low*.

### Prioritized shortlist

**Quick wins (high impact / low effort):**
1. Config defaults: `transition:"none"` + the full opinionated block; `slideNumber` `c/t` rewrite+quoting; fix `RevealDeck.tsx` to match. *(config, small-medium)*
2. `.has-light/dark-background` text switching. *(SCSS, small)*
3. Code-block border/scroll/max-height. *(SCSS, small-medium)*
4. Blockquote restyle; kbd; slide-number; figure-caption; column gutters; link styling. *(SCSS, small each)*
5. `.smaller` system + light/dark sentinel. *(SCSS, medium; needs helper layer)*

**Larger efforts (high value, phase them):** callouts; fancy title slide; footer+logo; auto-stretch; plugin foundation (menu/notes/line-highlight). The **Sass-helper foundation** is the prerequisite to sequence first.

### Suggested re-scoping (proposal — awaiting user decision)

The audit suggests inserting a **config-defaults fix + Sass-helper foundation +
SCSS quick-wins** stage *before* the originally-scoped Stage C, because:
- the `transition`/config divergences are as user-visible as the `center` bug and cheap to fix;
- the SCSS quick-wins (legibility, code blocks, `.smaller`) are high "at home" payoff;
- the helper foundation unblocks both the quick-wins and the larger Stage-C items (callouts).

Stage C's brand + callouts + docs would then build on that foundation.

## References

- `claude-notes/plans/2026-06-08-revealjs-presentations.md` (Phase 7 deferral)
- `external-sources/quarto-cli/src/core/sass.ts` (layer merge + defaults reversal)
- `external-sources/quarto-cli/src/format/reveal/format-reveal-theme.ts` (bundler, split SCSS compilation)
- `external-sources/quarto-cli/src/resources/formats/revealjs/quarto.scss` (master)
- `external-sources/quarto-cli/src/resources/formats/revealjs/themes/*.scss`
- `external-sources/quarto-cli/llm-docs/sass-theming-architecture.md` (Q1's split SCSS compilation + exposer; the Q1 doc itself calls this "two-pass")
- `external-sources/reveal.js/css/theme/template/{settings,theme,mixins}.scss` (reveal 6)
- Q2: `crates/quarto-core/src/revealjs/`, `crates/quarto-core/src/stage/stages/compile_theme_css.rs`, `crates/quarto-sass/src/{layer,bundle}.rs`, `resources/revealjs/`
