# User theme .scss compile error is swallowed: page silently ships DEFAULT_CSS (bd-jsvetdea)

**Date:** 2026-09-08
**Braid:** bd-jsvetdea (bug, p2, labels: css, diagnostics, theming)
**Checkout:** main checkout, branch `main` @ `b7e7c96a` (no worktree/branch created by the investigation)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The symptom reproduces at HEAD exactly as filed, the
swallow site is a single `Err` arm with an established structured-diagnostic
neighbour (Q-14-4) two screens above it, and the two related strands
(bd-36vmz7nk, bd-qmpygp02) are both resolved by the same change. The open
questions are posture (hard error vs. loud warning) and how much source
location to promise, not feasibility.

## Issue context

Filed 2026-09-08 by Carlos while setting up `_brand.yml` + custom SCSS for
cscheid-net-2026. A theme file containing only

```scss
/*-- scss:defaults --*/
$grid-body-width: 52rem;
```

makes the Bootstrap grid arithmetic fail in grass (`Incompatible units px and
rem`). The theme stage catches the error, emits a `Warn` trace event, and
returns the static ~7KB `DEFAULT_CSS`. The render reports "Rendered 1 of 1
files" with zero warnings; nothing is visible at `-v`, `-vv`, or via
`RUST_LOG`. Brand fonts, colors, and the user's rules are all gone.

## Dependency graph

No `blocks` edges in either direction; no `discovered-from` parent (the
strand *is* the discovery record). Three `related` edges:

- **bd-36vmz7nk** (question, p2, cderv, 2026-07-03) — "Decide: silent
  theme-compile fallback vs hard error." The policy strand. It was scoped out
  of the CRLF layer-marker fix (bd-3fgnmlco, closed) because the fallback had
  masked that bug for months. Two later comments (2026-08-14 light/dark audit,
  2026-09-08 this strand) both push toward a hard error; the light/dark note
  points out the fallback is now *per variant*, so a failing dark compile
  ships light `DEFAULT_CSS` under the dark key.
- **bd-qmpygp02** (bug, p3, 2026-08-08) — `InvalidScssFile` (existing file,
  no layer markers) is swallowed by the same `Err` arm. Its fix shape is the
  same wrapper change this strand needs: stop stringifying `SassError` in
  `compile_with_doc_vars_via_runtime`, match variants, route through
  `theme_diagnostic`. Discovered from bd-of20unsb (format-extension theme
  rebase, still `in_progress`), which added the up-front Q-14-4 check.
- **bd-5fseopxy** (bug, p2, same session) — brand font weight ranges collapse
  to 400; its typography-slot case emits `$font-weight-base: 400..700` and
  then hits *this* fallback, so two silent failures stack. Independent fix,
  but a useful second regression fixture for this strand.

Related plans: `claude-notes/plans/2026-08-08-format-ext-theme-rebase.md`
(Q-14-4 precedent), `claude-notes/plans/2026-08-14-light-dark-theme-epic.md`
(variant_css split, phase E audit).

## What the code looks like today

All paths in the strand still exist; the area was refactored into
`variant_css` by the light/dark epic but the swallow arm is unchanged.

- `crates/quarto-core/src/stage/stages/compile_theme_css.rs:727-736` — the
  `Err` arm: `trace_event!(Warn, "theme CSS compilation failed: {e}, using
  default CSS")` then `Ok(DEFAULT_CSS)`.
- `compile_theme_css.rs:1010-1032` — `compile_with_doc_vars_via_runtime`
  (native + wasm cfgs) maps `SassError` to `String`, discarding the variant.
- `compile_theme_css.rs:617-630` — the *default bundle* path has its own
  identical fallback (`compile_default` failure → `DEFAULT_CSS`).
- `compile_theme_css.rs:355-370` — the reveal path has a third one (compile
  failure → vendored stock theme CSS). Same posture question.
- `compile_theme_css.rs:636-670` — the Q-14-4 precedent: up-front check,
  `SassError::CustomThemeNotFound { location }` →
  `theme_diagnostic::sass_error_to_parse_error` → `PipelineError::Structured`.
- `crates/quarto-sass/src/error.rs:17-19` — `SassError::CompilationFailed {
  message: String }` carries only the stringified grass error; no location
  field, so `with_location` cannot attach a YAML span to it today.
- `crates/quarto-sass/src/compile.rs:263-274` / `547-556` — the grass/dart
  error is stringified into `CompilationFailed` at the crate boundary.
- `crates/quarto-core/src/theme_diagnostic.rs` — handles Q-14-1/2/4; the
  catch-all arm for other variants renders a code-less error. Q-14-3 was
  retired and removed from the catalog; Q-14-5 is the latest code.

### Why nothing surfaces at any verbosity

Two independent gaps, both confirmed by reading the wiring:

1. `RenderContext` defaults to `NoopObserver`
   (`crates/quarto-core/src/render.rs:462`), and the CLI never installs
   another one. The only way to get a real observer is `trace: true` /
   `trace: summary` in document metadata
   (`stage/stages/metadata_merge.rs:505-551`). So `trace_event!(Warn, …)` is
   discarded before any log filter sees it.
2. Even with `TracingObserver` (which maps `Warn` to `tracing::warn!`), the
   `-v` filters are `quarto=warn,q2=warn` etc.
   (`crates/quarto-util/src/verbose.rs:34-41`); those directives match the
   `quarto` and `q2` targets, not `quarto_core`, so a warning emitted from
   `quarto_core::stage::observer` would still be filtered.

This means every "graceful fallback with a warning trace" in the stage is
silent on the CLI, not just this one. Worth its own strand (see open
question 6).

### Repro at HEAD (confirmed 2026-09-08)

Fixture: `claude-notes/plans/theme-scss-compile-error-swallowed-investigation/repro/`
(`theme: [cosmo, theme.scss]`, no brand — the brand is not needed to trigger it).

```
$ cargo run -q --bin q2 -- render claude-notes/plans/theme-scss-compile-error-swallowed-investigation/repro
Rendering project: …/repro (type: website)
Rendered 1 of 1 files to …/repro/_site
$ ls -l repro/_site/site_libs/quarto/*.css
-rw-r--r--  6996  quarto-theme-e6964ef56c8a5065.css      # "styles.css / Default styles for Quarto HTML documents"
```

With `-vv`: only the `render_to_file` debug lines; no mention of the theme.
Control (`$grid-body-width: 830px`): 325166-byte compiled bundle.

With `trace: summary` added to `_quarto.yml`, the swallowed text is:

```
[trace] [warn] theme CSS compilation failed: SASS compilation failed: SASS compilation error: Error: Incompatible units px and rem.
     ╷
3269 │ $grid-body-column-min: quarto-math.min(500px, $grid-body-column-max) !default;
     │                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
     ╵
./stdin:3269:24
, using default CSS
```

**Important nuance for the design:** the strand says the grass error has "a
precise Sass location". It does — but the location is line 3269 of the
*assembled* bundle (`./stdin`), and the failing line is Quarto's own
`$grid-body-column-min` default, not the user's `$grid-body-width` line.
The user's file is not named anywhere in the message. Mapping the assembled
line back to (layer, source file, line) would need provenance bookkeeping in
`quarto_sass::bundle::assemble_scss`, which today just `push_str`s the layer
sections in order. Q1 has the same limitation and deliberately truncates the
dart-sass pointer to its temp file
(`external-sources/quarto-cli/src/core/dart-sass.ts:141-146`), but Q1 *does*
throw `"Theme file compilation failed:\n\n" + errMsg` — a hard error.

### Pre-flight verify

`cargo xtask verify --skip-hub-build` at `b7e7c96a`: Rust build clean,
**13740 Rust tests passed**. The hub-client vitest leg failed with 23 tests
in 6 files, all `localStorage` undefined under Node v26.8.1 — environmental
and unrelated to this strand; filed as **bd-lh30hlvd** (discovered-from this
strand).

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).**
  - Unit tests in `compile_theme_css.rs`: a custom theme whose `defaults`
    break the grid arithmetic (`$grid-body-width: 52rem`) makes
    `stage.run` return `Err(PipelineError::Structured)` with the new code and
    a message containing the grass text (`Incompatible units`); the dark
    variant (`theme: {light: cosmo, dark: [darkly, bad.scss]}`) fails the same
    way and never stores `DEFAULT_CSS` under the dark key; an existing file
    with no layer markers → its own code (folds in bd-qmpygp02); the
    no-theme default bundle path is unchanged.
  - `theme_diagnostic.rs` tests: new codes render with code + YAML span
    (candidate: the `theme:` entry span) and span-less when no location.
  - Integration test driving the real project render with the repro fixture
    (copied under `crates/quarto-core/tests/fixtures/`), asserting the render
    fails, the diagnostic carries the code, and no `DEFAULT_CSS`-sized
    `quarto-theme-*.css` is written.
  - Catalog/docs lint (`cargo xtask lint`) passes with the new codes.
- **Phase 1 — Preserve `SassError` through the wrapper.** Change
  `compile_with_doc_vars_via_runtime` (both cfgs) to
  `Result<String, SassError>`; give `CompilationFailed` a `location:
  Option<SourceInfo>` (and possibly a separate `excerpt`/`sass_location`
  field) so `with_location` works for it.
- **Phase 2 — Route the `Err` arm through `theme_diagnostic`.** Classify:
  compile failure on the themed / brand / doc-vars path → structured hard
  error with the first custom theme entry's span (from
  `theme_locations`) as the location; `InvalidScssFile` → its own code;
  decide the fate of the default-bundle and reveal fallbacks per question 1.
- **Phase 3 — Catalog + docs.** New `Q-14-6` (and `Q-14-7` if split) in
  `error_catalog.json`, pages under `docs/errors/theme/`, sidebar entries in
  `docs/_quarto.yml`. Do not reuse retired `Q-14-3`.
- **Phase 4 — End-to-end verification.** Re-run the repro through `q2
  render`; record the diagnostic output here. Update bd-36vmz7nk with the
  decision and close it; close bd-qmpygp02 if folded in.
- **Phase 5 (optional, per question 2) — Layer provenance.** Track each
  layer's line range in `assemble_scss` so a grass line inside a user layer
  renders as `theme.scss:2`, and a line inside Quarto's layers renders as
  "in Quarto's Bootstrap layer (triggered by your theme's defaults)".

## Open design questions for the user

1. **Posture (resolves bd-36vmz7nk).** Hard error for *any* compile failure
   on the themed / brand / doc-vars path, i.e. whenever user-authored SCSS or
   user-derived variables are in the bundle? I recommend yes (Q1 parity:
   `Theme file compilation failed` is fatal there). And the two other
   fallbacks — the no-user-input default bundle (`compile_default` →
   `DEFAULT_CSS`) and the reveal path (→ vendored stock theme) — can only
   fail on a Quarto bug or a broken install. Keep those as fallbacks but make
   them a real `ctx.add_diagnostic` warning so the summary is not clean, or
   make them hard errors too?
2. **How much location to promise.** grass points at line 3269 of the
   assembled bundle, at a Quarto-internal line, and never names the user's
   file. Minimum viable: the diagnostic carries the grass message + excerpt
   verbatim, with the YAML span of the `theme:` entry (or the whole `theme:`
   list) as its location and a hint that the failing expression may be in
   Quarto's own layer reacting to the theme's variables. Better: Phase 5's
   provenance mapping so user-layer lines render as `theme.scss:N`. Do the
   minimum now and file Phase 5 as a follow-up strand, or do both here?
3. **Codes.** One new code `Q-14-6` "Theme SCSS compilation failed" for grass
   errors, plus `Q-14-7` "Theme file has no layer boundary markers" for
   `InvalidScssFile` / `NoBoundaryMarkers` (folding bd-qmpygp02 into this
   strand since the wrapper change is shared)? Or keep bd-qmpygp02 separate
   and ship only Q-14-6 here?
4. **Which YAML span.** When the bundle has several custom entries
   (`[cosmo, a.scss, b.scss]`) we cannot tell which one caused the failure.
   Point at the first custom entry, at the whole `theme:` value, or emit no
   span and rely on the message? (Q-14-4 points at the exact entry because it
   knows which file is missing; here we do not.)
5. **Multi-file projects.** The stage runs per document, and only successful
   compiles are cached, so a 40-page website would print the same diagnostic
   40 times. Accept for now (Q-14-4 already behaves this way), or dedupe by
   fingerprint at the project level?
6. **The silent-observer gap.** Every `trace_event!(Warn, …)` in the stage
   is invisible on the CLI (no observer installed; `-v` filters do not match
   `quarto_core` anyway). Separate strand to route `Warn` events to
   `tracing::warn!` by default and widen the `-v` filter, or leave the
   observer as a trace-only channel and require `ctx.add_diagnostic` for
   anything user-facing?
7. **Preview / WASM.** The same stage runs in the hub-client preview; a hard
   error there replaces the unstyled-but-rendered page with an error card, as
   Q-14-4 already does. Confirm that is acceptable for the preview too.

## Risks / tradeoffs (draft)

- **Surfacing currently-masked failures.** bd-36vmz7nk warned that flipping
  the fallback may expose other silent compile failures on all platforms
  (that was the reason for scoping it out of the CRLF fix). Rendering the
  docs site and the in-tree fixtures with the change is the audit; anything
  that turns red is a real bug we are shipping unstyled today.
- **Misleading location.** If we attach the `theme:` span without the
  "may be in Quarto's layer" hint, users will stare at a correct-looking
  entry. The message must make the assembled-bundle nature clear.
- **Cache interaction.** Only successful compiles are cached, so no stale-CSS
  risk; but the fingerprinted `quarto-theme-<fp>.css` name means a previously
  good render leaves an old file in `_site/site_libs` that the failed render
  no longer references. Cosmetic.
- **WASM cfg duplication.** The wrapper exists twice (native sync, wasm
  async); both must change identically, and the wasm side cannot be run
  locally on Windows (see `.claude/rules/wasm.md`).
