---
title: 'Fix Windows path-separator assumptions in pampa quarto_api path tests (bd-picv)'
date: 2026-06-24
description: 'Makes the `resolve_path` Lua API function always return forward slashes and use `is_rooted` instead of `is_absolute`, so Windows path tests pass without platform-specific expectations.'
status: done  # Implemented 2026-06-24 (production normalize + `is_rooted`). pampa lib + targeted tests green on Windows. Full `cargo xtask verify` pending as pre-push gate.
braid:
  strand: bd-picv
  priority: P2
  labels: [lua, pampa, windows]
---

**Worktree:** `.worktrees/bd-picv-fix-windows-path-separator` (branch `braid/bd-picv-fix-windows-path-separator`, based on `main`)

## Triage verdict

**Ready to design → design now settled.** Root cause confirmed and reproduced; fix
direction chosen by user (option a: production-side forward-slash normalization +
`is_absolute()`→`is_rooted()` swap). Ready to implement TDD-first.

## Issue context

Strand filed 2026-04-28 (bug, P2, labels lua/pampa/windows). Original framing: \~10
tests in `crates/pampa/src/lua/quarto_api.rs` fail on Windows because expected paths
hardcode forward slashes while the Windows impl joins with backslashes via `Path::join`.
Strand offered a binary choice: (a) normalize output to forward slashes, or (b)
platform-correct test expectations.

**Fresh investigation reframed the strand:**

- **Native Windows is functionally fine — not a live bug.** `resolve_path` output flows
  only to `io.open`, `fs::read` (via `mediabag.fetch`), and `dofile`. All accept
  backslash paths on Windows. `q2 render` works on Windows; the backslashes are cosmetic.
  The only breakage is the test assertions.
- **There is a deeper, currently-masked bug.** `resolve_path` gates on `p.is_absolute()`
  (`quarto_api.rs:360`). The workspace's own `quarto-util::is_rooted` doc comment says use
  `is_rooted()` instead, because `is_absolute()` returns `false` on `wasm32` for
  `/project/...` VFS paths. bd-vxl8 already swept `io_wasm.rs`/`dofile_wasm.rs` to
  `is_rooted`; `resolve_path` was missed. No current caller passes an absolute path here
  (extension Lua uses relative names), so it's latent — but a real convention divergence.
- **Fix belongs in production.** `quarto_util::to_forward_slashes` already exists and
  bd-vxl8 set the precedent of normalizing separators at the boundary. Option (a) makes
  output deterministic cross-platform, matches quarto-cli `pathWithForwardSlashes` and this
  tree's convention, and lets tests assert forward slashes on all platforms with **no
  `#[cfg(windows)]` duplication**. Option (b) leaves production inconsistent and forces
  cfg-gated test expectations.

## Dependency graph

- **related → bd-3pe8** (closed). "Audit pampa production Lua code for Windows path
  escaping." Concluded production Lua *string interpolation* was safe (uses Lua C API, not
  `format!` into source); the test-side fix was correct there. bd-picv is the adjacent
  surface bd-3pe8 did **not** cover: the *return value* of a runtime function, not
  interpolation into source.
- The actual convention-setting strand is **bd-vxl8** (commit `b406cadc`): swapped
  `is_absolute`/`starts_with('/')` → `quarto_util::is_rooted` in `io_wasm.rs` /
  `dofile_wasm.rs`, and used `to_forward_slashes` when embedding native paths into Lua
  source strings in tests. This is the precedent bd-picv follows.

## What the code looks like today

Reproduced at HEAD on Windows (2026-06-24):

```
test_normalize_path_simple: left: "\\a\\b\\c"  right: "/a/b/c"
test_quarto_utils_resolve_path_relative_with_subdir: left: "\\ext\\dir\\sub\\data.json" right: "/ext/dir/sub/data.json"
```

Key code:

- `quarto_api.rs:358-369` — `resolve_path`: `if p.is_absolute() { return Ok(path); }`
  then `let resolved = PathBuf::from(&script_dir).join(&path); Ok(normalize_path(&resolved))`.
- `quarto_api.rs:476-496` — `normalize_path`: collapses `.`/`..`, then
  `result.to_string_lossy().to_string()` — emits native separators verbatim.
- Production `push_script_dir` callers feed native paths with backslashes:
  - `filter.rs:177-182` — `filter_path.parent().to_string_lossy()`
  - `shortcode.rs:136-142` — `script_path.parent().to_string_lossy()`
- Helper: `quarto-util/src/path.rs:23` `to_forward_slashes`, `:14` `is_rooted`
  (re-exported `quarto-util/src/lib.rs:8`).

The \~10 failing tests run twice (lib + bin/pampa integration), so \~18-20 visible failures
from one root cause.

## Proposed phases

### Phase 0 — Test plan (TDD)

The failing tests already encode the desired forward-slash behavior — they ARE the
failing-first tests. Confirm they fail at branch HEAD on Windows (done). Add coverage the
current tests lack:

- [ ] A test that `resolve_path` returns forward slashes when the **pushed script_dir
      itself contains backslashes** (simulates the real `filter.rs`/`shortcode.rs` input on
      Windows). Current tests push forward-slash dirs (`/some/extension/dir`), so they don't
      exercise the backslash-input path. Use a backslash literal dir and assert
      forward-slash output. → `test_quarto_utils_resolve_path_backslash_script_dir`.
- [x] A test that a rooted input is returned forward-slash-normalized and NEVER joined onto
      the script dir. → `test_quarto_utils_resolve_path_rooted_ignores_script_dir`. Pushes a
      `C:\some\dir` script dir, resolves `/abs/x.json`; pre-fix the `is_absolute` gate let it
      fall through and join (RED: `left: "C:\\abs\\x.json"`), pinning the `is_rooted` swap.

### Phase 1 — Production fix in `quarto_api.rs`

- [x] `resolve_path`: replaced `p.is_absolute()` with `quarto_util::is_rooted(p)`; rooted
      branch returns `quarto_util::to_forward_slashes(p)`.
- [x] `normalize_path`: trailing `result.to_string_lossy().to_string()` replaced with
      `quarto_util::to_forward_slashes(&result)`.
- [x] No-script-dir relative branch: **normalized** (residual Q1 resolved — see below). The
      branch now returns `to_forward_slashes(p)` so every return path is forward-slash.

### Phase 2 — Verify tests pass with NO cfg-gating

- [x] All 10 original assertions + 2 new pass on Windows with **no `#[cfg(windows)]`**.
- [x] Updated 3 stale assertions in `shortcode.rs` (`test_shortcode_resolve_path`,
      `..._multi_extension`) that encoded the old backslash output — now use
      `quarto_util::to_forward_slashes` on the expected temp path. These exercise the real
      production path (real temp-dir script dir → forward-slash output end-to-end).

### Phase 3 — Workspace verification

- [x] `cargo nextest run -p pampa` lib tests green; full-crate run green **except** 8
      pre-existing Windows failures (`test_json_writer`, `test_html_writer`,
      `unit_test_corpus_matches_*`, `unit_test_snapshots_{json,native}`,
      `test_qmd_roundtrip_consistency`, `test_metadata_source_tracking_002_qmd`). Confirmed
      via `git stash` they fail identically on clean branch HEAD — CRLF/snapshot failures from
      the broader Windows test campaign, unrelated to this change and out of scope.
- [x] `cargo build --workspace` clean (0 errors; 4 pre-existing unrelated warnings).
- [x] `quarto-util` is already a normal dependency of pampa (`Cargo.toml:56
      quarto-util.workspace = true`) — residual Q2 resolved, no promotion needed.
- [ ] `cargo xtask verify` — full leg (incl. WASM hub-build), since the `is_rooted` change
      is WASM-relevant. **Pending — run as the pre-push gate.**

## Open design questions for the user — RESOLVED

1. **No-script-dir relative branch.** Resolved: **normalized.** Single invariant —
   "resolve_path always returns forward slashes." Implemented.
2. **`quarto-util` dependency tier.** Resolved: already a regular (`.workspace = true`)
   dependency of pampa; no change needed.

## Risks / tradeoffs

- **Behavior change, low risk.** `resolve_path` output changes from backslash to
  forward-slash on Windows. All known consumers (`io.open`, `fs::read`, `dofile`) accept
  forward slashes on Windows, so no functional regression. WASM already produces
  forward-slash, unaffected.
- **`is_rooted` swap is behavior-neutral today** (no caller passes an absolute path), but
  forward-compatible: a future caller passing a `/project/...` VFS path in WASM would now be
  handled correctly instead of being wrongly joined to the script dir.
- **No DOM/document-output exposure found** — `resolve_path` output does not flow into
  hrefs/srcs in rendered output (only to file-read APIs), so this is not a rendered-artifact
  correctness fix, just a convention + latent-WASM-bug fix.
