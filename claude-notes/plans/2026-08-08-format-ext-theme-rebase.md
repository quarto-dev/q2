# Format-extension theme/css paths not rebased: contributes.formats.html.theme silently drops bundled SCSS (bd-of20unsb)

**Date:** 2026-08-08
**Braid:** bd-of20unsb (P2, bug)
**Checkout:** main worktree at `main` @ `e5fc4ffb` (post-merge of PR #474)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

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
precedent). Whether they *fail the render* or *warn-and-fallback* is design
question 2 below.

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).**
  - `extension/read.rs` unit tests: bundled `theme: [cosmo, fmt-theme.scss]` →
    `cosmo` stays Scalar, `fmt-theme.scss` becomes Path; non-existent file
    stays Scalar; `css:`/`include-in-header:` marking; nested
    `theme: {light: …}` leaves marked; existing template/filter tests stay
    green.
  - End-to-end integration test (per the repro shape, via the real render
    path): compiled `styles.css` contains the extension rule AND the builtin
    theme.
  - Diagnostic test: `theme:` naming a file that exists nowhere → Q-14-x
    surfaced (not silence, not bare DEFAULT_CSS).
- **Phase 1 — Half A** (shared walk helper + read-time marking).
- **Phase 2 — Half B** (Q-14-x diagnostic; register in catalog; snapshot).
- **Phase 3 — E2E verification** (repro fixture through `cargo run --bin q2 --
  render`, inspect `styles.css`; full `cargo xtask verify` — quarto-core is in
  the WASM closure).
- **Phase 4 — Docs**, if user-facing docs mention format extensions bundling
  assets.

## Open design questions for the user

1. **Scope of Half A's key set.** Mirror the `format.*` subset of
   `FRAGMENT_PATH_PATTERNS` exactly (`theme`, `css`, `include-in-header`,
   `include-before-body`, `include-after-body`, `format-resources`), or start
   narrower (`theme` + `css` only, per the strand title) and let the rest ride
   along? Mirroring exactly keeps the two tables unifiable in the shared
   helper; I lean toward the full subset.
2. **Severity of a dangling theme entry (Half B).** When `theme:` names a
   `.scss`/`.css` that resolves to no file: hard error (consistent with the
   `from_config_value` precedent at `compile_theme_css.rs:368`, and with
   "reveal theme unresolvable → error rather than wrong theme"), or visible
   warning + compile the remaining entries (more forgiving, but partial
   themes are their own confusion)? I lean toward hard error with a precise
   ariadne span; note it changes behavior for *pre-existing* broken configs
   that today silently get DEFAULT_CSS.
3. **Also fix "one bad entry nukes valid entries"?** Under warn-and-continue
   (Q2 option B) this needs per-entry error recovery inside
   `process_theme_specs`; under hard-error it's moot. If hard-error, I'd file
   nothing further.
4. **Unconditional vs existence-driven for the legacy keys.** Should
   `template`/`template-partials`/`shortcodes` stay unconditionally marked
   (current behavior, they're always paths), or unify on existence-driven
   marking? I lean strongly toward leaving them alone in this strand —
   behavior change there is out of scope.
5. **Where the shared helper lives.** `crates/quarto-core/src/extension/`
   (new module, e.g. `extension/paths.rs`) with `project/mod.rs` importing it,
   or keep two small copies? I lean toward extracting — the tables are the
   part that must not drift.

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
