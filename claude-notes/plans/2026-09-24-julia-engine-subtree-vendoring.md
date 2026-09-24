# Vendoring the julia engine as an extension subtree (epic Step 4)

**Status:** In progress (2026-09-24).
**Parent:** [2026-09-03-julia-engine-static-declarations-epic.md](2026-09-03-julia-engine-static-declarations-epic.md)
— this plan scopes Step 4's julia-specific remainder. The infrastructure
landed in [2026-09-23-extension-subtree-infrastructure.md](2026-09-23-extension-subtree-infrastructure.md)
(PR #717); this work stacks on that branch (`julia-engine-subtree` on top of
`feature/extension-subtree-infra` + the 2c-pivot plan commit).

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

- [ ] `crates/xtask/src/pull_extension_subtree.rs`: add the first real
      `SUBTREES` row — name `julia-engine`, prefix
      `resources/extension-subtrees/julia-engine`, remote
      `https://github.com/gordonwoodhull/quarto-julia-engine.git`, branch
      `q2-static-declarations`. Unit-test the row's shape (prefix ==
      `resources/extension-subtrees/<name>`; remote/branch non-empty) so a
      malformed future row fails in `cargo nextest -p xtask`, not mid-pull.
- [ ] Run `cargo xtask pull-extension-subtree julia-engine` (real `git
      subtree add --squash`; creates the squash + merge commits on this
      branch). Verify: prefix exists, `_extensions/julia-engine/_extension.yml`
      carries the static declarations, `git log --grep` finds the split.
- [ ] Gate: clippy + `cargo nextest run -p xtask`.

### Phase 2 — register the payload (native + WASM fix)

- [ ] **Tests first:**
  - [ ] native discovery test with **no env override and no user install**:
        `all_builtin_extension_roots` includes the extracted julia payload and
        `discover_extensions` finds `julia-engine` — this is the first
        exercise of the real `EXTENSION_SUBTREE_PAYLOADS` /
        `ResourceBundle` extraction leg (flagged as untested in PR #717).
  - [ ] static-claim test: the bundled `_extension.yml`'s `claims:` /
        `file-extensions:` reach the engine registry (pass-1 resolution,
        no engine load).
- [ ] `extension/mod.rs`: `JULIA_ENGINE_SUBTREE` `include_dir!` static scoped
      to `resources/extension-subtrees/julia-engine/_extensions` +
      `ResourceBundle`, registered in `EXTENSION_SUBTREE_PAYLOADS`.
- [ ] **WASM (F1):** replace the whole-dir embed in
      `populate_extension_subtrees` with a per-subtree embed of
      `julia-engine/_extensions` at
      `<prefix>/extension-subtrees/julia-engine/_extensions/…`, and make the
      WASM branch of `builtin_extension_subtree_roots` return the
      per-subtree `_extensions` VFS dirs (same shape contract as native).
- [ ] Update `resources/extension-subtrees/README.md` (no longer a
      placeholder-only dir) and the runbook
      (`claude-notes/instructions/extension-subtrees.md`) with the WASM
      per-subtree rule so the next subtree doesn't reintroduce F1.
- [ ] Gate: clippy + `cargo nextest run -p quarto-core`; full
      `cargo xtask verify` (WASM leg is in scope).

### Phase 3 — Q9 diagnostic (no Julia on the machine)

- [ ] Investigate where a missing Julia surfaces today (engine host startup /
      QuartoNotebookRunner instantiation) and pick the earliest detection
      point that has source context for a good diagnostic.
- [ ] **Test first:** `{julia}` cell, no `julia` on PATH → the new Q-* code,
      not a raw subprocess/engine-host error.
- [ ] Add the error-catalog entry **and** its `docs/errors/<subsystem>/<code>.qmd`
      page **and** the sidebar entry in the same commit (xtask lint rules
      `error-docs-page-missing` / `error-docs-sidebar-unlisted` enforce both).
- [ ] Gate: clippy + per-crate nextest.

### Phase 4 — fixture future + wrap-up

- [ ] Decide: the hand-maintained julia fixture (v0.2.1 + our hardened
      worker-close) vs. the bundled copy (v0.2.2 + declarations). The e2e
      tests deliberately copy the fixture into a temp project — replacing it
      with the bundled copy retires the drift item (epic Step 5) but loses
      the hardened close path. Document the decision in the epic.
- [ ] Reconcile epic Step 4 checklist; update runbook if the fixture decision
      changes anything.
- [ ] Full `cargo xtask verify`; stacked PR with base
      `feature/extension-subtree-infra`.

## Explicitly out of scope

- Re-filing the upstream PR to PumasAI (awaits quarto-cli#14936 in a stable
  Q1 release — epic Step 2b).
- `claims-files` `processor:` support (TS-engines epic Plan 7b; the schema
  side already shipped in quarto-cli#14936).
- Jupyter-over-julia default questions (F2 is Q1-side only).
