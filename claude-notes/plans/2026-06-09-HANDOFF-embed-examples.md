# Handoff — embed-example iframes + cross-referenceable Demos + preview

**Date:** 2026-06-09
**Branch:** `beads/bd-z1smhvuo-embed-example-iframe` (9 commits ahead of `main`,
**nothing pushed** — per project policy, do not push without explicit OK).
**Working tree:** clean.

Read this first, then the three plan files below. You can start cold from here.

## TL;DR — where we are

Three features shipped and committed this session; one is half-done and is the
**next task**:

1. ✅ **Embed feature** (bd-z1smhvuo) — `::: {.embed-example-iframe file="…"}` →
   live `<iframe>`. Sugar+render split, static-asset-only contract
   (Q-5-4/Q-5-5), page-relative `src`, `cargo xtask stage-doc-examples` staging,
   `resources: [examples]` in `docs/_quarto.yml`, 8 docs placeholders migrated.
   **Browser-verified in `q2 render`.**
2. ✅ **Cross-referenceable Demo blocks** (bd-t3cert81, **strand closed**) —
   `::: {.embed-example-iframe #demo-foo file="…"}` is auto-numbered "Demo N" and
   `@demo-foo` resolves to a "Demo N" xref, through the normal crossref
   machinery. **Verified in HTML + revealjs + the live docs site.**
3. ⏳ **Preview resolution** (bd-kjrpya2d) — make the embed iframes load under
   `q2 preview` / hub-client. **Part 1 done** (TS post-processor fallback).
   **Part 2 is THE NEXT TASK** (see below).

## THE NEXT TASK — bd-kjrpya2d part 2

**Goal:** make `.embed-example-iframe` decks actually render in `q2 preview docs/`
(and in real hub-client/Automerge projects). Part 1 already inlines an
artifact-rooted iframe from the VFS via `srcdoc` *with a source-path fallback*
(`ts-packages/preview-renderer/src/utils/iframePostProcessor.ts`,
`readArtifactOrSource`). But the deck **isn't in the preview VFS**, so the
fallback reads nothing.

**Scope (decided with user):** sync the `.html` files **visible via
`resources:`** (resources-scoped — NOT all project `.html`). Trust-boundary
hardening is split into **bd-teh4hbli** (don't reuse the publish-control
`resources:` as an upload-control; this resources-scoped sync is the interim it
tightens).

**Integration points (already traced):**
1. `crates/quarto-hub/src/discovery.rs` → `ProjectFiles::discover` classifies
   qmd/config/binary/extension/`.tsx`. `.html` is **not** a binary extension, so
   it falls through and is never discovered/synced. **Add a resources-scoped
   `.html` category**: resolve the project `resources:` (via
   `quarto-core::project_resources` + `ProjectConfig`) and include matched
   `.html`. (Note: `is_preview_relevant` in `watch.rs:207` /
   `WatchFilter::PreviewBroad` is a *separate* event gate — `discover` is the
   load path.)
2. Thread the new set through the hub **sync → VFS at the source path** (binary
   resources already flow this way — mirror `binary_files`).
3. **SPA population**: `vfsAddFile` them on the TS side (hub-client).

**Suggested first slice (verifiable, like part 1 was):** the Rust discovery
change in `discovery.rs` with unit tests (a `resources:`-matched `.html` is
discovered; a non-resources `.html` is not). Then the sync + SPA wiring.

**End-to-end verification (required before "done"):** full rebuild —
`cd hub-client && npm run build:wasm` → `cargo xtask build-q2-preview-spa` →
`cargo build --bin q2` — then `q2 preview docs/`, open
`http://127.0.0.1:<port>/?page=presentations/revealjs/index.qmd`, and confirm the
`#demo-fragments` iframe **inlines the real deck** (it's nested: descend into the
SPA preview `<iframe>`, then the `.embed-example` iframe; check `srcdoc` is set
and the inner doc has `.reveal .slides section`). Crossref ("Demo 1" caption +
`@demo` xref) **already works in preview** — verified.

## Plan files (authoritative detail)

- `claude-notes/plans/2026-06-09-website-example-iframe-embed.md` — embed feature
  (Phases 1–2 done; preview row points at bd-kjrpya2d).
- `claude-notes/plans/2026-06-09-crossreferenceable-examples.md` — Demo crossref
  (all phases done).
- `claude-notes/plans/2026-06-09-preview-embed-vfs-resolution.md` — **the active
  one**: part 1 done, part 2 scoped with the integration points above.

## Open strands

| Strand | What | Status |
|---|---|---|
| **bd-kjrpya2d** | Preview embed iframe via VFS source-path fallback | **active — part 2 next** |
| bd-teh4hbli | Security: decouple sync-upload from `resources:` | filed (relates; tighten after part 2) |
| bd-cic0dfdp | Lua API for `resolve_static_resource_href` | filed |
| bd-q3bxnq2e | Perf: WASM re-flushes ALL artifacts to VFS per render (`lib.rs:1417`) | filed (p1) |

## Key decisions / gotchas (so you don't relearn them)

- **Option B, not A.** We do NOT copy resources into the VFS artifact tree per
  render (A) — that would re-duplicate ~6 MB of decks into the VFS on *every*
  render (the artifact flush at `wasm-quarto-hub-client/src/lib.rs:1417` is
  per-render); in hub-client that tree is in the Automerge doc. B reads source
  bytes on demand. The CLI-only disk approach (a `vfs_root`-mode resolver branch
  + a `quarto-preview` disk route) was prototyped, browser-verified, then
  **reverted** — it can't serve a diskless hub-client project. `page_url_for`
  stays uniform; `resolve_static_resource_href` has no vfs special-case.
- **`exm` is taken** (theorem-like "Example", `registry.rs`). The embed crossref
  prefix is **`demo`** / kind **"Demo"** (`crossref/registry.rs`), deliberately
  distinct.
- **Fenced-div attribute order:** the qmd parser wants `#id` **first** —
  `::: {#demo-foo .embed-example-iframe file="…"}` (class-first errors).
- **Decks are self-contained** (slides.html embeds its assets; `slides_files/` is
  vestigial for these). So part-2 sync likely needs only the `.html`, not
  `slides_files/`.
- **Staging:** `cargo xtask stage-doc-examples` renders `examples/manifest.yml`
  projects → `docs/examples/<entry>/` (gitignored; regenerate, never commit).
  Needed before `q2 render docs/` or any preview check shows live iframes.
- **Verify gate:** `cargo xtask verify --skip-hub-build` (matches CI `-D
  warnings`) for Rust-only; full `cargo xtask verify` when the WASM leg is
  affected. quarto-core suite is ~2250+ tests.

## Commands you'll want

```bash
# Rust tests for the embed/crossref work
cargo nextest run -p quarto-core -E 'test(example_embed) | test(crossref_fixtures) | test(navigation_href)'
# Preview-renderer TS tests (part 1)
cd ts-packages/preview-renderer && npx vitest run src/utils/iframePostProcessor.embed.test.ts
# Stage example decks (gitignored output)
cargo xtask stage-doc-examples
# Full preview rebuild chain (needed to see Rust/WASM changes in q2 preview)
cd hub-client && npm run build:wasm && cd .. && cargo xtask build-q2-preview-spa && cargo build --bin q2
# Run the docs preview
cargo run --bin q2 -- preview docs/ --port 7656
```

## Commit ledger (this branch vs main)

```
76f64969 docs(plan): part-2 scope + discovery integration point; security strand
bce5975e docs(plan): option B part 1 done; part 2 scoped
bfff2d3e feat(preview): inline embedded-resource iframes from VFS source path (part 1)
39cb994a docs(plan): preview embed iframe via VFS source-path fallback
8a1ca4d5 feat(embed): cross-referenceable Demo blocks (@demo-… → "Demo N")
d4540d37 docs(plan): design for cross-referenceable Example blocks
867aa7c1 feat(embed): stage example output + page-relative iframe src + migrate docs
74c945b4 docs(plan): record Phase 1 done + Phase 2 design
de20ca96 feat(embed): ExampleEmbedTransform — .embed-example-iframe → live iframe
```
