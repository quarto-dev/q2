# `q2 preview` support for `render-components:` (GH #402 / bd-ue80chl0)

**Status:** APPROVED (2026-08-25) — executing on branch
`braid/bd-ue80chl0-preview-spa-render-components`.

### Settled decisions (review round, 2026-08-25)

- **Q1 (re-transpile cadence):** do NOT re-transpile on every
  `contentTick`. Re-transpile only when (a) the resolved
  `render-components` path list changes (as seen in the rendered AST
  meta — a stable `componentPathsKey` string, so `.qmd` keystrokes that
  don't touch the list are free), or (b) any `.tsx` file was touched
  (a dedicated `tsxTick` bumped from `onFileContent` for `.tsx` paths —
  no content hashing needed). Rationale: per-keystroke `.qmd` edits must
  not accumulate babel runs; `.tsx` touches and list edits legitimately
  invalidate the compiled components.
- **Q2 (single-file mode):** in scope for this series (Phase 3).
- **Q3 (warning lane):** merge component warnings into `render.warnings`
  — "compiling a component" is part of "rendering".
- **Q4 (hub-client cadence):** leave hub-client's paths-only re-transpile
  as is.
- **D1 confirmed:** reuse hub-client's `@babel/standalone` transpiler via
  a shared module; the SPA imports that module dynamically (lazy chunk).
  This is parity with hub-client, not new infrastructure — the dynamic
  `import()` only changes when the browser fetches/parses the chunk.

## Overview

A document with `render-components:` in its front matter loads user TSX
component overrides in hub-client, but `q2 preview` silently drops them —
the built-in components render instead, with no warning. GH #402 offers two
resolutions; the decision is to **implement the feature in the CLI preview
(resolution #1)**, accepting the SPA bundle cost.

The custom-components pipeline is a two-process design:

- **Iframe half** (shared): `ts-packages/preview-renderer/src/q2-preview/entry.tsx`
  handles `LOAD_CUSTOM_COMPONENTS`, imports compiled JS as blob ESM modules,
  and layers exports over the built-in registry via `buildCustomRegistry`.
  Fully format-agnostic and already shipped in both surfaces.
- **Parent half** (hub-client only today):
  `hub-client/src/components/render/ReactRenderer.tsx` walks
  `ast.meta['render-components']`, resolves each entry with
  `resolveComponentPath` (shared, `preview-renderer/utils/componentPath.ts`),
  transpiles with `@babel/standalone`
  (`hub-client/src/services/tsxTranspiler.ts`), and passes
  `customComponentsCode` to `Q2PreviewIframe`, which posts
  `LOAD_CUSTOM_COMPONENTS`.

The gap: `q2-preview-spa/src/PreviewApp.tsx` never builds
`customComponentsCode` — `Q2PreviewIframe` already accepts the prop; the SPA
just doesn't pass it.

### Facts established by source study (2026-08-25)

- `.tsx` files **already sync** into the preview session: hub discovery
  collects them as text files (`crates/quarto-hub/src/discovery.rs:142`),
  the `WatchFilter::PreviewBroad` watcher accepts `.tsx` edits
  (`crates/quarto-hub/src/watch.rs`), and the SPA can read their content
  synchronously via `getFileContent(path)` from `@quarto/preview-runtime`.
  No new server endpoint is needed to fetch TSX sources in project mode.
- `shouldRerenderForTextChange` in `PreviewApp.tsx` already passes `.tsx`
  edits through (non-`.qmd`/`.md` always bump `contentTick`), so the
  re-render trigger plumbing exists.
- **Single-file mode is a real gap**: `config::resolve_single_file_deps`
  (`crates/quarto-preview/src/config.rs`) resolves the include closure +
  images but does *not* read `render-components`, so
  `q2 preview deck.qmd` would not sync the TSX files at all. It already has
  the parsed `DocumentAst` in hand, so the meta is available for free.
- `@babel/standalone` (the transpiler hub-client uses): 3.0 MB minified,
  0.6 MB gzipped. The SPA `dist/` (embedded into the `q2` binary via
  `include_dir!`, with precompressed `.gz` siblings) is ~45 MB today, of
  which the WASM is 26 MB. A lazy-loaded babel chunk adds ≈3.7 MB to the
  binary embed (raw + gz sibling), ~8% growth.
- Precedent for heavy optional deps: the built-in `MermaidCodeBlock`
  dynamic-imports mermaid **from the CDN at runtime** (nothing bundled;
  diagram-free docs pay nothing; offline preview loses diagrams).
- A smoke fixture already exists with real assertions:
  `crates/quarto/tests/smoke-all/q2-preview/with-render-components/`
  (`requires_js: true`, so the CLI smoke runner skips it; it runs under
  hub-client's playwright smoke-all harness, i.e. it currently exercises
  only the hub-client parent).
- q2-preview-spa has a real-binary playwright e2e harness
  (`q2-preview-spa/e2e/`, spawns `target/debug/q2 preview`) — the right
  place for the end-to-end proof. (Note: `test:e2e` tiers are outside the
  CI merge gate today — bd-lkercidb.)

## Design decisions

### D1 — Where transpilation happens

| Option | Cost | Notes |
| --- | --- | --- |
| **A. `@babel/standalone` in the SPA, lazy-loaded (recommended)** | +≈3.7 MB binary embed; zero runtime cost for docs without `render-components` | Exact transpiler parity with hub-client (same hoisted package/version). Works offline. Laziness via dynamic `import()` of the transpiler module → Vite emits a separate chunk fetched only when the meta key is present. |
| B. Babel from CDN at runtime (mermaid precedent) | zero binary cost | Breaks offline preview for a *core-path* feature (mermaid degrades a diagram; this would drop the whole feature), and adds a supply-chain surface. Rejected. |
| C. Server-side transpile in Rust (swc/oxc) via a new `/api/preview/component` endpoint | zero SPA cost; heavy new Rust dep, slower builds | Two transpilers for one semantic contract (hub-client keeps babel) — output divergence between surfaces is exactly the class of bug #402 is about. Rejected. |
| D. Server-side via `deno` (already used for TS extension engines) | zero bundle cost | `deno` is optional-on-PATH; the feature would silently vanish without it — recreating the silent-divergence problem. Rejected. |

**Recommendation: A.** The user has explicitly accepted the size cost, and A
is the only option that keeps *one* transpiler implementation across both
surfaces and works offline.

### D2 — Code sharing between the two parents

Today the parent-half logic (meta walk + transpile) lives only in
hub-client. To prevent drift, extract it into the shared package:

1. **`@quarto/preview-renderer/utils/renderComponents.ts`** (new):
   `extractRenderComponentPaths(ast): string[]` — the
   MetaList → MetaInlines → Str walk, including hub-client's mid-typing
   guards (null bullet, empty MetaInlines). Unit-tested in
   preview-renderer. Hub-client's inline walk in `ReactRenderer.tsx`
   is replaced by a call to it.
2. **`@quarto/preview-renderer/utils/tsxTranspiler.ts`** (moved from
   `hub-client/src/services/tsxTranspiler.ts`, verbatim; adds
   `@babel/standalone` to preview-renderer's deps): sync `transpileTSX`.
   - Hub-client keeps its **static** import (unchanged sync `useMemo`).
   - The SPA imports the *module itself* dynamically
     (`await import('@quarto/preview-renderer/utils/tsxTranspiler')`), so
     babel lands in a lazy chunk of the SPA build without any changes to
     the module. Laziness comes from how the importer imports, not from
     the module.
   - Care: nothing in the iframe entry graph may import this module, or
     the iframe bundle grows. (Only parents import it — same rule as
     today.)

### D3 — SPA wiring and reactivity

New module `q2-preview-spa/src/customComponents.ts`:

```ts
buildCustomComponentsCode(
  astJson: string,
  currentFilePath: string,
  getContent: (path: string) => string | null,  // getFileContent
): Promise<Record<string, string>>   // {} when no render-components
```

- Parses meta via `extractRenderComponentPaths`; returns `{}` (and never
  loads babel) when the list is empty — the common path stays free.
- Resolves entries with the shared `resolveComponentPath` (leading `/` =
  project root; otherwise relative to the declaring document — matches
  the path-resolution contract in
  `claude-notes/designs/path-resolution-model.md`; note this feature
  resolves *document-relative*, like hub-client, since the key is
  front-matter-only).
- Missing file / transpile failure: `console.warn`/`console.error` parity
  with hub-client, plus surfaced in the diagnostics overlay (D5).

`PreviewApp.tsx` wiring:

- New state `customComponentsCode: Record<string, string>` (default `{}`).
- An effect keyed on `[state.astJson, state.activeFile, state.contentTick]`
  calls `buildCustomComponentsCode` (async, cancellation-guarded like the
  sibling effects) and stores the result. Keying on `contentTick` means a
  `.tsx` edit re-transpiles — slightly *better* than hub-client, which
  deliberately re-transpiles only when the path list changes. Cheap because
  the recompute is skipped entirely when the doc has no `render-components`.
- Pass `customComponentsCode` to `<Q2PreviewIframe>` (prop already exists;
  pass `undefined` when empty so the iframe post is skipped — preserves
  today's behavior for component-free docs).

**Iframe re-render after component (re)load** — shared fix in
`entry.tsx`: today `loadCustomComponents` rebuilds `customRegistry` but the
new registry only takes effect on the *next* `UPDATE_AST`. Cache the last
`UpdateAstPayload` at module level and re-run `updateAst(lastPayload)` after
a `LOAD_CUSTOM_COMPONENTS` completes (when a payload exists). This makes
live `.tsx` editing actually repaint in the SPA, and fixes the same latent
ordering gap for hub-client. Ordering safety: the existing
`componentsLoading` gate already serializes LOAD vs UPDATE_AST.

### D4 — Single-file mode (`q2 preview deck.qmd`)

Extend `resolve_single_file_deps` in `crates/quarto-preview/src/config.rs`:
after include expansion it already holds the parsed `DocumentAst`; read the
`render-components` meta list, resolve entries against the deck's directory
(leading `/` → the synthetic project root = deck dir), and append existing,
in-tree `.tsx` files to the *text* deps (`single_file_text_deps`), which
also enrolls them in the watcher (`single_file_deps` plumbing already
exists). Out-of-tree or missing entries are dropped, same as includes.

This can be **phase 3** (project mode ships without it), but it should land
in the same PR series — otherwise we re-create #402 one mode over.

### D5 — Error visibility

Silent divergence is the core complaint, so failures must be loud:

- Transpile error / missing component file → entry in the SPA's existing
  diagnostics overlay (warning lane), not just console. Simplest shape: the
  `buildCustomComponentsCode` helper returns
  `{ code, warnings: Diagnostic[] }` and PreviewApp merges the warnings
  into `render.warnings` for `computeOverlayInputs` — no new overlay
  surface needed.
- No warning for the happy path or for docs without the key.

### D6 — Out of scope

- `q2 render` ignoring `render-components` (native HTML render has no React
  runtime; expected, not part of #402).
- hub-client behavior changes beyond the shared-code extraction and the
  entry.tsx re-render fix (its sync `useMemo` transpile flow is untouched).
- `_extensions/**` watching (Q-B1 stands).
- CDN/offline story for mermaid — unrelated.

## Test plan (TDD — tests first in each phase)

1. **preview-renderer unit** (`utils/renderComponents.test.ts`, new):
   `extractRenderComponentPaths` — happy path, mid-typing null bullet,
   empty MetaInlines, absent key, non-list shapes. Port the transpiler's
   existing implicit coverage: `tsxTranspiler.test.ts` with a trivial TSX
   → asserts JS output contains `React.createElement` and preserves
   `export`s (mocking-free; babel is a dev dep of the package tests).
2. **preview-renderer iframe test**: `LOAD_CUSTOM_COMPONENTS` after an
   `UPDATE_AST` triggers a re-render with the new registry (the D3 entry.tsx
   fix) — RED first against current entry.tsx.
3. **SPA integration** (`q2-preview-spa/src/customComponents.integration.test.tsx`,
   vitest/jsdom, `@quarto/preview-runtime` mocked as in
   `PreviewApp.integration.test.tsx`; transpiler mocked as
   `code => 'JS:' + code` like hub-client's `ReactRenderer.integration.test.tsx`):
   - doc with `render-components` → iframe receives the transpiled map
     (assert on the posted `LOAD_CUSTOM_COMPONENTS` / captured prop);
   - doc without the key → no post, transpiler module never imported;
   - missing `.tsx` → warning surfaced in overlay inputs.
4. **Rust unit/integration** (`crates/quarto-preview/src/config.rs` tests):
   single-file deck with `render-components: [overrides.tsx]` → the `.tsx`
   appears in `single_file_text_deps`; missing file → dropped, no error.
5. **e2e** (`q2-preview-spa/e2e/render-components.spec.ts`, new): real
   `q2 preview` on a project fixture (reuse
   `crates/quarto/tests/smoke-all/q2-preview/with-render-components/`):
   assert `p.my-para` and `div.my-callout` visible, `div.callout` absent —
   the same assertions the fixture already declares for the hub-client
   smoke harness. A second test for single-file mode once D4 lands.
6. **hub-client regression**: existing `ReactRenderer.integration.test.tsx`
   and `e2e/q2-debug-render-components.spec.ts` stay green after the
   shared-code extraction (imports move; behavior identical).

## Work items

### Phase 1 — shared extraction (no behavior change)

- [x] Add `@babel/standalone` dep to `ts-packages/preview-renderer`;
      move `tsxTranspiler.ts` there; hub-client re-imports (delete its copy).
      (commit `1809256e6`)
- [x] New `utils/renderComponents.ts` + unit tests; refactor hub-client's
      `componentPathsKey` memo to use it. (commit `1809256e6`)
- [ ] Verify: hub-client `npm run build:all` + `test:ci` (deferred to the
      Phase 4 full verification); SPA-side chunk check done in Phase 2:
      iframe chunk did NOT grow (see measurement below).

### Phase 2 — SPA parent half (project mode)

- [x] Iframe re-render-after-load fix in `entry.tsx` + test (RED→GREEN):
      cached `lastAstPayload`, repaint after `LOAD_CUSTOM_COMPONENTS`;
      boot-order LOAD (no prior AST) does not render.
- [x] `q2-preview-spa/src/customComponents.ts` + unit tests (RED→GREEN):
      `extractComponentPathsKey` (stable effect key) +
      `buildCustomComponentsCode` (lazy transpiler import, warnings for
      missing file / transpile error).
- [x] Wire into `PreviewApp.tsx` (RED→GREEN, 5 integration tests):
      `tsxTick` (only `.tsx` touches re-transpile — Q1), stable
      `EMPTY_CUSTOM_COMPONENTS` identity, warnings merged into
      `render.warnings` (Q3), `customComponentsCode` prop.
- [x] e2e spec on the existing fixture; full rebuild chain
      (`npm run build:wasm` → `cargo xtask build-q2-preview-spa` →
      `cargo build --bin q2`) and run it. **End-to-end record
      (2026-08-25):** `npx playwright test render-components.spec.ts`
      drives the freshly-built `target/debug/q2 preview` against the
      `with-render-components` fixture in real Chromium; observed
      `p.my-para` and `div.my-callout` present, built-in `div.callout`
      absent, and a disk edit of `overrides.tsx` (class renamed to
      `my-para-v2`) live-repainted the preview with the new class and
      zero `p.my-para` remnants. 2 passed. Existing
      `basic-preview.spec.ts` (4 tests) still green.
- [x] Measure and record the actual dist growth. **Measured 2026-08-25:**
      babel lazy chunk `tsxTranspiler-*.js` = 2.9 MB raw / 664 KB gz;
      SPA `dist/` 45 MB → 49 MB; iframe chunk `q2-preview-*.js`
      unchanged at 1148 KB (babel did not leak into the iframe graph);
      `main-*.js` 68 KB → 72 KB (wiring + shared meta walk only).

### Phase 3 — single-file mode

- [x] Rust test for `resolve_single_file_deps` picking up
      `render-components` TSX (RED first; also a drops-test for
      missing / `../`-escaping / non-`.tsx` entries).
- [x] Implement meta read + text-dep append (GREEN; entries resolve
      deck-dir-relative, leading `/` = synthetic project root; land in
      the text-dep channel → synced as text + enrolled in the
      closure-scoped watcher via `all_files()`).
- [x] Single-file e2e proof: new `[single-file]` test in
      `render-components.spec.ts` (harness gained a `targetFile`
      option) — overrides fire under `q2 preview index.qmd` AND a disk
      `.tsx` edit live-repaints (watcher enrollment). 3/3 e2e green.

### Phase 4 — wrap-up

- [ ] End-to-end verification per CLAUDE.md: exact invocation + observed
      output snippet recorded here.
- [ ] `cargo xtask verify` (full, WASM leg affected) + `cargo xtask lint`.
- [ ] Docs: document `render-components` preview behavior under `docs/`
      if/where the feature is user-documented (check first — it may still
      be experimental/undocumented; if so, skip and note).
- [ ] Update bd-ue80chl0 (comment + close), comment on GH #402.

## Open questions for review

1. **Q1 (D3 cadence):** OK with the SPA re-transpiling on every
   `contentTick` bump for docs that carry `render-components` (i.e. any
   watched-file change, not only `.tsx` edits)? Alternative: key the
   effect on a hash of the resolved TSX contents to skip no-op recomputes.
   Babel on a few small files is fast; simplicity favored.
2. **Q2 (D4 scope):** confirm single-file mode is in scope for this series
   (recommended), vs. filing a follow-up strand.
3. **Q3 (D5 shape):** merging component warnings into `render.warnings`
   reuses the overlay verbatim but slightly blurs "render warning" vs
   "component warning". Acceptable, or do we want a distinct lane?
4. **Q4:** should hub-client also adopt content-driven re-transpile (its
   comment says paths-only was deliberate)? Default: leave hub-client
   as-is; file a follow-up if we want parity in the other direction.
