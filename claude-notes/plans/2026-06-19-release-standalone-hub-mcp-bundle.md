---
title: 'Ship the standalone `quarto-hub-mcp` bundle as a GH release artifact'
date: 2026-06-19
description: 'Attaches a universal, checksummed and signed Node bundle of the Hub MCP server to each GitHub release as a temporary stopgap until an npm distribution channel exists, reusing the existing bundle build.'
---

**Strand:** `bd-sca6g1tu`
**Status:** planned (awaiting go-ahead to implement)
**Date:** 2026-06-19

> ## ⚠️ TEMPORARY / STOPGAP
>
> **This is a deliberate stopgap, not the intended long-term distribution.**
> The clean way to let people use the Hub MCP server directly is to publish
> `@quarto/hub-mcp` to npm and have them run `npx @quarto/hub-mcp` — tracked
> by **`bd-3tak0lyy`** (Publish quarto-hub-mcp bundle to npm). We are *not*
> doing that yet because we don't want to set up npm publish credentials at
> this time.
>
> Until then, we attach the already-built esbuild bundle to each GitHub
> Release as a downloadable tarball. **Expected lifetime: a couple of
> months** (decision 2026-06-19). When the npm/npx channel lands, revisit
> whether this artifact is still worth shipping or should be removed to avoid
> two parallel distribution paths. Leave `bd-3tak0lyy` linked so the
> follow-up is discoverable.

## Overview

The Quarto Hub MCP server is a standalone TypeScript package
(`ts-packages/quarto-hub-mcp`, `@quarto/hub-mcp`) that speaks MCP over stdio.
Today the only way to get it is bundled *inside* the `q2` binary (`q2 mcp`,
the thin Rust launcher in `crates/quarto-mcp-launcher`). A colleague wants to
use the server directly (e.g. embed it in a separate TypeScript app, or run it
standalone), which means we need a way to ship the bundle on its own.

Full background on how the bundle is built/embedded:
`claude-notes/research/2026-06-19-hub-mcp-bundle-handoff.md`.

## Why this is almost free

The release workflow (`.github/workflows/release.yml`) **already builds the
bundle on every release**. In the `build` matrix job (lines 278–282) each
target runs:

```yaml
- name: Build hub MCP bundle
  env:
    KEYRING_PLATFORMS: ${{ matrix.keyring }}
  run: npm run bundle -w ts-packages/quarto-hub-mcp
```

The output `dist-bundle/` is currently embedded into `q2` via `include_dir!`
and then discarded — **never packaged or uploaded**. The hard parts
(per-platform `@napi-rs/keyring` `.node` staging, `npm pack` fallback for
non-host addons, fail-closed verification) are done and exercised every
release. All that's missing is: build one universal bundle, tar it, checksum
it, sign it, attach it.

## Chosen approach: Option B — one universal bundle

`index.mjs` is **byte-identical across platforms** (esbuild output is
platform-independent); only `node_modules/@napi-rs/keyring*` differs per
platform. The keyring loader does runtime platform/arch/libc detection, and
`scripts/stage-keyring.mjs` explicitly supports co-staging every platform
("co-staged platforms coexist by design"). So a *single* bundle that carries
**all** keyring addons runs everywhere with one download.

This is built by a dedicated job (modeled on the existing `web-payloads` job)
on `ubuntu-latest`, with `KEYRING_PLATFORMS` set to the full cross-platform
list. Non-host addon packages are fetched via `npm pack` at the loader's
locked version — a path the release already relies on (the macOS leg fetches
musl addons today).

Rejected alternative — **Option A (5 per-platform tarballs)**: mirrors the
binary layout and reuses each matrix leg's `dist-bundle/` for free, but forces
the user to pick the right platform file for what is otherwise a
platform-independent JS artifact. Not worth the worse UX.

## Work items

- [x] **New `release.yml` job** (`hub-mcp-bundle`): `ubuntu-latest`,
      Node 24, `npm ci`, then `npm run bundle -w ts-packages/quarto-hub-mcp`
      with the full keyring list:
      `darwin-x64,darwin-arm64,linux-x64-gnu,linux-x64-musl,linux-arm64-gnu,linux-arm64-musl,win32-x64-msvc,win32-arm64-msvc`.
      Includes a fail-closed `Verify bundle` step (real bundle, all addons
      staged, `node index.mjs --help` loads).
- [x] **Package the tarball**: `quarto-hub-mcp-${VERSION}.tar.gz` extracting
      into a self-describing `quarto-hub-mcp-<version>/` dir with `index.mjs`,
      `build-info.json`, and `node_modules/@napi-rs/*`. Writes `.sha256`,
      `upload-artifact`s as `hub-mcp-bundle`.
- [x] **README + NOTICE inside the tarball**: runner hint, the **Node 24+**
      requirement (raw bundle has no `MIN_NODE_MAJOR` guard), the
      no-embedded-OAuth-defaults caveat + the env vars to set, and MIT
      attribution for the inlined deps (`@modelcontextprotocol/sdk`, `jose`,
      `oauth4webapi`, `@automerge/automerge`, `ws`, `@napi-rs/keyring`).
- [x] **Extend the `release` job**: added `hub-mcp-bundle` to `needs`; a
      second `download-artifact` (its own name, so the `q2-*`
      platform-completeness check ignores it); added `quarto-hub-mcp-*.tar.gz`
      to the minisign loop; its `.sha256` flows into `checksums.sha256`
      automatically via the existing `cat -- *.sha256`; added the tarball +
      `.sha256` + `.minisig` to `gh release create`.
- [x] **Release notes**: added a "Standalone Quarto Hub MCP server" section
      (Node 24 requirement, OAuth-env caveat, temporary-channel note).
- [x] **Runbook**: documented in
      `claude-notes/instructions/release-runbook.md` ("What a release
      produces" + "Files involved").
- \[\~\] **Dry-run e2e verification**: done *locally* — built the universal
      bundle (all 9 keyring platforms; 8 fetched via `npm pack`), ran the
      exact YAML-dedented packaging script, extracted the tarball fresh, and
      confirmed `node index.mjs --help` exits 0 (keyring addon resolves on
      darwin-arm64). YAML validated with `actionlint`. **Still pending:** a
      real dry-run *tag* exercising the GH-hosted runner + multi-OS download.

## Verification performed (local, 2026-06-19)

- `KEYRING_PLATFORMS=<all 8> npm run bundle` → staged all 9 keyring packages
  (host `keyring-darwin-arm64` copied; the other 8 fetched via `npm pack` at
  the loader's locked version — the same code path the per-target release
  already exercises).
- Extracted the bundler's exact `run:` script via the YAML parser (so it's
  byte-for-byte what GH executes) and ran it: README/NOTICE render correctly,
  `tar` + `sha256sum -c` pass.
- Fresh `tar -xzf` → `node index.mjs --help` exits 0.
- `actionlint .github/workflows/release.yml` → no findings; `yaml.safe_load`
  parses.
- Not yet exercised: an actual tagged dry-run release (GH runners, the
  download/sign/attach round-trip, cross-OS download). Recommend a
  `workflow_dispatch` dry-run on a throwaway pre-release tag before relying on
  it (see runbook §"If a leg fails").

## Non-goals

- **npm/npx publishing** — tracked by `bd-3tak0lyy`. This stopgap needs no npm
  org or publish secrets, which is the whole point of doing it now.
- **Per-platform bundles** (Option A) — rejected above.
- **A standalone launcher / Node version guard** — out of scope; the bundle's
  README states the Node 24 requirement, and that's sufficient for a stopgap.

## Exit criteria / removal trigger

When `bd-3tak0lyy` ships the npm/npx channel, reassess: either keep this
tarball as a no-npm-required fallback or remove the job to avoid maintaining
two distribution paths. Record that decision on the strand at the time.

## References

- Strand: `bd-sca6g1tu` (this work); related `bd-81cfshmw` (q2 mcp epic),
  `bd-3tak0lyy` (npm channel — the proper fix).
- Handoff guide: `claude-notes/research/2026-06-19-hub-mcp-bundle-handoff.md`.
- Bundler: `ts-packages/quarto-hub-mcp/scripts/bundle.mjs` +
  `scripts/stage-keyring.mjs`.
- Design doc: `claude-notes/plans/2026-06-11-q2-mcp-hub-auth.md` (bd-81cfshmw).
- Release workflow: `.github/workflows/release.yml`.
