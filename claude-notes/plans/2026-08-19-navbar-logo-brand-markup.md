# Navbar logo unstyled: theme ships no `.navbar-logo` rule and brand markup drops Q1's `navbar-brand-logo` structure (bd-navbar-logo-unstyled-gbzd8vcu)

**Date:** 2026-08-19
**Braid:** bd-navbar-logo-unstyled-gbzd8vcu
**Checkout:** main @ `f387bd68` (investigation ran in the main checkout; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** Both defects are confirmed at HEAD, the fix sites are localized and well understood (`render_brand` in `crates/quarto-navigation/src/render_html.rs` + the quarto-nav port region of `resources/scss/bootstrap/_bootstrap-rules.scss`), and the Q1 reference markup/CSS has been captured verbatim below. The remaining questions are scope choices (how much of Q1's brand-container CSS to port alongside the markup), not missing information.

## Issue context

Filed 2026-08-19 by Carlos (fresh — no staleness risk), type `bug`, priority 2, label `navigation`, status `open`. Origin strand `br-navbar-logo-unstyled-varhly4s` lives in the connect-docs porting skein (different skein; not resolvable here).

A `website.navbar.logo` renders at the image's natural size — a 512px SVG swallows the navbar — instead of Q1's 24px logo beside the title. Two independent defects:

1. **No default sizing rule.** Q1 ships `.navbar-logo { max-height: 24px; width: auto; padding-right: 4px }` (`quarto-cli src/resources/projects/website/navigation/quarto-nav.scss:196`). q2 emits `class="navbar-logo"` on the img but no stylesheet anywhere targets it.
2. **Brand markup structure differs**, killing the Q1 user-CSS override path (`.navbar-brand-logo .navbar-logo { … }` selectors silently stop matching). Real-world hit: the Posit Connect docs.

## Dependency graph

**Empty in this skein** — `braid dep tree` shows only the strand itself; `braid dep list` shows no edges. The discovered-from context lives in the external connect-docs skein (`br-navbar-logo-unstyled-varhly4s`), summarized in the description. No incoming `blocks` pressure; urgency comes from the Connect-docs porting effort, not from other q2 strands.

## What the code looks like today

All paths in the description check out at HEAD (`f387bd68`):

- **Markup emission:** `render_brand` at `crates/quarto-navigation/src/render_html.rs:497-543` inlines the logo img into the single title anchor:
  ```html
  <a class="navbar-brand" href="./"><img src="logo.svg" alt="…" class="navbar-logo"> Title</a>
  ```
  Called from `navbar_to_html` (line 87), inside a plain `<div class="container-fluid">` (line 84; Q1 uses `navbar-container container-fluid`).
- **Navbar model:** `crates/quarto-navigation/src/navbar.rs:106-117` — `logo: Option<String>` + `logo_alt`/`logo_href` + paired `SourceInfo` fields. No light/dark variant support (Q1's `logo.light`/`logo.dark` normalizes richer YAML shapes).
- **Styles:** `resources/scss/bootstrap/_bootstrap-rules.scss` is where Q1's `quarto-nav.scss` rules are ported piecemeal, each with a Q1 source-line comment (e.g. lines 353, 412, 478, 572; navbar block at ~2377). **No `.navbar-logo`, `.navbar-brand-container`, or `.navbar-brand-logo` rule exists anywhere in `crates/` or `resources/`** — the only `navbar-logo` hits are tests asserting the img tag is emitted, plus the logo-copy machinery (`copy_navbar_logo` in `website_post_render.rs`, href rebasing in `navbar_render.rs` / `metadata_path_resolution.rs`), which is orthogonal and works.

### Q1 reference (captured from external-sources/quarto-cli)

`navbrand.ejs` — the target markup shape:

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

Notable deltas vs. q2 today: separate logo anchor with hook classes; title wrapped in `<span class="navbar-title">` (q2 emits no such span); container div with `mx-auto`; **Q1's title anchor href is `logo-href || '/index.html'`** while q2 falls back to `home_url` (relative `./`-style). q2's relative-href behavior is arguably better (root-relative-paths work, bd-root-relative-paths-design-fc5pvkcv); the fallback semantics need a decision, not blind copying.

`quarto-nav.scss` — CSS that accompanies the structure (lines 116-170 + 196):

```scss
.navbar-container { width: 100%; }
.navbar-brand { overflow: hidden; text-overflow: ellipsis; }
.navbar-brand-container {
  max-width: calc(100% - 115px);
  min-width: 0;
  display: flex;
  align-items: center;
  @include media-breakpoint-up(lg) { margin-right: 1em; }
}
.navbar-brand.navbar-brand-logo { margin-right: 4px; display: inline-flex; }
.navbar .navbar-brand-container { order: $navbar-title-order; }
.navbar .navbar-container > .navbar-brand-container { margin-left: 0 !important; margin-right: 0 !important; }
.navbar-logo { max-height: 24px; width: auto; padding-right: 4px; }
```

(The `order:` rules belong to Q1's navbar-component ordering system — `$navbar-title-order` etc. — which q2 has not ported; probably out of scope.)

### Repro at HEAD

Local fixture: `claude-notes/plans/navbar-logo-brand-markup-investigation/repro/` (mirrors the strand's external repro at `~/repos/github/cscheid/q2-connect-docs/llms-info/repros/navbar-logo-unstyled/`). Render with `cargo run --bin q2 -- render <fixture-dir>`; confirm via grep that the generated `site_libs/quarto/quarto-theme-*.css` has no `navbar-logo` rule and the HTML has the inlined-img brand shape. (Confirmed at f387bd68 — see investigation notes.)

## Proposed phases (draft)

Skeleton only — actual phase contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).** Failing tests first:
  - `render_html.rs` unit tests asserting the new brand markup shape (container div, separate `navbar-brand-logo` anchor, `navbar-title` span, logo+title / logo-only / title-only / neither cases).
  - An end-to-end website render test asserting the compiled theme CSS contains a `.navbar-logo` `max-height` rule (route through the real render path per end-to-end policy).
  - Inventory of existing assertions that will need updating: `render_html.rs:1236-1294` unit tests, `navbar_render.rs:872-984`, `shortcode_config_pipeline.rs` (4 sites), `navbar_footer_pipeline.rs`, `metadata_path_resolution.rs`, plus any HTML snapshots containing `navbar-brand`.
- **Phase 1 — SCSS default rules.** Add `.navbar-logo` sizing + the brand-container/brand-logo companion rules to the quarto-nav port region of `_bootstrap-rules.scss`, with Q1 source-line comments per house style.
- **Phase 2 — Brand markup restructure.** Rework `render_brand` to emit Q1's container + dual-anchor shape; update `navbar_to_html` container class if we adopt `navbar-container` (design question 3).
- **Phase 3 — End-to-end verification.** Render the investigation fixture, inspect HTML + theme CSS, record invocation + output snippet per repo policy; ideally eyeball in a browser.
- **Phase 4 — Docs.** Check `docs/` website navbar-logo docs mention the override hooks; note light/dark variant follow-up strand.

## Open design questions for the user

1. **How much companion CSS to port?** Just `.navbar-logo` + the minimal `.navbar-brand-container` / `.navbar-brand.navbar-brand-logo` rules that make the new structure lay out correctly — or also `.navbar-brand { overflow: hidden; text-overflow: ellipsis }`, `.navbar-container { width: 100% }`, and the `max-width: calc(100% - 115px)` clamp? (The clamp interacts with search/tools q2 may not render the same way.)
2. **Q1's navbar-ordering system (`$navbar-title-order` etc.)** governs the `order:`/`margin !important` rules on `.navbar-brand-container`. Port those two rules verbatim, or skip the ordering system entirely for now (my lean: skip; q2 has no toggle-position option)?
3. **`navbar-container` class:** q2 emits bare `container-fluid`; Q1 emits `navbar-container container-fluid` and user CSS may key on it too. Add it in the same change (my lean: yes, it's one string) or keep scope to the brand?
4. **Title-anchor href fallback:** Q1 uses `logo-href || '/index.html'` for *both* anchors; q2 currently uses `logo_href || home_url` (relative). Keep q2's relative fallback for both anchors (my lean: yes — consistent with the root-relative-paths design), matching Q1's class hooks but not its absolute-path fallback?
5. **Light/dark logo variants (`logo.light`/`logo.dark`, `light-content`/`dark-content` imgs):** the strand suggests deferring to general dark-mode support. File a follow-up strand linked `related`, and emit a single variant-less `<img class="navbar-logo">` inside the new anchor for now?

## Risks / tradeoffs (draft)

- **Snapshot/test churn:** every test asserting `<a class="navbar-brand" href="…">Title</a>` breaks; the inventory above looks complete but a workspace-wide grep after the change is mandatory.
- **`mx-auto` on the container** centers the brand when the navbar has free space; combined with q2's possibly-different navbar flex layout (no ported ordering rules) this could visibly move the title. Phase 3's browser eyeball is the guard.
- **Preview parity:** the SPA preview path renders navbars through the same `quarto-navigation` code, but theme CSS delivery differs — verify preview picks up the new rules (and remember the WASM rebuild chain if checking via `q2 preview`).
- Defect 1 alone (the SCSS rule) already fixes the "512px logo swallows the navbar" symptom; defect 2 is what un-breaks user overrides. If we want a minimal ship, Phase 1 is independently landable.
