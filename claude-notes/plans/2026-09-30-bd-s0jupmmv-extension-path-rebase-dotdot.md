# Space-aware `..` refusal in `adjust_paths_to_document_dir`

**Strands:** bd-s0jupmmv (research, `question`) · bd-9z2258af (implementation,
`bug`) · discovered-from bd-qi11c7fj · related bd-oejuizi9 (the path-resolution
epic) · liveness on the bd-1klbq2zd stack tip.

**Research:** `claude-notes/research/2026-09-30-extension-path-rebase-windows-max-path.md`
**Contract:** `claude-notes/designs/path-resolution-model.md`

## Overview

`adjust_paths_to_document_dir` (`crates/quarto-core/src/project/mod.rs:259`)
rebases every `ConfigValueKind::Path` value from its declaring dir to the
consuming document dir with `pathdiff::diff_paths`, whatever the two dirs are.
When the declaring dir is **outside** the document tree — built-in extensions
extracted to a temp dir (orange-book, julia-engine) — the stored value is a
long `..` chain climbing out of the project and back down into temp. Consumers
join it onto `document_dir` without normalizing (e.g. `filter_resolve.rs:269`),
and on Windows the joined string exceeds MAX_PATH so Lua `io.open` (C `fopen`)
fails with `cannot open ...\../../...orange-book.lua`. Linux/macOS build the
same form and only survive because they have no MAX_PATH.

The in-tree precedent already exists: `rebase_candidate` (project/mod.rs:764)
**refuses** a `..`-leading rebase and keeps the absolute path for
temp-extracted extensions. But that guard lives only on the *fragment* rebase;
the three per-document merge calls in `metadata_merge.rs` (lines 163, 267, 299)
call `adjust_paths_to_document_dir` with **no** guard.

### What Q1 does (confirmed in source)

Q1 splits by contribution kind: **absolute** paths for filters / shortcodes /
reveal-plugins / engines (`resolveFilterPath` / `resolveShortcodePath` /
`resolveRevealPlugin` in `src/extension/extension.ts`, `isAbsolute` join against
`extensionDir`), but **input-relative `..` chains** (`toInputRelativePaths` in
`src/project/project-shared.ts`) for format metadata (`css`, `theme`,
`include-*`, `template`, `template-partials`, `format-resources`). Q1 never
surfaces the bug because (a) the executed Lua filter list is absolute and
(b) Deno's `path.join` **normalizes** (collapses `..`) before any
`Deno.readFile`, while Rust's `Path::join` does not.

## Fix direction (from research)

Mirror `rebase_candidate`'s `..`-leading refusal inside
`adjust_paths_to_document_dir`, keeping the absolute declaring-dir-resolved
path — but **space-aware per key**:

- **Pure-filesystem keys** (`filters`, `template`, `template-partials`,
  `include-in-header/-before-body/-after-body`, `format-resources`,
  `reference-doc`) get absolute-if-outside. These consumers tolerate absolute
  (Rust `Path::join` with an absolute RHS returns it; `filter_resolve` already
  special-cases `is_absolute`).
- **URL-space keys** (`css`, `theme`) must **never** become
  filesystem-absolute, because the value can reach emitted HTML `<link href>`.
  These stay on the mechanism-3 marking; absolute-if-outside does **not** apply.
  Verify `transforms/format_css.rs` copy/href derivation before deciding where
  css is handled; any key deliberately left relative is a scope-out strand
  linked to bd-oejuizi9.

Seam: `adjust_paths_to_document_dir` keyed to a per-key space table — the
convergence point the contract already names (unified path-shaped-key registry,
bd-oejuizi9 / bd-hjv5o).

## Checklist

### Phase 0 — pin the failure (DONE in research session)
- [x] Failing test written: `project::tests::adjust_paths_outside_document_tree_does_not_produce_dotdot`
  (declaring dir outside document tree → must not yield `..`-leading).
  Confirmed RED 2026-09-30: got `../../tmp/ext/orange-book.lua`.
- [x] Companion passing test: `adjust_paths_inside_document_tree_stays_relative`
  (in-tree rebase stays doc-relative: `../_extensions/acm/filter.lua`).
- [x] Red test pinned with `#[ignore]` + strand ref so the suite stays green
  until the fix lands. **Implementation must remove `#[ignore]` as the first
  act.**

### Phase 1 — verify css URL-space handling (do before touching the rebase)
- [x] Read `crates/quarto-core/src/transforms/format_css.rs`: `css` is BOTH
  copied (FS read, `document_dir.join(doc_relative)` → copy intent) AND
  re-derived into a page-relative href (entry rewritten to `Path(href)`).
  An absolute filesystem value is not a valid page-relative href and would
  also trip the outside-project guard → css/theme excluded from
  absolute-if-outside.
- [x] css/theme home: mechanism-3 marking in `project/format_paths.rs` runs
  AFTER this rebase, so only extension-contributed css/theme
  (`FORMAT_ASSET_PATTERNS`) and explicit `!path` css reach the walk as
  `Path`. No change needed there.
- [x] Scope-out strand filed: **bd-f0h4ahai** (URL-space outside-tree css/theme
  keeps `..` chain), linked `related` → bd-oejuizi9.

### Phase 2 — implement the FS-space refusal
- [x] Removed `#[ignore]` from `adjust_paths_outside_document_tree_does_not_produce_dotdot`.
- [x] Added the refusal to `adjust_paths_recursive`. Design note: rather than
  reconstructing the project root from `metadata_dir`/`document_dir`
  (ambiguous — `/tmp` and `/project` share only `/`), threaded an explicit
  `project_root: &Path` parameter through `adjust_paths_to_document_dir` and
  all call sites (`metadata_merge.rs` ×3, `project/mod.rs` ×1). The guard is
  then a single `!abs_path.starts_with(project_root)` check — outside → keep
  the absolute declaring-dir-resolved path (forward slashes), mirroring
  `rebase_candidate`.
- [x] Space-aware via `URL_SPACE_REBASE_KEYS = ["css", "theme"]`, keyed by the
  top-level map key threaded through the recursion; FS-space keys get
  absolute-if-outside, css/theme stay relative.
- [x] Red test GREEN; companion relative test GREEN; added
  `adjust_paths_outside_document_tree_keeps_css_relative` (FS key absolute,
  css/theme relative). Commit c629420.

### Phase 3 — end-to-end + regression
- [x] End-to-end on Windows, bd-1klbq2zd stack tip:
  `SMOKE_FILTER=orange-book-margin cargo nextest run -p quarto -E 'test(smoke_all)'`
  → **PASS** (exit 0; previously `cannot open ...\../../...orange-book.lua`).
- [~] Full workspace `cargo nextest run --workspace` + `cargo xtask verify
  --skip-hub-build`: **deferred to CI** per user direction (2026-09-30).
- [x] Snapshot check: no `.snap` changes (source-only edit).

### Phase 4 — bookkeeping
- [x] Contract inventory: consumer behavior unchanged (fs consumers already
  tolerate absolute; css/theme explicitly unchanged) — no table edit needed;
  scope-out recorded in bd-f0h4ahai instead.
- [x] `braid comment bd-9z2258af` with commit + end-to-end evidence (c-g5l9ly6f).
- [x] Close bd-9z2258af on green.
- [x] hub-client changelog: N/A (no hub-client change).

## Notes / gotchas

- `#[ignore]` on the red test is **temporary**; leaving it is a silent hole.
- Do not "fix" by normalizing consumer joins alone — that collapses `..` for
  the OS but does not shorten the stored string, so MAX_PATH still trips.
- The absolute path is only safe for FS-space keys; never emit it as an href.
- `git status` shows untracked typst smoke fixtures under
  `crates/quarto/tests/smoke-all/typst/brand-yaml/**` — pre-existing, **not**
  part of this work; leave them alone.
