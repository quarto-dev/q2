# Plan: Delivery pipeline and the embedded hub (pandoc-host H4)

**Date:** 2026-10-01
**Epic:** [`2026-10-01-pandoc-host-epic.md`](2026-10-01-pandoc-host-epic.md)
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md) (D6, D7)
**Depends on:** H1 (the download script); H4b is startable once H1 exists. **Unblocks:** H5.
**Conventions:** the design's Execution conventions (gates, task preamble, one agent per phase, handoff, parallel lanes, STOP checkpoints) apply to every task here. One agent executes this phase and spawns no sub-agents or forks.
**Lane:** H (workspace-7). **Start gate:** H1 is fully ticked. If it is not met, change nothing, report which gate is open and stop. **Next in lane:** H3 (its Start gate decides whether it can begin).

## Overview

Ship the asset the way the hub ships assets, keep it out of the embedded hub, and give the embedded hub a way to get its downloads from the native pandoc that `q2 preview` already has.

## Decisions

- `pandoc.wasm.gz` lives under `hub-client/public/pandoc/` (outside `assets/`; a Vite middleware is the rejected alternative).
- The embed carries no wasm (D7); how it asks the native side to render is chosen here, picking the least invasive mechanism.

## Checklist

### Tasks
- [ ] **Asset pipeline.** `public/` is copied into `dist` and every E2E build, so the feature degrades when the file is absent (menu hidden, build warning). The asset must also be excluded from `dist-preview-embed` (a post-build removal in `build:preview-embed`, or a flag), because that directory is `include_dir!`-ed into the `q2` binary and the embed carries no wasm (likewise `public/typst/` and `public/pdfjs/` once H7 and H9 add them). `gz` is already in `scripts/gzip-skip-extensions.txt`; check that the Vite compress filter (`vite.config.ts:101-107`) leaves `application/gzip` alone. A **feature flag** hides the menu if the Lua gate or a release check fails. Documented in `dev-docs/`. Production is built by the separate `quarto-hub-deployment` repo (`build-and-deploy.yml` checks out q2 and runs `cd hub-client && npm run build:all`, then publishes `dist/` to S3 behind nginx), not by a workflow here (`release-pipeline.yml` only runs `build:wasm` in `hub-client`). So the download script (`scripts/fetch-pandoc-wasm.mjs`, landed in H1; H1 wired only vitest, `cargo xtask verify` and `ts-test-suite.yml`) runs inside the `build:all` chain and the entry points that bypass it (`test:e2e`, `test:harness`, `scripts/build-local-prod.sh`), in `--require` mode, which fails the build when the asset cannot be fetched or verified. The deployment repo's serving side is the next task. The E2E workflow's download step is H2's.
- [ ] **PR to `quarto-hub-deployment`** (a separate repo, `config/quarto-hub.nginx` and `.github/workflows/build-and-deploy.yml`; the plan's implementer opens it, and it can merge before or after the q2 change but must be live before the feature flag is enabled in production). Today nginx's catch-all `try_files $uri $uri/ /index.html` answers a missing `/pandoc/…` with status 200 and HTML, and `/` sends `Cache-Control: no-cache`. The PR adds a `location /pandoc/` (with its own `root /var/www/quarto-hub;` and the three security headers repeated, because a location-level `add_header` drops the server-level ones; the same block shape serves `/typst/` and `/pdfjs/` for H7 and H9) that (a) 404s a missing file (`try_files $uri =404`), so the loader reports a fetch failure with the URL instead of a corrupt body, (b) serves it as `application/octet-stream` without re-compression (the existing `gzip_types` already excludes it) and with `Cache-Control: public, max-age=31536000, immutable` (the asset name carries `PANDOC_PIN`, so a name never changes content; the loader's SHA check covers a stale copy), and (c) adds a workflow step after the hub-client build that fails when `hub-client/dist/pandoc/<asset_name>` is absent, only when the checked-out q2 ref has `resources/pandoc-wasm.json` (the workflow deploys any `q2_ref`). Done when the PR is merged and a production fetch of the asset returns 200, the gzip magic bytes and the long cache header.
- [ ] **H4b — embed native-render mechanism (D7):** its own task group, time-boxed, startable once H1 exists; H5's embedded-hub item depends on it. The embedded hub asks the native side to render and download (the preview server's existing routes, or the hub-provider channel in `crates/quarto-hub-provider`, which already has a beacon/request/consent flow); decide how hub-client knows it is in the embed (a named build-time flag; `VITE_EPHEMERAL_STORAGE` is related but is not that flag); the preview server has no render route today, and `quarto-preview` does not depend on `quarto-hub-provider` (whose channel serves remote hubs), so the expected shape is a new preview-server route (Rust) modelled on `re_execute_handler`. Decide what content the embedded hub renders: that handler reads files from disk, and with edits disabled the disk copy can be stale against the editor's content. Done when "Download as docx" works in a locally built `q2 preview`.
- [ ] **CSP:** hub-client sets none today (only `quarto-hub/server.rs` does, with auth on), so there is nothing to change; note `worker-src` and `wasm-unsafe-eval` for any deployment that adds headers.

## Verification

A hub build without the asset hides the menu and warns (a release build fails); the deployment-repo PR is merged and production serves the asset as above; the embed build contains no wasm (check the `include_dir!` archive); the embed's native download works in a local `q2 preview`; hub E2E still green.

## Exit

The asset ships correctly in the hub and not in the embed; the embed has a working path. H5 builds the control on both.

## Close-out

- [ ] Every Verification item above passes, and the phase-boundary gates have been run (the workspace nextest for a phase that touches Rust, its pass/skip delta against the live baseline accounted for; the hub-client and ts-package suites for a phase that touches TS).
- [ ] Checklist reconciled: this file re-read and every tick verified against what actually landed, wrong ticks corrected and committed.
- [ ] Handoff log current; branch rebased onto `feature/pandoc-wasm` and fast-forwarded into it (design: Parallel development).

## Handoff log

Append-only. Update it in the commit that ends each task and before any stop; a new agent starts here (design: Handoff).

- Branch and worktree: _none yet_
- Last commit; tasks ticked: _none_
- State and gotchas: _none_
- Next step: _the first unticked task_
