# Release runbook — cutting a `q2` binary release

How to ship a signed, multi-platform `q2` binary release through the
`Release` GitHub Actions workflow (`.github/workflows/release.yml`).

This procedure and its gotchas were established cutting **v0.1.0**
(strand bd-c6l13j79; the release tooling was ported from
cscheid/braid). Read the **Gotchas** section before your first release
— several steps are non-obvious and cost a four-iteration dry-run to
discover.

## What a release produces

A GitHub Release at tag `vX.Y.Z` with, for five platforms
(`linux_amd64`, `linux_arm64`, `darwin_amd64`, `darwin_arm64`,
`windows_amd64`):

- `q2-<version>-<platform>.tar.gz` (`.zip` on Windows) — just the `q2`
  binary; the hub MCP server, preview SPA, and trace viewer are all
  embedded in it via `include_dir!`.
- `q2-<version>-<platform>.tar.gz.sha256` — checksum.
- `q2-<version>-<platform>.tar.gz.minisig` — Ed25519 signature
  (Unix only; the Windows `.zip` is checksum-only, matching `install.ps1`).
- `checksums.sha256` — combined.

Plus, once per release (not per platform), a standalone Quarto Hub MCP
server bundle (bd-sca6g1tu):

- `quarto-hub-mcp-<version>.tar.gz` — the same MCP server that's embedded
  in `q2`, packaged as a self-contained Node bundle so it can be run
  directly (`node index.mjs`) without installing `q2`. One *universal*
  bundle (not per-platform): `index.mjs` is byte-identical everywhere and
  every `@napi-rs/keyring` platform addon is co-staged, so it runs on any
  OS/arch with Node 24+. Built by the `hub-mcp-bundle` job; includes a
  `README.md` and `NOTICE`.
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
git diff Cargo.lock | grep -E '^[+-]version' | grep -v 'OLD\|NEW'   # sanity: no surprise bumps
cargo build --bin q2 --locked && ./target/debug/q2 --version        # must print "q2 (quarto 2) X.Y.Z"
```

`cargo update --workspace` (not a full `cargo update`) touches only the
workspace members' own version entries — external deps stay pinned. The
`--locked` build is the real check: CI builds with `--locked`, so the
lockfile must already be in sync or every build leg fails.

Commit (Cargo.toml + Cargo.lock), push, open a PR, get it merged. A
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

After the bump PR is merged, tag `origin/main`'s new HEAD:

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

Job graph: `preflight` → `web-payloads` (WASM → preview SPA + trace
viewer, built once) → `build` matrix (5 platforms, each builds the
per-target MCP bundle then the binary, then a **verify gate**) →
`release` (combines checksums, signs every `.tar.gz`, publishes).

The per-target verify gate is the anti-stale-embed guard: it fails the
leg unless `q2 mcp --launcher-info` reports a real (non-placeholder)
bundle, all three hub defaults as `bundled`, and a keyring addon for
each platform in that leg's `KEYRING_PLATFORMS`.

### 5. If a leg fails: fix, merge, re-tag

The tag is just a pointer. To iterate:

```bash
# land the fix on main via PR, then:
git switch main && git pull --ff-only
git push --delete origin vX.Y.Z && git tag -d vX.Y.Z
git tag -a vX.Y.Z -m "q2 vX.Y.Z" && git push origin vX.Y.Z
```

Re-pushing the tag re-fires the workflow from the new commit. The
`release` job only publishes when **all five** legs succeed, so a
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

**The first release after the musl switch (bd-dofxhzaj) deserves one
extra check**, because CI proves the binary runs on the *runner*, not on
the distros the switch is for. Download the published linux artifact and
run it somewhere with no glibc:

```bash
docker run --rm -v "$PWD:/w" -w /w alpine:latest /w/q2 --version
```

Anything that dynamically links libc fails there instantly. Once a
release has passed this, later ones inherit the confidence — it is a
one-time check on the switch, not per-release ceremony.

### 7. Close out

Update the release strand, note the release URL, and (if the release
introduced user-facing changes) make sure the README install snippet
still matches.

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
  `musl-tools`' `musl-gcc` is the right compiler on each runner; the
  `Install musl-tools` step is gated `if: contains(matrix.target,
  'musl')`. History worth knowing: musl was originally blocked by
  `rusty_v8` (via `deno_core` → `quarto-system-runtime`), which shipped
  no musl prebuilts — both musl legs 404'd at the v8 download in the
  v0.1.0 dry-run, which is why the matrix was gnu from PR #280 until
  bd-dofxhzaj. That dependency was removed in bd-3e3sam51.
- **Because the binaries are static, the runner image no longer sets a
  compatibility floor.** The old `ubuntu-22.04` pin existed *only* to
  keep the glibc requirement low; with musl it bought nothing, so the
  linux legs track `ubuntu-latest`. Don't re-pin them without a reason
  that isn't glibc.
- **`musl-tools` is the only extra package the musl legs need.** In
  particular `aws-lc-sys` — long feared to be the hard part — is a
  non-issue at v0.40.0: it ships pregenerated bindings for both musl
  triples, so there is **no `bindgen` step and no `libclang`
  requirement**, and it compiled in ~17 s per leg in the bd-dofxhzaj
  spike (run 30375857883). If a future `aws-lc-sys` bump ever *does*
  start wanting cmake or libclang, that is a real regression worth
  pinning rather than papering over with extra apt packages.
- **`file` describes the two arches differently.** x86_64 comes out
  `static-pie linked`; aarch64 comes out plain `statically linked`. Any
  staticness check must accept **both** spellings — matching one passes
  on one arch and fails on the other. (`ldd` says *not a dynamic
  executable* on both, but exits non-zero, so it cannot be used as a
  bare assertion either.)
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
- **The version string's last token is the bare version.** Preflight
  and `install.sh` parse `${output##* }`. `q2 --version` prints
  `q2 (quarto 2) X.Y.Z`; anything appended to that string must keep the
  version last (guarded by `crates/quarto/tests/integration/version_cli.rs`).

## Files involved

| Path | Role |
|------|------|
| `.github/workflows/release.yml` | the workflow |
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
