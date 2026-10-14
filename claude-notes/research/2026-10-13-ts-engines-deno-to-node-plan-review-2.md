# Review 2: implementability of "move TS engine extensions from Deno to Node"

**Date:** 2026-10-13. **Plan:** `claude-notes/plans/2026-10-13-ts-engines-deno-to-node.md` (L = plan line).
**Method:** two Sonnet reviewers (A: q2 side P1a/P6/P3a/P2; B: autonomy audit P0/P4/P5/P7/test strategy). I re-checked the claims I act on; those are marked **\[V\]**. Nothing was edited, fetched, built or committed. Locked decisions were not re-litigated.

## 1. Verdict

**Yes, with the listed edits.** As written an agent would stall or ship red at P6 (CI has no Node/esbuild; esbuild binary delivery is unspecified), would mis-port the P3a fixtures, and could not tell whether it may run the workspace runs or push/release to the forks. All fixes are text edits with a default; only Windows/Gatekeeper remain unverifiable (section 4).

## 2. Blocking (executor fails or ships something wrong)

**X1. P6 is red in CI: the Rust job has no Node and no esbuild \[V\].** `test-suite.yml:158-159` says "no setup-node", with `remove_tool_cache: true` at L167. P6 makes `ensure_bundle`/`build_ts_extension` need Node + esbuild, but L160 provisions Node only in P2.
*Edit (P6 checklist, new item):* "In `test-suite.yml` Rust job, after 'Free disk space', add `actions/setup-node` node-version 24 and `npm ci`. Moved here from P2; P2's CI item keeps only 'remove Deno'." (Obsoleted if X2's variant b is taken.)

**X2. How the esbuild binary gets into `q2` is unspecified (L114, L10).** Fetch mechanism, offline/dev builds, the 6 targets, where the driver is committed, licensing, release verification are all missing. quarto-core has no `build.rs`; a 10-12 MB `include_bytes!` in quarto-core links into every test binary and the wasm graph, undoing bd-xvdop (`.claude/rules/integration-tests.md`).
*Edit (paste into P6), default design:*
1. `cargo xtask fetch-esbuild --target <triple>` does `npm pack @esbuild/<plat>@<pinned>`, checks the lockfile integrity hash, writes gitignored `ts-packages/esbuild-embed/<triple>/`; called by `build-all` and the release-pipeline per-target leg.
2. Embed lives in the `quarto` bin crate or a new `quarto-esbuild-embed` crate (build.rs with placeholder fallback, `cfg(not(wasm32))`), **not** quarto-core. Placeholder embedded => error "run cargo xtask fetch-esbuild" (mirrors the MCP launcher).
3. `BuildOptions.esbuild_binary: Option<PathBuf>`; resolution: option, then `ESBUILD_BINARY_PATH`, then `node_modules/@esbuild/<plat>/bin/esbuild` (what tests use).
4. Release-pipeline "Verify binary" (~L606) runs `q2 call build-ts-extension` on a tiny fixture (the only place musl/Windows get exercised); add esbuild MIT to the third-party notices (precedent `release-pipeline.yml` ~L410-421).
5. Host devDependency is `esbuild` **0.28.0** \[V `quarto-engine-host-deno/package.json:24`\], the spike used 0.28.2: bump exact, regenerate lockfile, rebuild the committed host bundle, or the freshness gate and the embedded version diverge.
*Optional simplification (A, F10):* spawn the esbuild binary directly from Rust (`cwd=ext_dir`, relative entry, `--bundle --format=esm --platform=node --target=node24 --alias:... --conditions=source,import --external:node:*`). That removes the driver, banner, `ESBUILD_BINARY_PATH` and Node as a build-time dependency, which also dissolves X1. Decision 10's Node driver is not on the locked list. **Recommended default: take this simplification**; byte-for-byte equality with the spiked driver output is unchecked, so P6 must assert byte-stability itself.

**X3. P3a/P6 "no alias needed" is wrong for fixtures \[V\].** `import type` erases, but fixtures have *value* imports, e.g. `alpha/src/alpha.ts:30: import { primary } from "@quarto/api/claims"`; A lists 7 (alpha, beta, behave, fallback-univ, interop-r, mismatch, whenclass-marimo), and marimo imports `path` and `deno.land/std/.../delay.ts`.
*Edit (P6 L115):* "Workspace tier aliases `@quarto/api`, `@quarto/api/claims`, `@quarto/types` to absolute `ts-packages/*/src/...` paths (as `resources/extension-build/deno.workspace.json` does today); needs no `node_modules`. Shipped tier has no value-import support for `@quarto/api`: fail with 'not published; build inside the q2 workspace or pass --workspace'." Also delete the sentence at L29 that says the shipped tier "may need no aliases" for value imports.

**X4. P3a's fixture list and the legacy-python rebuild are wrong \[V\].** `legacy-python/dist/legacy-python.js` is a 901-byte hand-written stub with no source and no `Deno.` use; it cannot be "rebuilt with the P6 builder" (L124). The 12 `Deno.`-using files are 10 hermetic engines plus marimo's `src/marimo-engine.ts` and the loader shim, which P0/P4 replace.
*Edit (P3a):* "Port the 10 engines `{alpha,beta,behave,content-claim,echo-engine,echo-legacy,fallback-univ,interop-r,mismatch,whenclass-marimo}/src/*.ts`. Do not hand-port marimo's two files (replaced by the P4 re-sync). Leave `legacy-python` unchanged. Done = `grep -rn 'Deno\.' crates/quarto-core/tests/fixtures/extensions --include='*.ts'` empty except marimo." Fix "12" at L11/L43/L123 accordingly.

**X5. Workspace-run authorization contradicts itself and the baseline pairing is impossible (L62-64 vs L77, L163; CLAUDE.md trial paragraph).** L62-63 mandate two full runs; L77/L163 say "recommend, don't run unprompted". L62 pairs the baseline with P0 and says "P0's final step waits for the run", but the order is P0 -> P1a, and P0 re-syncs a test fixture mid-run.
*Edit (replace L62-64 and L77/L163 wording):* "This plan authorizes exactly two workspace runs: a **baseline on clean HEAD before P0 starts** (save `cargo nextest list --workspace --message-format json` test names to scratch at the same time), and the P2 switch run. P0's cargo-free marimo-repo work runs during the baseline; P0's fixture re-sync and its cargo gate start after the run exits. Each run is captured once to a log under the job tmp dir and run in the background. The P2 delta accounts for the P0 fixture change. Any other workspace run is a recommendation in the final report only." Diff test *names*, not counts: self-skipping tests count as pass, so counts cannot show a skip shift.

**X6. P0 "keep `claimsFile` sniff for Q1 only" collides with the branch history \[V\].** `quarto-2-improvements` already contains f47802b (deletes `containsMarimoFence`/`claimsFile`), and the fork's `_extension.yml:27-29` says static `claims-files: []` "must agree with the runtime `claimsFile` (both say no)". L85-86 asks for the opposite, and L89's "static and runtime claims agree" test would fail on it.
*Edit (L86, with default):* "Do not rewrite f47802b. After the rebase add a NEW commit restoring `containsMarimoFence` and the `claimsFile` sniff, marked Q1-only in source. Keep `claims-files: []` in the yml and update the yml comment that says 'both say no'. The agreement test covers `claims` only and explicitly exempts `claimsFile` with a comment citing this rationale." Also state whether Q1 reads yml `claims-files` (unchecked by either reviewer; the P0 Q1 repro should settle it).

## 3. Should-fix (executor would have to guess)

**S1. P0 conflict surface is overstated \[V\].** `git merge-tree --write-tree` of the two branches conflicts **only** in `src/marimo-engine.ts`; `tests/claims-language.test.ts` auto-merges; `_extension.yml` is untouched by the bare-sql branch. *Edit L84:* "Expect conflicts in `src/marimo-engine.ts` only." Gap: `quarto-2-improvements` = upstream/main (78987bc, 2026-07-22) + 2; `q2-bare-sql-interop` = 76d6f1d + 4 (CI/dep churn plus engine commits).

**S2. P0 does not say where the rebase happens, and the checkout is dirty.** B reports `_quarto.yml` deleted, `index.qmd` modified, untracked files in `~/src/quarto-marimo`. *Edit:* "Do not touch that checkout's working tree. Tag old tips `backup/pre-p0-<branch>`, then `git worktree add <scratch>/quarto-marimo-port -b q2/node-port upstream/main` and cherry-pick in order (quarto-2-improvements commits, then q2-bare-sql-interop's four)." Rollback is then deleting the branch.

**S3. Pushes and the marimo release are unauthorized and have no rollback (P0 L91, P4 L133-134, P5).** Neither source branch is on origin; `gh auth` is live; `publish.yml` publishes on a `v*` tag; the notify step needs `SLACK_WEBHOOK_URL_RELEASES`, absent in the fork. *Edit (P4/P5 headers), defaults:*
- Authorized: push **new** branches `q2/node-port` to the two forks (never `main`, never force-push); rollback `git push origin --delete`.
- Make the shim download URL overridable by env var (default = fork release URL) so the Node end-to-end shim test runs against a local `python3 -m http.server` **before** any release.
- The fork-only commit replaces the Slack notify step and the shim URL.
- Tag/release is the **last** P4 step, after all local gates, fork only; version = next patch above 0.4.5 (default `0.4.6`; `-q2.N` suffix only if the `v*` trigger still matches); rollback `gh release delete <tag> --cleanup-tag -R gordonwoodhull/quarto-marimo`.
- Fork CI gate: committed fixture bundle equals the CI-built asset (needs identical esbuild version and fixed relative entry).
- P2's CI push: "push `research/ts-engine-node-port` to q2 origin; no PR, no merge, no force-push; poll `gh run list` / `gh run view --log-failed`."

**S4. P5 base branch is unspecified (L142).** `q2-static-declarations` already contains julia main and upstream/main (0 behind), so "reconcile" is not a merge. But `q2-close-busy-fix` and `worker-busy-recovery` carry "redirect the detached server's stdio to devnull" (b881e69, 41ba4dc), which L146 needs. *Edit:* "Base = tip of `q2-static-declarations`; cherry-pick b881e69 (and 4e6bc27, f7c9bfc only if q2's subtree needs them); record the choice in the commit." The julia repo has no unit tests: `tests/unit/` is new.

**S5. P2 is too big for one PR (L151-163).** Pre-stage, each green on its own: (a) `git mv ts-packages/quarto-engine-host-deno quarto-engine-host-node` plus package/workspace/lockfile/tsconfig/xtask/`.gitignore`/`ci_test_wiring.rs`/comment renames, keeping the embedded bundle name until the flip (P2 never says the directory moves); (b) `quarto-node-discovery` crate with launcher re-export and the `cfg(not(wasm32))` dependency, unused until the flip; (c) shared `node_available_for_tests()` replacing the copies, body still checking `deno` until the flip; (d) `DIALBACK_PREAMBLE` runtime-parameterised; (e) docs prepared. *Edit the P2 header:* "Atomic remainder = the three `Command::new("deno")` swaps, the embedded bundle swap, CI Deno removal, Deno file deletion."

**S6. `node_available()` has no home (L158).** A `tests/` helper cannot cross crates; copies are 11 `fn deno_available` in quarto-core integration files plus `quarto-preview/src/capture_driver.rs:489` and two in `quarto/tests`. *Default:* `#[doc(hidden)] pub fn node_available_for_tests() -> bool` in `quarto_core::engine::ts_process`, returns `is_available()` and panics under `QUARTO_CI=1` when false. P6 must also update the two readers of `deno.workspace.json` (`capture_driver.rs:543`, `engine_fixture_build.rs:50`) or P6 is red.

**S7. marimo/julia gates pass by skipping.** `marimo_engine_e2e` skips without uv/Rscript/knitr, julia without julia/`QUARTO_JULIA_PROJECT`. *Edit every such gate:* "Run with `--no-capture`, tee to a log, require `grep -c '^SKIP'` = 0 on this machine (uv, Rscript, julia 1.12.7 are installed). If a tool is missing, record the per-test SKIP count in the commit message and continue."

**S8. P1a naming/entry ambiguity (L98-106).** *Defaults:* `src/node-host.ts`, `src/node-main.ts`, esbuild entry output `dist/engine-host-node.mjs` (`.mjs` avoids a project-local `"type":"commonjs"` breaking the extracted bundle), committed in P1a with a freshness-gate step; always include the `createRequire` banner (answers the `blueimp-md5` question); async iterator for the socket reader (not `Readable.toWeb`); delete the stdio branch (answers "first confirm"). Add one vitest case importing an extension bundle from a temp dir that has a typeless `package.json`.

**S9. P6 config format and `deno.json` plumbing (L115-117).** Plumbing touches `resolve_build_config` (`build.rs:53-100`), `SHIPPED_DENO_JSON` (L138), the `q2-shipped-deno-` tempfile (L159), `run_deno_bundle` (L336-358), tests L403-576 and `crates/quarto/src/commands/build_ts_extension.rs:14`. *Default:* four tiers kept; config file becomes `esbuild.json` (`{"alias":{...}}`) in `resources/extension-build/`; ext-local `deno.json` is **not** read and a warning names it when no `esbuild.json` exists. Name `deno.workspace.json` explicitly in L117's retirement list.

**S10. Remaining "decide/if wanted" items, all defaultable:** `QUARTO_NODE` in CI = not set (`setup-node` puts Node 24 on PATH); Node in CI = `actions/setup-node` after "Free disk space"; P6 wider gate = `cargo nextest run -p quarto-core -p quarto -p quarto-preview` (L66); P0 "ignore `~/src/quarto-marimo-main`" is already stated.

**S11. P6a macOS check is autonomous (this machine is macOS).** *Edit L114:* "Pass = extract the embedded bytes to a fresh cache dir, `chmod +x`, `codesign -v`, run `<bin> --version` and one bundle build; on codesign failure fall back to ad-hoc `codesign -s -` after extraction. Windows and musl are not testable locally; they are covered by the release-pipeline verify step (X2.4); do **not** stop on them." Revise escalation (b) accordingly: only macOS failure after the ad-hoc fallback escalates.

**S12. Q1 re-verification is autonomous \[V\].** `~/src/quarto-cli/package/dist/bin/tools/aarch64/deno` reports 2.7.14 (`/opt/homebrew/bin/deno` is 2.9.0; do not use it for the Q1 leg). *Edit L137/L180:* name that path; "run bundles with that binary directly; never build inside `~/src/quarto-cli`".

**S13. Test strategy smaller points.** L67 ("confirm `QUARTO_CI=1` hard-fails Node-gated tests") cannot run before P2; move it to the P2 gate and make it `QUARTO_CI=1 PATH=<without node> cargo nextest run -p quarto-core ts_process::tests::node_available_when_quarto_ci` must FAIL. P7 grep (L170): pass = empty after `grep -rnw -i deno` minus an allowlist (`render_scripts.rs:473`, `crates/quarto/Cargo.toml:89`, `quarto-system-runtime`, `text_execute.rs`, `quarto-source-fetch/src/target.rs`); also sweep ~30 "Deno" comments in test headers. L171 says the tracker is `braid`, not beads. Host bundle config currently uses `platform:'neutral'`, `external ['jsr:*','node:*']`, and `build_engine_host_bundle.rs` docs say "Deno-ready": update. L11/L52 "12 copies" is 11 + the ts_process test + 2 import aliases (harmless).

## 4. Residual human touchpoints

| Touchpoint | Class |
|---|---|
| Windows behaviour (spawn, `.cmd`, PowerShell detach, `readSync(0)`) | **Unavoidable.** Write the checklist with pass criteria into the final report; mark Windows untested; not a stop |
| Gatekeeper path for a *downloaded* q2 release binary (quarantine) | **Unavoidable** residual risk; record in Open risks |
| Gordon's final whole-plan test | **Unavoidable** (stated intent) |
| Fork branch pushes, marimo release version/tag, CI push, julia base branch | **Defaultable** (S3, S4) |
| `claimsFile` Q1-only design (X6) | **Defaultable** |
| `QUARTO_NODE` in CI, config format, deno.json handling, socket reader style | **Defaultable** (S8-S10) |
| Q1 `AGENTS.md` build-doc sync (L169) | **Defaultable**: becomes a note in the final report (cannot be done in this repo) |
| Workspace-run authorization, baseline pairing, rebase on a dirty checkout, missing pass criteria (P7 grep, checklist reconcile) | **Can be removed** by text edits above |
| `git fetch upstream` outcome could trigger escalation (c) | **Unavoidable by design**; the fetch itself is authorized (read-only) |

## 5. What was NOT checked

- Whether esbuild CLI flags (X2 simplification) reproduce the driver's bytes; any esbuild run; macOS quarantine/codesign, Windows, musl.
- `cargo package`/crates.io size and publish settings; wasm32 graph beyond A's reading that `ts_process` and `extension::build` are native-only.
- Whether `git fetch upstream` changes P0 (marimo or julia); upstream was unfetched since ~Jul 22-24.
- Whether Q1 reads yml `claims-files`; the Q1 repro (unbraced fences, `firstClass`) was not run.
- That the q2 marimo fixture equals `q2-bare-sql-interop`; the `build-command.test.ts` and pytest gates.
- Windows `cfg`s in `ts_process.rs` beyond the `cfg(all(test, unix))` dialback helpers; extra `deno` uses in `q2-preview-spa/e2e` specs beyond grep hits.
- Real Julia server/HMAC handshake; first-review findings (not re-raised).
- No haiku experiment was run, no cargo, no CI inspection of runner Node versions.
