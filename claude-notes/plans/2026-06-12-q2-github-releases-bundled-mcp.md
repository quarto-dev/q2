# GitHub release assets for q2, with bundled quarto-hub.com MCP defaults

**Strand:** bd-c6l13j79
**Date:** 2026-06-12 (rewritten same day after PR #277 / bd-81cfshmw landed — see "Scope changes" below)
**Reference implementation:** `external-sources/braid` (release.yml + install.sh + install.ps1 + minisign)
**Related:** bd-3tak0lyy (npx publish channel, deferred), bd-81cfshmw (q2 mcp launcher, shipped)

## Overview

Today, running the quarto-hub MCP requires building the monorepo from
source **plus** three operator-distributed env vars
(`QUARTO_HUB_MCP_CLIENT_ID`, `QUARTO_HUB_MCP_CLIENT_SECRET`,
`QUARTO_HUB_SERVER`). The goal is to reduce that to: **install q2 + node,
done** — for users of the canonical quarto-hub.com hub.

Two pieces of work:

1. **Binary distribution.** Adopt the cscheid/braid release mechanism
   (tag-triggered GitHub Actions workflow → per-platform archives → SHA-256
   checksums → minisign Ed25519 signatures → GitHub Release, consumed by
   `install.sh` / `install.ps1` with a pinned public key). Source:
   `external-sources/braid/.github/workflows/release.yml`,
   `external-sources/braid/install.sh`, `external-sources/braid/install.ps1`.

2. **Bundled OAuth defaults.** The release CI embeds the quarto-hub.com
   Google OAuth Desktop-app client credentials and server URL into the
   released q2 binary as *defaults*, injected by the `q2 mcp` launcher
   into the node child's environment. Env vars keep working and always
   win, so private hub operators are unaffected; source builds without
   the CI values behave exactly like today.

## Scope changes after PR #277 (bd-81cfshmw)

The first draft of this plan assumed `q2 mcp` didn't exist and proposed
building it. PR #277 landed the whole launcher the same day, with a
*different* (better-tested) architecture than the draft proposed. What
that removes from this plan:

- ~~`q2 mcp` subcommand~~ — shipped: `crates/quarto/src/commands/mcp.rs`
  → `crates/quarto-mcp-launcher`. Thin launcher; TS server stays the
  canonical implementation. Design doc:
  `claude-notes/plans/2026-06-11-q2-mcp-hub-auth.md`.
- ~~"ship `mcp/` dir inside the release archive"~~ — the bundle is
  **embedded in the q2 binary** (`include_dir!` of
  `ts-packages/quarto-hub-mcp/dist-bundle/`, built by
  `cargo xtask build-hub-mcp-bundle`) and extracted at runtime to a
  per-user cache with locking + GC. **Release archives therefore contain
  only the q2 binary**, and install.sh stays as simple as braid's.
- ~~esbuild/keyring spike~~ — solved: `scripts/bundle.mjs` bundles to a
  single `index.mjs` (automerge steered to its base64-wasm entrypoint),
  with `@napi-rs/keyring` external as a mini `node_modules` carrying the
  platform `.node` package(s).
- ~~node discovery/version policy spike~~ — solved: `node.rs`
  (`MIN_NODE_MAJOR`, `QUARTO_NODE` override), bundle targets node24.
- ~~placeholder/no-bundle error story~~ — solved: build.rs embeds a
  `BUNDLE_NOT_BUILT` placeholder when dist-bundle/ is absent; `q2 mcp`
  fails with an actionable message; `q2 mcp --launcher-info` reports
  bundle hash + build-info (the stale-embed tripwire).

What remains is exactly: the release pipeline, the bundled-defaults
injection, the installers, and end-to-end verification.

### Why embedding the "secret" is OK (settled in session 2026-06-12)

The Google OAuth client is type **Desktop app**. Per RFC 8252 and Google's
own docs, its `client_secret` is a public client credential — it cannot
authenticate anything, and every major CLI (gcloud, gh, rclone) ships its
equivalent inside the artifact. Security comes from user consent + PKCE +
loopback redirect, not from the secret. We inject the values at CI time
(GitHub Actions secrets) rather than committing them **purely** to avoid
Google/GitHub automated `GOCSPX-` scanners flagging the public repo and
auto-revoking the client — an operational concern, not a security one.

## Architecture decisions

### D1. Defaults live in the Rust launcher, injected into the node child env

`option_env!("QUARTO_HUB_BUNDLED_CLIENT_ID")` (+ `_CLIENT_SECRET`,
`_SERVER`) in `quarto-mcp-launcher`. `delegate.rs` builds the child
`Command` — add `cmd.env(var, default)` there for each variable the user
has **not** set to a non-empty value (match `readNonEmpty` semantics in
`oauth-config.ts`: empty/whitespace counts as unset). Rationale:

- Zero TS changes: `oauth-config.ts` and `index.ts` keep reading env
  vars; their error messages stay the single source of truth.
- One injection point (release CI → rustc env), no generated JS config.
- `option_env!` means fresh-clone `cargo build` needs no secrets and no
  placeholders; PR CI never sees the values; only the release workflow
  sets them.
- Resolution order per variable: user env (set & non-empty) →
  compiled-in default (release builds) → unset (source builds; the TS
  server errors with its existing message).

`q2 mcp --launcher-info` additionally reports, per variable:
`env` / `bundled` / `absent` (value elided for the secret). This is both
the user-debugging surface and the release-workflow assertion hook.

Note `--launcher-info` handling in `lib.rs::run` returns before
delegation, so the injection code paths are launcher-internal and
testable without node.

### D2. Release archive layout: binary only

The MCP bundle is inside the binary (see scope changes), so archives are
braid-shaped: `q2-<version>-<platform>.tar.gz` containing the single `q2`
member (`.zip` with `q2.exe` on Windows). install.sh needs only
s/braid/q2/ + the new pinned pubkey + repo coordinates.

**Critical workflow ordering:** the release matrix must run
`npm ci` (repo root) + `cargo xtask build-hub-mcp-bundle` **before**
`cargo build`, or the binary ships the `BUNDLE_NOT_BUILT` placeholder.
Assert per-platform in the workflow: `q2 mcp --launcher-info` must NOT
report `PLACEHOLDER`, must report a bundle-hash, and must report all
three defaults as `bundled` (in the release workflow) — this is the
2026-05-20 stale-embed incident class, caught at release time.

### D3. Cross-target keyring wrinkle (NEW — replaces the old S2 spike)

`scripts/bundle.mjs` copies the `@napi-rs/keyring` platform package(s)
found in the **build host's** `node_modules`. The `.node` addon is loaded
at runtime by the *user's* node, so the embedded mini node_modules must
match the **target** platform of the q2 binary, not the runner's. Native
runners (linux_amd64, linux_arm64, darwin_arm64, windows_amd64) are fine.
**darwin_amd64 built on an arm64 macos-15 runner (braid's matrix) is the
cross case**: npm installs keyring-darwin-arm64, the x86_64 user needs
keyring-darwin-x64.

**Spike S1:** pick one of
(a) supplemental `npm install --cpu=x64 --os=darwin @napi-rs/keyring-darwin-x64`
    (npm supports cross-platform optional-dep fetch via `--cpu`/`--os`),
(b) have bundle.mjs accept a target-platform argument and verify the
    right platform package is present (fail closed — extend its existing
    `copied` sanity check),
(c) ship both darwin keyring packages in the darwin bundles.
Whichever lands, add a workflow assertion that the embedded bundle's
`build-info.json` `keyringPackages` includes the target platform.

### D4. Linux build target: spike musl, fall back to gnu

braid ships static musl. q2's dep tree is much larger (tokio, axum,
tree-sitter C, …). **Spike S2:** try `x86_64-unknown-linux-musl` for
`-p quarto`; if it fights back, ship gnu built on the oldest supported
ubuntu runner (glibc floor) and note the floor in release notes. Timebox:
one day.

### D5. Signing: new minisign keypair, dedicated to quarto-dev/q2

Do **not** reuse the braid key — different project, different blast
radius. Passwordless keypair (`minisign -GW`) because CI can't answer
prompts; secret half lives only in the `MINISIGN_SECRET_KEY` repo secret
(+ password-manager copy). Public half pinned in `install.sh`, README,
release notes. Keep braid's release-time self-check (sign, then verify
against the pubkey extracted from install.sh, so a mismatch fails the
release) and replay protection (trusted comment = archive filename,
compared by install.sh).

### D6. Versioning / tagging

Workspace `Cargo.toml` version (currently `0.1.0`) is the source of
truth; tag `vX.Y.Z` must match (braid's preflight job, ported). First
release `v0.1.0`, marked pre-release.

### D7. Out of scope

- npm publish of the bundle (bd-3tak0lyy, deferred on public-release
  readiness).
- Homebrew/Scoop/WinGet manifests.
- Hub-brokered auth (hub as OAuth AS, RFC 7591 DCR) — the long-term
  replacement for bundled credentials; see
  `claude-notes/plans/2026-05-28-hub-mcp-loopback-pkce.md` future work.
- macOS notarization / Windows Authenticode. install.sh users are fine;
  double-click users are not a target yet.

## Operator setup (Carlos's side — blocks Phase 3 dry-run only)

1. Generate the release signing keypair (one time, trusted machine):
   ```sh
   minisign -GW -p q2-release.pub -s q2-release.key
   ```
   Store both halves in the team password manager. `-W` (no password) is
   required for non-interactive CI signing.
2. Upload to quarto-dev/q2 (requires repo admin):
   ```sh
   gh secret set MINISIGN_SECRET_KEY --repo quarto-dev/q2 < q2-release.key
   gh secret set QUARTO_HUB_MCP_CLIENT_ID --repo quarto-dev/q2
   gh secret set QUARTO_HUB_MCP_CLIENT_SECRET --repo quarto-dev/q2
   gh variable set QUARTO_HUB_SERVER --repo quarto-dev/q2 --body "wss://quarto-hub.com/ws"
   ```
   The OAuth values go in *secrets* not because they're confidential
   (see above) but to keep them masked in logs and away from scanners.
   The server URL is a plain Actions *variable*.
3. Delete the local `q2-release.key` once stored.
4. Confirm the bundled client id is in the hub's
   `--additional-audiences` (it already is — same client id as today's
   env-var distribution).

## Phases & work items

### Phase 0 — Spikes (timeboxed)

- [x] S1: darwin_amd64 keyring cross-target strategy (D3); record the
      chosen mechanism + exact commands here
- [x] S2: musl build spike for `-p quarto` (D4); record outcome here
- [x] Operator setup complete (secrets + variable + keypair)

**S1 outcome (2026-06-12).** Mechanism (b) from D3, generalized: extend
`scripts/bundle.mjs` with an optional `KEYRING_PLATFORMS` list (e.g.
`darwin-arm64,darwin-x64`). For each requested platform: copy the
locally installed `@napi-rs/keyring-<plat>` if present (today's
behavior, keeps dev offline); otherwise fetch the **exact version of
the installed loader package** via `npm pack @napi-rs/keyring-<plat>@<ver>`
and extract the tarball's `package/` into the bundle's mini
node_modules. Fail closed if a requested platform can't be staged.
Verified on this arm64 mac: `npm pack @napi-rs/keyring-darwin-x64@1.3.0`
→ 254 KB tgz containing `keyring.darwin-x64.node` (Mach-O x86_64), no
platform checks interfere. The loader package does full runtime
platform/arch/musl detection, so co-staged platform packages coexist by
design (`bundle.mjs` even documents multi-platform staging already).
Platform packages are ~250–500 KB each (12 exist for v1.3.0).
Per-target staging lists (also covers the Rosetta/libc mismatch cases —
the addon must match the **user's node**, not the q2 binary):
  - linux_amd64 → `linux-x64-gnu,linux-x64-musl`
  - linux_arm64 → `linux-arm64-gnu,linux-arm64-musl`
  - darwin_amd64 / darwin_arm64 → `darwin-x64,darwin-arm64` (identical)
  - windows_amd64 → `win32-x64-msvc,win32-arm64-msvc`
`build-info.json` already records `keyringPackages` → workflow asserts
the expected list per target.

**S2 outcome (2026-06-12).** Static analysis of
`cargo tree --target x86_64-unknown-linux-musl -p quarto`:
- `openssl-sys` enters via samod (quarto-dev fork) → tokio-tungstenite
  `native-tls`. Fix: `[target.'cfg(target_env = "musl")']`
  `openssl-sys = { features = ["vendored"] }` in `crates/quarto`
  (static openssl built from source; needs perl+make, present on
  ubuntu runners).
- Second risk: `aws-lc-sys` (rustls crypto provider) — compiles C via
  cmake; musl support exists but is historically finicky.
- Everything else C-flavored (mlua-sys, tree-sitter, ring) is
  musl-clean.
Decision: release matrix attempts **musl + vendored openssl** first;
the workflow keeps the linux target as a matrix variable so falling
back to gnu (oldest-ubuntu glibc floor, still vendored openssl to
avoid a runtime libssl dep) is a one-line change. Final verdict at the
Phase 4 dry-run — cross-compiling musl from this mac is not worth the
toolchain setup. Longer-term sound alternative if both bite: move
samod's tungstenite feature to rustls (we own the fork) — file a
strand if needed.

**Operator setup verified (2026-06-12):** `gh secret list` shows
MINISIGN_SECRET_KEY, QUARTO_HUB_MCP_CLIENT_ID,
QUARTO_HUB_MCP_CLIENT_SECRET; `gh variable list` shows
QUARTO_HUB_SERVER=wss://quarto-hub.com/ws. Minisign pubkey (pinned in
install.sh, key ID 91F595A50BD20376):
`RWR2A9ILpZX1kVF3Q6uk5TRus8FDM25H2F+KKKHEuqlxv+JJSLyPalvN`.
`q2-release.{key,pub}` were found sitting in the repo root —
**gitignored now**; Carlos to move the secret half to the password
manager and delete locally.

### Phase 1 — Bundled defaults in the launcher (TDD)

- [x] Tests first: env-merge tests — user-env wins; empty/whitespace env
      treated as unset (mirror `readNonEmpty`); absent bundled → variable
      not injected; values never printed (sources only). Pure-logic tests
      live in `src/defaults.rs` `#[cfg(test)]` (the crate's precedent —
      `bundle.rs`); the un-exec-able delegation path is covered via a
      `build_command` seam with `get_envs()` assertions in `delegate.rs`.
- [x] `option_env!` plumbing for `QUARTO_HUB_BUNDLED_{CLIENT_ID,CLIENT_SECRET,SERVER}`
      (`src/defaults.rs`) + injection via `delegate::build_command`
      (single point ahead of the Unix-exec/Windows-spawn split)
- [x] `--launcher-info` reports `default <VAR>: env|bundled|absent`
      (also for placeholder builds — defaults are a property of the
      binary, not the bundle; values uniformly elided)
- [x] End-to-end check vs a local build with TEST values baked
      (real secret never touches a local build)

**Phase 1 end-to-end record (2026-06-12).** Built with
`QUARTO_HUB_BUNDLED_CLIENT_ID=test-client-id.apps.googleusercontent.com
QUARTO_HUB_BUNDLED_CLIENT_SECRET=TEST-not-a-real-secret
QUARTO_HUB_BUNDLED_SERVER=wss://example.invalid/ws cargo build --bin q2`.
Observed (output inspected):
- shell with Carlos's real exports → `default …: env` ×3 (user wins);
- `env -u …` all three → `default …: bundled` ×3;
- `QUARTO_HUB_MCP_CLIENT_ID=""` → still `bundled` (blank = unset);
- fake `QUARTO_NODE` child dump, no user env →
  `child CLIENT_ID=test-client-id.apps.googleusercontent.com`,
  `child CLIENT_SECRET=TEST-not-a-real-secret`,
  `child SERVER=wss://example.invalid/ws`;
- same + `QUARTO_HUB_SERVER=ws://localhost:3030/ws` → child sees the
  user server, bundled id/secret (per-variable independence);
- rebuild **without** the env vars → `default …: absent` ×3 and empty
  child env (fresh-clone path; also proves cargo env-dep tracking
  rebuilds on `option_env!` changes in both directions — no build.rs
  plumbing needed).
35/35 tests pass in `quarto-mcp-launcher` (9 new defaults + 2 new
delegate seam tests). Note: a *real* `q2 mcp` connect against
quarto-hub.com with release-injected values is deliberately deferred to
Phase 4 (that's the artifact-level check; locally it would exercise the
same code path with the same mechanism).

### Phase 2 — Installer scripts + offline tests

- [x] Adapt `install.sh` from braid (OWNER/REPO/BINARY_NAME=q2, new
      pinned pubkey); keep `--print-platform`, refusal paths, atomic
      install, trusted-comment check
- [x] Adapt `install.ps1` (checksum-only, matching braid's Windows story
      — note the signature gap)
- [x] Port braid's `bootstrap_sh.rs` offline test (`--artifact-url
      file://` + `--checksum`) into a workspace integration test
- [x] README install section + manual-verification instructions

**Phase 2 record (2026-06-12).** Decisions made during the port:
- Archive layout is **flat** (binary only) since the MCP bundle is
  embedded in the binary — no `~/.local/share` payload dir needed;
  install dir defaults to `~/.local/bin`, exactly like braid.
- Env vars renamed: `Q2_REPO_OWNER`/`Q2_REPO_NAME`/`Q2_MINISIGN`/
  `Q2_INSTALL_DIR`.
- Unsupported-platform advice says `--from-source` (braid said `cargo
  install --git`, which is untested for this workspace's bin layout).
- `--from-source` builds the MCP bundle when npm is available
  (`npm install && npm run bundle -w ts-packages/quarto-hub-mcp`),
  loudly warns + proceeds without it otherwise (`q2 mcp`
  non-functional, everything else works).
- Test home: `crates/quarto/tests/integration/bootstrap_sh.rs` (32
  tests; `pub mod bootstrap_sh;` registered alphabetized in main.rs;
  `sha2 = "0.11"` added to quarto dev-deps — 0.11 dropped LowerHex on
  digests, hence a local `sha256_hex` helper). All 32 pass locally;
  shellcheck-clean verified (shellcheck installed here).
- `test-suite.yml` gains minisign install steps (apt/brew) — the suite
  requires it loudly rather than skipping (braid's contract, kept).
- README gains an Installing section (pinned pubkey, manual
  verification, node 24+ note for `q2 mcp`, env-var override story).

### Phase 3 — Release workflow

- [x] `.github/workflows/release.yml` adapted from braid: preflight
      tag/version check; matrix (linux_amd64, linux_arm64, darwin_amd64,
      darwin_arm64, windows_amd64); Defender exclusion on Windows;
      archive + sha256; minisign (trusted comment = filename) +
      self-verify vs install.sh pin; combined checksums.sha256;
      `gh release create` (actionlint-clean)
- [x] Bundle-before-build ordering per D2 — **expanded**: a functional
      q2 embeds THREE payloads (MCP bundle, preview SPA, trace viewer).
      New `web-payloads` job builds the target-independent ones once
      (WASM toolchain mirrors hub-client-e2e.yml: nightly + rust-src +
      clang + lockfile-pinned wasm-bindgen-cli) and the matrix downloads
      them; the MCP bundle builds per target with `KEYRING_PLATFORMS`
- [x] Bundled-defaults injection from secrets/vars in the release
      workflow only, with a fail-fast guard if any value is empty
- [x] Per-platform workflow assertions: binary `--version` equals the
      tag version; `q2 mcp --launcher-info` shows non-placeholder
      bundle, `default …: bundled` ×3, and a keyring addon per entry in
      the target's KEYRING_PLATFORMS list
- [x] Release-notes generation (braid's table, experimental banner,
      node-24 note for `q2 mcp`, pubkey extracted from install.sh)
- [x] musl TLS: `[target.'cfg(target_env = "musl")']`
      `openssl-sys = { features = ["vendored"] }` in crates/quarto
      (Cargo.lock updated — release builds are `--locked`)

**Phase 3 decision record (2026-06-12).**
- **CLI version contract changed** (Carlos, in-session): `q2 --version`
  now reports the real workspace version (`quarto 0.1.0`) instead of
  the `99.9.9-dev` extension-compatibility placeholder — release
  artifacts must be verifiable against their tag, and Lua-side
  `quarto.version` already reported {0,1,0}. TDD'd in
  `quarto-util/src/version.rs` (red first). Consequence: extension
  minimum-quarto-version checks will see 0.1.0; accepted for now.
- The `99.9.9` strings in `error_catalog.json` / `docs/errors/` are
  `since_version` markers — a separate concern, deliberately untouched.

### Phase 4 — End-to-end verification (per CLAUDE.md, before declaring done)

- [ ] Dry-run the workflow (workflow_dispatch on a test tag or fork);
      download artifacts for all five platforms

**Phase 4 log.** PR #278 (all 5 CI checks green) squash-merged to main
by Carlos as `31222946` on 2026-06-12. Tag `v0.1.0` pushed at that
commit; release run:
https://github.com/quarto-dev/q2/actions/runs/27448388974

*Iteration 1 (run 27448388974):* preflight ✓, web-payloads ✓ (WASM +
SPA + trace viewer built cleanly on the release runner). Four matrix
legs failed, two distinct causes:
1. linux_amd64 / linux_arm64 / darwin_amd64 — `E0463 can't find crate
   for core/std`: the dtolnay action adds the matrix target to the
   *latest* nightly, but cargo resolves the dated nightly pinned in
   `rust-toolchain.toml` (bd-at72), which auto-installs with only its
   declared targets (wasm32). web-payloads only survived because wasm32
   is in the pin's own `targets`. Fix: explicit `rustup target add` step.
2. windows_amd64 — `spawnSync npm ENOENT` in stage-keyring's
   npmPackFetcher fetching keyring-win32-arm64-msvc: npm is `npm.cmd`
   on Windows; execFileSync needs `shell: true` (+ Node ≥20.12 rejects
   .cmd without it). Fix in npmPackFetcher.
darwin_arm64 (only leg whose target is host-default) ran the furthest —
its outcome validates the downstream pipeline (defaults injection,
verify gate, packaging, signing).
- [ ] `install.sh` one-liner on a clean machine/container → `q2 --version`
- [ ] `q2 mcp` with **no env vars set** connects to quarto-hub.com:
      browser consent → token → `connect_project` + `read_file` against a
      real project from an MCP client config with no `env` block
- [ ] Env-override check: `QUARTO_HUB_SERVER=ws://localhost:…` against a
      local hub still works (private-operator path unbroken)
- [ ] darwin_amd64 artifact specifically: keyring loads on an x86_64 mac
      (or under Rosetta `arch -x86_64 node`) — the D3 cross case
- [ ] Record invocations + observed output in this plan

## Risks / open questions

- **D3 cross-target keyring** is the main new risk; fail-closed assertion
  in the workflow regardless of chosen mechanism.
- **musl** (S2) may be a non-starter for q2's dep tree; gnu fallback in D4.
- **Windows signature gap**: install.ps1 verifies checksums only
  (matches braid). Acceptable for v1; note it.
- **Key compromise story**: anyone with quarto-dev/q2 admin can read the
  passwordless signing key. Same trust model as braid; document rotation
  (new keypair → update install.sh pin → re-sign current release).
- **Tag-push releases use repo secrets** — fine (fork PRs never see
  secrets; tag pushes are maintainer-only).
- **Release builds embed whatever dist-bundle/ exists** — the workflow
  builds it fresh in the same job, and the launcher-info assertion guards
  the ordering; local/dev builds remain subject to the stale-embed
  caveat documented in CLAUDE.md.
