# Native Windows ARM64 binaries in the nightly (and release) pipeline

**Date:** 2026-10-08
**Braid:** bd-windows-arm64-nightly-xms5p652 (feature, P2, labels release/ci/windows)
**Status:** Implementing. Phases 1–4 are written. The Phase 1 dry run and
Phase 5 are pending.
**Review log:** 2026-10-08 Carlos approved D1–D5 as proposed (D3: both
channels). He asked whether D4's detection could fail on Windows versions
too old to have planned for ARM64; the answer is under D4 ("Old Windows").

## Overview

A user asked for Windows ARM binaries in the q2 nightlies. Today every
release and nightly ships exactly one Windows artifact,
`q2-<version>-windows_amd64.zip` (`x86_64-pc-windows-msvc`). On ARM64
Windows that binary runs under Microsoft's x64 emulation (Prism). It works,
but it is slower, and it is the only one of our six OS/arch combinations
without a native build.

Goal: add a **`windows_arm64`** artifact
(`aarch64-pc-windows-msvc`, `q2-<version>-windows_arm64.zip`) built,
verified and published by the same pipeline, and make `install.ps1` pick it
automatically on ARM64 machines.

## How the Windows x64 build works today

The nightly is a thin caller around a shared reusable workflow. Nothing
about Windows is specific to the nightly; it all lives in the pipeline.

```
nightly.yml   gate ──▶ release-pipeline.yml ──▶ install-smoke ×3
release.yml   tag  ──▶ release-pipeline.yml
```

**`.github/workflows/nightly.yml`**
- `gate`: `scripts/nightly-gate.sh` decides build/skip and picks the
  version (`0.33.0-nightly.YYYYMMDD`) and sha.
- `pipeline`: calls `release-pipeline.yml` with `channel: nightly`,
  `tag: nightly`.
- `install-smoke`: matrix `[ubuntu-latest, macos-15, windows-latest]` runs
  the README one-liners (`install.ps1 -Nightly` on Windows) against the
  release that was just published. It asserts that `q2 --version` matches
  the gate's version.

**`.github/workflows/release-pipeline.yml`** (the parts that touch Windows)
1. `web-payloads` (ubuntu): builds the target-independent embeds once. These
   are the WASM, preview SPA, trace viewer and docs embed. Windows ARM
   needs nothing new here.
2. `hub-mcp-bundle` (ubuntu): the standalone MCP tarball. Its
   `KEYRING_PLATFORMS` **already includes `win32-arm64-msvc`**.
3. `build` matrix leg `windows_amd64`:
   - `os: windows-latest`, `target: x86_64-pc-windows-msvc`, `ext: zip`
   - `keyring: win32-x64-msvc,win32-arm64-msvc`. Both archs are included
     because an ARM64 user runs x64 q2 under emulation with a native arm64
     Node.
   - `cargo_flags: ''`, since TLS is schannel and there is no openssl.
   - Steps: rust nightly + `rustup target add` (onto the toolchain pinned
     in `rust-toolchain.toml`), rust-cache keyed per target, a Defender
     exclusion for the workspace, Node 24 + `npm ci`, download of the
     web-payloads, the per-target MCP bundle, then
     `cargo build --release --locked --target … -p quarto`.
   - The **verify** step (Git Bash) runs the binary itself: `--version`,
     `mcp --launcher-info` (bundle, keyring addons, bundled defaults),
     `docs llms --embed-info`. Then `preview --print-asset-manifest-hashes`.
     So **the binary must be able to execute on the build runner.**
   - Packaging: `Compress-Archive` → `.zip` plus a GNU-format `.sha256`.
4. `asset-manifest-check`: the platform list is hardcoded
   (`EXPECTED="… windows_amd64"`).
5. `release`: the completeness check is hardcoded
   (`… windows_amd64:zip`). Zips are **not** minisigned, because
   install.ps1 only checks SHA-256. The notes table is hardcoded and the
   upload globs (`q2-*.zip`) are generic.

**`install.ps1`**: `$Platform = 'windows_amd64'` is hardcoded (line 49).
A comment says ARM64 runs it under emulation. Asset selection for
`-Nightly`, stable and `-Version` all keys off `$Platform`.

**Other references to `windows_amd64`**:
`claude-notes/instructions/release-runbook.md` ("five platforms"),
`claude-notes/plans/2026-06-12-q2-github-releases-bundled-mcp.md`
(historical; leave it alone), and a comment in
`ts-packages/quarto-hub-mcp/scripts/stage-keyring.mjs` (historical; leave
it alone).

## Decisions (proposed, for review)

### D1. Build natively on `windows-11-arm`, not cross-compiled from x64

GitHub's hosted `windows-11-arm` runners are free for public repos. A
native build keeps the **verify step unchanged**: the binary runs on its
own runner, so the version check, placeholder embeds, keyring, docs embed
and asset-manifest gates apply as they do on every other leg.

Cross-compiling from `windows-latest` would avoid a new runner image, but
an x64 Windows host **cannot execute an ARM64 binary**. We would lose every
verify gate for this leg, or need a second job on an ARM runner anyway.
That is the same reasoning that put linux_arm64 on `ubuntu-24.04-arm`.

Risks to confirm in the spike (Phase 1):
- **Runner image contents.** We need Git Bash (the verify/build steps use
  `shell: bash`), pwsh, MSVC ARM64 build tools, and whatever the C deps
  need. The C deps are `aws-lc-sys` 0.45 (through `aws-lc-rs`/rustls),
  `ring`, tree-sitter and mlua's vendored Lua. `aws-lc-sys` on
  `aarch64-pc-windows-msvc` may want clang and/or cmake. This is the most
  likely snag.
- **Pinned nightly toolchain** `nightly-2026-04-28` must have an
  `aarch64-pc-windows-msvc` host build. It is a tier-2-with-host-tools
  target, so this should be fine, but we confirm it in the spike.
- **`dtolnay/rust-toolchain`, `actions/setup-node@v6` (Node 24 arm64),
  `Swatinem/rust-cache`** on Windows ARM. All are expected to work, and the
  spike confirms it.
- **Defender exclusion** (`Add-MpPreference`) works on the ARM image.
- **Build time.** These runners are smaller. The leg's 90-minute timeout
  may need raising.

### D2. Matrix leg shape

```yaml
- platform: windows_arm64
  target: aarch64-pc-windows-msvc
  os: windows-11-arm
  ext: zip
  keyring: win32-arm64-msvc,win32-x64-msvc
  cargo_flags: ''
```

The keyring list is the mirror of the amd64 leg, with both archs: a user
may run x64 Node under emulation on an ARM machine, just as the darwin legs
cover Rosetta. All existing steps are keyed on `runner.os == 'Windows'` or
`matrix.ext == 'zip'`, so they should apply as-is.

### D3. Ship it on both channels, but test it through a nightly dry run first

The pipeline is shared, and the completeness and asset-manifest checks are
channel-agnostic. The simplest correct change adds `windows_arm64` to
**both** channels. A nightly-only leg would need channel-conditional
platform lists in three places, for little benefit.

The rollout is gated by testing rather than by channel:
`workflow_dispatch` of Nightly from the branch with `force: true,
publish: false` runs the full pipeline as a dry run. Nothing is published
until the leg is green. The first real nightly after merge is the next
check, and the first release after that ships it to stable.

**Open question for Carlos:** should it be nightly-only for a while before
reaching stable releases? If so, I'd add a `channel == 'nightly'`
condition to the matrix `include` and make the two platform lists
channel-aware.

### D4. `install.ps1` detects the arch and falls back to amd64

- Detect the **OS** architecture, not the process architecture. An x64
  PowerShell running emulated on ARM reports `PROCESSOR_ARCHITECTURE=AMD64`.
  Use `[System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture`
  (available in Windows PowerShell 5.1 and pwsh). If that is unavailable,
  fall back to `$env:PROCESSOR_ARCHITEW6432` / `PROCESSOR_ARCHITECTURE`.
- `Arm64` → prefer `windows_arm64`. Anything else 64-bit →
  `windows_amd64`, as today.
- **Fallback.** Releases published before this change, and any `-Version`
  pin to an older tag, have no arm64 asset. On ARM64, if the arm64 asset
  is missing, use `windows_amd64` and print a step message ("no native
  ARM64 build for this release; installing x64 (runs under emulation)").
  - `-Nightly` and latest-release paths already have the release JSON, so
    choose by asset name.
  - The `-Version` path builds the URL directly. It needs a lookup by tag
    (one more API call), or a try-arm64-then-amd64 on download 404. I
    lean towards the release lookup, because it reuses `Invoke-GitHubApi`
    with its token and retry handling.
- Add a `-Platform <windows_amd64|windows_arm64>` override flag. This is
  useful for testing and for users who want x64 on purpose.

**As built.** Detection checks three sources, and any one of them
reporting ARM64 wins:

1. The machine-wide registry value
   `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment\PROCESSOR_ARCHITECTURE`.
   Windows seeds each process's environment variable from this value and
   overrides only the per-process copy under emulation.
2. `RuntimeInformation.OSArchitecture`.
3. `PROCESSOR_ARCHITEW6432` / `PROCESSOR_ARCHITECTURE`.

Each source is wrapped in try/catch. With no `-Platform`, ARM64 tries
`windows_arm64` first and then `windows_amd64`. The other paths work as
follows:
- Nightly: the API retry loop waits for the *preferred* asset, because a
  just-replaced release can list its assets only partially. It falls back
  only once the retries run out, which costs about 30 s, and only on the
  day before the first nightly that ships ARM64.
- Latest release: picks the platform from the release's assets.
- `-Version` on ARM64: one extra `releases/tags/<tag>` lookup.
- x64 behaves as before, with no extra API calls.

I tested this offline with portable pwsh 7.6 on macOS. RuntimeInformation
reports Arm64 there, so the ARM64 paths ran for real, against mocked
`Invoke-RestMethod`. Cases covered: native nightly, forced x64, fallback
on latest, fallback on `-Version`, native on `-Version` with no `v`,
fallback on an old nightly after the wait, forced arm64 against a release
without it (clear error), and an invalid `-Platform`.

**Old Windows (Carlos's question).** This is safe: no Windows version that
could run on ARM64 is too old for the detection.
- **ARM64 Windows first shipped in Windows 10 1709** (late 2017). That
  release ships .NET Framework 4.7.1, the first version with
  `RuntimeInformation`, and the registry value and environment variables
  are as old as NT.
- **Older Windows** (7, 8, 8.1, and 10 before 1709) only ran on x86/x64.
  On a 64-bit x86 machine every source either answers "AMD64" or throws
  inside its try/catch. Either way the result is `windows_amd64`, which
  is exactly today's behavior.
- **Windows RT** (8.x on 32-bit ARM) is rejected by the existing
  `Is64BitOperatingSystem` check. It could not run q2 anyway.
- The q2 binary itself already needs Windows 10+, because Rust's MSVC
  targets need it. Nothing in the installer narrows that further.

### D5. Smoke-test the real ARM install path every night

Add `windows-11-arm` to `nightly.yml`'s `install-smoke` matrix. (As built:
the Windows smoke tests run the installer twice, under PowerShell 7 and
under Windows PowerShell 5.1. Detection relies on different .NET runtimes
in each, and 5.1 is what most `irm | iex` users get.) Besides the
version assertion, assert that the installed `q2.exe` is **native ARM64**.
Read the PE header machine field: `0xAA64` is ARM64 and `0x8664` is x64. A
few lines of PowerShell do this. Without it, the fallback path would hide a
missing or misnamed arm64 asset, and the test would still pass.

## Phases

### Phase 1: Spike the build leg (branch, dry run)
- [x] Add the D2 matrix leg.
- [x] Add `windows_arm64` to the `asset-manifest-check` `EXPECTED` list and
      to the `release` job's completeness list (`windows_arm64:zip`).
- [ ] Dispatch Nightly from the branch with `force: true`,
      `publish: false`. Iterate on runner/toolchain/C-dep problems (D1
      risks), and record each fix with its run id in this plan.
- [ ] Record the leg's wall time and raise `timeout-minutes` if needed.
- [ ] Confirm the dry-run `release-set` artifact contains
      `q2-…-windows_arm64.zip` + `.sha256`. Confirm `checksums.sha256`
      covers it.

### Phase 2: Release notes + docs
- [x] Add a `Windows ARM64` row to the platform table in "Generate release
      notes".
- [x] Update the comments in the release-pipeline header (target count,
      "5-target binary build") and the `windows_amd64` keyring comment.
- [x] Update `release-runbook.md` ("five platforms" → six, and the
      platform list).

### Phase 3: `install.ps1` (D4)
- [x] Arch detection, `-Platform` override, arm64 → amd64 fallback on all
      three resolution paths (nightly, latest, `-Version`).
- [x] Update the header comment (it says "x86_64 Windows") and the
      emulation comment.
- [x] Check whether `crates/quarto/tests/integration/bootstrap_sh.rs`
      encodes anything about install.ps1 beyond the header note on line 5.
      Extend it if it encodes the naming contract. (It is unix-only and
      only mentions install.ps1 in a comment, so nothing needed to
      change.)
- [ ] ~~Manual test on Windows runners~~. Replaced by the offline mocked
      pwsh tests above plus the post-merge install smoke (Phase 5). The
      smoke only runs against a *published* nightly, so a branch dry run
      cannot exercise it.

### Phase 4: Install smoke (D5)
- [x] Add `windows-11-arm` to the `install-smoke` matrix, with a PE-machine
      assertion (ARM64 on the arm runner, x64 on `windows-latest`).

### Phase 5: Land and observe
- [ ] PR → merge. The first scheduled nightly publishes `windows_arm64`,
      and the ARM install smoke goes green. Record the run id here.
- [ ] Reply to the requester with the `-Nightly` one-liner.
- [ ] The next stable release ships it (unless D3's open question goes the
      other way).

## Out of scope

- Running the test suite on Windows ARM. Windows isn't in test CI at all
  today (see `.claude/rules/cross-platform.md`), so that is a separate
  decision.
- minisign for Windows zips. This is unchanged: install.ps1 is
  checksum-only on both archs.
- A `--from-source` path for Windows.
