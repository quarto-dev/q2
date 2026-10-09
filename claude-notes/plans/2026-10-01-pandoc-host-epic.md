---
title: 'Epic: pandoc.wasm host, "Download as" UI and PDF (browser side)'
date: 2026-10-01
---

**Date:** 2026-10-01
**Status:** Planned; nothing started
**Design (authoritative):** [`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md)
**Companion epic:** [`2026-10-01-pandoc-request-epic.md`](2026-10-01-pandoc-request-epic.md) (the Rust request seam)
**Integration branch:** `feature/pandoc-wasm` (from `origin/main`)
**Evidence:** [`../research/2026-10-01-pandoc-wasm-evidence.md`](../research/2026-10-01-pandoc-wasm-evidence.md); spike code in `../research/2026-10-01-pandoc-wasm-spike/`

## The problem

Hub-client must run the official `pandoc.wasm` in a browser, replaying a `PandocRequest` built by the Rust wasm, and offer it as a "Download as" control (docx, pptx, epub, typst source), with PDF to follow through a typst wasm compiler and a pdf.js viewer. This epic is the browser half: proving the wasm runs in WASI command mode with the vendored Lua, a DOM-free host package, a lazy loader with caching, the parity net against native output, the asset pipeline and the embedded-hub path, the UI, hardening, and the PDF chain.

## Key architecture decisions (see the design doc)

- The host is a DOM-free package in `ts-packages` exposing `execute(request, shareTree, { module, fault? }) -> result`; hub-client owns UI and click-time policy (design: Who owns what).
- `pandoc.wasm` ships as an opaque gzip file cached by the main-thread loader; the `Module` lives on the main thread and is posted to one short-lived worker per render (D2, D6); the PDF preview pane alone uses a small pool of warm workers (H10a, H10b; D2(b)).
- The embedded hub in `q2 preview` does not carry the wasm; it uses native pandoc (D7).
- Three-class format classification and a `PreviewRouter` mode enum (D8.7).
- PDF: lazy typst.ts compiler in its own worker, pdf.js viewer modeled on Q1's (T1-T7).

## Phases

| Plan | Scope | Depends on | Novel vs. port |
|---|---|---|---|
| **H0** [runtime gates](2026-10-01-pandoc-host-H0-runtime-gates.md) | Command mode in a browser with numbers; the Lua-under-wasm gate on captured pandoc-profile inputs; SVG-in-docx and crossref-index checks. **Stop/rethink point** | request R0 (capture wrapper, constants, extractor) | Novel verification |
| **H1** [host core](2026-10-01-pandoc-host-H1-host-core.md) | `ts-packages` host package, WASI command-mode host, mounting with limits and path rules, download-by-checksum script, vitest/CI wiring | H0; request R1's first task (schema and golden) | Novel |
| **H2** [loader](2026-10-01-pandoc-host-H2-loader.md) | Main-thread loader (fetch, sniff, cache, decompress, SHA, compile), worker lifecycle (abort, timeout, idle drop), failure taxonomy, Playwright smoke | H1 | Novel |
| **H3** [integration and parity](2026-10-01-pandoc-host-H3-integration-parity.md) | Dev harness on the real `render_pandoc_request`; docx parity net in Node and browser; CI net; browser measurements that fix the limits | H2; request R2 | Novel |
| **H4** [delivery and embed](2026-10-01-pandoc-host-H4-delivery-embed.md) | `public/pandoc/` asset pipeline, exclusion from the embed, feature flag, the embedded hub's native-render mechanism, CSP note, a PR to the `quarto-hub-deployment` repo (nginx and a presence check) | H1 (the download script); H4b is startable once H1 exists | Novel |
| **H5** [Download as UI](2026-10-01-pandoc-host-H5-download-ui.md) | Control next to print, status channel, progress, three-class classifier, `PreviewRouter` mode, accessibility, E2E, changelog | H3, H4 (embed task H4b for the embedded hub); request R9 for the book entries (a later landing); request R2 (resolver, format table), R3 (cell count), R4/R5 (typst/pptx/epub entries), R7 (project downloads) | Novel |
| **H6** [hardening](2026-10-01-pandoc-host-H6-hardening.md) | WebKit probe in CI, memory and large-document tests, taxonomy completeness, D2(b) spike (not run; measured later, implemented in H10) | H5 | Hardening |
| **H7** [typst worker](2026-10-01-pandoc-host-H7-typst-worker.md) | PDF budget measurement; standalone typst.ts worker with VFS, fonts, package registry, PDF export | H0 (independent of pandoc otherwise); may run in parallel with H1-H5 | Novel |
| **H8** [PDF chain](2026-10-01-pandoc-host-H8-pdf-chain.md) | Compile-side request additions; pandoc worker → typst worker; "Download as PDF" | H5, H7; request R4 | Novel |
| **H9** [viewer and parity](2026-10-01-pandoc-host-H9-viewer-parity.md) | pdf.js viewer with stable fingerprint and the PDF preview pane (chapter alone), time-boxed `incr_compile` spike, typst/PDF parity | H8 | Novel |
| **H10a** [warm executor](2026-10-02-pandoc-host-H10a-warm-executor.md) | The warm pandoc.wasm instance (D2(b)): measurement STOP, argv-to-defaults translator with a Rust drift guard (one `quarto-core` test, a fixture matrix and an allowlist file) and fresh-path fallback, `init.lua` env preamble, fd-2 and `/warnings` capture, poisoned-instance policy, warm-versus-fresh parity STOP. Nothing reachable from the UI | H9 | Novel |
| **H10b** [warm pool and preview](2026-10-02-pandoc-host-H10b-warm-pool-preview.md) | A two-worker warm pool on a second runner for the PDF preview only: no kill on supersede, adaptive grace rule, the preview's own typst runner, the overlapping-runs controller, pane wiring and flag, browser matrix | H10a | Novel |

Order: H0 → H1 → H2 → H3 → H5 → H6; H4 (the asset pipeline and the H4b embed route) needs only H1, so it can run beside H2/H3, and must finish before H5's embed item; H7 in parallel from H0; H8 → H9 → H10a → H10b after H5 and H7 (H10a adds one Rust test in `quarto-core`). H1, H2, H3 and H7 all edit `ts-test-suite.yml`, the hand-listed vitest aliases and `package-lock.json`, so merge them one at a time. Lane H (workspace-7) runs this epic; the design's Parallel development gives the lanes, the landing points and the hand-offs from lane R. Each phase is one agent.

## Progress

Tick a phase when its Close-out is complete and it has landed on `feature/pandoc-wasm`.

- [x] H0 [runtime gates](2026-10-01-pandoc-host-H0-runtime-gates.md)
- [x] H1 [host core](2026-10-01-pandoc-host-H1-host-core.md)
- [x] H2 [loader](2026-10-01-pandoc-host-H2-loader.md)
- [x] H3 [integration and parity](2026-10-01-pandoc-host-H3-integration-parity.md)
- [x] H4 [delivery and embed](2026-10-01-pandoc-host-H4-delivery-embed.md)
- [ ] H5 [Download as UI](2026-10-01-pandoc-host-H5-download-ui.md): everything but the book entries, which wait on R9 (the open box is its Close-out wire-up item)
- [x] H6 [hardening](2026-10-01-pandoc-host-H6-hardening.md)
- [x] H7 [typst worker](2026-10-01-pandoc-host-H7-typst-worker.md)
- [x] H8 [PDF chain](2026-10-01-pandoc-host-H8-pdf-chain.md)
- [x] H9 [viewer and parity](2026-10-01-pandoc-host-H9-viewer-parity.md)
- [x] H10a [warm executor](2026-10-02-pandoc-host-H10a-warm-executor.md)
- [x] H10b [warm pool and preview](2026-10-02-pandoc-host-H10b-warm-pool-preview.md)

## Dependencies on the request epic

| This epic needs | From | For |
|---|---|---|
| Capture wrapper, constants file, extractor CLI | R0 | H0, H1 |
| Published `pandoc-request.schema.json` and golden | R1 (first task) | H1 |
| `render_pandoc_request`, share-tree export, classify export, format table, resolver | R2 | H3, H5 |
| Unexecuted-cell count | R3 | H5 status |
| Typst, pptx and epub requests; project requests | R4, R5, R7 | H5 menu entries |
| Typst-source request; `pdf` format-table entry (hidden until H8), `post: compile_typst`, typst packages and fonts in a separate typst-assets export | R4 (last task) | H8 |
| Whole-book request (R9: `scope`, `stats.book`, resolver `book` field, `captures_by_path`, progress/cancel hook) | R9 | H5 book menu entries, status and per-chapter capture collection; H8 needs nothing |

## Definition of done

- [ ] "Download as" docx, pptx, epub and typst source work in hub-client in Chromium and WebKit (Firefox verified by hand), loaded lazily, cached, abortable, with the failure taxonomy implemented and tested.
- [ ] Output equals native pandoc 3.11 for the P7 fixtures (parity net in CI).
- [ ] The embedded hub uses native pandoc; the wasm is excluded from the embed.
- [ ] PDF: "Download as PDF" works through the typst worker, with a pdf.js viewer that keeps scroll position across recompiles.
- [ ] The PDF preview refreshes through the warm pool (H10a, H10b), byte-equal to the fresh path; Download as and parity use the fresh path.

## Known limitations (v1)

- Firefox is manual-only (CI Firefox is braid `bd-phu943t7`).
- Tab-kill risk on low-memory mobile devices is accepted; the size hint is the mitigation.
- Wasm Lua cannot run `os.tmpname`, `io.popen`, `pandoc.pipe`, network `mediabag.fetch` or `lfs`; SVG images in docx/pptx follow the H0 decision. Filters in pandoc.wasm see a `/` working directory and only the request's environment (design D9).
- Incremental PDF recompile is a time-boxed spike, not a feature.
- The warm preview path never shrinks wasm memory (a worker is dropped above 512 MB or when idle), honours no Haskell-side `SOURCE_DATE_EPOCH` (typst needs none), keeps two pandoc workers and a typst worker resident, and its real Safari and iOS memory is unmeasured.
