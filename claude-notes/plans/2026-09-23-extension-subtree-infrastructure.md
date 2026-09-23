# Extension-subtree infrastructure: `xtask pull-extension-subtree` + bundled-payload discovery

**Status:** Phase 2 done. **Unblocked** — deliberately independent of the two
external PRs ([quarto-dev/quarto-cli#14936](https://github.com/quarto-dev/quarto-cli/pull/14936),
[PumasAI/quarto-julia-engine#15](https://github.com/PumasAI/quarto-julia-engine/pull/15)).
**Parent:** [2026-09-03-julia-engine-static-declarations-epic.md](2026-09-03-julia-engine-static-declarations-epic.md)
— this plan is **Step 4's infrastructure, pulled out of the gated sequence**
(Gordon, 2026-09-23: "I don't think we need to wait on implementation of the
infra in this area — pull it out; it can use a fake extension for testing").
The julia-specific remainder stays in the epic's Step 4, gated as before;
once this plan lands, vendoring julia is one `SUBTREES` row + one embed
registration, not a project.

## Overview

Quarto 1 vendors whole extension repos as git subtrees under
`src/resources/extension-subtrees/<name>/`, kept in sync by a hidden dev
command (`src/command/dev-call/pull-git-subtree/cmd.ts`), and discovers them
via a **separate root** from `resourcePath("extensions")`
(`extension/extension.ts:680-695`). q2 has none of this (greppably zero
references), but has all the adjacent machinery:

- `ResourceBundle` + `include_dir!` embedding (`resources.rs:383-453`), used by
  the single `BUILTIN_EXTENSIONS` static (`extension/mod.rs:65-81`).
- `discover_extensions(input, project_dir, builtin_extensions_dir, runtime)`
  (`extension/discover.rs:27-80`) — built-ins scanned first, last-match-wins
  so user extensions override. The builtin dir is a **single** `Option<&Path>`.
- Synthetic engine fixtures + hermetic at-test-time bundling
  (`tests/fixtures/extensions/{alpha,echo-engine,…}`,
  `tests/integration/engine_fixture_build.rs::ensure_bundle`) — the
  fake-extension pattern this plan reuses.
- xtask: one file per command, clap enum in `main.rs`, `repo_root()`
  (`create_worktree.rs:536`), `nested_command("git")` (`util.rs:101`).

The work, in three pieces: **(1)** the maintenance command (a port of Q1's
`pull-git-subtree`), **(2)** pluralizing builtin-extension discovery so
subtree payloads are a second (third…) builtin root, **(3)** proving it
end-to-end with a **fake extension** — no real engine is vendored by this
plan.

## Design decisions (recommendations — confirm before Phase 1)

- **D1 (resolves epic Q7): list of builtin roots, mirroring Q1 — not merging
  into the existing bundle.** `ResourceBundle` is per-directory, and each
  subtree's payload needs its own `include_dir!` static anyway (embedding all
  of `resources/extension-subtrees/` would put whole repos — tests, CI
  config, 14M for julia — into the binary; F8). So: per-subtree payload
  statics, and `discover_extensions`/`discover_project_extensions` gain a
  `&[&Path]` builtin-roots parameter (or a sibling taking one) scanned in
  order before user extensions. This is a small, mechanical signature change
  at three direct call sites (`project/mod.rs:1685` inside
  `discover_extensions_only`, `project/mod.rs:2207`, `stage/context.rs:299`)
  plus the WASM branch inside `discover_extensions_only` itself
  (`project/mod.rs:1677-1684` — see the Phase 2 item below). Note
  `discover_extensions_only`'s own two callers (`project/mod.rs:1759` and
  `:1954`) don't take a builtin-dir parameter and need no signature edit —
  they inherit the fix once `discover_extensions_only`'s internals change.
- **D2 (epic Q8, scoped): keep `git subtree` merge tracking.** The fake costs
  KBs, so this plan doesn't force the 14M question; but the precedent it sets
  is "subtree the whole repo, embed only `_extensions/`", matching Q1. The
  final julia-size call stays with the epic's Step 4.
- **D3: the fake never ships in release binaries.** The fixture lives under
  `crates/quarto-core/tests/fixtures/extension-subtrees/` (NOT `resources/`),
  and the roots function honors a dev-only env override
  (`QUARTO_EXTENSION_SUBTREES_DIR`, following the `QUARTO_PERF_STATS` naming
  convention) checked before the embedded bundle. Tests point it at the
  fixture; production builds contain an embedded
  `resources/extension-subtrees/` holding only a README placeholder
  (`include_dir!` errors on a missing dir). No cargo features, no cfg games,
  no fake engine in shipped `q2`.
- **D4: no `filterBundledSubtreeEngines` analogue needed** (closes F8's
  untraced item). Survey finding: q2's extension-contributed engines are
  **registry-only** (`Contributes.engines` → `build_engine_registry`); they
  never enter the merged metadata handed to the writer. Q1's filter exists
  because Q1 puts resolved engine objects into pandoc metadata. Verified by
  test in Phase 3 rather than trusted.

## Work items

### Phase 1 — `cargo xtask pull-extension-subtree` (TDD)

Port of Q1's `src/command/dev-call/pull-git-subtree/cmd.ts` (SUBTREES table,
last-split detection, add-vs-pull, no-op), dropping the `QUARTO_ROOT`
dependency (xtask knows the repo root via `create_worktree::repo_root()`).

- [x] **Tests first**, as inline `#[cfg(test)] mod tests` in
      `pull_extension_subtree.rs` (xtask has no `tests/` directory anywhere
      and no `[[test]]` in its `Cargo.toml` today — every existing xtask test,
      e.g. `create_worktree.rs:896`, is an inline unit-test module; this is
      the first one to shell out to real `git`, which is fine, just new):
      build a fixture *remote* in a temp dir (`git init`, two commits adding
      `_extensions/fake/_extension.yml`), and a fixture *consumer* repo in a
      second temp dir **with an initial commit** (`git subtree add` fails
      with "ambiguous argument 'HEAD'" / "working tree has modifications" on
      a zero-commit repo — verified empirically; an empty `--allow-empty`
      commit is enough); run the command with `QUARTO_SUBTREE_ROOT=<consumer>`
      + `--table <json>` overrides:
  - [x] initial run performs `git subtree add --squash` — prefix created, merge
    commit carries the `git-subtree-dir: <prefix>` trailer (empirically: the
    trailer lands on the *squash* commit's body, not the merge commit's —
    `find_last_split`'s `git log --grep` walks all reachable commits so this
    doesn't matter for correctness, but it's why the test asserts via
    `--grep` rather than `log -1 --pretty=%b` on HEAD);
  - [x] immediate second run is a **no-op** ("No new commits to merge");
  - [x] after a new upstream commit, run performs `subtree pull --squash` and the
    new file appears under the prefix;
  - [x] unknown subtree name → error listing available names;
  - [x] omitting the name (or `all`) processes every table row.
- [x] `crates/xtask/src/pull_extension_subtree.rs`: `SubtreeConfig { name,
      prefix, remote_url, remote_branch }` (serde snake_case, matching the
      Rust field names — this is a q2-only dev seam, no need to mirror Q1's
      camelCase JS shape), `const SUBTREES: &[SubtreeConfig]`
      — **initially empty** (the julia row is the gated epic Step 4's one-line
      addition; the fake is a test fixture, not a production row).
      `find_last_split` via `git log --grep="git-subtree-dir: <prefix>$" -1
      --pretty=%b` → parse `git-subtree-split:` line; `fetch` → `rev-parse
      FETCH_HEAD`; prefix missing OR no split → `subtree add --squash`; else
      `git log --oneline <split>..FETCH_HEAD -1` empty → no-op; else `subtree
      pull --squash`. All git via `crate::util::nested_command("git")`.
      Deviates from Q1 in one place, per the plan's own contract rather than
      Q1's literal script: after `subtree add`, the Rust port returns
      immediately (`Added`) instead of falling through to also check the
      commit range and potentially run a second `subtree pull` in the same
      invocation — the JS does that fall-through (an apparent quirk, not
      verified as intentional), but nothing in this plan's contract calls
      for it.
- [x] Test-only overrides: `QUARTO_SUBTREE_ROOT` (operate on a repo other than
      the real one) and `--table <path-to-json>` (replace SUBTREES). Both are
      dev/test seams, documented as such in `--help`.
- [x] Register in `main.rs` (module, `Command` variant, match arm, doc-comment
      list).
- [x] Gate: `cargo clippy -p xtask --all-targets -- -D warnings` + `cargo
      nextest run -p xtask`. Both green (178 tests, up from 174; +4 new).
      Also smoke-tested the real `cargo run -p xtask -- pull-extension-subtree`
      binary end-to-end against a scratch fixture remote/consumer (unknown-name
      error path + a real add), not just the in-process tests.

### Phase 2 — multi-root builtin discovery + subtree roots plumbing

- [x] **Tests first** (`extension/discover.rs` unit tests, following the
      existing temp-builtin-dir tests at lines 660/700/746):
  - [x] two builtin roots both scanned, in order, before user extensions
    (`test_multiple_builtin_roots_scanned_in_order_before_user`);
  - [x] user `_extensions/` still overrides a subtree-bundled extension of the
    same name (last-match-wins preserved across the new root boundary)
    (`test_user_extension_overrides_subtree_bundled_extension`);
  - [x] roots function: env override honored; empty/missing embedded dir →
    empty list, discovery proceeds with user extensions only (current `None`
    behavior generalized) — `extension::tests::builtin_extension_subtree_roots_honors_env_override`
    and `..._empty_when_no_subtrees_registered` in `extension/mod.rs`.
    (Caught a real bug while writing the first two: the fixture `_extension.yml`s
    lacked a `contributes:` key, which `read_extension` requires — every
    extension silently failed to load and both new tests failed with 0 found.
    Fixed by adding a minimal `contributes.shortcodes` block, matching the
    existing passing fixtures' shape.)
- [x] `extension/mod.rs`: `builtin_extension_subtree_roots(runtime) ->
      Vec<PathBuf>` — env override (`QUARTO_EXTENSION_SUBTREES_DIR`, same
      name as D3 and the Phase 3 tests) → per-subtree embedded payloads
      (native `ResourceBundle`; none registered yet beyond the placeholder,
      so this list is empty in production until a real subtree is added) →
      WASM VFS path under `RESOURCE_PATH_PREFIX` (via `quarto_sass::RESOURCE_PATH_PREFIX`,
      already a quarto-core dependency). Also added `all_builtin_extension_roots`,
      the combined helper the plan's next item calls for.
- [x] Pluralize `discover_extensions` / `discover_project_extensions` builtin
      parameter (`Option<&Path>` → `&[&Path]`, chosen directly — the 3
      production call sites needed rewriting regardless, since none of them
      could keep a single-root signature once both the regular builtin dir
      and subtree roots must be scanned); updated the three direct call sites
      `project/mod.rs` (inside `discover_extensions_only`), `project/mod.rs`
      (`ProjectConfig::parse_config`), `stage/context.rs` (`StageContext::new`),
      plus all 13 existing test call sites in `discover.rs` itself.
- [x] Fixed the WASM inconsistency: `discover_extensions_only` now calls
      `all_builtin_extension_roots(runtime)` unconditionally (native and WASM
      alike) instead of hardcoding `None` on WASM — the `#[cfg(...)]` split
      that gave WASM no built-ins is gone entirely. Because the fix lives
      inside `discover_extensions_only`, neither of its two callers
      (`discover_extensions_and_build_registry` / `ProjectContext::discover`)
      needed its own edit. Phase 3's e2e test (single-file render) will
      confirm the fix reaches that specific path.
- [x] `resources/extension-subtrees/README.md` placeholder explaining the
      directory's purpose (so `include_dir!` has something to embed — git
      does not track empty directories, and both the native and WASM
      `include_dir!` call sites require the directory to exist at compile
      time).
- [x] WASM parallel: `wasm-quarto-hub-client/src/lib.rs` gains
      `populate_extension_subtrees` mirroring `populate_builtin_extensions`,
      embedded from `resources/extension-subtrees/` and wired into
      `populate_vfs_with_embedded_resources`.
- [x] Gate: clippy clean; `cargo nextest run -p quarto-core` green (4825
      passed, 0 failed, 31 skipped — includes the 4 new tests above). Full
      `cargo xtask verify` green: workspace nextest 14483 passed, 0 failed,
      200 skipped (+4 over the Phase 1 baseline of 14479, matching exactly
      the 4 new tests — no stray/duplicated tests crept in). Aside,
      unrelated to this plan: this worktree's `node_modules` was stale
      (dated from before the 2026-09-23 branch repurpose) and hit the
      documented `@esbuild/<platform>` lockfile bug (CLAUDE.md's hub-client
      section) on the first hub-client build attempt; fixed locally by
      installing the missing optional package directly rather than
      regenerating `package-lock.json` (regeneration didn't actually restore
      the platform entries — a deeper pre-existing npm/monorepo quirk, out
      of scope here). `package-lock.json` is untouched by this plan.
      `crates/wasm-quarto-hub-client/Cargo.lock` *did* pick up a large,
      legitimate diff — that crate's lockfile had never been resolved fresh
      in this worktree before; kept as-is since it reflects what the
      successful build actually used.

### Phase 3 — fake-extension end-to-end

- [ ] Fixture `crates/quarto-core/tests/fixtures/extension-subtrees/synth-echo/`
      shaped like a vendored repo: `_extensions/synth-echo/_extension.yml` +
      `src/synth-echo.ts`, modeled on the `echo-engine` fixture, claiming a
      `synthsub` language (`kind: primary`). Note `HERMETIC_FIXTURES` /
      `ensure_bundle` are keyed to `tests/fixtures/extensions/` — extend
      `engine_fixture_build.rs` with a path-aware variant (or a second fixture
      root) so the subtree fixture's `dist/` also regenerates hermetically at
      test time.
- [ ] **Tests first**, registered in `tests/integration/main.rs`:
  - discovery: with `QUARTO_EXTENSION_SUBTREES_DIR=<fixture root>`,
    `discover_extensions` finds `synth-echo` with **zero** `_extensions/`
    install;
  - e2e: a doc with a ` ```{synthsub} ` cell renders through the real path
    (`render_to_file` → discovery → resolution → Deno engine host, the
    `synth_engines_e2e.rs` shape) and the output contains the executed result
    — gated on `deno_available()` like its siblings. Run this **as a
    single-file render** (no `_quarto.yml`), so it exercises
    `discover_extensions_and_build_registry` (`project/mod.rs:1759`) — the
    other caller of `discover_extensions_only`, distinct from the
    project-mode path Phase 2's unit tests already cover at `:1954` — closing
    the loop on the WASM-branch fix from Phase 2;
  - D4 verification: the rendered document's template metadata contains no
    trace of the bundled engine (no `engines` key introduced by
    extension-contributed engines).
- [ ] Implement whatever the tests surface (expected: nothing beyond Phases
      1–2; this phase is the proof).
- [ ] Docs: short `claude-notes/instructions/extension-subtrees.md` (or a
      section in an existing note) — how to add a real subtree: one
      `SUBTREES` row, run `cargo xtask pull-extension-subtree <name>`, one
      `include_dir!` payload static + registration. This is the runbook the
      epic's Step 4 will follow for julia.
- [ ] Gate: workspace `cargo nextest run` + full `cargo xtask verify`
      (pre-commit checklist), report pass/skip delta vs live baseline.

## Explicitly out of scope

- Vendoring the **julia** engine (one `SUBTREES` row + payload registration
  once this lands) — that is the epic's Step 4, still gated on PumasAI#15
  providing a stable branch and on the Q8 14M sign-off.
- The fixture-convergence question (replacing the hand-maintained julia
  fixture with the bundled copy) — epic Step 5.
- Q9 (UX on a machine without Julia) — epic Step 4.
- `claims-files` `processor:` support in q2's parser — that's TS-engines
  epic Plan 7b, orthogonal.
