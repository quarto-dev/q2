# RevealJS themes: Quarto 1 → Quarto 2 (reveal.js 6)

**Strand:** bd-yown2ts4
**Branch:** `feature/revealjs-q1-themes` (epic integration line; stages land on sub-branches)
**Status:** Study phase + toolchain experiment complete; decisions D1–D5 resolved
(2026-06-16). **Awaiting user go-ahead to begin Stage A implementation.**

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

### Stage A — Quarto reveal layer (own branch)
- [ ] Reveal variant of `assemble_scss` in `quarto-sass` (reveal framework layer,
      not Bootstrap; single unified compilation per D1)
- [ ] Port reveal-6 `settings.scss` `:root{--r-*}` emission into the Quarto reveal
      layer (kebab-case); use reveal `theme.scss` as the rules framework
      (see §2 implementation note — avoids the `@use ... with` vs layered-`!default` clash)
- [ ] Define the `$presentation-*`/`$body-*` → reveal-6 kebab mapping layer (D2)
- [ ] Wire reveal branch of `CompileThemeCssStage` to compile the Quarto reveal layer
- [ ] Port Q1 `quarto.scss` defaults + core rules → reveal-6 layer (TDD: snapshot
      the compiled CSS; assert `text-align:left`, no `uppercase`, title-slide rules)
- [ ] End-to-end verify through `q2 render` AND `q2 preview` (parity); re-run the
      Chrome computed-style check from the study phase to confirm the flip

### Stage B — theme set + selection (own branch)
- [ ] Adapt 12 themes to reveal-6 form (kebab vars / `--r-*`), as `defaults` layers
- [ ] Real theme resolution (built-ins, `white→default`/`black→dark` aliases,
      user `theme: [name, custom.scss]` arrays) — replace hardcoded `resolve_theme_name`
- [ ] Tests per theme
- [ ] Font handling (reveal themes `@import url(./fonts/…)`; decide vendor vs link)

### Stage C — parity + extras + docs (own branch)
- [ ] `_brand.yml` integration, callouts/panels/tabsets/`.smaller`, light/dark sentinel
- [ ] Render/preview parity hardening; keep the vendored-asset drift guard meaningful
- [ ] **Docs (D1 caveat):** user-facing guide on the theming variables and a
      "migrating a Quarto-1 revealjs theme to Q2" how-to, incl. the `--r-*`
      runtime escape hatch (docs/ site — render with `q2`, not Q1). Open a doc strand.

## References

- `claude-notes/plans/2026-06-08-revealjs-presentations.md` (Phase 7 deferral)
- `external-sources/quarto-cli/src/core/sass.ts` (layer merge + defaults reversal)
- `external-sources/quarto-cli/src/format/reveal/format-reveal-theme.ts` (bundler, split SCSS compilation)
- `external-sources/quarto-cli/src/resources/formats/revealjs/quarto.scss` (master)
- `external-sources/quarto-cli/src/resources/formats/revealjs/themes/*.scss`
- `external-sources/quarto-cli/llm-docs/sass-theming-architecture.md` (Q1's split SCSS compilation + exposer; the Q1 doc itself calls this "two-pass")
- `external-sources/reveal.js/css/theme/template/{settings,theme,mixins}.scss` (reveal 6)
- Q2: `crates/quarto-core/src/revealjs/`, `crates/quarto-core/src/stage/stages/compile_theme_css.rs`, `crates/quarto-sass/src/{layer,bundle}.rs`, `resources/revealjs/`
