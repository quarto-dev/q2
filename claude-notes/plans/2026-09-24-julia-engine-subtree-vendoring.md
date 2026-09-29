# Vendoring the julia engine as an extension subtree (epic Step 4)

**Status:** DONE (2026-09-29), ready for PR. All four phases complete: payload
registered (native + WASM, F1 fixed), the Q9 diagnostic added (`Q-18-3`), the
hand-maintained fixture deleted, gates green (`cargo clippy -p quarto-core
--all-targets -- -D warnings`, `cargo xtask lint`, targeted
`cargo nextest -p quarto-core` including live julia+deno J1–J7 and the xtask
subtree-table tests, full `cargo nextest run --workspace`).
**Parent:** [2026-09-03-julia-engine-static-declarations-epic.md](2026-09-03-julia-engine-static-declarations-epic.md)
— this plan scopes Step 4's julia-specific remainder. The infrastructure
landed on `main` as PR #717 (`extension-subtree-infra`, merged 2026-09-24).
This branch (`julia-engine-subtree`) was rebased onto current `main`
(2026-09-29) — including a manual cherry-pick replay to drop and
freshly re-run the `git subtree add` step, since replaying a `git subtree`
squash/merge commit via `git rebase` corrupts it (the squash commit's diff
gets applied outside its prefix-scoped merge context, producing spurious
root-level conflicts). The `orange-book` subtree (landed on `main` after this
branch diverged, via the same infrastructure) required reconciling two
places that both added a table/registration row:
`crates/xtask/src/pull_extension_subtree.rs`'s `subtrees()` table and
`crates/quarto-core/src/extension/mod.rs`'s `EXTENSION_SUBTREE_PAYLOADS` /
`EXTENSION_SUBTREE_NAMES` — both now list both subtrees, and
`wasm-quarto-hub-client`'s `populate_extension_subtrees` (this plan's F1 fix)
now embeds both per-subtree `_extensions/` dirs, since `main` had never
gotten the WASM fix for `orange-book`.

## Resumption (2026-09-29)

Plan 7b (native content-processor registry, `processor:` on `claims-files`)
landed on `main` (`79f70d0ef`, `de2e64b52`). Gordon: "we changed course and
started using my fork of quarto-julia-engine. that unblocks everything on
that side. also the percent script transformations / processor field have
landed." Concretely:

1. Added `claims-files: [{extension: .jl, processor: {name: percent,
   language: julia}}]` to `gordonwoodhull/quarto-julia-engine`'s
   `q2-static-declarations` branch (commit `0a2b98f`), matching the syntax
   Plan 7b's own Phase 7 commit (`de2e64b52`) already used for the committed
   `tests/fixtures/extensions/julia-engine` manifest. Pushed to the fork.
2. Re-ran `cargo xtask pull-extension-subtree julia-engine` in this worktree
   (stashed the uncommitted Phase 2 work first — `git subtree pull` needs a
   clean tree — then reapplied it) to pick up the new commit. Vendored copy
   now carries `claims-files` too.
3. Q-16-10 no longer fires — confirmed by running the full `quarto-core`
   suite: the only failures left were the anticipated ones below, not the
   diagnostic.
4. Fixed the two collision tests that actually existed (not five — see
   below): `p1_4_name_collision_errors_and_names_both_contributors` (renamed
   its synthetic collision engine from `julia` to `collide-synth`, since
   `julia` now also collides with the real bundled engine) and
   `c3_path_map_entry_reserved_skip_no_error_no_order_change` (now asserts
   `contribution_order == ["julia"]` instead of empty, mirroring the
   already-fixed `p0_no_extension_project_builds_builtins_only`).
5. Fixed `julia_engine_e2e.rs`: all seven J1–J7 rows were failing with
   `Engine name collision: both 'julia-engine' and 'julia-engine' register
   engine 'julia'` — installing the old hand-maintained fixture into
   `_extensions/julia-engine` now collides with the bundled copy.
   `setup_julia_project`/`setup_julia_website_project` no longer install any
   extension; the engine resolves purely from
   `all_builtin_extension_roots`. Verified live (real deno + julia on PATH):
   all 11 targeted tests pass, including J7 (failed-run leak check) — the
   plain upstream v0.2.2 close-on-failure fix in the bundled fork is
   sufficient for that scenario. `PC4a` (busy-worker forceclose recovery)
   stays `#[ignore]`d/env-gated and untouched; it depends on q2's
   hand-rolled `errorRunClose` hardening
   (`worker-busy-recovery` branch on the fork, commits `41ba4dc`/`13efa45`,
   based on the same `e04ef86` tip as `q2-static-declarations`, not yet
   merged into it) which the bundled copy does not carry. Not addressed here
   — out of this plan's immediate scope; flagged for Phase 4/epic Step 5 if
   PC4a's scenario ever needs to be exercised against the bundled engine.
6. The hand-maintained fixture at
   `crates/quarto-core/tests/fixtures/extensions/julia-engine/` is now
   unreferenced by any test (the file-existence grep came up empty except
   for this file, and this file no longer uses it). Left in place, not
   deleted — Phase 4 below still owns the "fixture vs. bundled" decision
   explicitly, including whether to fold `worker-busy-recovery`'s hardening
   into the fork so the bundled copy is the same, since dropping the fixture
   now would preempt that call.

## Why paused (2026-09-24, historical)

Registering the real payload (Phase 2) surfaced 12 test failures in
`quarto-core`. Two categories:

1. **Fixture name collisions (5 tests)** — mechanical: test fixtures that
   declare a test engine also named `julia` now collide with the real bundled
   one. Trivial once unpaused.
2. **Q-16-10 fires on every render, unconditionally (4 tests + a real
   product-facing regression)** — the bundled manifest has no `claims-files`
   (correctly — Julia's `.jl` claim is a content sniff, `isPercentScript`,
   which q2 cannot statically declare without `processor:` support). So
   `build_engine_registry` warns "engine extension `julia-engine` does not
   declare static claims... not declared: claims-files" on **every single q2
   render**, project or single-file, whether or not Julia is touched. Verified
   this is real `project_diagnostics`, printed CLI output, not test plumbing.
   Confirmed against Gordon's local fork checkout
   (`~/src/quarto-julia-engine`, `q2-static-declarations` branch) — the
   manifest is identical to what's vendored; there's no fix hiding there.
   Declaring `claims-files: []` would be actively wrong (falsely tells q2
   "claims no files," silently breaking `.jl` file detection).

**Gordon's call (2026-09-24):** don't build a suppression mechanism for this —
implement [Plan 7b](2026-07-08-plan7b-native-content-processors.md) (the
native content-processor registry: `processor:` on `claims-files`, percent +
spin conversion, zero-Pass-1-launch) properly first, on a fresh worktree with
a dedicated agent. Once 7b's Phase 7 (TS-engine native path, julia `.jl`
validation flip) lands, the fork's manifest gains a real, truthful
`claims-files: [{extension: .jl, processor: {name: percent, language: julia}}]`
entry and Q-16-10 clears at the source — no suppression code needed.

**Resumed 2026-09-29 — see "Resumption" above for what actually happened**
(the plan had anticipated "5 fixture-collision tests"; ground truth was 2 in
`engine_registry_build.rs` + all 7 rows of `julia_engine_e2e.rs`, the latter
via a different mechanism — a real name collision with the fixture install,
not a Q-16-10-style diagnostic).

**Sign-offs (Gordon, 2026-09-24):**
- **Q8: whole repo.** Subtree all of `gordonwoodhull/quarto-julia-engine`
  (~14M in git history), embed only `_extensions/` (~68K) in binaries. Keeps
  `git subtree` merge tracking; mirrors Q1.
- **Q9: diagnostic.** A `{julia}` cell on a machine without Julia produces a
  clear Q-* diagnostic (with docs page), not silent fallback to jupyter.

## Overview

Step 2c's pivot (2026-09-24, PumasAI#15 closed unmerged) made the fork's
`q2-static-declarations` branch (commit `7d72bda`, = upstream v0.2.2 + the
static declarations) q2's upstream of record. That branch carries `name:` /
`claims:` / `file-extensions:` in `_extension.yml`, so the bundled copy gives
q2 pass-1 static resolution for Julia out of the box.

What this PR does: vendors the repo, registers the `_extensions/` payload,
fixes the WASM subtree shape (found while planning — see F1 below), adds the
Q9 diagnostic, and decides the hand-maintained fixture's future.

## Findings while planning

### F1 — the WASM subtree path has the wrong shape for a real payload (latent, untested)

`wasm-quarto-hub-client/src/lib.rs::populate_extension_subtrees` embeds
**all of `resources/extension-subtrees/`** (would be 14M once julia is
vendored, tests and CI config included), and
`builtin_extension_subtree_roots`'s WASM branch returns the
`extension-subtrees` VFS dir itself as a root — but
`discover_extensions` scans each root with `scan_extensions_dir`, i.e. a
root's **children must be extensions**. Native is correct (per-subtree
bundles point at `<subtree>/_extensions`); WASM would scan
`extension-subtrees/julia-engine/` as if it were an extension and find
nothing. Harmless while `EXTENSION_SUBTREE_PAYLOADS` is empty; broken the
moment a real payload lands. The synth-echo e2e was native-only, so this was
never exercised. **Phase 2 fixes WASM to per-subtree `_extensions/` statics,
mirroring native.**

## Work items

### Phase 1 — vendor the subtree

- [x] `crates/xtask/src/pull_extension_subtree.rs`: add the first real
      `SUBTREES` row — name `julia-engine`, prefix
      `resources/extension-subtrees/julia-engine`, remote
      `https://github.com/gordonwoodhull/quarto-julia-engine.git`, branch
      `q2-static-declarations`. Unit-test the row's shape (prefix ==
      `resources/extension-subtrees/<name>`; remote/branch non-empty) so a
      malformed future row fails in `cargo nextest -p xtask`, not mid-pull.
      *(Done as `subtrees()` — a fn, not a const: `SubtreeConfig` owns
      `String`s, which can't be built non-empty in a const context. TDD: RED
      against the empty table, GREEN with the row.)*
- [x] Run `cargo xtask pull-extension-subtree julia-engine` (real `git
      subtree add --squash`; creates the squash + merge commits on this
      branch). Verify: prefix exists, `_extensions/julia-engine/_extension.yml`
      carries the static declarations, `git log --grep` finds the split.
      *(Verified: v0.2.2 manifest with `name`/`claims`/`file-extensions`;
      squash tree is only 196K on disk — F8's "14M" was the upstream repo
      including its `.git` history, which `--squash` never imports;
      `_extensions/` payload 68K as predicted; split trailer points at the
      fork's `7d72bda`.)*
      **Found and fixed en route (TDD regression test):** the Phase 1 port's
      default root was `create_worktree::repo_root()`, which resolves the
      *main* checkout via `--git-common-dir` — from a linked worktree the
      command ran `git subtree add` against the main checkout's dirty tree
      and died with "working tree has modifications". Now uses
      `git rev-parse --show-toplevel` (the invoking worktree).
- [x] Gate: clippy + `cargo nextest run -p xtask`. *(180 passed, +2: the
      table-shape test and the worktree-root regression test.)*

### Phase 2 — register the payload (native + WASM fix)

- [x] **Tests first:**
  - [x] native discovery test with **no env override and no user install**:
        `all_builtin_extension_roots` includes the extracted julia payload and
        `discover_extensions` finds `julia-engine` — this is the first
        exercise of the real `EXTENSION_SUBTREE_PAYLOADS` /
        `ResourceBundle` extraction leg (flagged as untested in PR #717).
        *(`builtin_extension_subtree_roots_extracts_bundled_julia_payload`.)*
  - [x] static-claim test: the bundled `_extension.yml`'s `claims:` /
        `file-extensions:` reach the engine registry (pass-1 resolution,
        no engine load). *(`bundled_julia_engine_discovered_with_static_declarations`.)*
- [x] `extension/mod.rs`: `JULIA_ENGINE_SUBTREE` `include_dir!` static scoped
      to `resources/extension-subtrees/julia-engine/_extensions` +
      `ResourceBundle`, registered in `EXTENSION_SUBTREE_PAYLOADS`.
- [x] **WASM (F1):** replace the whole-dir embed in
      `populate_extension_subtrees` with a per-subtree embed of
      `julia-engine/_extensions` at
      `<prefix>/extension-subtrees/julia-engine/_extensions/…`, and make the
      WASM branch of `builtin_extension_subtree_roots` return the
      per-subtree `_extensions` VFS dirs (same shape contract as native).
      Verified it actually builds for `wasm32-unknown-unknown` (Homebrew LLVM
      clang + the documented `CFLAGS_wasm32_unknown_unknown`/wasm-sysroot
      env, per `dev-docs/wasm.md`) — clean build, no new warnings from this
      change. (`cargo clippy` on this crate hits two pre-existing
      `too_many_arguments` errors in unrelated functions authored 2026-05-01,
      not introduced here — not fixed, out of this plan's scope.)
- [x] Update `resources/extension-subtrees/README.md` (no longer a
      placeholder-only dir) and the runbook
      (`claude-notes/instructions/extension-subtrees.md`) with the WASM
      per-subtree rule so the next subtree doesn't reintroduce F1.
- [x] Gate: clippy (`-p quarto-core --all-targets -- -D warnings`, clean) +
      `cargo nextest run -p quarto-core` (targeted collision +
      `julia_engine_e2e` rows: 11/11 green, live deno+julia). Full workspace
      `cargo nextest run --workspace` run at this phase boundary per
      `CLAUDE.md`'s testing rule (result recorded below); full
      `cargo xtask verify` (hub-client build + WASM leg end-to-end via the
      npm scripts) not run — out of scope for this pass, left for Phase 4's
      wrap-up gate.

### Phase 3 — Q9 diagnostic (no Julia on the machine)

- [x] Investigate where a missing Julia surfaces today (engine host startup /
      QuartoNotebookRunner instantiation) and pick the earliest detection
      point that has source context for a good diagnostic. *(The `julia`
      subprocess is spawned deep inside the shared Deno host by the bundled
      `julia-engine.js`, where a missing binary surfaced as a raw, unhandled
      Deno spawn exception with no q2-side wrapping. Earliest point with
      source context that doesn't require touching the vendored fork:
      `TsEngine::ensure_loaded` in `crates/quarto-core/src/engine/ts_engine.rs`,
      gated on `self.name == "julia"` — the one place that both knows "this is
      the julia engine" and can return before any subprocess spawns.)*
- [x] **Test first:** `{julia}` cell, no `julia` on PATH → the new Q-* code,
      not a raw subprocess/engine-host error. *(`ensure_loaded_reports_runtime_not_found_when_julia_binary_missing`
      and `ensure_loaded_skips_julia_check_for_other_engines` in
      `ts_engine.rs`, plus `runtime_not_found_is_a_coded_error_naming_engine_and_runtime`
      in `engine/diagnostics.rs`.)*
- [x] Add the error-catalog entry **and** its `docs/errors/<subsystem>/<code>.qmd`
      page **and** the sidebar entry in the same commit (xtask lint rules
      `error-docs-page-missing` / `error-docs-sidebar-unlisted` enforce both).
      *(`Q-18-3` — reuses the existing `ExecutionError::RuntimeNotFound`
      variant, previously uncoded, so knitr's and jupyter's own
      runtime-not-found errors get the same coded diagnostic as a side
      effect.)*
- [x] Gate: clippy + per-crate nextest. *(`cargo clippy -p quarto-core
      --all-targets -- -D warnings` clean; `cargo xtask lint` clean.)*

### Phase 4 — fixture future + wrap-up

- [x] Decide: the hand-maintained julia fixture (v0.2.1 + our hardened
      worker-close) vs. the bundled copy (v0.2.2 + declarations). **Decided
      (Gordon, 2026-09-29): delete the fixture, do not merge the fork's
      `worker-busy-recovery` hardening.** `julia_engine_e2e.rs` already
      resolves purely from the bundled subtree (Phase 2); the one remaining
      user, `ts_engine.rs`'s `julia_fixture_jl_percent_converts_natively`,
      now reads the bundled subtree's `_extension.yml` instead (identical
      `claims-files` shape). J7 (failed-run leak check) passes live against
      the bundled copy without the hardening, confirming plain upstream
      v0.2.2's close-on-failure fix is sufficient.
- [x] Reconcile epic Step 4 checklist; update runbook if the fixture decision
      changes anything. *(Epic doc's Step 4 "decide the fixture's future" and
      Step 5 "fixture drift management" items both marked resolved/moot.)*
- [x] Full `cargo xtask verify`; PR against `main` (the infra branch merged as
      PR #717 on 2026-09-24, so this stacks directly on `main`, not on
      `feature/extension-subtree-infra`). *(Green: lint, fmt, workspace build,
      full `cargo nextest run --workspace` — 15317/15317 passed, 201 skipped,
      0 failed — ts-packages build, hub-client build including the WASM leg,
      and hub-client tests all passed.)*

## Explicitly out of scope

- Re-filing the upstream PR to PumasAI (awaits quarto-cli#14936 in a stable
  Q1 release — epic Step 2b).
- `claims-files` `processor:` support (TS-engines epic Plan 7b; the schema
  side already shipped in quarto-cli#14936).
- Jupyter-over-julia default questions (F2 is Q1-side only).
