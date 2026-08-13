# ts-engine-extensions ← main: Merge Runbook (2026-08-13)

> **For the engineer executing this:** this is a **merge runbook**, not a
> feature build. Most tasks are *resolve a conflict → rebuild/retest*. One task
> (Task 5, `discovery.rs`) is a genuine **re-port** of a branch feature onto a
> rewritten upstream module — that one needs design judgment, not a 3-way merge.
> Work top-to-bottom. Do **not** push (see Global Constraints).

**Goal:** Bring `feature/ts-engine-extensions` up to date with `main` via a
**merge** (not a rebase), resolving all 23 conflicts correctly and leaving
`cargo xtask verify` green.

**State at start:**

| | |
|---|---|
| Branch tip | `89f068364` |
| `main` tip | `0dcd7e831` |
| Merge base | `65a888b0a` (2026-07-24) |
| Commits ahead / behind | 10 / **340** |
| Branch diff | 491 files, +104,835 / −3,498 |
| — of which conflict | **23 files, +5,326 / −222** (all `crates/quarto-core` + repo files) |
| — of which clean | 468 files, +99,509 (mostly new `ts-packages/`) |
| Conflict hunks | **50** |
| Safety branch | `ts-engine-extensions-backup-premerge-20260813` |

## Why merge and not rebase (decided 2026-08-13)

Measured, not guessed — a naive `git rebase main` stops at **7 of 10 commits**
(23 file-resolutions), and the hub files recur across stops: `engine_execution.rs`
at 3 stops, `engine/context.rs` at 3, `project/mod.rs` at 2, `extension/read.rs`
at 2. You would resolve the same collision several times, and a wrong
intermediate resolution propagates silently into later stops. A merge surfaces
all 23 once.

`rerere` is enabled in this worktree (`rerere.enabled=true`,
`rerere.autoupdate=false` — deliberately *not* autoupdate, so each replayed
resolution gets inspected before staging). Measured behavior: **0/23 hits on a
first pass** (every rebase-step conflict is novel), but **23/23 on a retry**.
Its value here is making the inevitable second attempt cheap, not the first.
Corollary: **a bad resolution recorded now is replayed faithfully later** — use
`git rerere forget <path>` if you realize one was wrong.

## Global Constraints

- **NEVER push without explicit user permission** (project GIT PUSH POLICY).
  This runbook ends at "green locally + merge commit prepared," not at push.
- **Never resolve a hub file by taking a whole side.** Every hard conflict needs
  the branch's structure **and** main's new feature. Taking a side silently
  deletes upstream work and the damage lands in files that never conflicted
  (measured: a naive `--theirs` pass produced 54 compile errors across 14 files,
  10 of which merged clean; and dropping main's `is_external_url` re-export from
  `quarto-util` broke `quarto-brand`, which never appeared in the conflict list).
- **`.braid/snapshot.jsonl` is regenerated, never hand-merged** (snapshot
  policy): take either side, then `braid export > .braid/snapshot.jsonl`.
- **Lockfiles are regenerated, never hand-merged:** `Cargo.lock`,
  `crates/wasm-quarto-hub-client/Cargo.lock`, `package-lock.json`.
- Run from the worktree: `/Users/gordon/src/q2/.worktrees/ts-engine-extensions`.
- Rollback: `git merge --abort`, or nuclear
  `git reset --hard ts-engine-extensions-backup-premerge-20260813`.
- **Merge `--ours`/`--theirs` orientation:** in a merge, `--ours` = **the branch**
  and `--theirs` = **main** (the reverse of a rebase). The previous runbook
  (2026-07-22) documents the rebase orientation — do not copy it blindly.

## The 23 conflicting files, in resolution order

| Task | Files | Hunks | Category |
|---|---|---|---|
| 1 | `.braid/snapshot.jsonl`, lockfiles | 3 | Regenerate |
| 2 | `.gitignore`, both `tests/integration/main.rs`, `quarto-util/src/lib.rs`, `extension/mod.rs`, `engine/mod.rs`, `project/mod.rs` h2–h3, `document-profile-contract.md` | ~10 | Additive union |
| 3 | `extension/types.rs`, `filter_resolve.rs`, `metadata_merge.rs`, `shortcode_resolve.rs`, `extension/discover.rs`, `extension/read.rs` h4–h6 | ~12 | `Extension::title` sweep |
| 4 | `document_profile.rs`, `stage/stages/document_profile.rs` | 5 | Double version bump |
| 5 | **`project/discovery.rs`** | 7 | **Re-port** |
| 6 | `project/mod.rs` h1, `extension/read.rs` h2–h3 | 3 | Signature threading |
| 7 | `engine/context.rs`, `stage/stages/engine_execution.rs`, `stage/context.rs`, `orchestrator.rs`, `jupyter/text_execute.rs`, `transforms/shortcode_resolve.rs`, `filter_resolve.rs` | ~10 | Both-added adjacency |
| 8 | (none — compiler-driven) | — | Compile-landmine sweep |
| 9 | (none) | — | Verify + prepare commit |

---

## Task 0: Start the merge

- [ ] `git status --short` → clean; confirm safety branch exists
- [ ] `git merge main` → expect 23 conflicts
- [ ] `git status --short | grep -E '^(UU|AA|DU|UD)' | wc -l` → expect 23

## Task 1: Regenerated files

- [ ] Clear conflict on `.braid/snapshot.jsonl` + any lockfile (either side)
- [ ] `braid export > .braid/snapshot.jsonl`
- [ ] Regenerate Cargo.lock via `cargo build --workspace` (deferred to Task 8)
- [ ] `npm install` from repo root if `package-lock.json` conflicted

## Task 2: Additive unions (low risk)

Keep **both** sides' additions; no side is discarded.

- [ ] `.gitignore` — union (main's `.posit/assistant/`, branch's extension-dist entries)
- [ ] `crates/quarto-core/tests/integration/main.rs` + `crates/quarto/tests/integration/main.rs` — union the `pub mod` lists, alphabetized
- [ ] `quarto-util/src/lib.rs` — union: `pub use path::{is_external_url, is_rooted, to_forward_slashes};` **plus** the branch's `data_dir`/`runtime_dir` re-exports. (Dropping `is_external_url` breaks `quarto-brand`.)
- [ ] `extension/mod.rs` — union `pub use` + the branch's native-gated build module
- [ ] `engine/mod.rs` — union `pub(crate) use` (main's `find_rscript` + branch's additions)
- [ ] `project/mod.rs` hunks 2–3 — both sides added test modules at the same spot; keep both
- [ ] `claude-notes/designs/document-profile-contract.md` — reconcile to version 11 (follows Task 4)

## Task 3: `Extension::title` sweep — take main's side

Main changed `Extension::title` from `String` to `Option<String>`
(bd-8b0af414, Q1-compat intake: a missing title is no longer an error). The
branch still has `String` plus a "title defaults to `id.name`" behavior that
main **deliberately reversed**.

- [ ] `extension/types.rs` — `pub title: Option<String>` (main)
- [ ] `filter_resolve.rs`, `metadata_merge.rs`, `transforms/shortcode_resolve.rs` — `title: Some(name.to_string())` (main)
- [ ] `extension/discover.rs` hunks 2–7 — `title: Some("…".to_string())` (main)
- [ ] `extension/read.rs` hunk 1 — main's optional-metadata reads via `as_plain_text()`
- [ ] `extension/read.rs` hunks 4–6 — **delete** the branch's
      `test_read_extension_missing_title_defaults_to_id_name`; keep main's
      `test_read_extension_missing_title` asserting `title == None`
- [ ] Check for branch-only code that assumed `title: String` (new engine/extension
      code added by the branch, not in the conflict set) — Task 8 will surface it

## Task 4: `document_profile.rs` — double version bump (again)

Third occurrence of this collision (7→8 was resolved in `22a639307`). Branch is
at **8** (`engine_resolution`, Plan 6); main is at **10** (`authors_structured`
at 7 via bd-ez0hiowa, plus two further bumps).

- [ ] Keep **both** field sets and **both** supporting types
- [ ] Adopt main's `listing_content_globs: Vec<crate::glob::GlobPattern>`
      (the branch's `Vec<String>` is superseded — `dependency_graph.rs`,
      which merges clean, requires `&[GlobPattern]`)
- [ ] Bump `DOCUMENT_PROFILE_VERSION` to **11**; merge the changelog doc-comments
      under a single `11:` note rather than leaving two conflicting entries
- [ ] Update `document_profile_version_is_10` → `_is_11`
- [ ] `stage/stages/document_profile.rs` — keep main's glob-provenance resolution
      **and** the branch's `engine_resolution` stamping
- [ ] `cargo nextest run -p quarto-core -- document_profile profile_version`

## Task 5: `project/discovery.rs` — RE-PORT (the real work)

**This is not a 3-way merge.** Main rewrote the module; the branch's feature must
be re-implemented against the new shape.

Main's new surface: `render_patterns: &[RawGlob]` (provenance-tracking, replacing
`&[String]`), `walk_sources` + `select_from_walk` (replacing `walk_qmd`),
`is_renderable_source`, `has_renderable_extension` (fixed `.qmd` + `.md` set),
`effective_render_patterns`, `resolve_render_patterns`, `render_pattern_diagnostics`,
`unmatched_md_files`, `is_agent_instruction_md`.

The branch's contribution to keep: `RenderableExtensions` — the resolved
`FIXED_RENDERABLE ∪ engine-claims-file extensions` set (plan 1c.2). It exists
**nowhere** on main.

- [ ] **Delete** the branch's superseded hand-rolled glob code: `expand_patterns`,
      `normalize_pattern`, `glob_match`, `glob_match_path`, `segment_match`,
      `wildcard_match`, `path_to_forward_slashes`, `to_forward_slashes`.
      Main's `crate::glob::GlobPattern` / `RawGlob` system replaces all of it.
      **Check for out-of-file callers first** — `glob_match_path` and
      `path_to_forward_slashes` were `pub`.
- [ ] Add `renderable_extensions: &'a RenderableExtensions` to main's `DiscoveryConfig`
- [ ] Make `has_renderable_extension` consult the set instead of the fixed list
      (keep main's `.md` membership; the set is a superset)
- [ ] Keep `RenderableExtensions` + `ext_in_set` (adapted to main's call shape)
- [ ] Thread the set through `walk_sources` → `walk_rec` → `is_renderable_source`
- [ ] Re-port the branch's T6/T6b/T7 tests against the new function names, and
      confirm they still bind: the same exclusion rules (underscore, dot,
      output-dir) must apply to an engine-claimed `.echo` as to `.qmd`, and a
      non-member extension (`.ipynb`) must stay excluded
- [ ] Update the two `project/mod.rs` + `quarto/src/commands/render.rs` call sites
      that construct `RenderableExtensions`
- [ ] **DESIGN DECISION to confirm with the user (see Open Questions):** main added
      "`.md` inputs never execute engines" (`Q-2-40`, bd-6d2wj4zp S5) while
      admitting `.md` as renderable. Working assumption: keep that rule as-is, and
      engine-claimed extensions **do** execute (that is the point of plan 1c.2).
      `.md` stays renderable-but-never-executing even if an engine claims it.

## Task 6: Signature threading

- [ ] `extension/read.rs` hunks 2–3 — `parse_contributes` gained `runtime: &dyn SystemRuntime`
      on main and `extension_file: &Path` on the branch. **Thread both**; keep both doc comments.
- [ ] `project/mod.rs` hunk 1 — main rebound `config` from `Option<ProjectConfig>` to
      `ProjectConfig` (project-less profile resolution: `--profile bad/name` must error,
      `when-profile` must see the active set). The branch's `build_registry(…, config.as_ref(), …)`
      + `RenderableExtensions::new(…)` + `tabled_engine_names(config.as_ref())` sit exactly there.
      Keep main's rebinding, then adapt the branch's calls to the non-`Option` `config`.
- [ ] `parse_config` gained `cli_selection: Option<&[String]>` on main — update the
      branch's 2-arg call sites (5 occurrences)

## Task 7: Both-added adjacency in the engine/stage cluster

Keep both sides at each hunk.

- [ ] `engine/context.rs` — main's `project_env` field + doc **and** the branch's
      `handled_languages` / `cancellation` defaults; both in `Default`/constructor
- [ ] `stage/stages/engine_execution.rs` h1 — main's `.md`-never-executes guard
      (`Q-2-40`) **and** the branch's engine-resolution path
- [ ] `stage/stages/engine_execution.rs` h2 — both extended the same builder chain:
      main's `.with_project_env(…)` (incl. unconditional `QUARTO_PROFILE`) **and** the
      branch's `.with_handled_languages(…)`
- [ ] `stage/context.rs` — main's `load_project_variables` + `active_profile_names`
      **and** the branch's additions
- [ ] `project/orchestrator.rs` — main's unmatched-pattern diagnostic (bd-mt7a6uc4 D7)
      **and** the branch's additions
- [ ] `engine/jupyter/text_execute.rs` — main's `kernel_scope` guard (bd-hxhnnlzs)
      **and** the branch's cede/claim filtering

## Task 8: Compile-landmine sweep (the invisible conflicts)

Merge-tree only flags **textual** conflicts. Expect a body of clean-merged-but-broken
call sites, because the branch adds ~100k lines calling `quarto-core` APIs that main
changed underneath.

- [ ] `cargo build --workspace 2>&1 | tee /tmp/mergebuild.log | tail -40`
- [ ] Fix each error; re-run until clean. Known-genuine ones to expect:
      `dependency_graph.rs` wanting `&[GlobPattern]`; `quarto-brand` wanting
      `quarto_util::is_external_url`
- [ ] For every error, ask **"did I drop one of main's fields/modules in Tasks 2–7?"**
      before "adapt the caller" — that was the failure mode measured on the naive pass
- [ ] `cargo build --workspace` → 0 errors

## Task 9: Verify + prepare the merge commit

- [ ] `cargo nextest run --workspace` (run directly, never piped through `tail`)
- [ ] `cargo xtask verify` — **full**, not `--skip-hub-build`: `quarto-core` and
      `wasm-quarto-hub-client` both change
- [ ] `cargo xtask lint`
- [ ] End-to-end (project rule — tests alone are insufficient):
      `cargo run --bin q2 -- render docs/`, inspect output, plus a multi-engine
      fixture exercising an engine-claimed extension. Record invocation + output
      snippet below.
- [ ] Report snapshot changes: count added/modified/removed `.snap`, summarize,
      flag anything surprising (esp. the profile-version snapshot from Task 4)
- [ ] `git commit` the merge (do NOT push); report the commit
- [ ] Ask the user for push permission

```
(paste the verified end-to-end invocation + output snippet on completion)
```

---

## Open Questions

1. **`.md` + engine-claimed extensions (Task 5).** Working assumption stated
   above: `.md` stays renderable-but-never-executing; engine-claimed extensions
   execute. Flag to the user when Task 5 lands; do not silently pick a different
   rule.

## Post-merge follow-ups to watch

Main features that auto-merged cleanly but sit next to the resolved code —
spot-check they survived: project profiles (`when-profile`, `QUARTO_PROFILE`
write-back), brand resolution, listings/feeds, `project::aliases`,
title-block-parity transforms, i18n `LanguageResolveStage`, jupyter cell-options
and error policy. `cargo xtask verify` covers most; the docs render exercises
mermaid + title-block.

## Prior art

- `claude-notes/plans/2026-07-22-ts-engine-extensions-rebase-conflict-resolution.md`
  — the previous catch-up (merge-base `61e2d2276`). Its conflict inventory is
  stale, but its resolution philosophy ("branch structure + main's features,
  never a whole side") and its Task 6 compile-landmine idea carry over directly.
  Note its `--ours`/`--theirs` orientation is the **rebase** one.
