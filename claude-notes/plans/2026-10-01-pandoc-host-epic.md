# Epic: pandoc.wasm host, "Download as" UI and PDF (browser side)

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
- `pandoc.wasm` ships as an opaque gzip file cached by the main-thread loader; the `Module` lives on the main thread and is posted to one short-lived worker per render (D2, D6).
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
| **H5** [Download as UI](2026-10-01-pandoc-host-H5-download-ui.md) | Control next to print, status channel, progress, three-class classifier, `PreviewRouter` mode, accessibility, E2E, changelog | H3, H4 (embed task H4b for the embedded hub); request R2 (resolver, format table), R3 (cell count), R4/R5 (typst/pptx/epub entries), R7 (project downloads) | Novel |
| **H6** [hardening](2026-10-01-pandoc-host-H6-hardening.md) | WebKit probe in CI, memory and large-document tests, taxonomy completeness, optional D2(b) spike | H5 | Hardening |
| **H7** [typst worker](2026-10-01-pandoc-host-H7-typst-worker.md) | PDF budget measurement; standalone typst.ts worker with VFS, fonts, package registry, PDF export | H0 (independent of pandoc otherwise); may run in parallel with H1-H5 | Novel |
| **H8** [PDF chain](2026-10-01-pandoc-host-H8-pdf-chain.md) | Compile-side request additions; pandoc worker → typst worker; "Download as PDF" | H5, H7; request R4 | Novel |
| **H9** [viewer and parity](2026-10-01-pandoc-host-H9-viewer-parity.md) | pdf.js viewer with stable fingerprint, chapter-anchor navigation, time-boxed `incr_compile` spike, typst/PDF parity | H8; chapter navigation consumes anchors from a later book plan (request R7 stages 2-3) | Novel |

Order: H0 → H1 → H2 → H3 → H5 → H6; H4 (the asset pipeline and the H4b embed route) needs only H1, so it can run beside H2/H3, and must finish before H5's embed item; H7 in parallel from H0; H8 → H9 after H5 and H7. H1, H2, H3 and H7 all edit `ts-test-suite.yml`, the hand-listed vitest aliases and `package-lock.json`, so merge them one at a time. Lane H (workspace-7) runs this epic; the design's Parallel development gives the lanes, the landing points and the hand-offs from lane R. Each phase is one agent.

## Progress

Tick a phase when its Close-out is complete and it has landed on `feature/pandoc-wasm`.

- [ ] H0 [runtime gates](2026-10-01-pandoc-host-H0-runtime-gates.md)
- [ ] H1 [host core](2026-10-01-pandoc-host-H1-host-core.md)
- [ ] H2 [loader](2026-10-01-pandoc-host-H2-loader.md)
- [ ] H3 [integration and parity](2026-10-01-pandoc-host-H3-integration-parity.md)
- [ ] H4 [delivery and embed](2026-10-01-pandoc-host-H4-delivery-embed.md)
- [ ] H5 [Download as UI](2026-10-01-pandoc-host-H5-download-ui.md)
- [ ] H6 [hardening](2026-10-01-pandoc-host-H6-hardening.md)
- [ ] H7 [typst worker](2026-10-01-pandoc-host-H7-typst-worker.md)
- [ ] H8 [PDF chain](2026-10-01-pandoc-host-H8-pdf-chain.md)
- [ ] H9 [viewer and parity](2026-10-01-pandoc-host-H9-viewer-parity.md)

## Dependencies on the request epic

| This epic needs | From | For |
|---|---|---|
| Capture wrapper, constants file, extractor CLI | R0 | H0, H1 |
| Published `pandoc-request.schema.json` and golden | R1 (first task) | H1 |
| `render_pandoc_request`, share-tree export, classify export, format table, resolver | R2 | H3, H5 |
| Unexecuted-cell count | R3 | H5 status |
| Typst, pptx and epub requests; project requests | R4, R5, R7 | H5 menu entries |
| Typst-source request; `pdf` format-table entry (hidden until H8), `post: compile_typst`, typst packages and fonts in a separate typst-assets export | R4 (last task) | H8 |
| Whole-book request and chapter anchors | later book plan (R7 stages 2-3) | H9 chapter navigation |

## Definition of done

- [ ] "Download as" docx, pptx, epub and typst source work in hub-client in Chromium and WebKit (Firefox verified by hand), loaded lazily, cached, abortable, with the failure taxonomy implemented and tested.
- [ ] Output equals native pandoc 3.11 for the P7 fixtures (parity net in CI).
- [ ] The embedded hub uses native pandoc; the wasm is excluded from the embed.
- [ ] PDF: "Download as PDF" works through the typst worker, with a pdf.js viewer that keeps scroll position across recompiles.

## Known limitations (v1)

- Firefox is manual-only (CI Firefox is braid `bd-phu943t7`).
- Tab-kill risk on low-memory mobile devices is accepted; the size hint is the mitigation.
- Wasm Lua cannot run `os.tmpname`, `io.popen`, `pandoc.pipe`, network `mediabag.fetch` or `lfs`; SVG images in docx/pptx follow the H0 decision. Filters in pandoc.wasm see a `/` working directory and only the request's environment (design D9).
- Incremental PDF recompile is a time-boxed spike, not a feature.
