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
- **Linux ships gnu today** (`x86_64/aarch64-unknown-linux-gnu` on
  **ubuntu-22.04** runners, glibc 2.35 floor, `--features
  vendored-openssl` so the binary has no runtime `libssl` dependency;
  Alpine users need `gcompat`). Static musl was originally *blocked* by
  `rusty_v8` (via `deno_core` → `quarto-system-runtime`), which shipped
  no musl prebuilts — both musl legs 404'd at the v8 download in the
  v0.1.0 dry-run. **That dependency has since been removed
  (bd-3e3sam51), so musl is now viable** — the only remaining
  consideration is openssl/aws-lc, both vendorable/musl-buildable.
  Switching the matrix to musl (one artifact per arch, Alpine included)
  is unblocked future work; until someone does it, linux is gnu.
- **Signing happens in the `release` job, not the build matrix.**
  ubuntu-22.04 (jammy) has no `minisign` apt package; ubuntu-latest
  (the release runner) does. Centralizing also means the secret key is
  touched by one job and signs the exact published bytes.
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
| `ts-packages/quarto-hub-mcp/scripts/stage-keyring.mjs` | per-target keyring staging |
| `crates/quarto-util/src/version.rs` | version-string policy |

## Related

- `claude-notes/plans/2026-06-12-q2-github-releases-bundled-mcp.md` — the
  design + full dry-run log (bd-c6l13j79).
- `claude-notes/instructions/hub-mcp-operator-runbook.md` — running a
  *private* hub (the env-var path that bundled defaults replace for the
  canonical hub).
