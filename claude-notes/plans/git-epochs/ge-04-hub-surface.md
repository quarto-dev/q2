---
title: 'GE-4: Hub surface: the git flag, status route, startup warning'
date: 2026-11-18
description: 'Adds the experimental git flag to q2 preview, the git-status route that reports tracking, paused, and pending, and a startup warning when an extension has bound the project directory.'
status: approved
depends-on: [ge-3]
epic: ../2026-11-18-git-epochs-epic.md
---

**Epic:** [git epochs](../2026-11-18-git-epochs-epic.md) · **Waits for:** [GE-3](ge-03-hub-integration.md) ·
**Unblocks:** [GE-13](ge-13-hub-e2e.md), [GE-15](ge-15-windows-run-a.md) ·
**Branch:** `ge/04-hub-surface` off `feature/git-epochs`, merged by rebase and fast-forward; nothing is pushed.

## Goal

GE-3 made the hub git-aware behind a configuration option. This plan gives users and web clients the surface to reach
it: the `--git <track|ignore>` flag on `q2 preview`, a `GET /api/preview/git-status` route that web clients poll for
the "paused for git" indicator, and a warning at startup when the project directory is already bound by a VS Code
extension folder. It changes no sync behavior; it only exposes and configures what GE-3 built.

## Decisions implemented

- **G33.** `q2 preview --git <track|ignore>`: experimental, default `ignore`, a value is required (a bare `--git` is a
  parse error), conflicts with `--join`, `--static`, and `--hub-url`. `track` means this preview owns the project\'s
  pointer. The route `/api/preview/git-status` reports tracking, paused, pending, the marker names, and the decision
  ring; web clients show an indicator only (no read-only mode).
- **G28.** At startup `q2 preview` checks its project root for `.quarto-hub/project.json` and warns that running a hub
  on a directory an extension has bound syncs the same files twice. Warn only; do not refuse.
- **G5 and G3** are consumed, not changed: the route reports the paused state of the gate GE-3 put inside
  `sync_all`, using the markers GE-2 resolves.

## Context

- The `--ui` flag is the model. The CLI enum `PreviewUiArg` mirrors the library enum so the library has no clap
  dependency (`crates/quarto/src/main.rs:1220-1239`); the flag is declared with `value_enum` and `default_value_t` at
  `main.rs:339-340`; the library enum is `quarto_preview::PreviewUi` (`crates/quarto-preview/src/lib.rs:224-230`).
- The conflict lists are `main.rs:370` (`--join`), `:387` (`--static`), and `:448` (`--hub-url`). The `--hub-url`
  list does not name `ui` because the mirror supports `--ui editor`; `git` joins all three lists.
- `PreviewConfig` is at `lib.rs:234` and is built as a struct literal in many places (22 at the base commit, more on `origin/main`; recount with `grep -rn "PreviewConfig {" crates`): `lib.rs`,
  `crates/quarto/src/commands/preview.rs:444`, and 14 files under `crates/quarto-preview/tests/integration/` (`boot`,
  `render_flags`, `diagnostics_capture_failure`, `live_reexecute`, `render_scripts_boot`, `import_route`,
  `join_frontend`, `diagnostics_endpoint`, `staleness`, `render_routes_support`, `config_endpoint`, `join_tunnel`,
  `editor_ui`, `eager_capture`). A new field therefore touches every literal.
- `build_hub_config` (`lib.rs:684`) turns a `PreviewConfig` into a `HubConfig`; it already derives
  `disk_write_policy` from `allow_edit`. The CLI side builds `PreviewConfig` at `preview.rs:444` with `ui: args.ui`
  at `:462`, from `PreviewArgs` (`preview.rs:40-72`).
- Preview routes are registered in `extend_with_preview` (`lib.rs:778-837`); `/api/preview/config` is at `:836` with
  its handler at `:852`. Handlers reach the hub through `State<SharedContext>` (comment at `lib.rs:778`).
- The extension marker names are `META_DIR = '.quarto-hub'` and `META_FILE = 'project.json'`
  (`vscode-sync-experiment/src/project.ts:38-39`).
- GE-1 fixes the response shape of the status route (`tracking`, `paused`, `pending`, `markers`, `decisions`); GE-3 names the `HubConfig` option and the context accessor that
  reports status. Read both before starting and write the names in the notes.

## Test specifications

Written first and seen failing for the right reason (a missing flag, a missing route, a missing function), in this
order:

1. **Parse tests** in `cli_parse_tests` (`main.rs:1258`), following `preview_join_conflicts_with_*` (`:1707-1735`) and
   the `--ui` tests (`:1745-1760`):
   - `preview_git_defaults_to_ignore`; `preview_git_parses_track_and_ignore`.
   - A bare `--git` is a clap error whose message names `--git`; `--git bogus` is an error.
   - `--git` conflicts with `--join`, with `--static`, and with `--hub-url` (an argument-conflict error each).
   - `--git track` combines with `--ui editor`, `--allow-edit`, and `--share`.
   - `From<PreviewGitArg> for quarto_preview::PreviewGit` maps both values.
2. **Library unit tests** next to the existing `build_hub_config` tests (`lib.rs:~1451`): `PreviewGit::default()` is
   `Ignore`; `Track` sets the GE-3 hub option; `Ignore` leaves it off.
3. **Route integration test** `crates/quarto-preview/tests/integration/git_status.rs`, modeled on
   `config_endpoint.rs` (boot a real server on a fixture, GET the route):
   - With `Ignore`: `tracking`, `paused`, and `pending` are false, and `markers` and `decisions` are empty arrays.
   - With `Track` on a temp git repo: `tracking` is true and `paused` is false.
   - Create the file that `git rev-parse --git-path index.lock` names: `paused` becomes true; remove it: false again.
     Poll with a retrying helper until the state holds; no fixed sleeps.
   - `pending` is true while GE-3\'s decision is held and false after it is applied (use the held-decision hook GE-3
     provides for its own tests; GE-3 provides one, with `HubContext::git_status()`). After a real `git switch`, `decisions` lists it (kind, files,
     G9 step, `discarded`, at most 20 entries), and after a real commit it lists a `moved-forward` decision and `markers` names the marker holding the gate while one exists.
4. **Warning tests**: a unit test for `extension_bound(project_root)` (true only when
   `<root>/.quarto-hub/project.json` exists and its `mode` is not `editor-only`; built with `Path::join`), and a binary-level test in
   `crates/quarto/tests/integration/` that starts `q2 preview --no-browser` on a temp directory containing that file
   with `RUST_LOG=warn` and asserts the warning on stderr and that the server still starts; a second case with `"mode": "editor-only"` in the
   file prints no warning.

## Checklist

- [ ] Read GE-1\'s contract (status response shape) and GE-3\'s notes (the `HubConfig` option and the status
  accessor). Record both names in "Notes from the run".
- [ ] Write the parse tests (item 1) and run them: `cargo nextest run -p quarto preview_git`. They fail because the
  flag does not exist.
- [ ] Write the library tests (item 2) and the route test (item 3), registered in
  `crates/quarto-preview/tests/integration/main.rs` in alphabetical order. They fail to compile or fail.
- [ ] Write the warning tests (item 4). They fail.
- [ ] Add `PreviewGit` (`Ignore` default, `Track`) to `quarto-preview` next to `PreviewUi`, and `PreviewConfig::git`.
  Update every literal `PreviewConfig {` site (recount with the grep above) with `git: Default::default()`.
- [ ] Add `PreviewGitArg` and the `--git` argument in `main.rs`: `value_enum`, `default_value_t = PreviewGitArg::Ignore`,
  no `default_missing_value`. Add `git` to the three conflict lists. Write the help text: what `track` does (this
  preview owns the project\'s pointer: it keeps docs in step with git checkouts), that `ignore` is today\'s behavior,
  "(experimental)", and that colocated jj repos work.
- [ ] Carry `args.git` through `PreviewArgs` and into `PreviewConfig`; in `build_hub_config` set the GE-3 hub option
  from it.
- [ ] Add `GET /api/preview/git-status` to `extend_with_preview`, its handler reading the GE-3 accessor through
  `State<SharedContext>`. Return exactly the GE-1 shape; add no field.
- [ ] Add `extension_bound` and call it once after the project root is resolved in `commands/preview.rs`
  (`:286-300`), emitting `tracing::warn!` with the text: this directory is bound to a Quarto Hub folder
  (`.quarto-hub/project.json`), so running a hub here syncs the same files twice. Warn only. `extension_bound` is false when `project.json` says `mode: editor-only` (G28). Not run for
  `--no-project`, single-file mode, or `--hub-url` (no directory is served). No new Q error code, so no error
  catalog or `docs/errors` work.
- [ ] `--no-project` with `--git track` runs without git logic and logs one line saying so (detail chosen by the plan
  author; confirm). G33 lists no conflict for it.
- [ ] Run the tests until green; run the Gates below.
- [ ] End-to-end verification (below); record the invocation and output.
- [ ] Update this checklist and fill "Notes from the run".

## Gates

```bash
cargo clippy -p quarto -p quarto-preview -p quarto-hub --all-targets -- -D warnings
cargo nextest run -p quarto -p quarto-preview -p quarto-hub
cargo xtask lint
cargo clippy -p quarto -p quarto-preview --all-targets --target x86_64-pc-windows-msvc -- -D warnings
```

The Windows line is the cross-check from `.claude/rules/cross-platform.md`. If a crate cannot cross-check (the
`aws-lc` build scripts), say so in the notes; GE-15 covers it natively.

## End-to-end verification

The flag is user visible, so tests alone do not finish this plan (`AGENTS.md`).

1. `cargo build --bin q2`. The `quarto` crate changed; the preview SPA and the WASM are untouched, so the SPA chain
   is not needed here.
2. Make a temp git repo with a committed `index.qmd`. Run `cargo run --bin q2 -- preview --no-browser --git track
   <dir>` and read the banner.
3. `curl -s http://127.0.0.1:<port>/api/preview/git-status` returns tracking true, paused false. `touch` the file
   named by `git -C <dir> rev-parse --git-path index.lock`, curl again (paused true), remove it, curl again.
4. Run the same without `--git` (tracking false). Run `--git` bare and `--git track --join x` and read both errors.
5. `mkdir -p <dir>/.quarto-hub && echo {} > <dir>/.quarto-hub/project.json`, start `q2 preview` on it with
   `RUST_LOG=warn`, and read the warning.
6. Record each invocation and a snippet of the observed output here, with an explicit note that the output was
   inspected.

## Exit criteria

- The flag, the route, and the warning exist and behave as G33 and G28 say; every test in the specifications passes;
  the Gates are green; the end-to-end run is recorded in the notes.
- No behavior of `--git ignore` changed: the full `quarto-preview` suite passes untouched.

## Out of scope

- The sync behavior itself (GE-3) and the web indicator UI that polls the route (GE-7).
- User docs for the flag and the gaps list (GE-18).
- The e2e of git operations through browsers (GE-13).
- Refusing to start next to a bound folder, and a hub-written marker file for discovery (held items).

## Notes from the run

Not started.
