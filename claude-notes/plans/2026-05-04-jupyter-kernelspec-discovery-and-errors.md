---
title: 'Jupyter kernelspec discovery: fix venv blindness, improve error messages'
date: 2026-05-04
---

## Overview

Quarto 2's Jupyter engine fails to discover kernels installed inside Python
virtualenvs (e.g. `/Users/.../venv/share/jupyter/kernels/python3`), even
when `jupyter kernelspec list` from the same shell finds them. The
user-visible failure mode is a one-line error with no actionable
information:

```
error: …/convert-test-3.qmd: Error: Execution failed in jupyter: kernelspec 'python3' not found
```

This plan fixes both halves: the discovery gap that *causes* the failure,
and the error reporting that *masks* the cause.

## Root cause

`crates/quarto-core/src/engine/jupyter/kernelspec.rs` calls
`runtimelib::list_kernelspecs()`. That function searches a fixed list of
directories built by `runtimelib::dirs::data_dirs()`:

1. `JUPYTER_PATH` env var (if set)
2. `user_data_dir()` — `~/Library/Jupyter` on macOS, `%APPDATA%/jupyter`
   on Windows, `$XDG_DATA_HOME/jupyter` on Linux
3. `system_data_dirs()` — `/usr/local/share/jupyter`, `/usr/share/jupyter`
   (or `%PROGRAMDATA%\jupyter` on Windows)

The list does **not** include any virtualenv `share/jupyter/kernels`
directory. Upstream runtimelib has the gap documented at
`runtimelib-2.0.0/src/dirs.rs:115`:

```rust
// TODO: Use the sys.prefix from python and add that to the paths
```

Confirmed: `dirs.rs` is byte-identical between runtimelib 1.6.0 and
2.0.0, so the venv-blindness persists in the latest release.

Runtimelib already exposes a helper that asks the `jupyter` CLI directly
(`runtimelib::dirs::ask_jupyter()` — runs `jupyter --paths --json`), but
`list_kernelspecs()` and `find_kernelspec()` never call it.

## Why the error message doesn't help

Runtimelib 2.0 returns `RuntimeError::KernelNotFound { name, available:
Vec<String> }`. Our wrapper `JupyterError::KernelspecNotFound { name }`
discards the `available` list and adds nothing else, so the user sees
only the kernel name. We never report:

- which directories we searched,
- which kernelspecs we *did* find (so the user can compare against
  `jupyter kernelspec list`),
- that `jupyter --paths` would reveal the missing venv path,
- a workaround (e.g. `JUPYTER_PATH=…`).

If the search path had been visible in the error, the user would have
immediately seen the venv path was missing and worked around it.

## Strategy

We will not work around runtimelib in our own crate. Instead, fix it in
runtimelib (in our own fork) and contribute upstream. This keeps the
search semantics in one place and gives every other runtimelib consumer
the same fix.

Ordering principle: **finish all Quarto-side work first**, including
end-to-end validation against the original failing fixture, before
opening any upstream PR. Validating against a real workload may surface
API tweaks; rolling those back into a public PR after the fact is
expensive. Better to discover them while the fork is private.

Sequence:

1. **Upgrade runtimelib 1.4 → 2.0 in this repo** (small, isolated PR).
2. **Fork `runtimed/runtimelib`** to `cscheid/runtimelib`, branch from
   the 2.0.0 tag, add venv discovery + searched-path-bearing error.
3. **Patch `Cargo.toml`** to point at the fork via `[patch.crates-io]`,
   thread the new fields into `JupyterError::KernelspecNotFound`,
   render a remediation hint, and verify end-to-end against the
   original failing fixture *and* a venv shell that has `ipykernel`.
4. **(Last) Open upstream PR** once Phases 2 and 3 are validated and
   the fork API has not needed any further churn.

## Phase 1 — Upgrade to runtimelib 2.0 ✅ (DONE)

Status: complete in this session, awaiting commit.

- [x] Bump `runtimelib = "1.4"` → `"2.0"`, `jupyter-protocol = "1.4"` →
  `"2.0"` in `crates/quarto-core/Cargo.toml`.
- [x] Add a `_ => (mime_type, Value::Null)` fallback arm in
  `media_type_to_mime_entry` (`execute.rs:262`) — `MediaType` is now
  `#[non_exhaustive]` in `jupyter-protocol` 2.0.
- [x] `cargo build --workspace` clean.
- [x] `cargo nextest run --workspace`: 8360 passed.
- [x] `cargo xtask verify --skip-hub-build`: all steps green.
- [x] Reproduced the original failure end-to-end on
  `convert-test-3.qmd` to confirm the upgrade alone does not fix
  the bug (expected — `dirs.rs` is byte-identical between 1.6 and
  2.0).

Beads: **bd-fu0l** (covers the parent bug; this phase is a sub-step).

## Phase 2 — Fork runtimelib and add venv discovery

- [ ] Fork `runtimed/runtimelib` → `cscheid/runtimelib`.
- [ ] Branch off the `v2.0.0` tag as `feat/venv-kernelspec-discovery`.
- [ ] **Test (failing first)** in `runtimelib/src/kernelspec.rs`:
  `find_kernelspec` for a kernel name that lives only in a
  `data_dirs_with_jupyter_paths()` entry (e.g. a temp dir with
  `<dir>/kernels/fake/kernel.json`) succeeds when
  `ask_jupyter()` reports that dir. Use a small fixture-script-as-
  jupyter helper, or stub at the `data_dirs()` level by injecting
  via `JUPYTER_PATH` for the unit test (cleanest).
- [ ] **Test (failing first)**: `RuntimeError::KernelNotFound` carries a
  `searched_paths: Vec<PathBuf>` field; `Display` includes the
  paths.
- [ ] In `runtimelib/src/dirs.rs`, add
  `pub async fn data_dirs_with_jupyter_paths() -> Vec<PathBuf>`
  that:
    1. starts from `data_dirs()`,
    2. calls `ask_jupyter()` with a short timeout (≤2s) and
       gracefully degrades on error,
    3. extracts each path from `paths["data"]` (array of strings),
    4. de-duplicates while preserving order.
  Keep `data_dirs()` unchanged — the new function is additive.
- [ ] In `runtimelib/src/kernelspec.rs`, add
  `list_kernelspecs_with_jupyter_paths()` and have
  `find_kernelspec()` use the augmented dirs. Populate
  `searched_paths` on `KernelNotFound`.
- [ ] In `runtimelib/src/error.rs`, extend `RuntimeError::KernelNotFound`
  with `searched_paths: Vec<PathBuf>` and update its `thiserror`
  `#[error(...)]` template (multi-line message: name, searched
  paths, available kernels).
- [ ] Match upstream conventions: keep the `// TODO: sys.prefix` line
  in place (separate concern), `#[cfg(feature = "tokio-runtime")]`
  gating, the `Result<T>` alias, the same `thiserror` style, and
  tests parallel to `test_list_kernelspec_jsons`.
- [ ] `cargo test` clean inside the fork.
- [ ] Push branch to `cscheid/runtimelib`.

Beads: **bd-fu0l** discovered-from child, "Add venv-aware discovery to
runtimelib fork".

## Phase 3 — Wire the fork into Quarto 2

- [ ] In the workspace root `Cargo.toml`, add:
  ```toml
  [patch.crates-io]
  runtimelib = { git = "https://github.com/cscheid/runtimelib", branch = "feat/venv-kernelspec-discovery" }
  ```
  Pin to a specific commit (`rev = "..."`) once the branch is
  stable to avoid lockfile drift.
- [ ] `cargo update -p runtimelib`; confirm the patched version
  resolves.
- [ ] Update `crates/quarto-core/src/engine/jupyter/kernelspec.rs` to
  call `list_kernelspecs_with_jupyter_paths()`.
- [ ] Update `JupyterError::KernelspecNotFound` to carry `searched:
  Vec<PathBuf>` and `available: Vec<String>`; flesh out its
  `Display` to render searched paths, available kernels, and a
  `hint:` line pointing at `jupyter kernelspec list` and the
  `JUPYTER_PATH` env var.
- [ ] **End-to-end verification** (per CLAUDE.md): in a venv that has
  `ipykernel` installed, run
  `cargo run --bin q2 -- render <fixture>.qmd` from a shell where
  `jupyter` resolves to that venv, confirm the render succeeds.
  Record the invocation and a snippet of output here or in the
  session transcript.
- [ ] Negative path: with no kernel installed, confirm the new error
  lists searched paths, lists what was found, and shows the hint.
- [ ] Full verify: `cargo nextest run --workspace`, `cargo xtask
  verify --skip-hub-build`.

Beads: **bd-fu0l** discovered-from child, "Wire venv-aware kernelspec
discovery and improve error in Quarto".

## Phase 4 — Upstream the fix (LAST; only after Phases 2 + 3 are validated)

This phase is intentionally last. We open the upstream PR only after
Phases 2 and 3 have been merged into Quarto and verified end-to-end on
the original failing fixture from a real venv shell. The reason is to
avoid PR thrash: if validating against a real workload reveals an API
tweak (renaming, signature change, additional plumbing for paths), we
want to absorb that churn privately in our fork before exposing the
final shape to upstream review.

Entry criteria:
- bd-34wy (fork) closed.
- bd-ij1l (wire + diagnostics) closed.
- End-to-end render of the original fixture from a venv shell
  succeeds, recorded in bd-ij1l's close-out.
- No outstanding "we'll change the fork API" follow-ups.

- [ ] Squash/clean the fork branch history if it accumulated
  validation-driven churn.
- [ ] Open PR from `cscheid/runtimelib:feat/venv-kernelspec-discovery`
  against `runtimed/runtimelib:main`.
- [ ] If accepted, replace the `[patch.crates-io]` block with a normal
  version bump once a release ships.
- [ ] If rejected or stalled, leave the patch in place and document
  the situation in `CLAUDE.md`.

Beads: **bd-875x**, blocked by bd-34wy *and* bd-ij1l (deliberately —
upstream waits on validated wiring, not just on the fork existing).

## Phase 5 (deferred) — `sys.prefix` fallback

Only pursue if a user reports a case Phase 2/3 doesn't cover (e.g.
`jupyter` not on PATH but a venv `python` is). Out of scope for the
initial fix.

## Files affected (Quarto side)

- `crates/quarto-core/Cargo.toml` — version bump (Phase 1, done)
- `crates/quarto-core/src/engine/jupyter/execute.rs` — non-exhaustive
  `MediaType` arm (Phase 1, done)
- root `Cargo.toml` — `[patch.crates-io]` (Phase 3)
- `crates/quarto-core/src/engine/jupyter/error.rs` — error variant
  (Phase 3)
- `crates/quarto-core/src/engine/jupyter/kernelspec.rs` — discovery
  call site, error population, tests (Phase 3)

## Verification checklist (final, before declaring done)

- [ ] `cargo nextest run -p quarto-core`
- [ ] `cargo nextest run --workspace`
- [ ] `cargo xtask verify --skip-hub-build`
- [ ] End-to-end render of `convert-test-3.qmd` from a venv shell
- [ ] Error output for an *intentionally* missing kernel renders the
  diagnostic sections (regression check)
