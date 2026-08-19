# bd-oejuizi9 — declaration-site resolution for theme / include-* config paths

**Date:** 2026-08-19
**Braid:** bd-oejuizi9 (in_progress). Also partially resolves bd-rdcvjy2s
(leading-`/` for these keys) and GH #455.
**Branch:** `feature/path-resolution-class` (dedicated to the path-resolution
bug class; first commit `9b6f89f3` carries the contract + assessment).
**Contract:** `claude-notes/designs/path-resolution-model.md` (normative).
This fix is deliberately the first exercise of the contract's convergence
target — deviations we hit are contract feedback and get recorded in §Contract
feedback below.

## Overview

`include-in-header` / `include-before-body` / `include-after-body` and custom
`theme` SCSS declared in `_quarto.yml` or `_metadata.yml` resolve against each
**consuming document's** directory, so project-wide declarations silently drop
for subdirectory pages (GH #455; verified 2026-08-19 in the assessment doc).
`css:` was already fixed (37758160) via merge-time per-layer marking.

**Fix (mechanism 3 of the contract, generalized):** extend
`project/format_css.rs` into `project/format_paths.rs` with a table of
path-shaped format keys — the seed of the contract's unified registry. At each
metadata-merge layer, while the declaring base is still known, mark matching
string values as `ConfigValueKind::Path` rebased to document-relative form
(leading `/` anchored at the project root via `candidate_path`). Consumers
then work **unchanged**: `as_plain_text()`/`as_str()` pass `Path`-kind values
through, so `include_resolve`'s `doc_dir.join` and `ThemeContext.resolve_path`
become correct as written.

### Key table (FORMAT_PATH_KEYS)

| Key | Policy | Forms |
|---|---|---|
| `css` | existence-driven, Q-5-29 on miss (unchanged) | scalar \| array |
| `theme` | existence-driven, **silent** on miss (builtin names are the common case) | scalar \| array \| map `{light,dark}` (recurse) |
| `include-in-header` / `-before-body` / `-after-body` | **unconditional** (these strings are always paths) | scalar \| array; map items: mark `file:` value, leave `text:` |

Policy rationale:
- Unconditional marking for include slots means a *missing* file's Q-5-4 (at
  resolve time) now reports the declaration-resolved path instead of the
  bogus doc-joined path — no new diagnostic code needed.
- Theme marking must not diagnose: `theme: cosmo` names no file by design.
  A typo'd custom path stays Scalar and fails at theme load exactly as today.
- Edge accepted (same as extension `FORMAT_ASSET_PATTERNS`): a file literally
  named like a builtin theme (`./cosmo`) in the declaring dir would now be
  marked and shadow the builtin. Existence-driven marking shares this
  property everywhere; not worth special-casing.

### Behavior changes (user-visible, all fixes)

1. Project-level `include-in-header: x.html` reaches subdirectory pages
   (GH #455 main case).
2. `include-in-header: /x.html` = project-root-relative (bd-rdcvjy2s for
   these keys; previously OS-absolute, matching neither Q1 nor the contract).
3. Project-level `theme: [cosmo, custom.scss]` compiles `custom.scss` into
   subdirectory pages' bundles.
4. Q-5-4 messages for missing includes name the declaration-resolved path.

### Out of scope (tracked elsewhere)

- `template` / `template-partials` / `filters` / `title-block-banner` /
  engine-declared resources — same class, bd-hjv5o + bd-rdcvjy2s; adding them
  later is one table row each (that being trivial is the point of the table).
- `bibliography` / `csl` CWD bug — bd-oqoozmtr (different consumer crate).
- Root-level `_metadata.yml` silently ignored entirely (assessment §1 third
  variant) — separate merge-layer bug, strand filed during this work.
- WASM/VFS: marking is existence-driven against the VFS; files not synced
  degrade to today's behavior (same posture as css marking, whose
  diagnostics are already wasm-gated).

## Work items

### Phase 1 — failing tests first

- [x] Unit tests (in the generalized module): include string marked
      unconditionally (existing → rebased; missing → rebased, no diagnostic);
      `{file:}` marked / `{text:}` untouched; theme builtin untouched +
      silent; theme existing scss marked (scalar + mixed array); theme
      `{light,dark}` map recursed; leading-`/` include anchors at project
      root; css policy unchanged (existing tests keep passing).
- [x] Integration tests (`tests/integration/format_path_keys.rs`,
      `render_project` harness from `format_css.rs`): #455 repro (project
      include-in-header reaches root + subdir pages, zero Q-5-4);
      leading-`/` variant; subdir `_metadata.yml` declaration-site
      resolution; front-matter include regression guard; project-level
      `theme: [cosmo, custom.scss]` rule present in subdir page's theme CSS.
- [x] Verify the new tests fail at branch HEAD for the right reasons.

### Phase 2 — implementation

- [x] Rename `project/format_css.rs` → `project/format_paths.rs`; introduce
      `FORMAT_PATH_KEYS` table + `mark_format_path_values` generalizing
      `mark_css_path_values`; keep `missing_project_css_diagnostics`
      css-specific.
- [x] Update call sites (3 in `metadata_merge.rs`, orchestrator, `project/mod.rs`).
- [x] Run new tests green; full `cargo nextest run --workspace`.

### Phase 3 — end-to-end + bookkeeping

- [x] Real-binary verification: `cargo run --bin q2 -- render` on the #455
      fixture; inspect `_site/sub/index.html` for the header marker; record
      invocation + output snippet here.
- [x] Contract: update the inventory (move include/theme rows from
      VIOLATIONS to conforming; note residual keys); record any contract
      tweaks below.
- [ ] Braid: comment + close-or-update bd-oejuizi9 (theme+include done; css
      was done; residual keys under bd-hjv5o); update bd-rdcvjy2s; comment
      on GH #455 is the user's call.
- [ ] `cargo xtask verify --skip-hub-build` (quarto-core touched: consider
      full verify per CLAUDE.md before any push).

### Phase notes

- Failing-first evidence: the 3 integration bug tests
  (`project_include_in_header_reaches_subdir_pages`,
  `rooted_include_in_header_anchors_at_project_root`,
  `project_theme_custom_scss_reaches_subdir_pages`) failed at branch HEAD
  before implementation; the 2 guards passed. Post-implementation: all 25
  format_paths/format_path_keys/format_css tests green; full workspace
  `cargo nextest run --workspace` green (12870 passed).
- **E2E (real binary), 2026-08-19:** `cargo run --bin q2 -- render <fixture>`
  on the exact #455 fixture. Pre-fix: Q-5-4, `_site/sub/index.html` lacked
  `<!-- hdr -->`. Post-fix: zero warnings; `grep -c hdr` = 1 in both
  `_site/index.html` and `_site/sub/index.html`, for both the bare and the
  leading-`/` spelling. Output inspected.

## Contract feedback (filled during implementation)

- **Convergence prediction held exactly:** generalizing mechanism 3 made
  `include_resolve`'s `doc_dir.join` and `ThemeContext.resolve_path` correct
  with zero consumer changes — the whole diff is the marking module + call
  sites + tests.
- **One contract addition made:** the registry needs a per-key *marking
  policy* dimension the contract text didn't anticipate — some keys'
  strings are only sometimes paths (`theme` shares its namespace with
  builtin names → existence-driven silent; `include-*` → unconditional).
  Recorded in the contract's mechanism-3 entry.
- No changes needed to the two rules, the two-spaces framing, or the author
  rules.
