# GE-17 (c2 `-BuildEmbed`) Windows investigation, GWWIN, 2026-11-20

Branch `windows/git-epochs`. Host `x86_64-pc-windows-msvc`, Windows 11, node v24.21.0, rustc 1.97.0-nightly (2026-04-27, the `rust-toolchain.toml` pin).
Written by a Claude Code session working with the Windows box's owner. Scope: why `run.ps1 -Only c2 -BuildEmbed` failed before it reached the cross-client scenarios, and what it takes to get them to run.

## Bottom line

`c2 -BuildEmbed` now builds the embed end to end and the cross-client scenarios run on Windows for the first time. Of the 9 cross-client tests, 7 pass (H1, H2, H3, H5, H6, H7, registry-crosscheck). H2e and L1-L4 do not reliably pass. `extension-flows` (19 tests) is flaky: four runs, four different outcomes. c2 is **not** green yet.

There are five distinct blockers, and each had a different owner:

| # | Blocker | Owner | Status |
|---|---|---|---|
| 1 | `wasm-opt` rejects wasm-bindgen's output (193 `call param types must match`) | **upstream dev-setup** (walrus 0.24.4) | root cause confirmed; env fixed here; upstream fix proposed |
| 2 | `npm run build:sw` / `build:preview-embed`: `'BUILD_TARGET' is not recognized` | **upstream repo** (POSIX-only npm scripts) | fixed + tested, commit `337dc8c8c`, cherry-pickable |
| 3 | `wasm-quarto-hub-client` symlink checked out as a text file | **env** (`core.symlinks=false`) | env fixed here; harness gap |
| 4 | No Playwright browser installed; 7 of 9 cross-client tests fail at `chromium.launch()` | **harness gap** | env fixed here |
| 5 | `q2.exe` embeds the *placeholder* editor ("The q2 preview editor UI is not built") although the embed exists | **harness / shared `CARGO_TARGET_DIR`** | cause mostly explained; mitigated by hand; harness fix proposed |

## 1. wasm-opt validation failure (confirmed)

Symptom: `wasm-opt -Oz` on the wasm-bindgen output fails with `call param types must match` on `__wbindgen_object_drop_ref (local.get $n)` (193 sites, all that one call).

What was ruled out (each tested, not assumed): the wasm-opt binary (npm shim is an emscripten/JS build of binaryen 132.0.0; a native `wasm-opt.exe` version_132 fails identically, so does the old cargo-installed 116); feature flags (`--all-features` and explicit `--enable-*` give the same error); the toolchain (matches the pin); wasm-bindgen version (CLI 0.2.108 = crate 0.2.108); the raw cargo output (`wasm_quarto_hub_client.wasm` is valid and passes `wasm-opt -Oz`).

Cause: the invalid bytes come from the `wasm-bindgen` CLI itself. Its externref pass (`rewrite_calls`) walks functions with walrus `dfs_pre_order_mut`. In **walrus 0.24.4** that function (`src/ir/traversals.rs:264-269`) pushes only the `try` body and never the `catch`/`catch_all` handlers (its sibling `dfs_in_order` does visit them). `JsValue::drop` is inlined into `catch_all` cleanup paths, so those `drop_ref` calls keep their `i32` argument while the import is retyped to `externref`. In one failing function: 31 sites rewritten (all outside handlers), 15 left over (all inside `catch_all`).

Evidence the walrus version is the variable:

| wasm-bindgen CLI | output sha256 (prefix) | `wasm-opt -Oz` |
|---|---|---|
| installed `~/.cargo/bin` exe, walrus 0.24.4 | `85D63FA3` | 193 errors |
| `cargo install --locked` into a scratch root, walrus 0.24.4 | `85D63FA3` (byte-identical) | 193 errors |
| `cargo install` unlocked into a scratch root, walrus **0.24.5** | `072A48FC` | **pass**, 44,117,404 bytes |

The CI releases agree: the `nightly` and `v0.47.0` `quarto-hub-client-wasm` assets validate cleanly and are 43.9 MB. CI installs `cargo install -f wasm-bindgen-cli --version ...` **without** `--locked`, which resolves walrus 0.24.5 (released 2026-01-30; `^0.24.2` allows it).

**Upstream proposal:** `crates/xtask/src/dev_setup.rs:101` installs `wasm-bindgen-cli` with `cargo install --locked`, and may use `cargo binstall` first. Either path produces a CLI with walrus 0.24.4, so anyone who follows the documented `cargo xtask dev-setup` gets a CLI that emits invalid WASM from this crate (not Windows-specific). Suggest dropping `--locked` for this one tool (as CI does), or installing a CLI known to carry walrus >= 0.24.5, plus a post-install check. Not changed here.

Env change on this machine: `cargo +stable install -f wasm-bindgen-cli --version 0.2.108` (now embeds walrus 0.24.5).

## 2. `cross-env` for the npm scripts (fixed, tested)

npm runs scripts through `cmd.exe` on Windows, which cannot parse `VAR=value command`. Commit **`337dc8c8c`** adds `cross-env@^10.1.0` and prefixes the inline-env commands in `hub-client` (`build`, `build:preview-embed`) and `quarto-hub-sandboxed-preview` (`build:sw`). The sandboxed preview is *not* an npm workspace member (own `package-lock.json`, installed by `build:sandboxed`), so it carries its own devDependency. Patch: `ge17-cross-env.patch` (4 files, +128/-3).

Tests run on Windows (baseline failure first, then pass):

- `build:sw` alone: failed before (`'BUILD_TARGET' is not recognized`), exit 0 after, output is `serviceWorker.js` from 2 modules (proves `BUILD_TARGET` took effect).
- `npm run build` (hub-client): failed before (`'NODE_OPTIONS' is not recognized`), exit 0 after.
- `cross-env` with the exact `build:preview-embed` variable set, probed from a node script: every value arrives as written, including the empty `VITE_GOOGLE_CLIENT_ID=` (`""`), `VITE_DEFAULT_SYNC_SERVER=/ws`, and `NODE_OPTIONS` (heap limit 4288 MB).
- `cargo xtask build-hub-client-embed`: exit 0. Output inspected: `/ws` default baked into two bundles, no `sw.js`, no manifest/registerSW, no Google client id, no `pandoc/` dir, 188 files gzip-precompressed (3.37x), hub WASM 44,118,810 bytes.
- `npm run build:all` (the AGENTS.md hub-client gate): exit 0.
- `npm ci --dry-run` at the root and in `quarto-hub-sandboxed-preview`: both exit 0 (lockfiles consistent).
- Same scripts under Git Bash as npm's `script-shell`: `build:sw` exits 0; the env words arrive correctly. Caveat found: Git Bash rewrites `/ws` to `C:/Program Files/Git/ws` by MSYS path conversion; the original inline form does exactly the same, so this is not a regression and only affects someone who sets `script-shell` to Git Bash.

**Not verified:** Linux/macOS (no such host here; `build:preview-embed` is only used by the release pipeline and the xtask, not per-PR CI, so a mistake would surface late). Scripts left on the POSIX form and not exercised: `test:e2e`, `test:e2e:author-id`, `test:e2e:ui`, `test:harness`. An upstream PR also needs a `hub-client/changelog.md` entry (AGENTS.md two-commit rule); not done on this scratch branch.

## 3. `hub-client/wasm-quarto-hub-client` is a symlink (env)

`vite build` failed with `ENOENT .../hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client.js`. The path is a tracked symlink (mode 120000, 13 in the repo). With `core.symlinks=false` git writes it as a 28-byte text file containing `../crates/wasm-quarto-hub-client/pkg`. Developer Mode is on here, so symlink creation works. Fix applied: `git config --local core.symlinks true`, then `git checkout --` of all 13 tracked symlinks (no `git status` noise afterwards). `claude-notes/instructions/windows-dev.md` has no symlink guidance, and `run.ps1` does not check `core.symlinks`; suggest adding a preflight check.

## 4. Playwright browser missing (harness gap)

No `ms-playwright` directory existed, so 7 of 9 cross-client tests failed at `chromium.launch()` (`chromium_headless_shell-1254`). Upstream CI runs `npx playwright install --with-deps chromium` for this suite (`vscode-extension-e2e.yml:120`); `run.ps1` has no equivalent. Fixed here with `npx playwright install chromium` from `vscode-sync-experiment`. Suggest a `run.ps1` preflight/install step.

## 5. `q2.exe` embeds a placeholder editor (explained in part)

After the browser fix, every browser scenario timed out waiting for the `index.qmd` tree item. A hand reproduction (`q2 preview --ui editor` on a tiny git repo, Chromium via Playwright) showed the page title "q2 preview - editor UI not built": the binary under test carried the placeholder, although `hub-client/dist-preview-embed/` existed and was fresh. `cargo build --bin q2` reported "Finished in 2.3 s" and `q2.exe` (1:24 PM) predated the embed (1:41 PM).

Facts: the `quarto-preview` build-script outputs under `target\debug\build` were from 11/19, with a 586-byte `editor-embed.tar.zst` (placeholder); one of them recorded `rerun-if-changed` on `...\q2-baseline-20261119-171347-GWWIN\q2-preview-spa\dist` (a different checkout) and did not list `dist-preview-embed` at all. `run.ps1` sets `CARGO_TARGET_DIR` to the main checkout's `target` while building baseline copies (`run.ps1:438`). After bumping `crates/quarto-preview/build.rs`'s mtime (no content change), cargo rebuilt `quarto-preview` and `quarto`, `q2.exe` grew 138.6 to 163.6 MB, and the editor loaded (tree `_quarto.yml`, `index.qmd` within 5 s). Afterwards cargo's change detection worked normally: touching one file under `dist-preview-embed/` produced `Dirty quarto-preview: the file hub-client\dist-preview-embed has changed`.

**Suspected, not proven:** the stale fingerprint came from the shared `target\` across the `q2-baseline-*` checkout. I did not reproduce the stale state from scratch. **Proposal (harness, not product):** after `cargo xtask build-hub-client-embed`, have `run.ps1` force a re-embed (touch `build.rs`, or `cargo clean -p quarto-preview`) and fail the step if `q2 preview --ui editor` serves the placeholder.

## Per-run c2 results

| run | embed build | extension-flows | cross-client | timing |
|---|---|---|---|---|
| `123232` (pushed as `43cb0ded9`) | FAIL: `BUILD_TARGET` (blocker 2) | 18 pass, 1 FAIL: smoke A (`timed out waiting for the typed edit on the hub`) | not reached | pass (65 ms) |
| `132321` | ok | **19 pass** | 1 pass, 8 FAIL: 7 no Chromium, H2e `git switch` abort | pass (60 ms) |
| `133812` | ok | 18 pass, 1 FAIL: clone-mode "Stay detached" (`locator.click` 30 s timeout) | 1 pass, 8 FAIL: placeholder editor (blocker 5) + H2e | pass |
| `140245` | ok | 17 pass, 2 FAIL: share-folder x2 (clipboard timeout) | **7 pass**, 2 FAIL: H2e, L1-L4 (clipboard timeout) | pass |
| manual, full `--cross` | n/a | n/a | 8 pass, 1 FAIL: H2e | n/a |
| manual, `-g H2e` alone | n/a | n/a | **pass** | n/a |

## Remaining failures

- **H2e, suspected test race.** Fails in the full suite (3 of 3 incl. two official runs), passes alone (1 of 1). A background poller snapshotting the editor-only clone caught it: at 14:21:30 the clone was still on `main` but its `a.qmd` already held `Version FEATURE of a.` (status ` M a.qmd`). The Quarto Hub extension wrote the hub's flipped text to the working tree *before* the test's own `git switch feature`, so git refused to overwrite the modified file. The test expects the window to be suspended and to follow the flip *after* its checkout. Likely timing: on Windows the `git` spawn is slower than on macOS/Linux, which gives the extension time to write first. CRLF was ruled out (system `core.autocrlf=input`, files stayed LF throughout, a hand repro of clone + `autocrlf=false` + switch was clean).
- **Clipboard timeouts** (`timed out waiting for the q2 preview command on the clipboard`): share-folder x2 and L1-L4 in run `140245`; the same L1-L4 passed in the manual rerun and share-folder passed in `132321`. Cause not established. UI-driven VS Code tests are plausibly sensitive to desktop focus/clipboard state while the machine is in use.
- **extension-flows** is flaky as a whole: smoke A, clone-mode "Stay detached", and share-folder each failed in one run and passed in another; one run was fully green. I did not investigate further.
- Noise seen in the browser during the manual probe but not a failure: `/auth/me` returns 401 and a `WASM module not initialized` pageerror precede a working editor.

## Env changes made on this machine (undo notes)

- `~/.cargo/bin/wasm-bindgen*.exe` replaced (same version, walrus 0.24.5).
- `git config --local core.symlinks true` in this checkout, and the 13 tracked symlinks re-checked out as real symlinks.
- Playwright Chromium (`chromium-1254`, `chromium_headless_shell-1254`, ffmpeg, winldd) under `%LOCALAPPDATA%\ms-playwright`.
- `crates/quarto-preview/build.rs` mtime bumped once (no content change).
- Scratch work (native binaryen 132, scratch wasm-bindgen builds, probe scripts) is under the session scratchpad in `%TEMP%`, not in the repo.

## What to send upstream

1. `337dc8c8c` (cross-env) as a PR, with a `hub-client/changelog.md` entry; decide whether to convert `test:e2e*`/`test:harness` too.
2. `dev_setup.rs`: do not install `wasm-bindgen-cli` with `--locked` (walrus 0.24.4 emits invalid WASM here); add a post-install validation.
3. Windows docs/harness: `core.symlinks`, Playwright browser install, and a forced re-embed (or placeholder check) after `build-hub-client-embed`.
4. H2e: have the owner of the cross-client tests decide whether the extension writing before the test's checkout is a test race or a product question.
