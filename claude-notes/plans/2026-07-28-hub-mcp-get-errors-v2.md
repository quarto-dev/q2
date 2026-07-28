# hub-mcp `get_errors` v2: validate locally with the QuartoHub WASM pipeline

## Overview

Agents using the Quarto Hub MCP server can read and write project files but
cannot see render errors. **v1** (branch `feature/hub-mcp-get-errors`,
plan `claude-notes/plans/2026-07-16-hub-mcp-get-errors.md`) had the browser
preview publish its diagnostics into an automerge index-doc sidecar that the
MCP read back with a content-hash staleness flag. It was implemented and
verified end-to-end, but review guidance from Carlos killed the architecture:

> Don't try to chase synchronization with the CRDT. Just grab the content of
> the file/project you care about and have an API entry point to check for
> the validity. You're never going to be able to know if the document you
> just changed ends up looking exactly how you expected it to, because it's
> a distributed system.

**v2** (this branch): `get_errors` renders the project files the MCP already
holds — using the *same WASM module the browser preview runs*
(`wasm-quarto-hub-client`) hosted in the MCP's Node process — and reports the
diagnostics of exactly what it rendered. Deterministic, no cross-peer
choreography, no schema change, hub-client untouched. The only cross-peer
data still read is the existing `captures` sidecar (execution errors happen
elsewhere and cannot be recomputed locally).

Feasibility proven 2026-07-28 in a Node spike: esbuild-bundle the
wasm-bindgen JS with three aliases (`/src/wasm-js-bridge/{cache,fetch,sass}.js`
→ `ts-packages/wasm-js-bridge/src/*`), `sass` external (never needed for
diagnostics), init from bytes (`init(await readFile(wasmPath))`), then
`vfs_add_file` + `render_page_in_project('index.qmd')` returned the identical
structured diagnostic the browser shows (`[Q-2-13] Unclosed Strong Star
Emphasis`, line 5 col 24) for a broken fixture.

## Design

New module `ts-packages/quarto-hub-mcp/src/local-render.ts`:

- `initRenderer(wasmBytes | wasmPath)` — one-time init (lazy, on first
  `get_errors` call; keeps server startup fast).
- `renderDiagnostics(files: Map<string, FilePayload>, path: string)` —
  `vfs_clear()`, `vfs_add_file('/project/' + p, text)` for every text file
  (`vfs_add_binary_file` for binaries), then `render_page_in_project(path)`;
  returns `{ diagnostics, warnings, pass1Failures, error }` mapped from the
  WASM `RenderResponse`. Serialize renders with a promise chain (the VFS is
  a module-global in the WASM instance).

`get_errors` tool (kept name, args `{ project, path? }`, read-only mode):
- `path` given → render that file; omitted → render every `.qmd` in the
  project (pass-1 failures attribute sibling errors to their own paths, so a
  single `index.qmd` render already surfaces most project-wide breakage —
  render each remaining `.qmd` for completeness, capped and noted).
- Output per file: `{ path, checkedContentSha256, errors, warnings }` plus
  `execution: { state, lastError }` from the `captures` sidecar. No `stale`
  flag — the response describes exactly the bytes that were rendered.
- Tool description teaches the loop: read → fix via patch_file → call
  get_errors again (it validates the new content immediately; no waiting).

Bundling (two consumers):
- `tsc` dev build (`dist/`, used by tests): a Node loader in
  `local-render.ts` resolves the wasm-bindgen JS + `.wasm` from
  `hub-client/wasm-quarto-hub-client/` via an env override
  (`QUARTO_HUB_MCP_WASM_DIR`) falling back to a path probe.
- esbuild bundle (`dist-bundle/`, embedded in `q2 mcp`): extend
  `crates/xtask`'s build-hub-mcp-bundle with the three bridge aliases +
  `sass` external, and copy `wasm_quarto_hub_client_bg.wasm` (~38 MB) into
  `dist-bundle/`. Note: the q2 binary already embeds a second copy of this
  WASM for the preview SPA — dedupe is a follow-up, not v1.

## Work items (TDD)

### Phase 1 — local renderer
- [ ] Tests first (`src/local-render.test.ts`, real WASM, no mocks): broken
      YAML → error diagnostic with line/col; clean doc → empty; sibling
      pass-1 failure attributed to sibling path; binary files tolerated;
      sequential renders don't interleave
- [ ] Implement `local-render.ts` (loader, VFS fill, render, mapping)

### Phase 2 — get_errors tool rework
- [ ] Rework `src/get-errors-handler.test.ts` (ported from v1): shapes,
      captures surfacing (error surfaced / idle suppressed / running
      surfaced), path filter, checkedContentSha256 present, renderer mocked
      at the module seam
- [ ] Rework `src/get-errors-live.test.ts`: real server binary + test hub +
      real WASM — create broken project via MCP, `get_errors` returns the
      diagnostic; `patch_file` fix; `get_errors` immediately returns clean
- [ ] `tools.ts`: reimplement handler on local render + captures;
      keep `onCapturesChange` wiring in connection-manager (v1's
      `sidecars.captures`), drop everything diagnostics-sidecar
- [ ] Tool lists in both modes (`hub-mcp.test.ts`) include `get_errors`

### Phase 3 — bundling
- [ ] `cargo xtask build-hub-mcp-bundle`: bridge aliases, `sass` external,
      wasm copy into dist-bundle; `q2 mcp --launcher-info` freshness check
- [ ] `bundle.test.ts` covers the wasm asset presence

### Phase 4 — verification
- [x] Package suites green; e2e recorded below. v2 contains ZERO Rust
      changes and does not touch hub-client or the schema/sync packages
      (all verified green at their upstream state), so the Rust verify
      legs are unaffected; CI covers them on the PR.

## End-to-end verification record (2026-07-28)

Throwaway Rust hub (`target/debug/hub --data-dir <tmp> --port 3105
--allow-insecure-auth`); real MCP server (`dist/index.js`, which loads
the real WASM host) driven over stdio in a single session:

1. `create_project` with `index.qmd` containing
   `Hello **unclosed strong` → indexDocId `2APRALdSKxe8RbrcF3JckcFnbDQL`.
2. `get_errors { project }` → inspected output:
   `errors: [ { kind: "error", title: "Unclosed Strong Star Emphasis",
   code: "Q-2-13", problem: "I reached the end of the block before
   finding a closing '**' …", start_line: 5, start_column: 24, details:
   [ { kind: "info", content: "This is the opening '**' mark.", … } ] } ]`
   plus `checkedContentSha256: sha256:4305d4…` naming the exact text
   rendered. (The ANSI `rendered` snippet observed in this first run is
   stripped from tool output as of the follow-up commit — structured
   fields only.)
3. `patch_file` closing the emphasis → `get_errors { project, path }`
   immediately returned `errors: [], warnings: []` with the new
   `checkedContentSha256: sha256:7aef66…`. No polling, no other peer.

Also verified via the committed integration test
`src/get-errors-live.test.ts` (real server binary + in-process test
hub + real WASM: same loop), and `bundle.test.ts` pins that
dist-bundle ships `wasm-host.mjs` + the `.wasm`; the embedded `q2 mcp`
bundle rebuilt and `--launcher-info` confirmed 15 bundle files at the
branch commit.

## Carried over from v1 (independent of architecture)
- [x] `scripts/local-prod-server.mjs`: WS proxy no longer crashes on client
      ECONNRESET (unhandled socket 'error') — verified by hard-killing a
      live WS client
- Strand backlog (braid still awaiting the q2 skein doc id on this machine):
  1. samod wedge: connection close with pending sync state busy-loops the
     hub and stops all doc exchange until restart (see v1 plan's BLOCKER
     section for log signatures + repro; p0/p1)
  2. deleteFile/renameFile leave stale `captures` sidecar entries
  3. preview red-bars `date-modified: last-modified` (keyword unresolvable
     in browser VFS) as if it were a document error
  4. dedupe the two embedded copies of wasm_quarto_hub_client_bg.wasm in q2

## v1 disposition
`feature/hub-mcp-get-errors` (sidecar publish/read, fully implemented and
tested) is preserved as a branch. If a human-facing "see collaborators'
preview state" feature is ever wanted, that work is a starting point — but
it is intentionally NOT part of this PR.
