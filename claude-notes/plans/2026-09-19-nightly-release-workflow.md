# Nightly release workflow: build `main` whenever it has unreleased changes

**Date:** 2026-09-19
**Braid:** bd-p4ljdp2e (feature, P2, labels release/ci)
**Status:** Done. PR #697 merged 2026-09-19; first nightly published the
same day (run 35465215381, all gates + install smoke green). One
observation outstanding: the 2026-09-20 scheduled run should skip.
**Review log:** 2026-09-19 Carlos accepted Decision 1 (prerelease channel)
and added two requirements: (i) installing a nightly must be as easy as
installing a release, so the README gets a nightly one-liner and the
installers gain a nightly mode (Decision 6, Phases 3–4); (ii) `q2 --version`
must print the same nightly string the release and its assets carry (the
"one version string" invariant under Decision 2). Later the same day Carlos
accepted the PowerShell flag form (`-Nightly` via `scriptblock`, no
`Q2_CHANNEL` env var; few Windows prerelease users, revisit on feedback) and
asked what the `Q2_RELEASES_API_BASE` test seam signs us up for; the threat
model is recorded under Decision 6.

## Overview

Today every `q2` release is cut by hand from the runbook
(`claude-notes/instructions/release-runbook.md`): bump the workspace version
on a PR, merge, tag, push, watch `release.yml`. That flow has produced 32
green releases (v0.1.0 → v0.32.0) and its gates are trusted. The cadence has
become near-daily (22 releases between 2026-08-05 and 2026-09-18), which is
exactly the shape a scheduled workflow should absorb.

Goal: a **`Nightly` GitHub Actions workflow** that runs on a schedule, checks
whether `main` has changes that no release has shipped, and if so builds the
full five-platform artifact set through the *same* jobs `release.yml` uses,
publishing them as a rolling **prerelease** tagged `nightly`. When `main` is
already released, it exits in seconds and burns no runners.

The plan reuses the release pipeline rather than copying it: `release.yml`'s
build/verify/sign/publish jobs move into a **reusable workflow**
(`workflow_call`), and both the tag-triggered release and the nightly become
thin callers of it. That keeps every existing gate (placeholder-embed check,
Alpine no-glibc check, asset-manifest parity, minisign verification against
the pinned pubkey) active for nightlies without maintaining two copies.

## What the previous runs tell us (evidence)

Inspected with `gh run list --workflow=release.yml` and `gh run view --json
jobs` on 2026-09-19.

**Outcomes.** 39 runs since 2026-06-12. 35 green, 4 red. Every red run was a
first attempt at a new gate (three during the v0.1.0 dry-run: missing
toolchain target, musl/v8 404, minisign missing on jammy; one for v0.27.0:
fresh-clone docs render needed `docs/examples/` staged). No run has ever
failed on a flaky build. Reruns after a fix have always gone green on the
next tag push.

**Duration** (v0.32.0, run 35397698240, the most recent):

| Job | Wall time |
|---|---|
| preflight (tag == Cargo.toml) | 10 s |
| hub-mcp-bundle (universal MCP tarball) | 35 s |
| web-payloads (WASM + SPA + trace viewer + docs embed) | 40 min |
| build × 5 (after web-payloads) | 10–24 min each, in parallel; Windows slowest |
| asset-manifest-check | 7 s |
| release (combine, sign, `gh release create`) | 30 s |
| **Total** | **65 min** |

Inside web-payloads, `cargo xtask build-agents-docs` alone is 31 of the 40
minutes (bd-9nf0ucdf tracks the recompile-per-example waste). The WASM build
is 6 min. Earlier runs (v0.11–v0.26, before the docs embed existed) took
25–35 min total. A nightly therefore costs roughly one hour of wall time and
~2.5 runner-hours (one Ubuntu, one Ubuntu-ARM, two macOS, one Windows), on a
public repo where hosted runners are free.

**What the pipeline requires** (from `release.yml` and the runbook):

- Secrets `MINISIGN_SECRET_KEY`, `QUARTO_HUB_MCP_CLIENT_ID`,
  `QUARTO_HUB_MCP_CLIENT_SECRET`; variable `QUARTO_HUB_SERVER`. All present
  (`gh secret list` / `gh variable list`). A scheduled workflow on the default
  branch has access to all of them.
- `permissions: contents: write` for `gh release create` and tag push.
- Toolchains: pinned Rust nightly with `wasm32-unknown-unknown` +
  matrix targets, clang, `wasm-bindgen-cli` at the `Cargo.lock` version,
  binaryen 132, Node 24, `musl-tools` on the linux legs, Docker for the Alpine
  gate (present on both Ubuntu images).
- The **tag must equal `[workspace.package].version`** (preflight). This is
  the one requirement a nightly cannot satisfy as-is: `main` carries the
  *last released* version (0.32.0 today), the bump lands on `main` only at
  release time, and a nightly must not commit to `main`. See Decision 2.
- Every build uses `--locked`, so the lockfiles must stay untouched on the
  runner. This rules out editing `Cargo.toml` on the runner without also
  regenerating both lockfiles (see Decision 2 for why we avoid that).
- The verify gate compares the binary's `--version` last token to the
  preflight version, checks all embeds are real (not placeholder), all
  keyring addons for the leg are present, and that the docs embed's
  `commit:` equals `HEAD`. A `(dirty)` marker is only a warning; bd-8e96g942
  tracks why v0.32.0's tree was dirty.
- Branch protection on `main`: force-push and deletion blocked; **no
  required status checks and no required reviews are set** at the API level
  (checked 2026-09-19; CLAUDE.md's "one approving review" note is stale).
  There are no rulesets and no tag protection rules, so a workflow token can
  create, move, and delete a `nightly` tag.

**Precedent in this repo.** `hub-client-e2e.yml` already runs on a
`schedule:` (cron `0 7 * * *`, 02:00 EST) with `run-name` branching on the
trigger and a "did anything relevant change" step that gates the slow part.
The nightly release workflow follows the same shape.

## Decisions (please review; recommendations marked)

### Decision 1 — What the nightly *is*: a prerelease channel, not an auto-cut release

Two readings of "runs nightly whenever there are unreleased changes":

- **(A) Nightly prerelease channel (recommended).** The nightly publishes a
  rolling `nightly` prerelease. Stable releases stay human-cut via the
  runbook. `install.sh` / `install.ps1` keep resolving `releases/latest`,
  which GitHub defines as the newest *non-prerelease*, so users on the
  one-liner are unaffected. Nightlies are for people who opt in (and for us,
  to learn about release-pipeline breakage the morning after a merge instead
  of the afternoon we want to ship).
- **(B) Auto-cut a real release every night `main` moved.** The workflow
  would have to bump `Cargo.toml` + both lockfiles, commit to `main`, tag,
  and publish. This removes the human from version selection (patch vs
  minor), commits from a bot onto `main`, and makes every green nightly a
  supported version. The recent cadence suggests you might want this
  eventually, but it is a process decision more than a workflow one.

The rest of the plan assumes **(A)**. (B) can be layered on later: with the
reusable workflow in place it is one extra caller.

### Decision 2 — How the nightly gets a distinguishable version without touching `main`

The binary must report something other than `0.32.0` (users and bug reports
need to tell a nightly from the release it followed), the workflow must not
commit to `main`, and the build must stay `--locked`.

- **Recommended: build-time override via `option_env!`.** Add
  `QUARTO_VERSION_OVERRIDE` (name to be confirmed) read in
  `quarto-util/src/version.rs`, the same pattern `quarto-mcp-launcher`
  already uses for the bundled hub defaults. Unset (every local and release
  build): behaviour is byte-identical to today. Set by the nightly caller to
  the full string: `--version` prints `q2 (quarto 2) 0.33.0-nightly.20260919`.
  It has to be a full override rather than a suffix appended to
  `CARGO_PKG_VERSION`, because the base is the *next* minor, not the one in
  `Cargo.toml` (see "Version string" below). `Cargo.toml` and both lockfiles
  are untouched, `--locked` holds, the tree stays clean, and no `(dirty)`
  warnings appear. `cli_version_display()` currently returns `&'static str`
  via `concat!`; with an override it becomes a `OnceLock<String>` (or
  `Box::leak`), still handing clap a `&'static str`. The release caller never
  sets the variable, and the verify gate compares `--version` to the tag
  version regardless, so a stray override cannot ship in a stable release.

**One version string (requirement from review).** The gate job computes the
nightly version exactly once and it flows, unchanged, into every place a
version appears: the `QUARTO_VERSION_OVERRIDE` build env (so `q2 --version`
prints it), the asset filenames (`q2-<version>-<platform>.tar.gz`), the
release title, the `<meta name="generator">` tag of rendered documents, the
`quarto_build_id` cache key, and the verify gate's expected value. The gate
is what enforces it: the leg fails if the binary's `--version` last token is
not the gate's string, exactly as it does for tags today.
- Rejected: editing `Cargo.toml` on the runner. Requires `cargo update
  --workspace` for **both** lockfiles on the runner, dirties the tree
  (docs-embed and MCP `build-info.json` both record dirtiness), and diverges
  the nightly from "exactly what is on `main`".

**Version string.** `<next-minor>-nightly.<YYYYMMDD>`, e.g.
`0.33.0-nightly.20260919` when `main` is at 0.32.0. Rationale: SemVer orders
`0.32.0-nightly.x < 0.32.0`, so suffixing the *current* version would make a
nightly that contains everything in 0.32.0 sort *below* it and fail any
`quarto-required: ">=0.32.0"` extension check. Guessing the next minor is
right for every release so far except two patch releases (0.1.1, 0.4.1), and
being wrong is harmless for a channel that is replaced every night. The
commit SHA goes in the release notes and title, not the version string
(`+build` metadata would put a `+` in asset filenames and download URLs).

**Consumers of `CARGO_PKG_VERSION` that should or should not see the suffix**
(inventory from `grep`, to be settled in Phase 1):

| Site | Nightly should report | Notes |
|---|---|---|
| `quarto-util::cli_version_display` (`q2 --version`) | suffixed | the contract the workflow verifies |
| `quarto-util::cli_version` (`<meta generator>`) | suffixed | lets rendered HTML identify a nightly |
| `quarto-core::version()` → `HostGlobalConfig.quarto_version` | suffixed | passed to engines; informational |
| `quarto-core::project::cache_key::quarto_build_id` | suffixed | a nightly must not share caches with the release it followed; the doc comment already anticipates a per-build id |
| `quarto-core::template.rs` (`quarto-version` template var) | suffixed | |
| `quarto-lsp` server info | suffixed | |
| `pampa` Lua `quarto.version` | unchanged | hardcoded `{0,1,0}` today, a separate pre-existing bug (file a strand; out of scope) |
| `extension/read.rs` `quarto-required` | unchanged | parses the requirement, not our version; verify it compares with prerelease-aware semver |

### Decision 3 — Rolling `nightly` tag + one prerelease, replaced each night

- **Recommended: one rolling release.** Tag `nightly` is force-moved to the
  built commit; the previous `nightly` release is deleted and recreated
  (`gh release delete nightly --cleanup-tag`, then `git push -f origin
  nightly`, then `gh release create nightly --prerelease`). Release title:
  `q2 nightly 0.33.0-nightly.20260919 (192a231d)`. Asset filenames follow
  the existing contract with the nightly version in the `<version>` slot:
  `q2-0.33.0-nightly.20260919-linux_amd64.tar.gz` and so on. The releases
  page shows exactly one nightly; `git tag` grows by one entry, ever.
- Rejected: a dated tag per night (`v0.33.0-nightly.20260919`). Accumulates
  hundreds of tags and prereleases with no consumer, and a `v*` tag name
  would look like it should trigger `release.yml` (it would not, because
  events created with `GITHUB_TOKEN` never trigger workflows, but that is a
  trap for the next reader).

The tag name `nightly` does not match `v*`, so `release.yml`'s trigger is
untouched either way.

### Decision 4 — "Unreleased changes" means: `main` HEAD is neither the newest `v*` tag nor the current `nightly` tag

The gate job fetches tags and computes:

1. `LATEST_RELEASE=$(git describe --tags --abbrev=0 --match 'v*' HEAD)` and
   its commit. If HEAD equals it, `main` is fully released; skip.
2. The commit `nightly` points at (if any). If HEAD equals it, last night's
   build already covers this tree; skip.
3. Otherwise build.

Both checks are exact-commit comparisons on `origin/main`, no path filters.
A commit that only touches `claude-notes/` still triggers a build; that is
deliberate for v1 (path filters are a cheap follow-up if the noise bothers
us, and a nightly that only re-embeds docs is still a valid nightly).

`workflow_dispatch` gets a `force: boolean` input to bypass the gate for
testing, and a `publish: boolean` (default true) so the pipeline can be
dry-run on a branch without touching the `nightly` release.

### Decision 5 — Schedule

`cron: '0 8 * * *'` (03:00 EST / 04:00 EDT), one hour after the e2e nightly
so the two do not compete for the same rust-cache write window and so a
nightly release never starts before the previous day's merges have had CI.
Concurrency group `nightly`, `cancel-in-progress: false`. GitHub disables
schedules after 60 days without repo activity; not a concern here.

Failure notifications for scheduled runs go to the user who last committed
the workflow file. Whoever lands this should expect the emails.

### Decision 6 — Installing a nightly is one flag away from installing a release

Requirement from review: the README must make a nightly as easy to install
as a release. Today both installers resolve `releases/latest`, which by
design never returns a prerelease, so without changes a nightly is only
installable by hand-downloading the archive.

- **README.** The "Installing" section gets a second pair of one-liners
  directly under the stable ones:

  ```sh
  curl -fsSL https://raw.githubusercontent.com/quarto-dev/q2/main/install.sh | bash -s -- --nightly
  ```
  ```powershell
  & ([scriptblock]::Create((irm https://raw.githubusercontent.com/quarto-dev/q2/main/install.ps1))) -Nightly
  ```
  with two sentences: nightlies are built from `main` every night it
  changes, report a version like `0.33.0-nightly.20260919`, and are
  replaced daily. The PowerShell form is the standard way to pass a switch
  to an `irm | iex` script; if it reads too heavy, the alternative is an
  environment variable (`$env:Q2_CHANNEL = 'nightly'; irm ... | iex`), and
  `install.sh` can honour the same `Q2_CHANNEL` so both installers share
  one story. **Pick one in review**; the plan assumes the flag.
- **`install.sh --nightly`.** Resolves the `nightly` release through
  `GET /repos/quarto-dev/q2/releases/tags/nightly` and picks the asset
  whose name matches `q2-*-<platform>.tar.gz` (the version is in the asset
  name, not the tag, so the tag-derived URL path used for `v*` releases does
  not apply). Everything after resolution is unchanged: the `.sha256` and
  `.minisig` sidecars are fetched from the same URL stem, the signature is
  verified against the pinned pubkey with the trusted comment equal to the
  filename. `--nightly` together with `--version` is an error; `--version
  X-nightly.Y` dies with a hint to use `--nightly` (old nightlies are gone
  by construction, so there is nothing to pin to). `--help` lists the flag,
  which the existing `help_lists_every_flag_and_exits_zero` test checks.
- **`install.ps1 -Nightly`.** Same resolution through `Invoke-RestMethod`
  on the tag endpoint and the asset list. Checksum-only, as today.
- **Offline testability.** `bootstrap_sh.rs` is offline by construction
  (`--artifact-url file://` + `--checksum`). The resolution step needs one
  seam: the API base URL read from `${Q2_RELEASES_API_BASE:-https://api.github.com/repos/$OWNER/$REPO}`,
  which the tests point at a `file://` directory holding a
  `releases/tags/nightly` JSON fixture (curl reads `file://` natively). The
  variable is deliberately undocumented in `--help` and commented as a test
  seam; it changes only *where the release list is read from*, never the
  checksum or signature checks. **Review point:** if a test-only knob in the
  installer is unwelcome, the fallback is to test resolution only in the
  post-publish smoke job (Phase 3), which is real but not offline.

  *Threat model for the seam (answer to review, 2026-09-19).* The variable
  is read in exactly one place, as the prefix of the one `GET
  .../releases/tags/nightly` request in `resolve_nightly`; the stable path
  through `releases/latest` never consults it. The only value extracted from
  the response is the platform's asset URL. Controlling the variable lets an
  attacker choose the archive and its `.sha256` sidecar (the sidecar is
  fetched from the same URL stem, as it is today for every URL), but not the
  signature: the `.minisig` must verify against the public key hardcoded in
  the script with a trusted comment equal to the filename, which only the
  repo secret can produce. A redirected API therefore ends in a refused
  install unless the user also passes `--insecure-skip-signature`, which
  already warns loudly. The documented `--artifact-url` flag already accepts
  arbitrary URLs including `file://` (every offline test uses it), so the
  seam exposes strictly less than an existing public flag. The `file://`
  case reads one JSON document with `curl` and echoes only the version
  parsed from the asset name; no file content reaches disk or the network.
  Implementation carries a comment saying exactly this above the variable.
- **The stale claim in `bootstrap_sh.rs`.** Its header says `install.ps1`
  is "smoke-tested by the release workflow". No workflow runs either
  installer today (grep over `.github/workflows/`). The nightly's
  post-publish smoke job (Phase 3) makes that claim true for both
  installers and the header is corrected to point at it.

## Architecture

```
.github/workflows/
  release-pipeline.yml   NEW  on: workflow_call
                              inputs: ref, version, tag, title, prerelease,
                                      publish, notes_range, rolling
                              secrets: inherit
                              jobs: web-payloads, hub-mcp-bundle, build×5,
                                    asset-manifest-check, release
                              (moved verbatim from release.yml; the
                               `needs.preflight.outputs.*` references become
                               `inputs.*`; `Build release binary` gains
                               QUARTO_VERSION_OVERRIDE from inputs)
  release.yml            KEEP on: push tags v*, workflow_dispatch
                              jobs: preflight (unchanged) → uses:
                              ./.github/workflows/release-pipeline.yml
  nightly.yml            NEW  on: schedule, workflow_dispatch(force, publish)
                              jobs: gate → uses:
                              ./.github/workflows/release-pipeline.yml
                              → install-smoke (ubuntu, macos, windows:
                                run the README one-liners with the nightly
                                flag, assert `q2 --version` == gate version)
install.sh / install.ps1     --nightly / -Nightly (Decision 6)
README.md                    nightly one-liners under the stable ones
```

The `release` job in the reusable workflow grows one branch: when
`inputs.rolling` is true it deletes any existing release+tag named
`inputs.tag` and force-pushes the tag to `inputs.ref` before `gh release
create`. When `inputs.publish` is false it stops after signing and uploads
the signed set as a workflow artifact instead.

Cache interplay is a bonus: `Swatinem/rust-cache` keys stay
`release-<target>` / `release-web-payloads`, and caches saved by the nightly
on `main` are readable by tag-triggered runs (GitHub lets any run read the
default branch's caches). A nightly the night before a release keeps the
release's caches warm.

## Checklist

### Phase 0 — Tests first (TDD, per CLAUDE.md)

- [x] `quarto-util/src/version.rs`: unit tests for a pure
      `effective_version(cargo: &str, override: Option<&str>)` helper:
      `None` returns the Cargo version unchanged; `Some("0.33.0-nightly.20260919")`
      returns it verbatim; an override that is not a valid SemVer string is
      rejected at build time (a `build.rs` or `const` assertion, so a typo in
      the workflow fails the build, not the verify gate); the display form
      keeps the version as the last whitespace token in both cases.
- [x] `bootstrap_sh.rs`: offline tests for `--nightly` (Decision 6) against
      a `file://` API fixture: resolves the platform's asset from the
      `nightly` release JSON; downloads, verifies checksum + signature,
      installs, and the installed binary's `--version` ends in the fixture's
      nightly version; `--nightly --version v1` is an error naming both
      flags; `--version 0.33.0-nightly.20260919` dies pointing at
      `--nightly`; a `nightly` release with no asset for the platform dies
      cleanly; `--help` lists `--nightly`.
- [x] `crates/quarto/tests/integration/version_cli.rs`: an assertion that a
      binary built *without* the env var still prints exactly
      `CARGO_PKG_VERSION` (the existing tests already cover this; extend the
      module doc to name the env var and the nightly contract).
- [x] `cache_key.rs`: a test that `quarto_build_id()` reflects the suffix
      when set (compile-time, so the test asserts the wiring, not a runtime
      switch).
- [x] Gate logic: put the "should we build" decision in
      `scripts/nightly-gate.sh` (pure bash over `git`), with a test in
      `crates/quarto/tests/integration/` that drives it against a temporary
      repo (released HEAD → skip; nightly-tag HEAD → skip; new commit →
      build; no tags at all → build). Mirrors how `bootstrap_sh.rs` tests
      `install.sh` offline.

### Phase 1 — Version suffix in Rust

- [x] Add `QUARTO_VERSION_OVERRIDE` (`option_env!`) handling in
      `quarto-util/src/version.rs`; route the consumers in the Decision 2
      table through it; keep `cli_version_display()` returning `&'static
      str` for clap. Consumers rerouted: `quarto_core::version()`,
      `cache_key::quarto_build_id()`, the `version` template variable,
      and `quarto-lsp`'s server info (new `quarto-util` dep). Compile-time
      guard verified: `QUARTO_VERSION_OVERRIDE=v0.33.0 cargo check -p
      quarto-util` fails with the E0080 message naming the variable; the
      dated form checks clean.
- [x] Local end-to-end check of the override (2026-09-19, output inspected):
      ```
      $ QUARTO_VERSION_OVERRIDE=0.33.0-nightly.20260919 cargo build --bin q2
      $ ./target/debug/q2 --version
      q2 (quarto 2) 0.33.0-nightly.20260919
      $ ./target/debug/q2 render doc.qmd && grep -o '<meta name="generator"[^>]*>' doc.html
      <meta name="generator" content="quarto-rust-0.33.0-nightly.20260919">
      $ cargo build --bin q2 && ./target/debug/q2 --version
      q2 (quarto 2) 0.32.0
      ```
      The override build was a 26 s incremental rebuild (quarto-util and
      its dependents), not a cold build.
- [x] `version_cli.rs` module doc updated. (The runbook's "version
      string" gotcha is folded into the Phase 4 runbook work.)
- [x] Filed bd-5qepzwst for the hardcoded Lua `quarto.version` `{0,1,0}`
      (discovered-from bd-p4ljdp2e); not fixed here.
- [x] `cargo xtask verify --skip-hub-build` green (2026-09-19, under
      Node 24 via fnm; all Rust + ts-package suites passed).

### Phase 2 — Extract `release-pipeline.yml` (behaviour-preserving)

- [x] Move `web-payloads`, `hub-mcp-bundle`, `build`, `asset-manifest-check`,
      `release` into `release-pipeline.yml` under `on: workflow_call`;
      replace `needs.preflight.outputs.{tag,version}` with `inputs.*`;
      keep every comment block (they are the institutional memory of four
      dry-run iterations). Inputs as built: `ref`, `version`, `tag`,
      `channel` (`release`|`nightly`), `publish`, `notes_range`. A single
      `channel` switch replaced the sketch's separate `prerelease` /
      `rolling` / title inputs — fewer inconsistent combinations, and a
      new `check-inputs` job rejects a channel/tag/version mismatch
      before any runner is spent.
- [x] `release.yml` becomes preflight + `uses:` with `secrets: inherit`
      (plus a `publish` dispatch input for dry runs).
- [x] Add `inputs.publish` (dry-run: sign, then upload `release-set`
      artifact, skip `gh release create`); rolling behaviour is implied by
      `channel: nightly`.
- [x] Add `QUARTO_VERSION_OVERRIDE` to the `Build release binary` env and to
      the docs-embed step's host `q2` build, so the embedded docs and the
      binary agree. Verify gate compares against `inputs.version`.
      Exported only when non-empty: an empty value trips the compile-time
      assertion (`option_env!` yields `Some("")`).
- [x] Dry-run on a branch: `workflow_dispatch` `release.yml` against the
      existing `v0.32.0` tag with `publish: false`. **Run 35456861792**
      (2026-09-19, from `feature/bd-p4ljdp2e-nightly-release-workflow`):
      every job green — preflight, `check-inputs`, hub-mcp-bundle,
      web-payloads, all 5 build legs (both Alpine gates included),
      asset-manifest check, and the release job's dry-run branch. Verified
      from the `release-set` artifact (output inspected):
      - file set identical to the published v0.32.0 release (18 assets);
      - `shasum -c checksums.sha256` OK for all 6 archives;
      - every `.minisig` verifies with local minisign against the pubkey
        pinned in `install.sh`;
      - `gh release list` unchanged (v0.32.0 still Latest, published
        2026-09-18T22:42:44Z); no tag touched;
      - the darwin_arm64 binary, run natively: `q2 (quarto 2) 0.32.0`,
        `default … : bundled` ×3, docs embed `source: real` at
        `192a231d…` (`(dirty)`, same as the real v0.32.0 run —
        bd-8e96g942).
      Not byte-identical to the published archives, and that expectation
      was wrong: the tar member mtime is the build time, and the binary
      itself embeds build timestamps (MCP `build-info.json`, docs embed),
      so the two binaries differ in hash at identical size (106907824 B).
      Release notes differ in exactly one line — the Changes heading now
      reads `## Changes (v0.31.0 → 192a231d8da2)`.

### Phase 3 — `nightly.yml`

- [ ] `gate` job: checkout `main` with `fetch-depth: 0` + tags, run
      `scripts/nightly-gate.sh`, emit `build`, `version`, `sha`,
      `prev_release_tag` outputs. Compute `<next-minor>` from `Cargo.toml`.
- [ ] `pipeline` job: `if: needs.gate.outputs.build == 'true'`, `uses:
      ./.github/workflows/release-pipeline.yml` with `ref: main` (pinned to
      the gate's SHA, not the branch name, so a merge during the run cannot
      change what is built), `tag: nightly`, `prerelease: true`, `rolling:
      true`, `notes_range: <prev_release_tag>..<sha>`.
- [x] `install.sh --nightly` and `install.ps1 -Nightly` per Decision 6
      (tests from Phase 0 go green here). `--help` text, the header
      comment, and the `resolves_latest_version_from_github` ignored test
      gain a nightly sibling (`resolves_nightly_from_github`, also ignored,
      run by hand in Phase 4). `install.ps1` has no `Q2_RELEASES_API_BASE`
      seam (no offline suite exists for it; the smoke job is its test) and
      could not be parsed locally (no `pwsh` on this machine) — the
      Windows smoke leg is its first execution.
- [x] Release notes: the rolling-tag caveat, the exact SHA, "changes since
      v0.32.0" from `git log`, and the same two install one-liners the
      README carries (with the nightly flag).
- [x] `install-smoke` job, `needs: pipeline`, matrix `ubuntu-latest`,
      `macos-15`, `windows-latest`: run the README one-liner for the
      platform with the nightly flag from `raw.githubusercontent.com/.../main`
      (the installers land on `main` before the first nightly, so this is the
      real user path, not a checkout), then assert the installed binary's
      `--version` last token equals the gate's version. This is the
      end-to-end verification CLAUDE.md requires, done by the workflow every
      night instead of by hand once. Skipped when `publish: false`.
- [x] `run-name` distinguishes scheduled / manual / forced / dry-run
      (`run-name` cannot see job outputs, so the version and SHA go to the
      job summary and the gate job's name instead of the run title).
- [x] Post-merge nightly **dry run**: run 35461772435 (`main` @ `6b7be8f0`,
      `force=false publish=false` — the gate's real `unreleased` path).
      Every job green; `install-smoke` skipped as designed. `release-set`
      inspected: 17 nightly-named assets, darwin_arm64 binary prints
      `q2 (quarto 2) 0.33.0-nightly.20260919`, renders
      `generator" content="quarto-rust-0.33.0-nightly.20260919"`, docs
      embed real at `6b7be8f0`; notes use the nightly template with
      `## Changes (v0.32.0 → 6b7be8f07fbf)`. The web-payloads log shows
      `building the docs-render q2 as 0.33.0-nightly.20260919`.
- [x] First real run (`force=false publish=true`): **run 35465215381**
      (2026-09-19). Every job green, including all three `install-smoke`
      legs (ubuntu, macos-15, windows-latest — the Windows leg was the
      first ever execution of `install.ps1 -Nightly`). Published:
      `q2 nightly 0.33.0-nightly.20260919 (6b7be8f0)`, prerelease, 18
      assets, `nightly` tag at `6b7be8f0` == `origin/main`;
      `releases/latest` still `v0.32.0`.
- [ ] Next scheduled run (2026-09-20 08:00 UTC) must **skip**. Local
      preview on `main` with the tag fetched already reports
      `build=false reason=nightly-current`; confirm from the Actions run.
      **Branch-side dry run is not possible:** `gh workflow run
      nightly.yml --ref <branch>` returns `HTTP 404: workflow nightly.yml
      not found on the default branch` — GitHub registers a
      `workflow_dispatch` workflow only once it exists on `main`. So the
      nightly channel's first execution is after merge: run it first with
      `force=true publish=false` (dry run of the override build, no tag
      touched), then `force=true publish=true` for the first real nightly.
      The release channel's dry run (below, Phase 2) does exercise the
      shared pipeline from the branch.

### Phase 4 — Verification and docs

- [x] End-to-end (CLAUDE.md rule), by hand on this machine (2026-09-19,
      output inspected), in addition to the `install-smoke` job:
      ```
      $ curl -fsSL https://raw.githubusercontent.com/quarto-dev/q2/main/install.sh | bash -s -- --nightly --dest /tmp/q2-nightly-verify/bin
      ✓ done: q2 (quarto 2) 0.33.0-nightly.20260919
      $ /tmp/q2-nightly-verify/bin/q2 mcp --launcher-info | grep -c '^default .*: bundled'
      3
      $ curl -fsSL https://raw.githubusercontent.com/quarto-dev/q2/main/install.sh | bash -s -- --dest /tmp/q2-stable-verify/bin
      ✓ done: q2 (quarto 2) 0.32.0
      $ cargo nextest run -p quarto --test integration -E 'test(resolves_latest_version_from_github) | test(resolves_nightly_from_github)' --run-ignored ignored-only
      2 tests run: 2 passed
      ```
      Render + `generator` tag + docs-embed commit were checked on the
      dry-run artifact of the same commit (Phase 3 above).
- [x] README "Installing": the nightly one-liners under the stable ones
      (Decision 6 wording), with a short paragraph on what a nightly is
      and that it is signed with the same key.
- [x] Runbook: new section "Nightlies" (what they are, how to force one,
      how to dry-run the pipeline on a branch, that `release.yml` now
      delegates to `release-pipeline.yml`, that the installers have a
      nightly mode). Fixed the stale "one approving review" note in
      CLAUDE.md and the stale "smoke-tested by the release workflow"
      header in `bootstrap_sh.rs`.
- [x] `cargo xtask lint` has no rule for workflows; noted in CLAUDE.md's
      verify/CI drift paragraph that `nightly.yml` and the release
      pipeline are deliberately outside `verify`'s mirror (they publish,
      they do not gate).
- [x] Close-out: strand commented and closed with run ids and the first
      nightly URL (https://github.com/quarto-dev/q2/releases/tag/nightly).
      Runs: 35456861792 (release dry run, branch), 35461772435 (nightly
      dry run, main), 35465215381 (first nightly, main).

## Follow-ups (file as strands, not in scope)

- Auto-cut stable releases (Decision 1 option B) as a second caller of
  `release-pipeline.yml`, if wanted.
- Path filters for the gate (skip when only `claude-notes/` changed).
- bd-9nf0ucdf (docs embed recompiles q2 per example) is now paid nightly;
  worth prioritizing.
- Lua `quarto.version` hardcoded to `{0,1,0}`.

## Risks

- **Reusable-workflow refactor touches the release path.** Mitigated by the
  Phase 2 dry-run against `v0.32.0` before the nightly exists, and by
  keeping `release.yml`'s trigger and preflight unchanged.
- **Rolling tag surprises `git fetch` users.** Standard for nightly channels
  (neovim, zig do the same); documented in the release notes.
- **Nightly reveals `main` was broken for release for days.** That is the
  point; a red nightly is a signal, and it should be treated like a red
  `test-suite` run.
- **Two scheduled workflows now depend on the same rust-cache.** Cache
  entries are read-shared and each job writes its own key, so the only cost
  is cache-storage pressure (10 GB repo limit; the e2e and release keys
  already coexist).
