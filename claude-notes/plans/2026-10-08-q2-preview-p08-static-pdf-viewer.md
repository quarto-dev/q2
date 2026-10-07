# Plan 8: a Quarto-1-style pdf.js viewer for `q2 preview --static`

Part of the epic [`2026-10-08-q2-preview-hub-parity-epic.md`](2026-10-08-q2-preview-hub-parity-epic.md) (E12); the execution model and landing procedure are there.

## Execution

- **Lane:** C (`workspace-3`), binding: one agent per plan, in this order: Plan 1, Plan 4, Plan 7, Plan 8, Plan 11. This plan is number 4 of 5 in the lane.
- **Waits on:** Plan 4 merged into `feature/q2-preview-hub-parity` (the pdf.js tree is bundled, served with correct types, and Plan 4's shared `pdfjs_response` with the locale alias). Nothing inside this plan waits on another plan. It uses none of the Rust render work.
- **Start gate:** every plan under "Waits on" is merged. Check from your worktree: `git -C ../workspace-7 branch --merged feature/q2-preview-hub-parity | grep -q 'q2-preview/p04-pdfjs-packaging' && echo ready`. If it does not print `ready`, do not start: report which plans are missing and stop (plan branches are never deleted after landing, so `--merged` is the test). Before coding, read the Handoff log at the end of each plan you wait on.
- **Branch:** `q2-preview/p08-static-pdf-viewer`, cut from the tip of `feature/q2-preview-hub-parity` when you start.
- **Merges to:** `feature/q2-preview-hub-parity`, by the landing procedure in the epic.
- **Unlocks:** Plan 13 (documents it and closes the epic).
- **Next in lane:** Plan 11 (`2026-10-08-q2-preview-p11-spa-pdf-pane`); the next agent starts only when its own start gate passes.

## Goal

`q2 preview --static` on a `format: typst` page or project shows the PDF in a bundled pdf.js viewer that reloads when the render finishes. Today a single typst file is refused outright, and a typst project opens the raw `.pdf` in the browser's own viewer with no reload.

## Background facts

- `--static` never loads the SPA or the hub (`static_mode/mod.rs:6-7`): a separate router serves the rendered output directory and injects an SSE reload client into every `.html` response (`static_mode/server.rs:125,415`, `EVENTS_PATH = /__q2-preview/events`); its watch policy ignores dot-directories (`watch_policy.rs:96-103`) and `note_page_view` counts only `.html`. `q2 preview --static doc.qmd` with `format: typst` exits 1 today ("Format 'typst' is not supported with --static (only html and revealjs can be served)", ran): `check_servable_format` (`crates/quarto/src/commands/preview_static.rs:84-99`, called at `:102` and `:306`; comment at `:79-81`), pinned by unit tests at `:751-763` and `preview_cli.rs:~246-266` (`preview_static_rejects_unservable_format`). A project with `format: typst` in `_quarto.yml` already works: it serves `doc.pdf` with a 302 from `/` and 200 `application/pdf` (ran), opened in the browser's own viewer with no reload.
- **Reload exists already.** `finish_render` sends a reload after every successful render (`preview_static.rs:~712-719`). `watch_policy::classify` ignores every path in `ctx.outputs` on purpose ("a render beside the sources never re-triggers itself") and its `Action` means render, not reload; treating a changed `.pdf` as a trigger would loop. The viewer page only lacks an SSE client.
- **Quarto 1's PDF preview** (`quarto-cli-v1.11.3/src/core/pdfjs.ts`, `src/command/preview/preview.ts:151,241,616-620`): the preview's initial path for a PDF output is `web/viewer.html` in a bundled pdf.js tree; the rendered PDF is served as a sibling file; `viewer.js` is patched to default to that file and hide the sidebar in an iframe; the worker is patched to a constant fingerprint so zoom, page and sidebar prefs persist across reloads; the viewer HTML gets the same injected reloader as any page, and on re-render clients reload `/web/viewer.html` after the render queue finishes.
- **Spike results (ran against pdf.js 6.3.289, Playwright, worker patched with the repo's `patchWorker`).** `viewer.html` carries a CSP meta (`script-src 'self'; style-src 'self'`): the inline script `inject_client` splices in is blocked (`window.__injected` stays false) and the client's `setAttribute("style")` badge and panel would be blocked too. With no hash, page and zoom survive `location.reload()` and a recompile to a different PDF (4 to 6 pages, page 2 and 150% kept); with `#pagemode=none` or `#view=FitH` both reset to page 1 / auto (hub-client's own `viewerUrl` documents this, `pdfViewer.ts:35-40`). The sidebar is closed by default. The viewer's own fetch of the PDF arrives as `sec-fetch-mode: cors`, which `note_page_view` skips; a navigation to the `.pdf` arrives as `mode=navigate, dest=document`.
- The reserved prefix `/__q2-preview/` passes `decode_segments`; an explicit route before the fallback takes precedence. `lookup_embedded` (`quarto-preview/src/lib.rs`) is private but reachable from the same crate's child module. `mime_guess` knows `.mjs`.

## Decisions

- **P8-D1 Viewer location.** `viewer.html` (Q1's name) under the reserved prefix `/__q2-preview/pdfjs/web/` (not Q1's `/web/` at the root, which could collide with user output); the PDF stays at its original path.
- **P8-D2 URL.** `viewer.html?file=<path>` with NO hash parameter (a hash resets page and zoom); persistence uses the already-patched worker fingerprint. No `viewer.js` string patches (they do not survive pdf.js bumps).
- **P8-D3 Reload client under the CSP.** The viewer gets the reload client as an external script (`/__q2-preview/client.js`) whose UI is built with CSSOM, or the static server strips the CSP meta when serving this one page; task 8.3 chooses and records it.

## Tasks

Per task: `cargo clippy -p quarto --all-targets -- -D warnings` plus `cargo nextest run -p quarto` (and `-p quarto-preview` where touched). Workspace nextest at the end of the plan. Integration tests go in `crates/<crate>/tests/integration/<name>.rs` plus `pub mod` in `main.rs`. Tests that need the real SPA dist skip with a clear reason when `EMBEDDED_SPA` is the placeholder.

- [ ] 8.1 Baseline `cargo nextest run --workspace` on the integration tip.
- [ ] 8.2 Re-confirm the spike results on the final pdf.js tree (`?file=`, no hash, persistence across `location.reload()`); read `preview_static.rs` and `static_mode/*` and record how reload events are emitted; record any deviation and the fallback (capture and restore the hash in the injected script) if persistence fails.
- [ ] 8.3 Single-file typst gate: let typst through `check_servable_format` at both call sites (`:102`, `:306`) and rewrite the three pinning tests to expect a served viewer for typst (other non-html formats stay refused); decide and record what a refused format does before the render starts. Then the static server: serve the embedded pdf.js tree under `/__q2-preview/pdfjs/` (same bundle as Plan 4, no extra fetch) through Plan 4's shared `pdfjs_response` (so the `en-US` locale alias and the 404 rule apply here too) with `viewer.html` as the page, sent without `no-store` (the tree changes only with the binary; `file_response` today sends `no-store` on every asset); a browser navigation to a `.pdf` (`Sec-Fetch-Mode: navigate`, `Sec-Fetch-Dest: document`, not the viewer's own cors `fetch`) and `/` for a PDF-only output are redirected to `viewer.html?file=<path>`; the raw bytes stay at the original path; path-traversal handling unchanged; the viewer page gets the reload client per P8-D3 (record the choice; the existing `client.js` builds its badge and panel with `setAttribute("style")`, which the page's CSP blocks, so choosing the external script means rewriting that UI with CSSOM). When no pdf.js tree is embedded (placeholder build), serve the raw PDF as today.
- [ ] 8.4 Reload and page views: no change to `watch_policy::classify`; the existing `finish_render` reload fires after a successful render; a PDF page view is recorded at the redirect for the `.pdf` navigation (lazy execution depends on it, because the viewer's own fetch is cors and skipped); `note_page_view` (`server.rs:102`) counts only `.html`, so widen it for `.pdf` navigations. Check that a reload event with `target=/doc.pdf` ends on the viewer (it does `location.replace` through the redirect, not an in-place reload) and keeps page and zoom.
- [ ] 8.5 Tests: integration over the real static server, with the pdf.js lookup injected so they run in `test-suite.yml` (where the SPA dist is a placeholder) as well as against the real tree (single-file typst accepted, viewer served, `.pdf` navigation redirected to the viewer, raw bytes intact, reload event fired after a re-render, a page view recorded at the redirect); a browser check of persistence per 8.2.
- [ ] 8.6 Plan end: workspace nextest against 8.1; reconcile this checklist; write the Handoff log; land per the epic.

## Risks

- pdf.js bumps need `patchWorker` updates; it throws loudly when its anchor is missing.
- If stripping the CSP meta is chosen, the viewer page runs with a weaker policy than pdf.js ships; the external-script option avoids that.

## Handoff log

Written by the implementing agent in the plan-end task, committed with the checklist: what landed (commit range), any task that deviated from the plan and why, names and signatures of the interfaces later plans consume exactly as they ended up, the workspace nextest baseline and final counts, and anything the next agent in the lane or a plan that waits on this one must know. Empty until the plan lands.
