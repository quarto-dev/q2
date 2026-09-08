# User theme .scss compile error is swallowed: page silently ships DEFAULT_CSS (bd-jsvetdea)

**Date:** 2026-09-08
**Braid:** bd-jsvetdea (bug, p2, labels: css, diagnostics, theming). Folds in bd-qmpygp02; resolves bd-36vmz7nk.
**Checkout:** main checkout, branch `main` @ `b7e7c96a`
**Status:** Design settled with user 2026-09-08 (answers recorded below). Implemented and verified 2026-09-08; strands closed.

## Overview

A grass failure while compiling the theme bundle is caught in
`variant_css` (`crates/quarto-core/src/stage/stages/compile_theme_css.rs`),
logged as a `Warn` trace event, and replaced by the static ~7KB
`DEFAULT_CSS`. The render reports success. The trace event goes to a
`NoopObserver`, so nothing reaches the CLI at any verbosity. The same
`Err` arm swallows `InvalidScssFile` (bd-qmpygp02), and the per-variant
split means a failing dark compile ships *light* default CSS under the dark
key.

Fix: every theme compile failure becomes a structured hard error through
`theme_diagnostic`, with two new codes. `DEFAULT_CSS` remains only for the
explicit `theme: none` opt-out.

## Design decisions (user answers, 2026-09-08)

1. **Posture.** Compile errors are fatal. That includes the no-user-input
   default bundle and the reveal path: those can only fail on a Quarto bug
   or broken install, and a hard error is the right signal. Resolves
   bd-36vmz7nk.
2. **Location.** Minimum viable now: the diagnostic carries the grass
   message and excerpt verbatim, anchored at the whole `theme:` value, with
   a hint that the reported line is in Quarto's assembled bundle and may sit
   in Quarto's own layer reacting to a theme variable. Layer-provenance line
   mapping (assembled line → user file:line) is a follow-up strand; the
   cross-tool source-tracking infrastructure is not there yet.
3. **Codes.** `Q-14-6` "Theme SCSS compilation failed" (grass / dart-sass
   errors and any other unclassified compile-path `SassError`), and
   `Q-14-7` "Theme file has no layer boundary markers" (`InvalidScssFile`,
   folding bd-qmpygp02 in). `Q-14-3` is retired and is not reused.
4. **Span.** Point at the whole `theme:` value. For `Q-14-7` we know the
   offending path, so point at that entry when it can be matched against
   `theme_locations`, falling back to the whole value.
5. **Multi-file projects.** `quarto-error-reporting::coalesce_by_source`
   (bd-9hlja) already groups structured pass2 failures by resolved source
   location, and `q2 render` runs it over `pass2_failures`. A `_quarto.yml`
   span therefore collapses N pages into one report with an "Affected files"
   tail. Phase 4 verifies this end to end; if it does not coalesce, file a
   follow-up rather than hand-rolling dedup here.
6. **Silent-observer gap.** Keep `trace_event!` as a trace-only channel and
   use `ctx.add_diagnostic` for anything user-facing. The noise concern is
   not really about volume (only 10 `Warn` sites exist, each fires only on a
   failure); the stronger reason is that `add_diagnostic` feeds the summary
   counts, `--warnings-as-errors`, JSON output, and coalescing, none of
   which a routed `tracing::warn!` would. Follow-up strand: audit the
   remaining `Warn` sites (bootstrap_js / tabsets_js "skipping", capture
   splice parse failures) and the `-v` filter mismatch (`quarto=warn` does
   not match `quarto_core` targets).
7. **Preview / WASM.** A hard error in the hub-client preview replaces the
   page with an error card, as Q-14-4 already does. Accepted.

## Work items

### Phase 0 — Tests (TDD; written first, verified failing)

- [x] `theme_diagnostic.rs`: `Q-14-6` renders with code, grass text, and a
      span at a `_quarto.yml` `theme:` value; span-less without a location.
- [x] `theme_diagnostic.rs`: `Q-14-7` renders with code, the file path, and
      a span; span-less without a location.
- [x] `theme_diagnostic.rs`: catalog-registration test covers the new codes.
- [x] `compile_theme_css.rs` unit: `theme: [cosmo, bad.scss]` where
      `bad.scss` sets `$grid-body-width: 52rem` → `Err(Structured)` with
      `Q-14-6`, message contains `Incompatible units`, no `css:theme:*`
      artifact stored.
- [x] `compile_theme_css.rs` unit: dark variant (`light: cosmo`,
      `dark: [darkly, bad.scss]`) → same error; no `css:theme-dark:*`
      artifact and no light artifact either.
- [x] `compile_theme_css.rs` unit: an existing `.scss` with no layer markers
      → `Err(Structured)` with `Q-14-7` naming the file.
- [x] CLI e2e `crates/quarto/tests/integration/theme_compile_error.rs`
      (registered in `main.rs`): single doc → exit non-zero, `Q-14-6` and
      `Incompatible units` on stderr, no `doc.html`; no-markers file →
      `Q-14-7`; two-page website with the theme in `_quarto.yml` → the grass
      excerpt appears exactly once on stderr (coalesced).
- [x] Run the new tests; confirm each fails for the expected reason
      (exit 0 / `Ok(DEFAULT_CSS)` / code missing).

### Phase 1 — Preserve `SassError` through the wrappers

- [x] `compile_with_doc_vars_via_runtime`, `compile_default`,
      `compile_reveal` (both cfgs each) return `Result<String, SassError>`.
- [x] `SassError::InvalidScssFile` gains `location: Option<SourceInfo>`;
      `with_location` covers it; the one constructor in `themes.rs` passes
      `None`.
- [x] `theme_diagnostic`: new arms for `InvalidScssFile` (`Q-14-7`) and the
      compile-failure catch-all (`Q-14-6`), plus a
      `sass_error_to_parse_error_at(err, fallback_location, candidates)`
      entry point so the stage can anchor location-less variants at the
      `theme:` value without adding a field to every `CompilationFailed`
      constructor (32 sites).

### Phase 2 — Route every compile `Err` arm to a structured error

- [x] `run` computes the whole-`theme:` location from
      `doc.ast.meta.get("theme")` and passes it into `variant_css` and the
      reveal branch.
- [x] `variant_css` themed path: `Err` → `PipelineError::Structured` via
      `theme_diagnostic`; for `InvalidScssFile`, match the path against the
      variant's custom entries to pick the precise `theme_locations[i]`.
- [x] `variant_css` default-bundle path: `compile_default` `Err` →
      `Q-14-6` hard error (hint says no user theme is configured, likely a
      Quarto bug).
- [x] Reveal branch: `compile_reveal` `Err` → `Q-14-6` hard error instead of
      the vendored stock theme.
- [x] Remove the now-dead `DEFAULT_CSS` fallbacks and update the doc
      comments on `variant_css` / the module header.
- [x] `cargo build --workspace` clean; `cargo xtask lint` clean.

### Phase 3 — Catalog + docs

- [x] `error_catalog.json`: `Q-14-6`, `Q-14-7` (subsystem `theme`).
- [x] `docs/errors/theme/Q-14-6.qmd`, `Q-14-7.qmd` (template in
      `docs/errors/README.md`).
- [x] `docs/_quarto.yml` errors sidebar: add both entries in code order.
- [x] `cargo xtask lint` (error-docs-page-missing, error-docs-sidebar-unlisted).

### Phase 4 — End-to-end verification

- [x] `cargo run --bin q2 -- render <repro>` fails with `Q-14-6`; record the
      stderr in this plan.
- [x] Two-page website repro: the diagnostic appears once with an "Affected
      files" tail (coalescing works) — or file a follow-up.
- [x] Render `docs/` with Q2 to check nothing currently masked turns red
      (bd-36vmz7nk's audit concern). 271 of 271 pages render; the 36
      warnings are pre-existing Q-13-4 missing-link warnings, no Q-14-6.
- [x] `cargo nextest run --workspace` (13753 passed); clippy clean;
      `cargo xtask lint` clean; hub-client `npm run build:all` (WASM leg)
      succeeded. The hub-client vitest leg of `cargo xtask verify` is red
      for the unrelated Node 26 `localStorage` issue (bd-lh30hlvd).
- [x] Update bd-36vmz7nk (decision: fatal) and close; close bd-qmpygp02;
      close bd-jsvetdea.

### Follow-ups to file

- [x] Layer provenance: map grass's assembled-bundle line back to
      (layer, user file, line) so `Q-14-6` can say `theme.scss:2`.
      Filed as bd-c1gf9xmk.
- [x] Audit the remaining `trace_event!(Warn, …)` sites for user-facing
      conditions that should be `ctx.add_diagnostic`; fix the `-v` filter
      so `quarto_core` warnings are visible. Filed as bd-e1psgogt.

## End-to-end verification record (2026-09-08)

Invocation, after the fix, on the repro fixture (`theme: [cosmo, theme.scss]`,
`$grid-body-width: 52rem`):

```
$ cargo run --bin q2 -- render claude-notes/plans/theme-scss-compile-error-swallowed-investigation/repro
Rendering project: …/repro (type: website)
error: while rendering …/repro/index.qmd
Error: [Q-14-6] Theme SCSS compilation failed
   ╭─[ …/repro/_quarto.yml:5:12 ]
   │
 5 │     theme: [cosmo, theme.scss]
   │            ─────────┬─────────
   │                     ╰─────────── compiling the theme SCSS bundle failed:
Error: Incompatible units px and rem.
     ╷
3269 │ $grid-body-column-min: quarto-math.min(500px, $grid-body-column-max) !default;
     │                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
     ╵
./stdin:3269:24
───╯
ℹ Does a variable in a theme file set a value Bootstrap's arithmetic cannot use (…)? …

Rendered 0 of 1 files to …/repro/_site — 1 error
$ echo $?
1
```

`_site/site_libs/quarto/` contains no `quarto-theme-*.css` (previously a
6996-byte `DEFAULT_CSS`). Output inspected.

Two-page website (same `_quarto.yml`, `index.qmd` + `about.qmd`): the block
above prints **once**, followed by

```
Affected files: …/two/about.qmd, …/two/index.qmd
Rendered 0 of 2 files to …/two/_site — 2 errors
```

so the source-location coalescing (bd-9hlja) covers this diagnostic with no
extra work; no follow-up needed for design question 5.

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
  same wrapper change this strand needs. Discovered from bd-of20unsb
  (format-extension theme rebase, still `in_progress`), which added the
  up-front Q-14-4 check.
- **bd-5fseopxy** (bug, p2, same session) — brand font weight ranges collapse
  to 400; its typography-slot case emits `$font-weight-base: 400..700` and
  then hits *this* fallback, so two silent failures stack. Independent fix.

Related plans: `claude-notes/plans/2026-08-08-format-ext-theme-rebase.md`
(Q-14-4 precedent), `claude-notes/plans/2026-08-14-light-dark-theme-epic.md`
(variant_css split, phase E audit).

## What the code looked like at investigation time

- `compile_theme_css.rs:727-736` — the `Err` arm: `trace_event!(Warn, "theme
  CSS compilation failed: {e}, using default CSS")` then `Ok(DEFAULT_CSS)`.
- `compile_theme_css.rs:1010-1032` — `compile_with_doc_vars_via_runtime`
  (native + wasm cfgs) maps `SassError` to `String`, discarding the variant.
- `compile_theme_css.rs:617-630` — the *default bundle* path has its own
  identical fallback (`compile_default` failure → `DEFAULT_CSS`).
- `compile_theme_css.rs:355-370` — the reveal path has a third one (compile
  failure → vendored stock theme CSS).
- `compile_theme_css.rs:636-670` — the Q-14-4 precedent: up-front check,
  `SassError::CustomThemeNotFound { location }` →
  `theme_diagnostic::sass_error_to_parse_error` → `PipelineError::Structured`.
- `crates/quarto-sass/src/error.rs:17-19` — `SassError::CompilationFailed {
  message: String }` carries only the stringified grass error.
- `crates/quarto-core/src/theme_diagnostic.rs` — handles Q-14-1/2/4; the
  catch-all arm renders a code-less error.

### Why nothing surfaces at any verbosity

1. `RenderContext` defaults to `NoopObserver`
   (`crates/quarto-core/src/render.rs:462`), and the CLI never installs
   another one. The only way to get a real observer is `trace: true` /
   `trace: summary` in document metadata
   (`stage/stages/metadata_merge.rs:505-551`).
2. Even with `TracingObserver`, the `-v` filters are `quarto=warn,q2=warn`
   etc. (`crates/quarto-util/src/verbose.rs:34-41`); those match the
   `quarto` and `q2` targets, not `quarto_core`.

### Repro at HEAD (confirmed 2026-09-08)

Fixture: `claude-notes/plans/theme-scss-compile-error-swallowed-investigation/repro/`.

```
$ cargo run -q --bin q2 -- render claude-notes/plans/theme-scss-compile-error-swallowed-investigation/repro
Rendering project: …/repro (type: website)
Rendered 1 of 1 files to …/repro/_site
$ ls -l repro/_site/site_libs/quarto/*.css
-rw-r--r--  6996  quarto-theme-e6964ef56c8a5065.css      # "styles.css / Default styles for Quarto HTML documents"
```

Control (`$grid-body-width: 830px`): 325166-byte compiled bundle. With
`trace: summary` in `_quarto.yml`, the swallowed text is:

```
[trace] [warn] theme CSS compilation failed: SASS compilation failed: SASS compilation error: Error: Incompatible units px and rem.
     ╷
3269 │ $grid-body-column-min: quarto-math.min(500px, $grid-body-column-max) !default;
     │                        ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
     ╵
./stdin:3269:24
, using default CSS
```

The location is line 3269 of the *assembled* bundle (`./stdin`), at Quarto's
own `$grid-body-column-min` default; the user's file is never named. Q1 has
the same limitation and truncates the dart-sass pointer
(`external-sources/quarto-cli/src/core/dart-sass.ts:141-146`) but throws
`"Theme file compilation failed:\n\n" + errMsg` — a hard error.

### Pre-flight verify

`cargo xtask verify --skip-hub-build` at `b7e7c96a`: Rust build clean,
13740 Rust tests passed. The hub-client vitest leg failed with 23 tests in
6 files, all `localStorage` undefined under Node v26.8.1 — environmental
and unrelated; filed as bd-lh30hlvd.

## Risks / tradeoffs

- **Surfacing currently-masked failures.** Flipping the fallback may expose
  other silent compile failures. Rendering `docs/` and the in-tree fixtures
  is the audit (Phase 4); anything that turns red is a real bug shipping
  unstyled today.
- **Misleading location.** The `theme:` span is where the user *configured*
  the theme, not where the failure is. The hint must make the
  assembled-bundle nature clear.
- **Cache interaction.** Only successful compiles are cached, so no
  stale-CSS risk.
- **WASM cfg duplication.** Each wrapper exists twice; both must change
  identically.
