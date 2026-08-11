# Mermaid runtime is imported from jsDelivr at page load, not bundled into the site (bd-mermaid-runtime-not-bundled-vxejw159)

**Date:** 2026-08-11
**Braid:** `bd-mermaid-runtime-not-bundled-vxejw159`
**Branch:** `main` @ `001cb6a5` (investigated in place; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design** — and the investigation *closes* the strand's stated open question ("where do the bytes come from"), because upstream mermaid already ships exactly the artifact we need: a self-contained 2.62 MiB `dist/mermaid.min.js` that is **SHA-256 identical** to the copy Quarto 1 vendors. The remaining decisions are about cost and scope, not feasibility.

Two premises in the strand description turned out to be wrong in ways that materially change the plan; both are corrected below.

## Issue context

Type `bug`, priority 2, label `parity`, opened 2026-08-11 by Carlos Scheidegger — filed today, so no staleness risk. Origin strand in the connect-docs porting skein: `br-u5v95mpd`.

Every HTML page containing a diagram gets an after-body module script importing the mermaid runtime from jsDelivr; nothing is written into the rendered site. Three separable consequences: air-gapped/closed-network hosting is impossible; published docs execute third-party code at load time and leak reader IPs to a CDN; a CDN outage silently degrades every diagram page. Real-world hit is the Posit Connect docs — 14 pages, 33 diagrams, the sharpest case being `admin/appendix/airgapped/index.qmd`, the guide to running Connect *without internet access*, which itself needs internet access to render its two diagrams.

## Dependency graph

Nearly empty, which is itself informative:

- **`dep tree`**: single node, no children. Not part of an epic.
- **`related` → `bd-yvz2xqrm`** (closed): the same class of problem for the hub client — Monaco loaded from jsDelivr, editor hung when the CDN was slow/blocked/offline; fixed in PR #411 by bundling via `loader.config({ monaco })` + worker wiring, with an e2e guard that blocks jsDelivr. Comment `c-j1zq3nyv` on that strand says explicitly: *"Mermaid's lazy jsDelivr import noted as out of scope."* So this dependency was seen and deliberately deferred, not judged acceptable. This strand is that follow-up.
- No **incoming `blocks`** edges — nothing is formally waiting on this. Urgency comes from the Connect-docs port, not from the graph.

Worth carrying forward from `bd-yvz2xqrm`: bundling Monaco pushed the vite build past the default ~2 GB Node heap and needed `--max-old-space-size=4096` (comment `c-iw7g59gg`). If we also bundle mermaid into the *preview/hub-client* path (see Question 3), expect a similar build-memory conversation. The `q2 render` path proposed here does not go through vite and does not carry that risk.

## What the code looks like today

All paths in the description still exist and are unchanged at `001cb6a5`.

`crates/quarto-core/src/transforms/mermaid.rs`:
- `MERMAID_VERSION = "11.12.0"` (line 62)
- `mermaid_script_block()` hard-codes the jsDelivr URL (line 170)
- unit test asserts that exact URL (line 287)
- script appended once per document to `rendered.includes.after-body` via `MERMAID_JS_SENTINEL`

Symptom confirmed at HEAD from the repro's committed output (`/Users/cscheid/repos/github/cscheid/q2-connect-docs/llms-info/repros/mermaid-runtime-not-bundled/`):

```
$ grep -o "https://cdn.jsdelivr.net[^']*" _site/index.html
https://cdn.jsdelivr.net/npm/mermaid@11.12.0/dist/mermaid.esm.min.mjs
$ find _site -iname '*mermaid*'      # (nothing)
$ ls _site/site_libs
bootstrap  quarto
```

### Correction 1 — there *is* a vendored-JS precedent, and it is a complete one

The strand says: *"crates/quarto-core/resources/ currently holds only styles.css, so there is no vendored-JS precedent in that crate to follow."* True of the *crate-local* dir, but the precedent lives at **repo root** `resources/revealjs/` and matches this problem point for point:

- `resources/revealjs/{reveal.js,reveal.css,reset.css,theme/white.css,LICENSE}` — vendored from `node_modules/reveal.js/dist/`
- a `README.md` recording source, version, per-file provenance, and update procedure
- embedded at compile time via `include_str!` (`crates/quarto-core/src/revealjs/assemble.rs:29-35`)
- `register_reveal_assets()` (same file, line 191) stores each asset as an `ArtifactScope::Project` artifact at path `revealjs/<filename>` → lands at `_site/site_libs/revealjs/<filename>`, deduplicated across pages, with relative `../site_libs/...` URLs computed by the existing resolver
- a **drift test** (`vendored_reveal_assets_match_npm_package`, line 694) compares the embedded bytes to `node_modules/reveal.js/dist/`, normalizes line endings for Windows checkouts, and skips when `node_modules` is absent

This is a working template for mermaid, including the CI-enforced staleness guard. It is also already blessed by the External Sources Policy in `CLAUDE.md` (`resources/scss/`, "`resources/` (future) — Other resources as needed").

Also: `RenderContext` already carries `pub artifacts: ArtifactStore` (`crates/quarto-core/src/render.rs:209`), and `AstTransformsStage` bridges it **in both directions** — `render_ctx.artifacts = std::mem::take(&mut ctx.artifacts)` on the way in (`crates/quarto-core/src/stage/stages/ast_transforms.rs:182`) and `ctx.artifacts = render_ctx.artifacts` on the way out (line 227). So the mermaid transform can register a `Project`-scoped artifact directly and it will reach the pipeline's artifact writer. **No new plumbing is required** — this was the one thing that could have forced a different design, and it checks out.

### Correction 2 — the file we currently import is a 26 KB stub, not the runtime

This is the crux, and it makes the strand's "vendor the ESM build" option much worse than it sounds. Measured against the real 11.12.0 tarball (full output in `mermaid-runtime-not-bundled-investigation/dist-shape-measurements.md`):

| Artifact | Size | Self-contained? |
| --- | ---: | --- |
| `dist/mermaid.esm.min.mjs` (what q2 imports today) | 26 KB | **No** — 10 static chunk imports + ~25 dynamic ones |
| `dist/chunks/mermaid.esm.min/` (146 files) | 13 MB | — |
| `dist/mermaid.min.js` (UMD) | **2,748,992 B (2.62 MiB)** | **Yes** — 0 dynamic imports, 0 chunk refs |

The dynamic imports are **per diagram type** (`flowDiagram-*`, `ganttDiagram-*`, `classDiagram-*`, `sequenceDiagram`, `mindmap-definition-*`, …), resolved relative to the module URL. Two consequences:

1. Vendoring "the file we already import" would not fix anything — it would still need the whole 13 MB / 146-file chunk tree shipped alongside it.
2. It also means **today's CDN path makes an extra per-diagram-type round trip at render time**, beyond the entry fetch. A naive offline test using a flowchart could pass while a gantt page silently fails.

The UMD build sidesteps all of it. Its tail is `globalThis["mermaid"] = globalThis.__esbuild_esm_mermaid_nm["mermaid"].default;`, so it works as a classic `<script src="…">` followed by bare `mermaid.initialize(...)` / `mermaid.run(...)` — which is exactly how Q1's `mermaid-init.js` uses it.

And the provenance is clean — Q1 is not shipping a Q1-specific artifact:

```
07e37dfa97b337ccc85365d57eddf99b9706f09db3b59b260d0333b23b343c4b  (npm) mermaid@11.12.0 dist/mermaid.min.js
07e37dfa97b337ccc85365d57eddf99b9706f09db3b59b260d0333b23b343c4b  external-sources/.../html/mermaid/mermaid.min.js
```

Identical. So "vendor the upstream UMD" and "do what Q1 does" are the *same* decision, needing no esbuild step of our own and no build-time fetch. The strand's framing of the choice as "vendor a multi-megabyte asset *or* fetch at build time" resolves to: vendor one upstream file, 2.62 MiB, regenerable from npm and drift-testable.

## Proposed phases (draft)

- **Phase 0 — Test plan (TDD, failing first).**
  - Unit: emitted script references a relative `site_libs/…/mermaid.min.js` and contains **no** `cdn.jsdelivr.net`.
  - Unit: `register_mermaid_assets()` stores a `Project`-scoped artifact at `mermaid/mermaid.min.js`.
  - Drift test mirroring `vendored_reveal_assets_match_npm_package` (skip when `node_modules` absent).
  - Integration (through `render_document_to_file`, per the end-to-end rule): a website render with a diagram writes `_site/site_libs/mermaid/mermaid.min.js`; a nested page gets `../site_libs/…`; a diagram-free page ships neither the asset nor the script.
- **Phase 1 — Vendor + register.** Add `resources/mermaid/{mermaid.min.js,LICENSE,README.md}`; add `mermaid` as a root devDependency to anchor the drift test; `register_mermaid_assets()` alongside the reveal pattern, called from the transform (the artifact bridge is confirmed to carry it back).
- **Phase 2 — Emit a relative script.** Replace the module-import block with the classic-script + init form; keep the `MERMAID_JS_SENTINEL` idempotence contract.
- **Phase 3 — End-to-end verification.** Run `q2 render` on the repro **with the network blocked** and confirm an actual rendered SVG. Include a non-flowchart diagram type (gantt or class) to prove the chunk problem is genuinely gone.
- **Phase 4 — Docs + changelog.**

## Open design questions for the user

1. **Is 2.62 MiB in the `q2` binary acceptable, unconditionally?** `include_str!`/`include_bytes!` is compile-time, so every `q2` binary carries mermaid whether or not the user ever writes a diagram — this is how reveal.js already works, but reveal is ~175 KB and this is ~15×. Debug `q2` is currently 164 MB, so it's small in relative terms; release is the number that matters for the download. Accept it, or is a `--features` / download-on-first-use escape hatch wanted?

2. **Bundle by default, or opt-in?** The strand floats an opt-in project/format key as a fallback "if bundling by default is judged too heavy." My read is that default-on is the right call — it is what Q1 does, it is what makes a rendered site self-contained, and an opt-in key means the airgapped-docs case stays broken for anyone who doesn't know the key exists. But it is your call, and a `mermaid-runtime: bundled|cdn` key is cheap to add either way.

3. **Scope: does this cover the preview/hub-client path too?** `ts-packages/preview-renderer/src/q2-preview/blocks/MermaidCodeBlock.tsx` has its *own* jsDelivr dynamic import, with a version-parity test pinning both sides to 11.12.0 (`MermaidCodeBlock.test.tsx:119`). Fixing only the Rust side leaves `q2 preview` and hub-client CDN-dependent — arguably fine (preview is a live tool, not a published artifact), but it is the same class of defect `bd-yvz2xqrm` fixed for Monaco. Separate strand, or in scope here? If in scope, note the vite heap issue from `bd-yvz2xqrm`.

4. **Do we also port Q1's mermaid CSS/theming?** Q1 ships `mermaid.css`, `embed-mermaid.css`, and a `mermaid-init.js` that wires `themeCSS` from a `meta[name="mermaid-theme"]` tag so diagrams follow the document theme. q2 currently ships none of that. Strictly out of scope for "make it offline-correct," but we will be touching exactly this code, and it is a visible parity gap. Fold in, or file separately?

5. **Version-bump ergonomics.** With a drift test anchored to `node_modules`, bumping mermaid becomes: bump devDependency → re-copy → update `MERMAID_VERSION` in the Rust *and* TSX files. Is a `cargo xtask` sync step (like `build-hub-mcp-bundle`) wanted, or is a README procedure enough, as with reveal.js?

## Risks / tradeoffs (draft)

- **Binary size** is the real cost, and it is unconditional (Question 1). It is now the only *architectural* unknown left — the artifact-registration path is confirmed (see above), so the remaining questions are all judgement calls rather than feasibility risks.
- **Not a regression risk for existing output:** the change is confined to a Finalization-phase transform that already self-gates on HTML-family formats and is excluded from the preview pipeline (`Q2_PREVIEW_TRANSFORM_EXCLUDED`), so the preview path is untouched unless we choose Question 3.
- **Licensing** is clean — mermaid is MIT; vendor the LICENSE alongside, as `resources/revealjs/` does.
- **Behavior change for CDN users:** pages stop being able to pick up a mermaid patch release without a q2 upgrade. That is the intended trade (reproducible, pinned output), and it matches the exact-pin that is already in place.

## Pre-flight note (`cargo xtask verify --skip-hub-build`)

Rust legs are green: lint, fmt, build, tree-sitter, and `cargo nextest run --workspace` at **11626/11626 passed**. Two caveats, neither a real failure at HEAD and neither related to this strand:

- One verify run failed a single Rust test, and the same suite passed clean on an immediate re-run and on a standalone full-workspace run. Flaky; not identified. Not investigated further.
- The hub-client leg fails 3 `smoke-all` WASM cases (`metadata/project-profiles/index.qmd`, and two on `quarto-test/callout-title-attribute.qmd`). This is the documented stale-WASM trap: `--skip-hub-build` does not rebuild the WASM, and `hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm` is dated Aug 10 17:34 while HEAD's commits land at 19:11. The failing fixtures exercise callout-title and project-profile features, not mermaid. A full `cargo xtask verify` would rebuild and, expected, clear these — **not verified**, since it was not needed to scope this strand.
