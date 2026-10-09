---
title: 'Theme-config diagnostic overhaul'
date: 2026-05-22
description: 'Reports the unsupported `theme` map shape in `_quarto.yml` once with a source-located diagnostic, instead of repeating a plain error on every rendered page.'
status: draft  # drafting — pending user review
braid:
  strand: bd-l26u6  # parent epic
  priority: P2
  labels: [diagnostics, theme, website]
---

**Children:**
- [theme diagnostic — structured](2026-05-22-theme-diagnostic-structured.md)
- [cross-page diagnostic coalescing](2026-05-22-diagnostic-coalescing.md)

## Motivation

Reproducer: `cargo run --bin q2 -- render external-sources/quarto-web`.

The user's `_quarto.yml` uses the unsupported `theme: {light: [...], dark: [...]}`
map shape. Today that produces, for every single page in the project:

```
error: /…/external-sources/quarto-web/404.qmd: Error: Invalid theme configuration: theme must be a string or array of strings
error: /…/external-sources/quarto-web/about.qmd: Error: Invalid theme configuration: theme must be a string or array of strings
…hundreds more…
```

Two problems:

1. **Wrong renderer.** The other configuration / parse errors in the same run
   (`[Q-2-39] Grid tables are not supported`, etc.) are formatted via the
   structured `DiagnosticMessage` → ariadne path and include an annotated
   source-span pointing at the offending YAML/markdown. The theme error
   takes the plain `eprintln!("error: {}: {}", path, err)` path
   (`crates/quarto/src/commands/render.rs:716`) and carries no span — the
   user has no idea which file the offending `theme:` lives in.

2. **N copies of one root cause.** Theme config is resolved per-page (each
   page's merged metadata is re-validated by `CompileThemeCssStage`), so
   one bad key in `_quarto.yml` produces one error per rendered page. For
   quarto-web that's hundreds of identical lines.

The user explicitly said: don't fix the underlying limitation (no
light/dark support) right now. The goal is to fix how the diagnostic is
*reported*, not the parser's coverage.

## Scope

Two independent pieces of work, tracked as children of this epic:

1. **Structured diagnostic for the theme error.** Convert the
   `SassError::InvalidThemeConfig` → `PipelineError::stage_error(…, e.to_string())`
   chain in `CompileThemeCssStage` so the error reaches the CLI as a
   `DiagnosticMessage` with:
   - a `Q-X-Y` code (allocated under a new `sass`/`theme` subsystem),
   - a `SourceInfo` pointing at the offending `theme:` value in
     `_quarto.yml` (the offending `ConfigValue` already carries
     `source_info` — see `crates/quarto-pandoc-types/src/config_value.rs:150-159`),
   - ariadne-rendered output at the CLI.

   See child plan: `2026-05-22-theme-diagnostic-structured.md`.

2. **Cross-page diagnostic coalescing.** Add a reporting-layer pass that
   groups per-page diagnostics whose underlying cause is the same source
   location (so all hundreds of pages collapse into one report). The
   render summary's `pass2_failures: Vec<FileFailure>` already carries
   per-page input paths and diagnostics, but the CLI prints each one
   verbatim today. The goal is one diagnostic block per distinct
   (code, source-location) tuple, listing the affected pages — e.g.

   ```
   Error: [Q-X-Y] Invalid theme configuration
      ╭─[ /…/_quarto.yml:685:5 ]
   685 │     theme:
       │     ─┬───
       │      ╰── theme must be a string or array of strings
       ╰─
   Affected: 404.qmd, about.qmd, docs/advanced/index.qmd (and 247 others)
   ```

   See child plan: `2026-05-22-diagnostic-coalescing.md`.

## Sequencing

The two children are **independent in implementation**:

- Child 1 only changes how the theme error is constructed and
  propagated. It still emits N copies; it just makes each copy look
  like the rest of our errors.
- Child 2 only changes how the CLI renders the render summary. It
  groups diagnostics that already exist as structured `DiagnosticMessage`s.

They are **synergistic in user-visible value**: Child 1 alone gives a
prettier error N times; Child 2 alone has nothing to coalesce until the
theme error is structured. **Ship Child 1 first**, then Child 2 — the
coalescing test fixture can be `quarto-web` itself.

## Out of scope

- Adding `light:` / `dark:` theme-map support to the parser. The user
  has confirmed this is *not* what we're solving.
- General `SassError` → `DiagnosticMessage` cleanup. Child 1 picks the
  one error variant that quarto-web trips; other `SassError` variants
  (brand-token, unknown-builtin, etc.) can follow the same pattern in
  later issues but are not part of this epic.
- A `--quiet`/`-W` style verbosity knob for diagnostics. Coalescing
  reduces noise without needing one.

## Decisions (resolved 2026-05-22)

1. **Coalescing key = source location.** Just the canonical
   location, not `(code, location, title)`. Two diagnostics pointing
   at the same span are presumed the same error. If a real collision
   ever shows up (two unrelated checks coincidentally landing on the
   same span), we'll revisit; the v1 cost of the simpler key is low.
2. **Code-allocation subsystem = `theme`.** Not `sass` — naming
   the user-facing concept, not the current implementation
   technology, so we can swap sass out later without renaming.
3. **`SourceInfo::Concat` / `FilterProvenance` opt out of
   coalescing** in v1, passing through as singletons.
4. **Display cap and order**: implementer's call for v1; iterate
   once it lands. Not a bikeshed-target.
5. **Crate-dependency boundary.** The coalescer lives in
   `quarto-error-reporting`, which already depends on
   `quarto-source-map` (for the existing
   `DiagnosticMessage.location` field). The coalescer must not pull
   in `quarto-pandoc-types`, `quarto-core`, or `quarto`. Its public
   input type is `(PathBuf, DiagnosticMessage, Option<SourceContext>)`
   — all `std` + own-crate + already-existing dep.
6. **Two issues, single merge stream.** No PRs; we merge each
   directly to `main`. Issues remain separate so the work is
   reviewable in two chunks, but commits flow continuously.
