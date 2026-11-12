# Release runbook — cutting a `q2` binary release

How to ship a signed, multi-platform `q2` binary release through the
`Release` GitHub Actions workflow (`.github/workflows/release.yml`).

This procedure and its gotchas were established cutting **v0.1.0**
(strand bd-c6l13j79; the release tooling was ported from
cscheid/braid). Read the **Gotchas** section before your first release
— several steps are non-obvious and cost a four-iteration dry-run to
discover.

## What a release produces

A GitHub Release at tag `vX.Y.Z` with, for six platforms
(`linux_amd64`, `linux_arm64`, `darwin_amd64`, `darwin_arm64`,
`windows_amd64`, `windows_arm64`):

- `q2-<version>-<platform>.tar.gz` (`.zip` on Windows) — just the `q2`
  binary; the hub MCP server, preview SPA, trace viewer, and the docs
  site's llms.txt mirror (`q2 docs llms`) are all embedded in it via
  `include_dir!`.
- `q2-<version>-<platform>.tar.gz.sha256` — checksum.
- `q2-<version>-<platform>.tar.gz.minisig` — Ed25519 signature
  (Unix only; the Windows `.zip` is checksum-only, matching `install.ps1`).
- `checksums.sha256` — combined.

Plus, once per release (not per platform), a standalone Quarto Hub MCP
server bundle (bd-sca6g1tu):

- `quarto-hub-mcp-<version>.tar.gz` — the same MCP server that's embedded
  in `q2`, packaged as a self-contained Node bundle so it can be run
  directly (`node index.mjs`) without installing `q2`. One *universal*
  bundle (not per-platform): `index.mjs` is byte-identical everywhere,
  every `@napi-rs/keyring` platform addon is co-staged, and the
  platform-independent `wasm-qmd-parser` pkg (CAP-11) is built once in
  `web-payloads` and staged by every bundle leg without rebuilding, so it
  runs on any OS/arch with Node 24+. Built by the `hub-mcp-bundle` job;
  includes a `README.md` and `NOTICE`.
- `quarto-hub-mcp-<version>.tar.gz.sha256` / `.minisig` — checksum +
  signature (same minisign key as the binaries; its `.sha256` is also
  folded into `checksums.sha256`).

> **Temporary channel.** This tarball is a stopgap until the MCP server is
> published to npm (`npx @quarto/hub-mcp`, bd-3tak0lyy). When that lands,
> revisit whether to keep or drop the `hub-mcp-bundle` job. Plan:
> `claude-notes/plans/2026-06-19-release-standalone-hub-mcp-bundle.md`.
> Note this bundle does **not** carry the quarto-hub.com OAuth defaults
> (those are embedded into the `q2` binary only) — its `README.md`
> documents the OAuth env vars a direct user must set.

Plus, once per release (bd-m7oy2asc), the hub-client WASM package for
Connect satellite editor builds:

- `quarto-hub-client-wasm-<version>.tar.gz` — the compiled
  `crates/wasm-quarto-hub-client/pkg/` tree (wasm-bindgen `--target web`,
  `wasm-opt -Oz`), packaged once in `web-payloads` with repo-relative
  paths. Connect satellites extract it at the root of a q2 checkout of
  the same tag (`tar -xzf ... -C /path/to/q2`) so `build-editor.sh` does
  not need a nightly-Rust WASM rebuild. Plan:
  `claude-notes/plans/2026-10-20-release-hub-client-wasm-artifact.md`.
- `quarto-hub-client-wasm-<version>.tar.gz.sha256` / `.minisig` — checksum +
  signature (same minisign key as the binaries; its `.sha256` is also
  folded into `checksums.sha256`).

Released binaries carry the bundled quarto-hub.com OAuth defaults, so
`q2 mcp` connects with zero configuration. Users install with the
one-liner in the README / release notes.

## Prerequisites (one-time, already done for quarto-dev/q2)

These live in repo settings and rarely change. Verify with
`gh secret list --repo quarto-dev/q2` and `gh variable list`:

| Name | Kind | Purpose |
|------|------|---------|
| `MINISIGN_SECRET_KEY` | secret | Ed25519 signing key (passwordless). Public half pinned in `install.sh` (`MINISIGN_PUBKEY`, key id `91F595A50BD20376`) and the README. |
| `QUARTO_HUB_MCP_CLIENT_ID` | secret | Bundled Google OAuth client id (public client; secret only to dodge `GOCSPX-` scanners). |
| `QUARTO_HUB_MCP_CLIENT_SECRET` | secret | Bundled client secret (same). |
| `QUARTO_HUB_SERVER` | variable | `wss://quarto-hub.com/ws`. |
| `HOMEBREW_TAP_TOKEN` | secret | Fine-grained token, Contents read/write on `quarto-dev/homebrew-tap` only. The `homebrew-tap` job pushes the formula with it; unset, that job skips (the `homebrew-formula` and `homebrew-test` jobs still run). |

The signing **secret key** lives only in the repo secret and the team
password manager. If it is ever rotated: regenerate with
`minisign -GW`, update the repo secret, update `MINISIGN_PUBKEY` in
`install.sh` + README, and re-sign the current release.

## The procedure

### 1. Decide the version and pin the scope

- Pick `X.Y.Z` (SemVer). The release contains exactly what is on
  `origin/main` at the commit you tag — so first make sure every PR you
  want shipped is **merged to main** (not just open). Open PRs are not
  in the release.
- The CLI reports the workspace version verbatim (no placeholder; see
  `quarto-util/src/version.rs`), so `vX.Y.Z` and the binary's
  `--version` will agree.

### 2. Bump the version (on a branch → PR → merge)

The release workflow's preflight job **fails the release** unless the
git tag exactly equals `[workspace.package].version` in the root
`Cargo.toml`. So bump first, on `main`:

```bash
cargo xtask switch-task <strand>           # or: git switch -c release/vX.Y.Z main
# edit root Cargo.toml: [workspace.package] version = "X.Y.Z"
cargo update --workspace                    # rewrites Cargo.lock workspace versions only
cargo update --manifest-path crates/wasm-quarto-hub-client/Cargo.toml --workspace   # SECOND lockfile, see below
git diff '**/Cargo.lock' | grep -E '^[+-]version' | grep -v 'OLD\|NEW'   # sanity: no surprise bumps
cargo build --bin q2 --locked && ./target/debug/q2 --version        # must print "q2 (quarto 2) X.Y.Z"
```

`cargo update --workspace` (not a full `cargo update`) touches only the
workspace members\' own version entries — external deps stay pinned. The
`--locked` build is the real check: CI builds with `--locked`, so the
lockfile must already be in sync or every build leg fails.

**There are two lockfiles.** `crates/wasm-quarto-hub-client/` declares
its own `[workspace]` so cargo does not traverse up into the monorepo,
which means it carries a *separate* `Cargo.lock` recording the versions
of every path dependency it pulls from `crates/`. A root
`cargo update --workspace` does not touch it. Miss it and nothing fails
loudly — the WASM build simply rewrites the file, so the next person to
run `cargo xtask verify` (without `--skip-hub-build`) finds \~17 lines of
unrequested churn in their working tree. The v0.17.0 bump missed it;
v0.15.0 and v0.16.0 did not. Both locks should appear in the bump commit.

Commit (Cargo.toml + both Cargo.lock files), push, open a PR, get it merged. A
version bump is small but still goes through a PR — `main` is gated
(CI clippy gate, bd-3zst4hwy).

A version bump **should not break any test.** Rendered output embeds
the version in a `<meta name="generator" content="quarto-rust-X.Y.Z">`
tag, but byte-identity/snapshot tests normalize it away (e.g.
`artifact_scoping_pipeline.rs` replaces the crate version with a
placeholder before hashing). If a bump *does* break a test, harden the
test to absorb version churn — do not blindly re-capture a snapshot to
the new version, or it just breaks again next cadence (bd-yomgkxoc).

### 3. Tag the merged commit and push

After the bump PR is merged, tag `origin/main`\'s new HEAD:

```bash
git switch main && git pull --ff-only
git tag -a vX.Y.Z -m "q2 vX.Y.Z"
git push origin vX.Y.Z
```

Pushing a `v*` tag triggers the `Release` workflow. (You can also
re-run an existing tag from the Actions UI via `workflow_dispatch` with
the `tag` input — useful for a transient-infra retry without moving the
tag.)

A tag with a pre-release suffix (`vX.Y.Z-rc1`) is auto-marked
**prerelease** by `gh release create`. But preflight still requires
`Cargo.toml` to match exactly — so an `-rc1` tag needs
`version = "X.Y.Z-rc1"` in the manifest too.

### 4. Watch the run

```bash
gh run list --repo quarto-dev/q2 --workflow=release.yml --limit 1
gh run watch <run-id> --repo quarto-dev/q2     # or watch in the Actions UI
```

Job graph: `preflight` → **`release-pipeline.yml`** (a reusable
`workflow_call` workflow, shared with the Nightly — see § Nightlies):
`check-inputs` → `web-payloads` (WASM → preview SPA + `--ui editor`
embed + trace viewer + the `q2 docs llms` docs embed, built once) →
`build` matrix (6 platforms,
each builds the per-target MCP bundle then the binary, then a **verify
gate**) → `release` (combines checksums, signs every `.tar.gz`,
publishes). In the Actions UI the pipeline's jobs appear nested under
the `Release pipeline` job of the `Release` run.

`workflow_dispatch` also takes a `publish` checkbox; untick it for a
**dry run** that builds, verifies and signs an existing tag and uploads
the would-be release as a `release-set` artifact without creating or
touching any release. That is how to test changes to the pipeline
itself from a branch (`gh workflow run release.yml --ref <branch> -f
tag=vX.Y.Z -f publish=false`).

The per-target verify gate is the anti-stale-embed guard: it fails the
leg unless `q2 mcp --launcher-info` reports a real (non-placeholder)
bundle, all three hub defaults as `bundled`, a keyring addon for each
platform in that leg's `KEYRING_PLATFORMS`, and `q2 docs llms
--embed-info` reports `source: real` at exactly this tag's commit. The
asset-manifest step additionally fails the leg unless `q2 preview
--print-asset-manifest-hashes` reports a real (non-`none`) hash for
both the viewer and the `--ui editor` embeds (GH #869), and the
`asset-manifest-check` job then requires those hashes to be identical
across platforms. A
`(dirty)` marker on that commit is a **warning**, not a failure — the
docs still came from this commit, but something modified tracked files
during the `web-payloads` job and is worth a look.

The docs embed is staged in `web-payloads` (`cargo xtask
build-agents-docs`), not per-leg: it is target-independent, and a
cross-compiled leg could not run the `q2` that renders the docs
anyway. Its `commit:` is captured *before* the render, so it records
the sources the docs were built from rather than the render's own
byproducts.

### 5. If a leg fails: fix, merge, re-tag

First decide which kind of failure it is. This applies to a red check on
the bump PR, a red Release run, and a red dry run alike.

- **Flake or runner infrastructure.** Examples: a timing assertion that
  missed by a millisecond, or a runner-setup step that errored before any
  of our code ran. Confirm the failure is unrelated to the release's
  changes. Check whether `main` or recent nightlies have shown it before,
  and file a strand for it (with run id and log excerpt) so it gets fixed
  eventually. Then retry with `gh run rerun <id> --failed` and carry on.
  Examples from v0.34.0: bd-7kp7kjdq (test timing) and bd-x8ihp4qu
  (Defender step on `windows-11-arm`).
- **A substantive defect.** The code on `main` is wrong: a compile error,
  a real test failure, a broken artifact. Stop the release and **change
  gears**:
  1. **Diagnose** until you can state the cause. That means the failing
     step, the offending commit, a reproducer that runs locally if one
     exists, and *why per-PR CI didn't catch it*. A failure that only the
     release pipeline sees almost always means a CI gap, and the gap is
     often the more important defect.
  2. **File a self-contained braid strand.** Someone who has never seen
     the session should be able to act on it. Include the symptom (run id
     and log excerpt), the root cause, the reproducer, the proposed fix
     and why it is the right one, the CI gap, and the release status
     (what is tagged where, and that nothing was published). Link it
     `discovered-from` the strand whose work introduced the defect.
  3. **Work it through the normal workflow, not as a release-side
     patch.** Use `/investigate-beads`: reproduce, write the plan file,
     and iterate on the plan with the user. Then fix it on its own branch
     and PR, with a test. An obvious one-line fix still goes through a
     reviewed PR. Ask the user before folding the CI-gap fix into the
     same PR. Keeping the release fix minimal is usually right; the CI
     gap can follow in its own PR.
  4. **Resume the release** once the fix is merged. Usually that means
     moving the tag (below). Before re-tagging, prove the pipeline step
     that failed now passes. If per-PR CI could not have caught the
     defect, per-PR green proves nothing. Run a non-publishing dry run of
     the fix branch (`gh workflow run nightly.yml --ref <branch> -f
     force=true -f publish=false`) and check that the failed step passed.

  First case: v0.34.0, bd-yt3x1gwk. pampa did not compile without
  `lua-filter`. The parser-tier CI step silently skipped on the build
  failure, so only the release's `web-payloads` job caught it.

The tag is just a pointer. To iterate:

```bash
# land the fix on main via PR, then:
git switch main && git pull --ff-only
git push --delete origin vX.Y.Z && git tag -d vX.Y.Z
git tag -a vX.Y.Z -m "q2 vX.Y.Z" && git push origin vX.Y.Z
```

Re-pushing the tag re-fires the workflow from the new commit. The
`release` job only publishes when **all six** legs succeed, so a
partial run leaves no half-published release.

### 6. Verify the published release

Don't trust "the run went green" — exercise the artifacts (CLAUDE.md
end-to-end rule):

```bash
# real installer one-liner into a temp dest
curl -fsSL https://raw.githubusercontent.com/quarto-dev/q2/main/install.sh \
  | bash -s -- --dest /tmp/q2-verify/bin
/tmp/q2-verify/bin/q2 --version                      # → q2 (quarto 2) X.Y.Z

# bundled hub defaults present (no env vars set)
env -u QUARTO_HUB_MCP_CLIENT_ID -u QUARTO_HUB_MCP_CLIENT_SECRET -u QUARTO_HUB_SERVER \
  /tmp/q2-verify/bin/q2 mcp --launcher-info | grep '^default'   # → bundled ×3

# the previously-#[ignore]d network installer test now resolves the real release
cargo nextest run -p quarto -E 'test(resolves_latest_version_from_github)' \
  --run-ignored ignored-only
```

For a thorough release, also drive a real `q2 mcp` session against
quarto-hub.com (`connect_project` + `read_file`) and spot-check the
`darwin_amd64` artifact runs under Rosetta with both darwin keyring
addons. See bd-c6l13j79's plan (Phase 4 record) for a worked example.

**You do not need to check the linux binaries against a glibc-less
distro by hand** — CI does it every release (bd-3b47pxmm). Each musl leg
runs its freshly built binary inside `alpine:latest` (which has musl and
no glibc at all) and asserts `--version`, in the `Assert the binary needs
no glibc (Alpine)` step, *before* anything is packaged or published.
Each leg runs its own arch's container natively, so both `linux_amd64`
and `linux_arm64` are covered — which a hand-run on an Apple Silicon
laptop never was. If that step ever goes red, the release stops with
nothing shipped.

### 7. Close out

Update the release strand, note the release URL, and (if the release
introduced user-facing changes) make sure the README install snippet
still matches.

## Nightlies (bd-p4ljdp2e)

`.github/workflows/nightly.yml` runs every night at 08:00 UTC and, **only
if `main` has unreleased changes**, builds it through the same
`release-pipeline.yml` and publishes the result as the rolling **`nightly`
prerelease**. Nothing here changes the release procedure above; this
section is what to know when the two interact.

- **What "unreleased" means.** `scripts/nightly-gate.sh` skips iff `main`\'s
  HEAD is exactly the newest `v*` tag (you just released) or exactly the
  current `nightly` tag (last night already built it). Any other HEAD
  builds — a docs-only commit included. A skipped night is one small
  runner for a few seconds.
- **The version.** `<next minor>-nightly.<YYYYMMDD>` — with `0.32.0` on
  `main`, tonight's nightly is `0.33.0-nightly.20260919`. It sorts above
  the release it contains (so `quarto-required: ">=0.32.0"` accepts it)
  and below the real `0.33.0`. It is injected at build time via
  `QUARTO_VERSION_OVERRIDE` (`quarto-util/src/version.rs`); `Cargo.toml`
  and both lockfiles are untouched and `--locked` still holds. **One
  string everywhere:** `q2 --version`, the asset filenames, the release
  title, the HTML `generator` tag, the project cache key, and the verify
  gate all use it. If you ever add another place that reports "the
  Quarto version", read it from `quarto_util::cli_version()`, not
  `CARGO_PKG_VERSION`.
- **Rolling tag.** Each nightly deletes the previous `nightly` release and
  tag and recreates both at the built commit. `git fetch --tags` will
  report the tag as moved; that is expected. Only the latest nightly's
  assets exist.
- **Stable installs are unaffected.** `releases/latest` never returns a
  prerelease. Nightlies install with `install.sh --nightly` /
  `install.ps1 -Nightly` (README has the one-liners); the installers
  resolve the `nightly` release by tag and pick the platform asset by
  name. Same signing key.
- **It is also the installers\' only CI.** After publishing, the
  `install-smoke` job runs both README one-liners on linux, macOS and
  Windows against the fresh nightly and asserts `q2 --version`. A red
  `install-smoke` with a green pipeline means the installers or the
  release-notes contract regressed, not the build. The smoke steps pass
  the job's `GH_TOKEN` so the installers\' API lookup is not subject to
  the shared-runner anonymous rate limit, and the installers retry the
  lookup through the seconds right after the release is replaced
  (bd-n9yh30c8). The installer's error names the HTTP status: a 403
  means rate limiting, a 404 or "lists no … archive" after all retries
  means the release really is missing or incomplete.
- **A red nightly is a signal.** The pipeline gates are the release
  gates; a nightly failure means the next release would fail the same
  way. Fix it before tagging. Failure emails go to whoever last
  committed `nightly.yml`.
- **Forcing / dry-running.** `gh workflow run nightly.yml -f force=true`
  builds a released HEAD (e.g. to reissue a nightly after a pipeline
  fix); `-f publish=false` builds, verifies and signs without touching
  the `nightly` release. From a branch (`--ref <branch>`), the gate
  evaluates that branch's HEAD, which is how to test pipeline changes
  before they reach `main`.
- **Cache interplay is a bonus.** The pipeline's `rust-cache` keys are
  shared, and caches saved on `main` are readable by tag runs, so a
  nightly the night before a release keeps the release's caches warm.

## Engine-record writer gate (EXR-13b, RIU-1)

**The rule: never hand a record to a reader that cannot parse it.** It is stated per
writer and reader pair, not per release. (The earlier wording, "do not flip it in the same
release that first ships the reader", was a stricter stand-in for this rule.) The gate
decides only the **form of the hub document payload** (a record document or the legacy
capture array); it does not decide whether the local cache writes `<key>.record.bin`, and
a plain `q2 render` never consults it. The default is per surface
(`WriterSurface` in `crates/quarto-core/src/engine/capture_files.rs`), and
`QUARTO_ENGINE_RECORD_WRITE` (`1`/`true`/`on`/`yes`, `0`/`false`/`off`/`no`) overrides both.

| Surface | Can a record be stranded? | Default |
|---|---|---|
| Local `q2 preview`, and `--share` | No, by version: writer and reader are one binary, the SPA and WASM are embedded in it, and the store is a temp directory deleted on exit. A guest on another binary tunnels the host UI. | **On**, in the first release that carries the reader |
| `q2 provide-hub` to a remote hub | Yes: the reader is whatever hub-client that hub deployed | **Off** until the hosted hub runs a `q2_ref` that contains the reader |
| A persisted `--data-dir` opened by an older binary after a downgrade | Yes, observed | On (documented, not gated) |

What strands, observed: an older `q2 preview` pointed at a store holding a record document
replaces the whole page with "Render Error: Failed to parse capture: JSON parse failed:
invalid type: map, expected a sequence". It is not shown as source. The one local-preview
case that strands is a kept `--data-dir` opened by an older binary, and the release notes
that carry the flip say so. `v0.39.0` has no `CapturePayload::Record`, so the flip waits
for the next release that carries the epic branches.

The hub exposes no version or capability a writer can read, so for `provide-hub` the rule
is operational. The hosted hub is not deployed by release number: the `quarto-hub` binary
and hub-client are built from a `q2_ref` input and deployed by hand, so "the hub has the
reader" means "the deployed `q2_ref` contains the reader commits" (`cat
/etc/quarto-hub/version.json` on the instance says which). An operator can opt in earlier
with `QUARTO_ENGINE_RECORD_WRITE=1`. Flipping the `provide-hub` default is a separate,
later change (RIU-7).

**Rollback.** Reverting the flip is one commit and changes what new runs write; it does
not change what is stored. Hub documents already written stay record documents, so a
revert, or a `quarto-hub-rollback` of the hosted hub to a build without the reader,
strands them: turn the gate off first, then provide the documents again. A persisted
`--data-dir` keeps its record documents and its cache files, so an older binary opening it
shows the Render Error above (observed 2026-11-06 with 0.39.0: "Failed to parse capture: JSON
parse failed: invalid type: map, expected a sequence"). Recovery, also observed: a new
`--data-dir` renders the page, and so does deleting the old one (the older binary records
the file again and writes only `<key>.bin`). Deleting `captures/` alone is not enough: the
capture refs live in `automerge/`. The default local preview has nothing to clean up.

**Engines that build no record.** A document that uses an engine that builds no execution
record is stored whole in the legacy capture format and gets the warning `Q-18-15`, with
every output still shown. This holds until EXR-17 deletes the legacy path.

Plans: `claude-notes/plans/2026-10-28-exr-13-hub-wire.md` (Phase D) and
`claude-notes/plans/2026-11-02-riu-01-enablement-and-writer-flip.md`.

## Gotchas (learned the hard way in the v0.1.0 dry-run)

- **Tag must equal `Cargo.toml` version** — preflight enforces it.
  Bump the manifest *before* tagging.
- **`--locked` everywhere** — the lockfile must be committed and in
  sync, or every build leg fails. Always `cargo update --workspace`
  after a version bump.
- **Linux ships static musl** (`x86_64/aarch64-unknown-linux-musl` on
  `ubuntu-latest` / `ubuntu-24.04-arm`, `--features vendored-openssl` so
  the binary has no runtime `libssl` dependency). One artifact per arch,
  no glibc floor, Alpine works with no `gcompat` shim. **There is no gnu
  artifact** — anyone who needs a dynamically-linked build uses
  `install.sh --from-source`. Both legs build *natively*, so
  `musl-tools`\' `musl-gcc` is the right compiler on each runner; the
  `Install musl-tools` step is gated `if: contains(matrix.target,
  'musl')`. History worth knowing: musl was originally blocked by
  `rusty_v8` (via `deno_core` → `quarto-system-runtime`), which shipped
  no musl prebuilts — both musl legs 404'd at the v8 download in the
  v0.1.0 dry-run, which is why the matrix was gnu from PR #280 until
  bd-dofxhzaj. That dependency was removed in bd-3e3sam51.
- **Windows ships two native builds**: `windows_amd64`
  (`x86_64-pc-windows-msvc` on `windows-latest`) and `windows_arm64`
  (`aarch64-pc-windows-msvc` on `windows-11-arm`). The ARM64 leg builds
  natively for the same reason `linux_arm64` does: an x64 host cannot
  execute an ARM64 binary, and the verify steps run it. `install.ps1`
  picks the asset from the OS architecture and falls back to
  `windows_amd64` (which ARM64 Windows runs under emulation) for releases
  that predate the ARM64 asset (bd-windows-arm64-nightly-xms5p652).
- **Because the binaries are static, the runner image no longer sets a
  compatibility floor.** The old `ubuntu-22.04` pin existed *only* to
  keep the glibc requirement low; with musl it bought nothing, so the
  linux legs track `ubuntu-latest`. Don't re-pin them without a reason
  that isn't glibc.
- **`musl-tools` is the only extra package the musl legs need.** In
  particular `aws-lc-sys` — long feared to be the hard part — is a
  non-issue at v0.40.0: it ships pregenerated bindings for both musl
  triples, so there is **no `bindgen` step and no `libclang`
  requirement**, and it compiled in \~17 s per leg in the bd-dofxhzaj
  spike (run 30375857883). If a future `aws-lc-sys` bump ever *does*
  start wanting cmake or libclang, that is a real regression worth
  pinning rather than papering over with extra apt packages.
- **`file` describes the two arches differently.** x86_64 comes out
  `static-pie linked`; aarch64 comes out plain `statically linked`. Any
  staticness check must accept **both** spellings — matching one passes
  on one arch and fails on the other. (`ldd` says *not a dynamic
  executable* on both, but exits non-zero, so it cannot be used as a
  bare assertion either.) This gotcha has a live consumer: the `Assert
  the binary needs no glibc (Alpine)` step in `release.yml` greps for
  both spellings. Don't "tidy" it down to one.
- **Staticness is a *default*, not a pin.** `crt-static` comes from the
  `*-unknown-linux-musl` targets, so a stray `RUSTFLAGS`, cargo config,
  or build-script link flag could turn the linux artifacts dynamic while
  every other gate stays green — they'd still run fine on the runner.
  The Alpine step is what catches that; it is a real gate, not ceremony
  (bd-3b47pxmm).
- **Signing happens in the `release` job, not the build matrix.** The
  secret key is touched by exactly one job, which signs the exact bytes
  being published — and the macOS/Windows build runners have no
  `minisign` anyway. (The original reason was narrower: jammy had no
  `minisign` apt package. The linux legs are no longer on jammy, but
  centralizing is still the right shape.)
- **`darwin_amd64` cross-compiles on an arm64 runner.** Its keyring
  addon must match the *user's* mac, so `KEYRING_PLATFORMS` stages both
  `darwin-x64` and `darwin-arm64` (fetched via `npm pack` when not
  installed locally — and on Windows `npm` is `npm.cmd`, which needs
  `shell: true`). The verify gate checks the right addons shipped.
- **The pinned nightly needs its target added explicitly.** The
  toolchain action adds the matrix target to the *latest* nightly, but
  cargo resolves the dated nightly in `rust-toolchain.toml`; the
  workflow runs `rustup target add <target>` to bridge that, or builds
  die with `E0463: can't find crate for core/std`.
- **The version string's last token is the bare version.** The
  pipeline's verify gate and `install.sh` parse `${output##* }`. `q2
  --version` prints `q2 (quarto 2) X.Y.Z` (or `X.Y.Z-nightly.YYYYMMDD`
  for a nightly, see § Nightlies); anything appended to that string must
  keep the version last (guarded by
  `crates/quarto/tests/integration/version_cli.rs`). Never export
  `QUARTO_VERSION_OVERRIDE` empty: `option_env!` sees `Some("")` and the
  compile-time check fails the build (the pipeline guards this with
  `[ -n ]`, not a plain `env:` entry).
- **The docs embed needs `docs/examples/` staged, and only the xtask does it.**
  `docs/_quarto.yml` declares the gitignored `docs/examples/` tree (output of
  `cargo xtask stage-doc-examples`) as a project resource, and `q2 render`
  fails hard on a missing declared resource. A dev checkout usually has the
  tree lying around from some earlier run, so `cargo xtask build-agents-docs`
  passes locally and nobody notices; the release runner is a fresh clone. The
  first v0.27.0 run (32809606624) died in `web-payloads` on exactly this —
  the docs-embed step was new in that release (bd-hwop1zii landed after
  v0.26.0) and had never run on a clean checkout. `build-agents-docs` now
  stages the examples itself before rendering; if you ever move that render
  elsewhere, carry the staging with it. To reproduce the fresh-clone
  condition locally: `mv docs/examples /tmp/` and rerun the xtask.
  The same run then exposed a second fresh-machine assumption: the docs
  render must not need an *optional runtime*. `docs/guides/authoring/figures.qmd`
  shows cells with Quarto 1's ```` ```{{python}} ```` escape, and the engine
  resolver was treating those as real `{python}` cells — fine on a laptop
  with Jupyter, a hard "runtime is not available" error on the runner
  (bd-3tq5u8vm, fixed in `engine_cell_lang`). If the docs render ever
  fails only on CI, check the runner for a runtime it lacks before
  installing one: the docs should render with nothing but `q2`.
- **Pin the embedded TS-extension-build jsr specifiers before shipping.**
  `resources/extension-build/deno.json` (the shipped tier-4 config `q2
  build-ts-extension` embeds via `include_str!` for installed binaries —
  see `crates/quarto/src/commands/build_ts_extension.rs`) currently
  imports unpinned `jsr:@quarto/api` / `jsr:@quarto/types`. Once those
  packages are published to JSR, pin them to a version
  (`jsr:@quarto/api@^X.Y`) as part of cutting a release, so installed
  binaries resolve a stable, tested API surface instead of latest.

## Files involved

| Path | Role |
|------|------|
| `.github/workflows/release.yml` | the release trigger + preflight; calls the pipeline |
| `.github/workflows/release-pipeline.yml` | the reusable build/verify/sign/publish pipeline (both channels) |
| `.github/workflows/nightly.yml` | the nightly trigger + gate + install smoke; calls the pipeline |
| `scripts/nightly-gate.sh` | "does main have unreleased changes?" + the nightly version |
| `crates/quarto/tests/integration/nightly_gate.rs` | tests for the gate script |
| `Cargo.toml` `[workspace.package].version` | source of truth for the version |
| `install.sh` / `install.ps1` | installers; pinned `MINISIGN_PUBKEY` |
| `crates/quarto/tests/integration/bootstrap_sh.rs` | offline installer tests |
| `crates/quarto/tests/integration/version_cli.rs` | `--version` output contract |
| `crates/quarto-mcp-launcher/src/defaults.rs` | bundled hub defaults (`option_env!`) |
| `ts-packages/quarto-hub-mcp/scripts/stage-keyring.mjs` | per-target keyring staging (and the universal co-stage for the standalone bundle) |
| `ts-packages/quarto-hub-mcp/scripts/bundle.mjs` | esbuild bundler producing `dist-bundle/` |
| `crates/quarto-util/src/version.rs` | version-string policy |

## Related

- `claude-notes/plans/2026-06-12-q2-github-releases-bundled-mcp.md` — the
  design + full dry-run log (bd-c6l13j79).
- `claude-notes/plans/2026-06-19-release-standalone-hub-mcp-bundle.md` — the
  standalone MCP bundle artifact (bd-sca6g1tu; temporary, pre-npx).
- `claude-notes/instructions/hub-mcp-operator-runbook.md` — running a
  *private* hub (the env-var path that bundled defaults replace for the
  canonical hub).
