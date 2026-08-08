# Format-extension theme/css paths not rebased: contributes.formats.html.theme silently drops bundled SCSS (bd-of20unsb)

**Date:** 2026-08-08
**Braid:** bd-of20unsb (P2, bug)
**Checkout:** main worktree at `main` @ `e5fc4ffb` (post-merge of PR #474)
**Status:** Implemented 2026-08-08, all phases complete on local `main`
(commits `e2cda047` → docs). Full `cargo xtask verify` green (incl. WASM
leg). **Not pushed — awaiting user review + push approval.** Follow-up
filed: bd-qmpygp02 (`InvalidScssFile` silent fallback).

## Triage verdict

**Ready to design.** The bug reproduces at HEAD exactly as filed, the root cause is
pinned to two specific code sites, and the machinery the candidate fix wants to
reuse (bd-ad7i1pc6 Phase 4's existence-driven rebase) merged with PR #474 today.
The remaining decisions are scoping/severity questions, listed at the bottom.

## Issue context

Filed today (2026-08-08) by Carlos, discovered during bd-ad7i1pc6 Phase 4
verification (custom project types, PR #474 — now merged). A format extension
declaring

```yaml
contributes:
  formats:
    html:
      theme: [cosmo, fmt-theme.scss]
```

with `fmt-theme.scss` bundled next to `_extension.yml` silently loses the theme:
the bundled SCSS never reaches the compiled CSS and no diagnostic is emitted.
Pre-existing gap, independent of PR #474's changes (that PR fixed the analogous
problem for `contributes.project` fragments only).

## Dependency graph

- **discovered-from: bd-ad7i1pc6** (in_progress, P1) — the custom-project-types
  epic. Its Phase 4 built exactly the machinery this strand wants to reuse:
  `rebase_fragment_paths` / `rebase_candidate` / `FRAGMENT_PATH_PATTERNS` in
  `crates/quarto-core/src/project/mod.rs:681-814`, an existence-driven,
  key-path-pattern-guarded rebase of extension-bundled paths. PR #474 merged
  2026-08-08 20:42Z, so main now has it.
- **Sibling strand worth knowing about:** bd-o76p01wb (P1, from the same
  session) — the `theme: {light: […], dark: […]}` map form is a separate Q2 gap.
  Whatever we do here for nested theme values only matters once that lands, but
  handling map leaves costs nothing (the Phase 4 walker already does it).
- No incoming `blocks` edges — nothing is waiting on this.

## What the code looks like today

### Reproduction (confirmed at `e5fc4ffb`)

Fixture: `claude-notes/plans/format-ext-theme-rebase-investigation/repro/`
(single `doc.qmd` with `format: fancyfmt-html`; `_extensions/fancyfmt/`
contributes `theme: [cosmo, fmt-theme.scss]` with the SCSS bundled next to
`_extension.yml`, carrying a `.fmt-theme-marker` rule).

```
$ cargo run --bin q2 -- render doc.qmd
Rendering single file: .../repro/doc.qmd          # no warning, no error
$ grep -c fmt-theme-marker doc_files/styles.css
0
$ wc -c doc_files/styles.css
6996                                              # DEFAULT_CSS fallback
```

Two findings beyond the strand description:

1. **The failure is worse than "drops the bundled SCSS": the *entire* theme
   list is dropped.** `styles.css` is the 7 KB static `DEFAULT_CSS` — even the
   valid `cosmo` entry is gone (a compiled cosmo is ~321 KB). One bad entry
   nukes the whole theme.
2. **The warning is invisible even with `-v`.** The fallback site logs via
   `trace_event!(EventLevel::Warn, …)`, which routes to `tracing::warn!`;
   nothing surfaced in the `-v` render output.

Control: copying `fmt-theme.scss` next to `doc.qmd` produces a 321 KB
`styles.css` containing `.fmt-theme-marker` — confirming the theme stage
resolves custom theme paths relative to the *document* directory, and the
missing piece is exactly the ext-dir → doc-dir rebase.

### Root cause chain

1. **Read side** — `crates/quarto-core/src/extension/read.rs:218`:
   `PATH_VALUED_KEYS = ["template", "template-partials", "shortcodes"]` (plus
   special-cased `filters`). `mark_path_valued_keys` flips only those keys'
   string values to `ConfigValueKind::Path`. `theme`, `css`, `include-*` etc.
   stay `Scalar`.
2. **Merge side** — `crates/quarto-core/src/stage/stages/metadata_merge.rs:268-273`:
   the extension format layer is rebased via
   `adjust_paths_to_document_dir(&mut config, &ext_dir, &document_dir)`, which
   (`project/mod.rs:227-270`) only touches `Path`-kind values. Scalar
   `fmt-theme.scss` passes through unrebased.
3. **Consume side** — `quarto-sass` parses `fmt-theme.scss` as
   `ThemeSpec::Custom` (extension-based sniffing, `themes.rs:259-279`),
   resolves it against the document dir, and `load_custom_theme`
   (`themes.rs:573-615`) returns `SassError::CustomThemeNotFound`.
4. **Silent drop** — `crates/quarto-core/src/stage/stages/compile_theme_css.rs:560-568`:
   any compile error → `trace_event!(Warn, …)` + `store_default_css(ctx)`.
   Not a user-facing diagnostic; the whole theme (including valid entries)
   falls back to `DEFAULT_CSS`.

### The machinery to reuse (bd-ad7i1pc6 Phase 4)

`project/mod.rs:681-814`: `FRAGMENT_PATH_PATTERNS` (a key-path pattern table —
`["format", "*", "theme"]`, `…"css"`, `…"include-in-header"`, etc.; arrays
transparent; pattern exhaustion rebases every string leaf underneath, which is
what makes `theme: {light: […], dark: […]}` work), `rebase_fragment_paths`
(pattern walk), and `rebase_candidate` (the *existence check*: a string is a
bundled file exactly when it exists under the extension dir — builtin names
like `cosmo` simply don't exist there and pass through verbatim).

**Key difference for this strand:** `contributes.project` fragments merge into
*project config once*, so Phase 4 rewrites strings to project-root-relative
`Path`s at parse time. `contributes.formats` layers merge *per document*, and
`metadata_merge.rs:270` already rebases `Path`-kind values ext-dir → doc-dir.
So here we do **not** need to rewrite the string at all — we only need to
**mark** bundled-file strings as `ConfigValueKind::Path` (existence-driven,
leaving the value ext-dir-relative), and the existing merge machinery does the
rest. That is a strictly smaller intervention than Phase 4's.

## Proposed fix (draft)

Two independent halves, matching the strand's candidate fix:

**Half A — existence-driven Path-marking for `contributes.formats`.**
At read time (`extension/read.rs`), after the common-merge in `parse_formats`,
walk each format's config with a pattern table equivalent to the `format.*.…`
subset of `FRAGMENT_PATH_PATTERNS` (`theme`, `css`, `include-in-header`,
`include-before-body`, `include-after-body`, `format-resources`) and flip
string leaves to `Path` **iff the file exists under the extension dir**
(thread `runtime` into `parse_contributes` — already available in
`read_extension_with_org`). `template`/`template-partials`/`shortcodes`/
`filters` keep their current unconditional marking. Likely refactor: extract
the pattern-walk + existence-check core from `project/mod.rs` into a shared
helper (parameterized by pattern table and "mark only" vs "rewrite to
project-relative"), so the two sites can't drift.

**Half B — a real diagnostic instead of the silent DEFAULT_CSS fallback.**
`compile_theme_css.rs:560-568` currently treats every compile error the same.
Split config-shaped errors (`CustomThemeNotFound`, `InvalidScssFile`) from
internal compiler failures: config-shaped errors get a structured ariadne
diagnostic via the existing `theme_diagnostic` infrastructure (new Q-14-x code
in `quarto-error-catalog`, pointing at the offending `theme:` entry — the
`from_config_value` error path at `compile_theme_css.rs:368-388` is the
precedent). Per design decision 2, these **fail the render** (hard error).

## Design decisions (aligned with user, 2026-08-08)

1. **Key set: the full `format.*` subset of `FRAGMENT_PATH_PATTERNS`** —
   `theme`, `css`, `include-in-header`, `include-before-body`,
   `include-after-body`, `format-resources`.
2. **Dangling theme entry → hard error** with an ariadne span. Rationale
   (user): Q1 guessed at user intent because it lacked source-mapping infra;
   Q2 is deliberately stricter because it can produce precise, idiomatic
   errors. This intentionally changes behavior for configs that are already
   broken-but-silent today (they currently get DEFAULT_CSS).
3. **"One bad entry nukes valid entries" is moot** under hard-error; nothing
   further to file.
4. **Legacy keys (`template`/`template-partials`/`shortcodes`) stay
   unconditionally marked.** Their values are always paths semantically (no
   builtin-name ambiguity), and existence-driven marking there would convert
   a clear missing-file error into silent document-dir fallback. No
   overengineering now: the schema may eventually signal path entries in-band
   (`file: …` object keys), which would obsolete sniffing entirely.
5. **Shared helper, extracted** — pattern-walk + existence-check core moves to
   a shared module (e.g. `crates/quarto-core/src/extension/paths.rs`);
   `project/mod.rs` imports it. The pattern tables are the part that must not
   drift; the unconditional-vs-existence-driven distinction is expressed and
   documented there.

## Phases

- **Phase 0 — Test plan (TDD; tests written and failing before any fix).**
  - `extension/read.rs` unit tests: bundled `theme: [cosmo, fmt-theme.scss]` →
    `cosmo` stays Scalar, `fmt-theme.scss` becomes Path; non-existent file
    stays Scalar; `css:`/`include-in-header:` marking; nested
    `theme: {light: …}` leaves marked; existing template/filter tests stay
    green.
  - End-to-end integration test (per the repro shape, via the real render
    path): compiled `styles.css` contains the extension rule AND the builtin
    theme.
  - Diagnostic test: `theme:` naming a file that exists nowhere → Q-14-x hard
    error with a span at the offending entry (not silence, not DEFAULT_CSS).
  - One `css:` case end-to-end (downstream Path-kind handling in the HTML
    writer link-emission path was not audited during investigation).
- **Phase 1 — Half A: shared helper + read-time marking.** Extract the
  pattern-walk/existence-check core from `project/mod.rs` into
  `extension/paths.rs`; refactor `rebase_fragment_paths` onto it (no behavior
  change there); add existence-driven marking of the decision-1 key set in
  `parse_formats` (thread `runtime` into `parse_contributes`).
- **Phase 2 — Half B: Q-14-x hard error.** In `compile_theme_css.rs:560-568`,
  split config-shaped errors (`CustomThemeNotFound`, `InvalidScssFile`) from
  internal compiler failures; route the former through `theme_diagnostic`
  as a new Q-14-x catalog code (following the `from_config_value` precedent
  at `compile_theme_css.rs:368-388`); keep the DEFAULT_CSS fallback only for
  internal failures.
- **Phase 3 — E2E verification.** Repro fixture through
  `cargo run --bin q2 -- render doc.qmd`; inspect `styles.css` for the marker
  rule + cosmo; full `cargo xtask verify` (quarto-core is in the WASM
  closure).
- **Phase 4 — Docs.** Check whether `docs/` mentions format extensions
  bundling assets; document the bundled-asset behavior if there's a natural
  home.

## Work items

- [x] Phase 0: failing tests (read-time marking, e2e theme, Q-14-x
      diagnostic, css case) — 7 unit tests in `extension/read.rs` (6 fail
      as expected, 1 no-change guard passes); smoke-all fixture
      `extensions/format-with-theme/` (fails all 3 assertions — confirms
      `css:` links are dropped too, not just theme); CLI test
      `theme_missing_file.rs` (Q-14-3 test fails on exit 0 as expected;
      present-file control passes). Note: `as_str()` handles Path-kind
      (config_value.rs:641), so the css template path is safe by
      construction — risk retired.
- [x] Phase 1: shared helper extracted (`extension/paths.rs`);
      `rebase_fragment_paths` refactored onto it; existence-driven marking
      wired into `parse_formats`. Commit `84f88c9f`; full workspace suite
      green; smoke fixture passes.
- [x] Phase 2: Q-14-3 registered in `quarto-error-catalog`; up-front
      validation in `compile_theme_css` (before cache-key computation) hard-
      errors on dangling custom theme entries with an ariadne span at the
      offending `theme:` entry; DEFAULT_CSS fallback retained for internal
      compile failures only. `ThemeConfig.theme_locations` (parallel field,
      deliberately not inside `ThemeSpec` — keeps the spec a pure value type
      for Eq/Display/cache-key identity);
      `SassError::CustomThemeNotFound.location`. Scope note:
      `InvalidScssFile` (file exists, content lacks boundary markers) still
      falls back silently — it only surfaces inside the compile call, whose
      error is stringified; filed as bd-qmpygp02 (discovered-from this
      strand) rather than widening this change untested. Commit follows
      `84f88c9f`; full workspace suite green.
- [x] Phase 3: e2e verified via real binary. (1) Original repro
      (`claude-notes/plans/format-ext-theme-rebase-investigation/repro/`):
      `cargo run --bin q2 -- render doc.qmd` → exit 0, `doc_files/styles.css`
      is 321 KB with `.fmt-theme-marker` present (was 7 KB DEFAULT_CSS with
      0 hits). (2) Dangling entry (`theme: [cosmo, nope.scss]`): exit 1,
      Q-14-3 ariadne diagnostic with span pinned at `nope.scss` (line 5 col
      20) and resolved path in the message; output inspected in both cases.
      Full `cargo xtask verify` (WASM leg included): **passed** (exit 0,
      2026-08-08).
- [x] Phase 4: docs — new `docs/errors/theme/Q-14-3.qmd` (listing index
      auto-globs it) + format-extension paragraph in the extensions guide's
      "Bundled files" section; verified via `q2 render docs/` (189/189,
      page renders, cross-link resolves).

## Risks / tradeoffs (draft)

- **Existence-driven marking is load-order-sensitive by design**: a file
  added/removed next to `_extension.yml` changes classification. Same accepted
  tradeoff as Phase 4 (documented at `project/mod.rs:676-680`).
- **Hard-error choice in Q2 is a behavior change** for configs that are
  *already* broken-but-silent today (they currently render with DEFAULT_CSS).
  This is the same posture shift PR #473 made for unknown project types.
- **`css:` consumers were not audited in this investigation** — Half A marks
  them and the merge rebases them, but I did not verify every downstream
  consumer of `css:` handles `Path`-kind values (the HTML writer link-emission
  path). Phase 0's tests should cover one `css:` case end-to-end.
- The `theme: {light:, dark:}` map form remains inert until bd-o76p01wb lands;
  the marking walker should still handle map leaves so that fix composes.

## Investigation artifacts

- `claude-notes/plans/format-ext-theme-rebase-investigation/repro/` — minimal
  reproduction (doc + fancyfmt extension). Render it with
  `cargo run --bin q2 -- render doc.qmd` from inside the repro dir;
  `grep fmt-theme-marker doc_files/styles.css` is the pass/fail probe
  (0 hits = bug present, ≥1 = fixed). `doc_files/` output is not committed.
