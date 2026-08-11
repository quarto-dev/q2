# Mermaid runtime is imported from jsDelivr at page load, not bundled into the site (bd-mermaid-runtime-not-bundled-vxejw159)

**Date:** 2026-08-11
**Braid:** `bd-mermaid-runtime-not-bundled-vxejw159`
**Branch:** `main` @ `001cb6a5` (investigated in place; no worktree created)
**Status:** Design settled 2026-08-11 (see §Resolved decisions). Ready to implement.

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

## Implementation mechanics (verified before writing code)

- **`RenderContext.resource_resolver`** is populated for transforms (`ast_transforms.rs:195` clones it from `StageContext`), and `page_nav_render`, `navbar_render`, `sidebar_render`, `footer_render`, `link_rewrite`, and `resource_collector` already use it. The transform can compute its own page-relative URL.
- **`ResourceResolverContext::html_url_for(scope, artifact_path)`** (`resource_resolver.rs:252`) is the URL function: `Project` scope + `mermaid/mermaid.min.js` → `site_libs/mermaid/mermaid.min.js` at the site root, `../site_libs/…` one level down, `../../../site_libs/…` three down (its own tests at lines 484-541 cover exactly this).
- **Fallback when the resolver is absent** follows `collect_artifact_urls` (`apply_template.rs:376`): bare relative path with `\` → `/`.
- **`Artifact`** stores `content: Vec<u8>`; `from_string`/`from_bytes` both copy once. The vendored file is valid UTF-8, so `include_str!` is used (it also lets the version guard read the text).
- The UMD bundle ends with `globalThis["mermaid"] = …`, so a classic `<script src>` followed by bare `mermaid.initialize(...)` / `mermaid.run(...)` is the right emission shape — no module import.

### Staleness guard: one deviation from the reveal.js precedent, with reason

Reveal's `vendored_reveal_assets_match_npm_package` compares embedded bytes against `node_modules/reveal.js/dist/` because reveal has **two** live sources that must agree (vendored for render, npm for preview). Mermaid has **one** source today, so replicating that test's *form* would mean adding mermaid as a devDependency — **66 MB unpacked, 793 files** — purely to make a comparison with nothing to disagree with. That is cost without the purpose.

The guard is split so we keep the protection without the dependency:

- **Always-on:** the vendored bundle's own embedded `version:"…"` string must equal `MERMAID_VERSION`. That string occurs **exactly once** in the file, so it is an unambiguous anchor. Catches the real bug class — bumping `MERMAID_VERSION` without re-vendoring, or vice versa.
- **Always-on:** the bundle must be self-contained (no `import(`, no `chunks/`). A direct regression guard for the trap this strand uncovered: the ESM entry point is a 26 KB stub over a 13 MB chunk tree. Re-vendoring the wrong dist file fails here.
- **Conditional:** the reveal-style byte comparison against `node_modules/mermaid/dist/mermaid.min.js`, skipped when absent — exactly reveal's skip contract. Costs nothing now, and starts working automatically if `bd-1vwtdwtq` (preview path) adds mermaid to `node_modules`, which it likely will.

Mechanisms 1-4 of the reveal precedent (vendored dir, README, `include_str!`, Project-scoped artifact registration) are followed exactly, so the uniformity that motivated decision #5 holds.

## Work items

### Phase 0 — Tests (TDD: written and failing first) ✅

- [x] Unit: emitted script contains no `cdn.jsdelivr.net` (and no absolute URL at all)
- [x] Unit: emitted script references the resolver-computed `site_libs/mermaid/mermaid.min.js`
- [x] Unit: nested page gets `../site_libs/…`; 3-deep page gets `../../../…`
- [x] Unit: `register_mermaid_assets()` stores a `Project`-scoped artifact at `mermaid/mermaid.min.js`
- [x] Unit: transform registers the asset when a diagram is present
- [x] Unit: transform registers nothing when no diagram is present (and nothing for non-HTML formats)
- [x] Unit: sentinel idempotence still holds (asset registered once, script appended once)
- [x] Guard: vendored bundle's embedded version matches `MERMAID_VERSION`
- [x] Guard: vendored bundle is self-contained (no `import(`, no `chunks/`, assigns `globalThis.mermaid`)
- [x] Guard: conditional byte-compare vs `node_modules/mermaid/dist/mermaid.min.js` (skips when absent)
- [x] Integration: website render writes `_site/site_libs/mermaid/mermaid.min.js`
- [x] Integration: nested page references `../site_libs/…` and it resolves on disk
- [x] Integration: diagram-free page ships neither asset nor script
- [x] Verified the new tests fail for the right reason before implementing (5 of 6 failed; the 6th is a regression guard that passes both before and after)
- [x] **Added during Phase 2:** integration test for a **revealjs deck** (see the discovered issue below)
- [x] **Added during Phase 2:** unit test that the runtime is *not* keyed under `js:`

### Phase 1 — Vendor + register ✅

- [x] Add `resources/mermaid/mermaid.min.js` (upstream npm 11.12.0, 2,748,992 B, SHA-256 `07e37dfa…`)
- [x] Add `resources/mermaid/LICENSE` (mermaid MIT)
- [x] Add `resources/mermaid/README.md` (source, version, provenance, update procedure, and a prominent "use the right dist file" warning)
- [x] `register_mermaid_assets()` in the transform module
- [x] Call it from the transform when a diagram is found

### Phase 2 — Emit a relative script ✅

- [x] Replace the module import with classic `<script src>` + init, using the resolver URL
- [x] Preserve the `MERMAID_JS_SENTINEL` idempotence contract
- [x] Update the existing unit test that asserted the CDN URL
- [x] Update the two smoke-all fixtures (`mermaid/basic.qmd`, `mermaid/revealjs.qmd`) that asserted the CDN URL — now assert a relative `<script src>` and **forbid** `cdn.jsdelivr.net`
- [x] Full workspace suite green (11644/11644)

#### Discovered during Phase 2: the artifact key must not be `js:`

The first implementation keyed the runtime `js:mermaid:mermaid.min.js`, following the convention in `dependency.rs` and the reveal assets. The integration tests caught the consequence immediately: **two** `<script src>` tags per page. `ApplyTemplateStage` collects every `js:` artifact and emits a head `<script>` for it (`apply_template.rs:167`), so the runtime loaded twice — 5.2 MiB per page.

The fix is not simply "drop our own tag and let the template emit it", because the revealjs scaffold collects only `js:revealjs:*` (`apply_template.rs:305`) — a deck deliberately does not take the Bootstrap asset set. A `js:`-keyed runtime would therefore emit **no** tag at all in presentations, silently breaking every diagram in a deck.

So the transform keeps ownership of its own emission (which also puts the `<script src>` immediately next to the `initialize`/`run` call that depends on it), and the artifact is keyed `mermaid:runtime` to stay out of the template's collection. Writing to disk is unaffected: the flush is path-driven, not prefix-driven (`artifact_flush.rs:109`).

Both halves are now pinned by tests — `runtime_is_not_keyed_as_a_template_script` (unit) and `revealjs_deck_bundles_the_runtime` (integration). The reveal case is the one a future "tidy-up" would break while unit tests stayed green, which is why it is called out in the fixture comment too.

### Phase 3 — End-to-end verification ✅

- [x] `q2 render` a website fixture; asset written, pages reference it relatively
- [x] Loaded the rendered page in a real browser; confirmed real SVGs render
- [x] Included **three** diagram types (flowchart, gantt, class) to prove the chunk trap is gone
- [x] Confirmed **zero** external network requests
- [x] Verified the nested page's `../site_libs/…` URL resolves and shares the same file
- [x] Verified a revealjs deck and a single-doc render
- [x] Record invocation + observed output in this plan (below)

#### Observed output

Fixture: a website with a 3-diagram-type root page, a nested `docs/nested.qmd`, and a diagram-free `plain.qmd`.

```
$ q2 render .
Rendered 3 of 3 files to …/e2e/_site

$ find _site -iname '*mermaid*'
_site/site_libs/mermaid
_site/site_libs/mermaid/mermaid.min.js

$ ls _site/site_libs
bootstrap  mermaid  quarto

$ grep -rl "jsdelivr\|cdn\." _site
(none)

# root page
<script src="site_libs/mermaid/mermaid.min.js"></script>
mermaid.initialize({ startOnLoad: false });
mermaid.run({ querySelector: 'pre.mermaid' });

# nested page
<script src="../site_libs/mermaid/mermaid.min.js"></script>

# diagram-free page: 0 mermaid references
```

Served over `127.0.0.1` and loaded in Chrome. **All three diagram types drew as real SVG** — each would be a *separate lazily-loaded chunk* under the ESM build, which is the precise failure this design avoids:

```js
{"mermaidBlocks":3,"blocksWithSvg":3,
 "svgSizes":["flowchart-v2: 203x70","gantt: 799x124","class: 124x270"],
 "mermaidGlobal":"object"}
```

Full network log for the page — **every request is local, none external**:

```
GET http://127.0.0.1:8899/index.html                                [200]
GET http://127.0.0.1:8899/site_libs/bootstrap/bootstrap-icons.css   [200]
GET http://127.0.0.1:8899/site_libs/quarto/quarto-theme-*.css       [200]
GET http://127.0.0.1:8899/site_libs/quarto/bootstrap.bundle.min.js  [200]
GET http://127.0.0.1:8899/site_libs/quarto/clipboard.min.js         [200]
GET http://127.0.0.1:8899/site_libs/quarto/code-copy-init.js        [200]
GET http://127.0.0.1:8899/site_libs/mermaid/mermaid.min.js          [200]
GET http://127.0.0.1:8899/favicon.ico                               [404]
```

The only console error is the unrelated favicon 404. The nested page fetched the *same* runtime URL and got a `304`, confirming one shared copy. Screenshot of the three rendered diagrams:
`mermaid-runtime-not-bundled-investigation/e2e-three-diagram-types.png`.

Single-doc path checked separately via `examples/diagrams/01-mermaid-basic/` →
`<script src="document_files/mermaid/mermaid.min.js"></script>`, asset present, no CDN.

### Phase 4 — Docs ✅

- [x] Update module docs in `mermaid.rs` (they described the CDN design)
- [x] Update `resources/mermaid/README.md` with the "use the right dist file" warning
- [x] Update user-facing `docs/guides/authoring/diagrams.qmd` — the "How diagrams render" section documented the CDN behavior and stated *"Viewing a page with diagrams requires network access"*, which is now false for rendered output. Rewritten, with a callout noting `q2 preview` still uses a CDN (bd-1vwtdwtq) so the doc does not overclaim.
- [x] `cargo xtask verify` (full, including the WASM/hub leg) — **all steps passed**

No changelog entry: `hub-client/changelog.md` covers hub-client changes, and this change does not touch hub-client.

## Measured cost

| | |
| --- | --- |
| Vendored file | 2,748,992 B (2.62 MiB) |
| `wasm_quarto_hub_client_bg.wasm` | 38,935,233 → 41,758,814 B (**+2.82 MB, +7.3 %**) |

The WASM number is the one worth flagging, and it was **not** anticipated in the design discussion. `quarto-core` is linked into `wasm-quarto-hub-client`, so `include_str!` embeds the bundle there too. Confirmed directly rather than inferred from the size delta (the baseline WASM predated some unrelated commits): distinctive mermaid strings (`__esbuild_esm_mermaid_nm`, `Bezier curve function generator`) are present in the `.wasm`.

Today those bytes are dead weight in the client: the preview pipeline excludes the mermaid transform (`Q2_PREVIEW_TRANSFORM_EXCLUDED`) and `MermaidCodeBlock.tsx` fetches its own copy from a CDN. But this **helps** `bd-1vwtdwtq` rather than complicating it — the direction chosen there (serve the vendored bytes as a per-project HTML dependency instead of bundling mermaid into the client) can read the bytes that are *already embedded*, adding nothing further to the client. It also raises the value of `bd-43gpsd7c` (compressed embedding), which would shrink the native binary and the WASM together.

Deliberately **not** addressed here by `#[cfg(not(target_arch = "wasm32"))]`-gating the constant: that would make `bd-1vwtdwtq`'s intended design harder, and WASM-side HTML export would then silently produce CDN-dependent output. Worth revisiting only if `bd-1vwtdwtq` concludes otherwise.

## Resolved decisions (2026-08-11)

All five open questions settled with the user. The questions are kept below for the record; each is annotated with its answer.

1. **Binary size — accepted.** 2.62 MiB unconditionally in the `q2` binary is acceptable for now. Filed **`bd-43gpsd7c`** to investigate a compressed compile-time embedding macro (`include_compressed_str!`-style) so the growing vendored-asset set doesn't accumulate uncompressed in `.rodata`.
2. **Bundle by default.** No opt-in key. Matches Q1 and keeps the airgapped case working for users who never learn a key exists.
3. **Render path only; preview split out.** Filed **`bd-1vwtdwtq`**. Direction recorded there: do *not* bundle mermaid into the hub-client the way Monaco was bundled — mermaid is 2.62 MiB and only some documents use it. Serve it from the same vendored bytes as an HTML dependency attached to preview projects that actually contain diagrams.
4. **Theming/CSS parity split out.** Filed **`bd-93ensbpn`**.
5. **Follow the reveal.js precedent** for version-bump ergonomics — vendored dir + README procedure + `node_modules` drift test, no bespoke `xtask` sync step. Rationale: keep every vendored asset on one mechanism so a future improvement fixes all of them at once.

### Precedent correction carried into `bd-1vwtdwtq`

The direction for #3 was framed as "like we do for revealjs." Checked, and revealjs does not currently do this — the split is:

- **render**: vendored `resources/revealjs/` via `include_str!` → `site_libs/revealjs/` artifacts (`crates/quarto-core/src/revealjs/assemble.rs`)
- **preview**: JS from the **npm** packages bundled into the client (`@revealjs/react` 0.2.0 + `reveal.js` 6.0.0 in `hub-client/package.json` and `ts-packages/preview-renderer/package.json`); CSS imported from the **vendored** files via vite (`RevealDeck.tsx`, `reveal-reset-scope.test.ts`)

So "vendored bytes served to preview as a per-project HTML dependency" is a **new mechanism to design**, not a pattern to copy. Reveal is also a defensible thing to bundle (core to every deck) in a way mermaid is not. This is recorded in `bd-1vwtdwtq` so whoever picks it up isn't misled.

## Open design questions for the user *(answered — see §Resolved decisions)*

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
