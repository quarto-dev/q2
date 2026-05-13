---
date: 2026-05-11
updated: 2026-05-13
branch: beads/bd-hfjj-hub-client-decomposition-shared
beads: bd-hfjj (sub-epic of bd-kw93)
status: approved 2026-05-13; Phases 0–5 complete; Phase 6 next (Phase 5 ↔ Phase 4 order swapped on 2026-05-13 — see §Phase ordering note)
---

# Hub-client decomposition: shared preview-pane packages for hub-client + q2-preview-spa

## Goal

Carve the q2-preview React rendering stack out of `hub-client/` into
two new npm-workspace packages — `@quarto/preview-renderer` (pure
React: framework, q2-preview format components, iframe wrappers) and
`@quarto/preview-runtime` (WASM + automerge glue: `wasmRenderer`,
`automergeSync`, `assetWalker`, user-grammar services).

Create a new top-level `q2-preview-spa/` skeleton that imports from
those packages and produces a buildable (but non-functional) bundle.

After this sub-epic, hub-client's preview pane and the future
`q2 preview` SPA will render the q2-preview format through the **same**
React components — the `§Crate / SPA layout` invariant from the epic
([2026-05-11-q2-preview-epic.md](2026-05-11-q2-preview-epic.md))
is satisfied by construction.

This is the dependency that blocks Phase A of bd-kw93. No engine
work, no samod wiring, no `crates/quarto-preview/` here — that is
Phase A.

## Decisions resolved with user (2026-05-11)

1. **Two shared packages.** `@quarto/preview-renderer` (pure React,
   no WASM imports of its own) + `@quarto/preview-runtime` (WASM
   glue + automerge sync). Renderer can be unit-tested without
   WASM init; runtime owns the side-effecting initialisation.
2. **Top-level `q2-preview-spa/`.** Sibling to `hub-client/` and
   `trace-viewer/`. Mirrors trace-viewer's shape. Editor code
   cannot leak in by accident.
3. **MVP scope = React preview path only.** Move the framework,
   q2-preview components, `Q2PreviewIframe`, `PreviewErrorOverlay`,
   `MorphIframe`/`DoubleBufferedIframe`, and the supporting types/
   utils/contexts. **Keep in hub-client:** `Preview.tsx` (HTML
   iframe + Monaco-aware scroll/selection sync), `PreviewRouter.tsx`
   (format dispatcher), `ReactPreview.tsx`, `ReactRenderer.tsx`
   (variant dispatcher), `Q2DebugIframe.tsx`, `ReactAstSlideRenderer
   .tsx`, `RevealjsReactAstSlideRenderer.tsx`. These can move later
   as the SPA grows; they're not on the bd-hfjj critical path.
4. **WASM access via existing alias.** Keep
   `hub-client/wasm-quarto-hub-client →
   crates/wasm-quarto-hub-client/pkg` symlink as-is. Each consumer
   (hub-client, `q2-preview-spa`, the runtime package's vitest
   config) declares the same `wasm-quarto-hub-client` alias in its
   Vite/Vitest config. The runtime package's *source* never names
   the alias target — only the import string — so it stays
   bundler-agnostic.
5. **SPA = skeleton placeholder.** `q2-preview-spa/src/main.tsx`
   imports something from `@quarto/preview-renderer` (e.g.
   `<PreviewErrorOverlay>` rendered with placeholder text) and
   produces a buildable `dist/`. No samod, no WASM init, no
   automerge — those are Phase A. The skeleton's only job is to
   *prove the cross-package boundary works* by linking a second
   consumer against the new packages.

## Workspace layout (end state)

```
/q2/
├── hub-client/                        (existing — editor SPA)
│   ├── src/components/
│   │   ├── Editor.tsx                 (untouched)
│   │   ├── FileSidebar.tsx            (untouched)
│   │   ├── ...                        (auth, tabs, dialogs — untouched)
│   │   └── render/
│   │       ├── Preview.tsx            (stays; HTML iframe path)
│   │       ├── PreviewRouter.tsx      (stays; imports from shared)
│   │       ├── ReactPreview.tsx       (stays)
│   │       ├── ReactRenderer.tsx      (stays; variant dispatcher)
│   │       ├── q2-debug/              (stays)
│   │       ├── ReactAstSlideRenderer.tsx          (stays)
│   │       └── RevealjsReactAstSlideRenderer.tsx  (stays)
│   ├── src/services/
│   │   ├── authService.ts             (stays)
│   │   ├── projectStorage.ts          (stays)
│   │   ├── presenceService.ts         (stays)
│   │   └── ...                        (other editor-only services)
│   ├── wasm-quarto-hub-client/        (symlink, unchanged)
│   └── vite.config.ts                 (alias kept; updated entry list)
│
├── q2-preview-spa/                    (NEW — skeleton SPA)
│   ├── package.json                   (name: q2-preview-spa, private)
│   ├── vite.config.ts                 (alias to hub-client's WASM symlink)
│   ├── index.html
│   ├── tsconfig.json
│   └── src/
│       └── main.tsx                   (~20 lines, placeholder)
│
├── ts-packages/
│   ├── preview-renderer/              (NEW — pure React)
│   │   ├── package.json               ("@quarto/preview-renderer")
│   │   ├── tsconfig.json
│   │   ├── vitest.config.ts
│   │   ├── vitest.integration.config.ts
│   │   └── src/
│   │       ├── index.ts               (public re-exports)
│   │       ├── framework/             (Ast.tsx, dispatch, registry, types)
│   │       ├── q2-preview/            (entry, dispatchers, registry,
│   │       │                          PreviewDocument, blocks/, inlines/,
│   │       │                          custom/, contexts, utils)
│   │       ├── iframe/                (Q2PreviewIframe, MorphIframe,
│   │       │                          DoubleBufferedIframe)
│   │       ├── overlays/              (PreviewErrorOverlay,
│   │       │                          PreviewStaticInfoViews)
│   │       ├── types/                 (project, diagnostic, artifactPaths,
│   │       │                          sourceInfo, intelligence)
│   │       ├── contexts/              (ThemeContext)
│   │       └── utils/                 (vfsPaths, iframeLinkHandlers,
│   │                                  iframePostProcessor, componentPath,
│   │                                  stripAnsi)
│   │
│   ├── preview-runtime/               (NEW — WASM + automerge glue)
│   │   ├── package.json               ("@quarto/preview-runtime")
│   │   ├── tsconfig.json
│   │   ├── vitest.config.ts           (with WASM alias)
│   │   └── src/
│   │       ├── index.ts               (public re-exports)
│   │       ├── wasmRenderer.ts        (initWasm, renderToHtml,
│   │       │                          parseQmdToAst, renderPageInProject,
│   │       │                          vfsReadFile, vfsAddFile, ...)
│   │       ├── automergeSync.ts       (createSyncClient, getFileContent, ...)
│   │       ├── assetWalker.ts         (buildAssetManifest)
│   │       └── userGrammar/           (Discovery, Cache, Highlight)
│   │
│   └── (existing packages unchanged: annotated-qmd, pandoc-types,
│        quarto-automerge-schema, quarto-hub-mcp, quarto-sync-client,
│        sync-test-harness)
│
└── crates/
    ├── wasm-quarto-hub-client/        (unchanged — WASM source)
    └── (no new crates in this sub-epic)
```

## File-move catalogue

The list below is the **complete** authoritative map. If a file
isn't listed, it doesn't move. Paths are relative to repo root.

### Moving to `ts-packages/preview-renderer/src/`

**framework/**
- `hub-client/src/components/render/framework/` → `framework/`
  (entire subtree — `Ast.tsx`, `dispatch.tsx`, `RegistryContext.tsx`,
  `customNode.ts`, `meta.ts`, `plainText.ts`, `types.ts`,
  `index.ts`, plus colocated `.test.ts` files)

**q2-preview/**
- `hub-client/src/components/render/q2-preview/` → `q2-preview/`
  (entire subtree — `entry.tsx`, `dispatchers.tsx`, `registry.ts`,
  `PreviewDocument.tsx`, `AssetManifestContext.tsx`,
  `PreviewContext.tsx`, `NoteNumberingContext.tsx`,
  `quartoClasses.ts`, `theoremEnvs.ts`, `utils.tsx`,
  `blocks/**`, `inlines/**`, `custom/**`, plus colocated tests
  including `registry.test.ts`, `assetWalker.test.ts`,
  `*.integration.test.tsx`)

  Note: `assetWalker.ts` itself moves to **preview-runtime**
  (it depends on `vfsReadBinaryFile`), but its test
  (`assetWalker.test.ts`) moves with it.

**iframe/**
- `hub-client/src/components/render/Q2PreviewIframe.tsx` →
  `iframe/Q2PreviewIframe.tsx` (+ `.integration.test.tsx`)
- `hub-client/src/components/render/MorphIframe.tsx` →
  `iframe/MorphIframe.tsx`
- `hub-client/src/components/render/DoubleBufferedIframe.tsx` →
  `iframe/DoubleBufferedIframe.tsx`

**overlays/**
- `hub-client/src/components/render/PreviewErrorOverlay.tsx` →
  `overlays/PreviewErrorOverlay.tsx` (+ `.integration.test.tsx`)
- `hub-client/src/components/render/PreviewStaticInfoViews.tsx` →
  `overlays/PreviewStaticInfoViews.tsx`

**types/** — the types used by the moving components
- `hub-client/src/types/project.ts` → `types/project.ts`
- `hub-client/src/types/diagnostic.ts` → `types/diagnostic.ts`
- `hub-client/src/types/artifactPaths.ts` → `types/artifactPaths.ts`
- `hub-client/src/types/sourceInfo.ts` → `types/sourceInfo.ts`
- `hub-client/src/types/intelligence.ts` → `types/intelligence.ts`

Hub-client still needs these — it will re-import from
`@quarto/preview-renderer`. Audit during Phase 2 to confirm no
editor-only fields leak into these types; if so, split.

**contexts/** — **deferred (2026-05-13)**

`ThemeContext.tsx` was scheduled to move, but inspection during
Phase 2 surfaced two facts:

1. **No moving file uses it.** Only `App.tsx`, `ProjectSelector
   .tsx`, and `Editor.tsx` consume `ThemeProvider` / `useTheme` —
   all three stay in hub-client.
2. **It is coupled to `services/preferences/`** (localStorage-
   backed user prefs), which is editor-only.

Moving it now would either pollute preview-renderer with
localStorage I/O or break the import. Neither is desirable; we
also don't *need* it moved for Phase 4. The sound refactor —
DI'ing `getPreference`/`setPreference` through props — is a
small but real design change that has no caller yet. **Decision:
keep `ThemeContext.tsx` in hub-client; do the DI refactor when
the SPA actually needs a theme provider.** Note this as a
follow-up issue (see §Open follow-ups). The `./contexts/*`
sub-path export was removed from `preview-renderer/package.json`
accordingly.

`ViewModeContext.tsx` stays in hub-client (it controls editor
layout — meaningless to the SPA).

**utils/** — the utils used by the moving components
- `hub-client/src/utils/vfsPaths.ts` (+ `.test.ts`) →
  `utils/vfsPaths.ts`
- `hub-client/src/utils/iframeLinkHandlers.ts`
  (+ `.integration.test.ts`) → `utils/iframeLinkHandlers.ts`
- `hub-client/src/utils/iframePostProcessor.ts`
  (+ `.test.ts`, `.integration.test.ts`) →
  **deferred to Phase 5** (imports `vfsReadFile` /
  `vfsReadBinaryFile` from `services/wasmRenderer`, which itself
  moves to `@quarto/preview-runtime` in Phase 5; moving it
  together avoids either a wrong-direction
  preview-renderer→hub-client back-import or a premature
  DI refactor against iframe wrappers that themselves move in
  Phase 4)
- `hub-client/src/utils/componentPath.ts` (+ `.test.ts`) →
  `utils/componentPath.ts`
- `hub-client/src/utils/stripAnsi.ts` (+ `.test.ts`) →
  `utils/stripAnsi.ts`
- `hub-client/src/utils/customRegistry.ts` (+ `.test.ts`) →
  `utils/customRegistry.ts`
  *(Phase 0.2 drift: imported by `q2-preview/entry.tsx` (moving) and
  `q2-debug/entry.tsx` (staying). After the move, q2-debug imports
  from `@quarto/preview-renderer`.)*
- `hub-client/src/utils/atomicCustomNodes.ts` →
  `utils/atomicCustomNodes.ts`
  *(Phase 0.2 drift: imported by `framework/dispatch.tsx`.)*
- `hub-client/src/utils/sourceInfo.ts` (+ `.test.ts`) →
  `utils/sourceInfo.ts`
  *(Phase 0.2 drift: distinct from `types/sourceInfo.ts`; imported
  by `framework/dispatch.tsx`.)*

Note: `types/project.test.ts` moves with `types/project.ts`.

### Moving to `ts-packages/preview-runtime/src/`

- `hub-client/src/services/wasmRenderer.ts` → `wasmRenderer.ts`
  (+ any colocated tests)
- `hub-client/src/services/automergeSync.ts` → `automergeSync.ts`
  (+ tests)
- `hub-client/src/services/userGrammarDiscovery.ts` →
  `userGrammar/Discovery.ts`
- `hub-client/src/services/userGrammarCache.ts` →
  `userGrammar/Cache.ts`
- `hub-client/src/services/userGrammarHighlight.ts` →
  `userGrammar/Highlight.ts`

**Note (2026-05-13):** the original plan also moved
`q2-preview/assetWalker.ts` here. Re-deciding: it stays *with*
`q2-preview/` (i.e., it moves to preview-renderer in Phase 4 along
with the rest of `q2-preview/`). Rationale: assetWalker is an
AST-walker that *uses* VFS, not a VFS service itself, and moving it
into runtime would force a circular preview-runtime → preview-renderer
dependency (assetWalker imports `vfsPaths` from preview-renderer; the
plan's stated invariant is unidirectional renderer → runtime). The
plan's prior reasoning ("the test moves alongside since it tests the
runtime function") was a weak signal — the test exercises the manifest
walk, not the runtime per se.

### Staying in hub-client (explicit list, for review)

To make the boundary review-able, here are the preview-adjacent
files that explicitly **stay** in `hub-client/src/`:

- `components/render/Preview.tsx`
- `components/render/PreviewRouter.tsx`
- `components/render/ReactPreview.tsx`
- `components/render/ReactRenderer.tsx`
- `components/render/q2-debug/` (entire subtree)
- `components/render/ReactAstSlideRenderer.tsx`
- `components/render/RevealjsReactAstSlideRenderer.tsx`
- `components/render/parity.integration.test.tsx` (compares HTML
  vs React path; tests cross both packages — best kept in
  hub-client which is where both paths are reachable)
- `hooks/useScrollSync.ts`, `hooks/useSelectionSync.ts` (Monaco-
  coupled; not preview-pane-renderable concerns)
- `services/authService.ts`, `projectStorage.ts`,
  `presenceService.ts`, etc. (editor only)
- `components/ViewModeContext.tsx`
- All `components/tabs/`, `components/auth/`, `Editor.tsx`,
  `FileSidebar.tsx`, etc.

If anything below feels like it should move, raise it before
Phase 2 — the move sequence depends on this list being final.

## Workspace plumbing (mechanical, applies to both new packages)

Each new package mirrors the existing `ts-packages/quarto-sync-client/`
pattern.

**`package.json` shape:**

```jsonc
{
  "name": "@quarto/preview-renderer",   // or preview-runtime
  "version": "0.0.1",
  "private": true,
  "type": "module",
  "main": "dist/index.js",
  "types": "src/index.ts",
  "exports": {
    ".": {
      "types": "./src/index.ts",
      "source": "./src/index.ts",     // Vite picks this via the
      "import": "./dist/index.js"     // 'source' resolve.condition
    }
  },
  "files": ["dist"],
  "scripts": {
    "build": "tsc",
    "typecheck": "tsc --noEmit",
    "clean": "rm -rf dist",
    "test": "vitest run",
    "test:integration": "vitest run --config vitest.integration.config.ts",
    "test:watch": "vitest"
  },
  "dependencies": { /* see per-package below */ },
  "devDependencies": { "typescript": "~5.9.3", "vitest": "^4.0.17" }
}
```

**preview-renderer dependencies:**
- `react`, `react-dom`, `morphdom` (peer-style: declared dep so
  TypeScript can find types; hub-client and the SPA carry their
  own copies via npm hoisting)
- `@quarto/pandoc-types` (for AST type imports if used)
- `katex` (already in root devDeps; if any q2-preview block uses
  it directly add here)

**preview-runtime dependencies:**
- `@quarto/quarto-sync-client`, `@quarto/quarto-automerge-schema`
  (workspace `*`)
- `@automerge/automerge`, `@automerge/automerge-repo` (the runtime
  is what holds the automerge integration)
- `web-tree-sitter` (used by `userGrammar/Highlight`)
- React is **not** a runtime dep — the runtime is pure JS/TS, no
  components

**`tsconfig.json` shape** (cribs from `quarto-sync-client/tsconfig.json`):

```jsonc
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "lib": ["ES2022", "DOM", "DOM.Iterable"],   // renderer needs DOM; runtime needs DOM (assetWalker uses URL)
    "jsx": "react-jsx",                          // renderer only; omit for runtime
    "strict": true,
    "skipLibCheck": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "verbatimModuleSyntax": true,
    "outDir": "./dist",
    "declaration": true,
    "rootDir": "./src"
  },
  "include": ["src"],
  "exclude": [
    "src/**/*.test.ts",
    "src/**/*.test.tsx",
    "src/**/*.integration.test.ts",
    "src/**/*.integration.test.tsx"
  ]
}
```

**`vitest.config.ts` for preview-runtime** must declare the WASM
alias so unit tests for `wasmRenderer` and `automergeSync` resolve
the same way Vite does in app bundles:

```ts
import { defineConfig } from 'vitest/config'
import path from 'path'

export default defineConfig({
  resolve: {
    conditions: ['source', 'import', 'module', 'browser', 'default'],
    alias: {
      'wasm-quarto-hub-client': path.resolve(
        __dirname,
        '../../hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client.js'
      ),
    },
  },
  test: {
    environment: 'jsdom',
    // mirror hub-client/vitest.config.ts as needed
  },
})
```

**Top-level workspace `package.json`** already has
`workspaces: ["ts-packages/*", "hub-client", "trace-viewer",
"q2-demos/*"]`. Adding `q2-preview-spa` to the list is a one-line
change.

## `q2-preview-spa/` skeleton (decision 5)

Minimal, but real:

```
q2-preview-spa/
├── package.json
├── vite.config.ts
├── tsconfig.json
├── index.html
└── src/main.tsx
```

`package.json`:

```jsonc
{
  "name": "q2-preview-spa",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc -b && vite build",
    "typecheck": "tsc --noEmit"
  },
  "dependencies": {
    "@quarto/preview-renderer": "*",
    "@quarto/preview-runtime": "*",
    "react": "^19.2.0",
    "react-dom": "^19.2.0"
  },
  "devDependencies": {
    "@vitejs/plugin-react": "^5.1.1",
    "vite": "^7.2.4",
    "vite-plugin-wasm": "^3.5.0",
    "typescript": "~5.9.3"
  }
}
```

`vite.config.ts` mirrors `hub-client/vite.config.ts`'s alias and
`source` condition, with a single entry `index.html`. No proxy
configuration (Phase A adds it).

`src/main.tsx` (the placeholder — proves the import chain works):

```tsx
import { createRoot } from 'react-dom/client'
import { PreviewErrorOverlay } from '@quarto/preview-renderer'

createRoot(document.getElementById('root')!).render(
  <PreviewErrorOverlay
    title="Q2 Preview SPA"
    message="Under construction — connect via `q2 preview` (not yet shipping)."
  />
)
```

(`PreviewErrorOverlay`'s actual prop shape is to be checked when
the move lands; the placeholder content is interchangeable.)

## Phasing

Seven phases, each independently mergeable and verifiable. Per
project policy (CLAUDE.md), after each phase: `cargo xtask verify
--skip-rust-tests` from repo root, plus per-package
`npm run typecheck && npm run test` for any affected package.

The TDD policy applies as "tests-stay-green-across-move." We are
not adding behavior; we are relocating it. Each phase's invariant
is: the same set of tests passes before and after the move.

### Phase ordering note (2026-05-13)

The phases below are labeled in their *original* order. As of
2026-05-13, execution order is **0, 1, 2, 3, 5, 4, 6, 7** — Phase 5
(services to preview-runtime) is done *before* Phase 4 (q2-preview /
iframe wrappers / overlays).

Reason: most Phase-4 files import `services/wasmRenderer` or
`utils/iframePostProcessor` (which itself was deferred from Phase 2 to
Phase 5). Moving services first means Phase 4's imports already point
at `@quarto/preview-runtime` by the time we move them. The original
"renderer first" ordering was a hedge against WASM-test breakage;
"services first" turns out to be *safer* because hub-client's full
test surface keeps validating the renderer-side code during the
services move, so any regression surfaces immediately.

### Phase 0 — Pre-flight (no code changes)

- [x] Verify the starting workspace builds clean.
      `cd hub-client && npm run build:all && npm run test:ci`.
      *(2026-05-13: build green; 84/84 tests pass across
      `test`, `test:integration`, `test:wasm`.)*
- [x] Confirm the file lists in "File-move catalogue" against
      current `git ls-files`. Patch the plan with any drift.
      *(2026-05-13: added `customRegistry`, `atomicCustomNodes`,
      `utils/sourceInfo` to renderer moves; documented colocated
      tests inline; flagged q2-debug's reverse import of
      `customRegistry` after the move.)*
- [x] Decide on the test-helper / mock files under
      `src/test-utils/` and `src/__mocks__/`: which move with
      `preview-renderer`, which with `preview-runtime`, which
      stay in hub-client. Update catalogue.
      *(2026-05-13: dispositions recorded in §Test-helper
      placement below.)*

**Acceptance:** workspace builds and tests pass on the current
branch; catalogue is final. ✓

### Test-helper placement (Phase 0.3 resolution)

Cross-referencing imports across `hub-client/src` shows only three
files consume `test-utils/`:

| Helper | Consumers | Disposition |
|---|---|---|
| `test-utils/mockSyncClient.ts` | `services/automergeSync.test.ts` *(moving to preview-runtime)* | **Moves with preview-runtime** → `ts-packages/preview-runtime/src/test-utils/mockSyncClient.ts` |
| `test-utils/mockWasm.ts` | none in source tree; exported via `test-utils/index.ts` for renderer-style tests | **Moves with preview-runtime** → `ts-packages/preview-runtime/src/test-utils/mockWasm.ts` (mocks the WASM renderer the runtime owns) |
| `test-utils/visibility.ts` | `hooks/useAutomergeSync.test.ts`, `services/presenceService.test.ts` *(both stay in hub-client)* | **Stays in hub-client** |
| `test-utils/setup.ts` | `vitest.integration.config.ts` (hub-client) | **Stays in hub-client**; new packages get their own small `setup.ts` per `vitest.integration.config.ts` (pure jsdom polyfills — duplication is cheap and keeps boundaries clean). Promote to a shared `@quarto/test-setup` package only if the file grows. |
| `test-utils/index.ts` | barrel for the above | Hub-client's copy keeps `visibility.ts` exports only; preview-runtime grows its own barrel. |
| `__mocks__/userSettings.ts` | hub-client editor settings (auto-mock) | **Stays in hub-client** (editor-only). |

`components/render/experimental-components/` (`.tsx.txt` and `.jsx`
scratch files under `new/`) is untouched by the move and stays in
hub-client — those files are not imported by the build.

### Phase 1 — Empty workspace packages

- [x] Create `ts-packages/preview-renderer/` with `package.json`,
      `tsconfig.json`, `vitest.config.ts`,
      `vitest.integration.config.ts`, empty `src/index.ts`
      exporting nothing.
- [x] Create `ts-packages/preview-runtime/` with the same
      skeleton, including the WASM alias in `vitest.config.ts`
      and `vitest.integration.config.ts`.
- [x] Run `npm install` from repo root. Confirm new workspaces
      register (`npm ls @quarto/preview-renderer @quarto/preview-runtime`).
      *(2026-05-13: both registered as workspace symlinks.)*
- [x] Add a placeholder `test.ts` in each `src/` that runs a
      trivial assertion; confirm `npm test --workspace
      @quarto/preview-renderer` (and runtime) pass.
      *(2026-05-13: 1 test/package, both green; typecheck + build
      also succeed.)*
- [x] Confirm hub-client can `import '@quarto/preview-renderer'`
      and `import '@quarto/preview-runtime'`. Update
      `tsconfig.app.json` references if hub-client uses project
      references for workspace deps.
      *(2026-05-13: temporary probe imports in `main.tsx` compiled
      cleanly through `tsc --noEmit` and `vite build`. Reverted
      after verification. tsconfig.app.json needed no changes —
      hub-client doesn't use TS project references for workspace
      deps; it relies on npm's `node_modules/@quarto/...` symlinks
      + tsc's "bundler" `moduleResolution`.)*

**Acceptance:** both new packages exist, are recognized by npm
workspaces, have passing test commands, and are importable from
hub-client (even though they export nothing yet). ✓

### Phase 2 — Move types, contexts, and utils

The lowest-risk moves. Pure data + pure functions; no React tree.

- [x] Move the five `types/` files to
      `ts-packages/preview-renderer/src/types/`.
      *(2026-05-13: project + project.test, diagnostic,
      artifactPaths, sourceInfo, intelligence moved with `git mv`.)*
- [x] **Deferred** `ThemeContext.tsx` — see §contexts/ above.
      Tracked as `bd-hfjj-fu-theme`.
- [x] Move the `utils/` files to
      `ts-packages/preview-renderer/src/utils/`.
      *(2026-05-13: 7 of 8 moved — vfsPaths, iframeLinkHandlers,
      componentPath, stripAnsi, customRegistry, atomicCustomNodes,
      sourceInfo. iframePostProcessor deferred to Phase 5 because
      it imports from services/wasmRenderer.)*
- [x] Add re-exports to `ts-packages/preview-renderer/src/index.ts`.
      *(2026-05-13: minimal — only a design comment for now.
      Sub-path exports in package.json handle the Phase-2 surface;
      barrel exports grow with Phases 3–4.)*
- [x] Update every importer in hub-client.
      *(2026-05-13: 64 imports across 44 files rewritten to
      `@quarto/preview-renderer/types/<m>` and `/utils/<m>` via
      `/tmp/rewrite-imports.py`. Required adding a
      `@quarto/preview-renderer` alias to hub-client's three
      vitest configs — vitest's exports resolution doesn't honor
      the `source` condition the way Vite's prod build does, so
      we follow the existing sync-client/automerge-schema
      alias convention.)*
- [x] Audit types/utils for editor-only fields.
      *(2026-05-13: no splits needed. `project.ts` mentions
      "hub-client" in a docstring but the IndexedDB-stored
      `ProjectEntry` type is generic enough that the SPA can
      consume or ignore it.)*

**Acceptance:**
- `cd hub-client && npm run typecheck && npm run test:ci &&
   npm run build:all` passes. ✓
- `cargo xtask verify --skip-rust-tests` passes. ✓
- `npm test --workspace @quarto/preview-renderer` passes. ✓
  (81 unit + 7 integration tests including the moved suites.)

### Phase 3 — Move framework/

- [x] Move `components/render/framework/` entire subtree to
      `ts-packages/preview-renderer/src/framework/`.
      *(2026-05-13.)*
- [x] Expose as a single barrel via `package.json` exports
      `./framework`. Wildcards rejected because the subtree mixes
      `.tsx` (Ast, dispatch, RegistryContext) and `.ts` (the rest)
      and Node's exports map can't pattern-match both extensions
      cleanly. Every framework symbol re-exports through
      `framework/index.ts`, so sub-file imports are not needed.
- [x] Update import paths in everything that imports from
      `components/render/framework/...`.
      *(2026-05-13: 107 imports across 60 hub-client files
      rewritten to `from '@quarto/preview-renderer/framework'`.
      Also caught one dynamic `await import('./framework')` in
      `parity.integration.test.tsx`.)*
- [x] Convert framework's internal cross-dir imports (Phase 2
      Phase-2-style `@quarto/preview-renderer/...` self-imports
      from Ast/RegistryContext/dispatch) to relative paths
      (`../types/sourceInfo`, `../utils/sourceInfo`, etc.).
      Self-package imports work but are unidiomatic.
- [x] Framework tests run via
      `npm test --workspace @quarto/preview-renderer`.
      *(2026-05-13: 133 tests / 10 files including the moved
      `customNode.test.ts`, `meta.test.ts`, `plainText.test.ts`.)*

**Acceptance:** same as Phase 2. ✓
- hub-client `typecheck`, `test:ci`, `build:all` all green.
- `cargo xtask verify --skip-rust-tests` green.

### Phase 4 — Move q2-preview/, iframe wrappers, overlays

*(Executed after Phase 5 — see §Phase ordering note.)*

The biggest single move (~50 files including tests). Done in one
phase so the q2-preview registry, dispatchers, and components stay
internally consistent.

- [x] Move `components/render/q2-preview/` (entire subtree, 50+
      files) → `preview-renderer/src/q2-preview/`.
- [x] Move `Q2PreviewIframe.tsx` (out of `q2-preview/`),
      `MorphIframe.tsx`, `DoubleBufferedIframe.tsx` →
      `preview-renderer/src/iframe/`. The q2-preview barrel
      re-exports `Q2PreviewIframe` for back-compat.
- [x] Move `PreviewErrorOverlay.tsx`,
      `PreviewStaticInfoViews.tsx` → `preview-renderer/src/overlays/`.
- [x] Move colocated tests including the integration tests:
      `Q2PreviewIframe.integration.test.tsx`,
      `q2-preview.integration.test.tsx`,
      `PreviewDocument.integration.test.tsx`,
      `custom-components.integration.test.tsx`,
      `entry.integration.test.tsx`,
      `PreviewErrorOverlay.integration.test.tsx`,
      `custom/PreviewTitleBlock.integration.test.tsx`.
- [x] **DI refactor `PreviewErrorOverlay`.** Replaced
      `usePreference('errorOverlayCollapsed')` with optional
      `collapsed` + `onToggleCollapsed` props. Uncontrolled
      fallback uses `useState(true)`. Hub-client's two call sites
      (`ReactPreview.tsx`, `Preview.tsx`) wrap with
      `usePreference`. The SPA can pass any state or omit.
      *(2026-05-13: integration tests rewritten to pass
      `collapsed={false}` for expanded-mode assertions.)*
- [x] Convert self-package imports in the moved subtree to
      relative paths via `/tmp/relativize-self-imports.py`
      (100 rewrites across 55 files). Pattern matches
      `@quarto/preview-renderer/<X>` → depth-aware relative.
- [x] Wire `preview-renderer/package.json` exports map with new
      sub-paths: `./q2-preview` (barrel), `./q2-preview/entry`
      (specific — needed for hub-client's stub re-import; see
      below), `./iframe/*` (wildcard, `.tsx`), `./overlays/*`
      (wildcard, `.tsx`). Top-level `src/index.ts` grows a
      proper public-API barrel:
      `Q2PreviewIframe`, `MorphIframe`/`DoubleBufferedIframe`
      (+ Handle types), `PreviewErrorOverlay`, `ErrorView` /
      `FallbackView` / `NonQmdPlaceholderView`, plus re-exports
      from the q2-preview sub-barrel (Block, Inline,
      PreviewDocument, previewRegistry, PreviewContext,
      AssetManifestContext, buildAssetManifest).
- [x] Update hub-client imports of moved files via
      `/tmp/rewrite-phase4-imports.py` (9 rewrites across 6 files).
      Manually caught two extra patterns the regex missed:
      `import type { ... } from '../components/render/<Iframe>'`
      (had a `components/render/` segment) and the
      `vi.mock('./q2-preview/Q2PreviewIframe', ...)` in
      `ReactRenderer.integration.test.tsx`.
- [x] **Stub file for the iframe HTML entry.** `hub-client/q2-preview.html`
      contains `<script type="module" src="/src/components/render/q2-preview/entry.tsx">`,
      which Vite resolves relative to hub-client's project root.
      The real entry is now under `@quarto/preview-renderer`;
      we keep the original path stable by recreating a one-line
      stub at `hub-client/src/components/render/q2-preview/entry.tsx`
      that simply re-imports from the workspace package. The
      `parity.integration.test.tsx`'s dynamic `import('./q2-preview/entry')`
      also goes through this stub.
- [x] `parity.integration.test.tsx` **stays in hub-client** —
      compares the HTML iframe path (`Preview.tsx` — hub-client)
      against the React path (`@quarto/preview-renderer` —
      moved). Hub-client is the only place that reaches both.
- [x] Test-config plumbing in preview-renderer's
      `vitest.integration.config.ts`: add aliases for
      `@quarto/quarto-sync-client`, `@quarto/preview-runtime`,
      `wasm-quarto-hub-client` (points at hub-client's symlink so
      the JS shim loads; tests don't invoke WASM), and
      `/src/wasm-js-bridge` (so the lazy
      `import('/src/wasm-js-bridge/sass.js')` in `wasmRenderer.ts`
      resolves at transform time).

**Acceptance:**
- Same as Phase 2.
- All q2-preview integration tests run from the new package and
  pass.
  *(2026-05-13: preview-renderer integration suite 129 tests / 9
  files green; unit suite 156 tests / 13 files green.)*
- hub-client `test:ci`, `build:all` clean; preview-runtime tests
  unchanged at 60/6. `cargo xtask verify --skip-rust-tests`
  green.

  **End-to-end UI check:** not run in this session — the worktree
  is on a headless dev environment. The plan's acceptance asks
  for a `npm run dev` browser smoke; deferring that for a session
  where a browser is available. Per CLAUDE.md §End-to-end
  verification: tests pass, the real render path was not
  exercised in a browser this session.

### Phase 5 — Move services to preview-runtime

*(Executed before Phase 4 — see §Phase ordering note.)*

- [x] Move `wasmRenderer.ts` (+ test) → `preview-runtime/src/`.
- [x] Move `automergeSync.ts` (+ test) → `preview-runtime/src/`.
- [x] ~~Move `assetWalker.ts` (from `q2-preview/`) →
      `preview-runtime/src/assetWalker.ts`.~~
      *Re-decided 2026-05-13 (see §"Moving to preview-runtime"
      note): assetWalker stays with `q2-preview/` and moves in
      Phase 4. Rationale there.*
- [x] Move `iframePostProcessor.ts` (+ `.test.ts`,
      `.integration.test.ts`) from hub-client to
      `preview-renderer/src/utils/` — deferred from Phase 2
      because it imports `vfsReadFile`/`vfsReadBinaryFile` from
      `wasmRenderer`. After this phase the import resolves via
      `@quarto/preview-runtime`.
- [x] Move the three `userGrammar*` files →
      `preview-runtime/src/userGrammar/` (renamed: `Discovery.ts`,
      `Cache.ts`, `Highlight.ts`).
- [x] Move colocated tests (Discovery.test, Cache.test,
      Highlight.wasm.test). Updated `Highlight.wasm.test.ts`'s
      `repoRoot` computation from `../../..` (relative to
      `hub-client/src/services/`) to `../../../..` (relative to
      `ts-packages/preview-runtime/src/userGrammar/`).
- [x] `iframePostProcessor`'s consumers (now in preview-renderer)
      import from `@quarto/preview-runtime`. This is a
      renderer→runtime dependency — declared in
      `preview-renderer/package.json`'s `dependencies` (workspace `*`).

      Tradeoff to note: this means preview-renderer is no longer
      "pure React with no WASM transitive." It pulls in
      preview-runtime, which pulls in the WASM module. That's
      acceptable because (a) the WASM module is lazy-loaded at
      runtime via `initWasm()`, (b) Vite can tree-shake unused
      runtime exports for SPA consumers that don't call them.
      The "renderer is purely React" framing in Decision 1 was
      aspirational; the practical split is "renderer = React
      components that drive a render, runtime = the things they
      call out to." That's still useful as a split.

- [x] Update every hub-client import of these services to
      `@quarto/preview-runtime`.
      *(38 imports across 31 files via `/tmp/rewrite-phase5-imports.py`,
      including short-form intra-`services/` imports — the script ran in
      two passes. Caught the unusual cases manually: `vi.mock('./...')`
      and inline `import('...').T` type imports.)*
- [x] Configure `preview-runtime/vitest.config.ts` (and
      `vitest.integration.config.ts`) with the WASM alias plus
      workspace-package aliases (mirrors hub-client's pattern).
- [x] Set up the type plumbing so both tsc (per-package build) and
      hub-client's transitive compilation see ambient module
      declarations: `vite-shims.d.ts` for the `*.wasm?url` and
      `/src/wasm-js-bridge/*.js` paths, plus `wasm-quarto-hub-client.d.ts`
      copied alongside it. Pulled in by triple-slash references at the
      top of `preview-runtime/src/index.ts`.
- [x] Update hub-client's three vitest configs to alias
      `@quarto/preview-runtime` to `ts-packages/preview-runtime/src`
      (Vite resolves through the `source` condition; vitest needs the
      explicit alias on fresh clones).

**Acceptance:**
- Same as Phase 4, plus:
- `npm test --workspace @quarto/preview-runtime` passes,
  including any WASM-using tests. ✓ (60 tests / 6 files)
- hub-client `test`, `test:integration`, `test:wasm`, `build:all`
  all green. ✓
- `cargo xtask verify --skip-rust-tests` green. ✓
- preview-renderer tests still pass (133 unit + 8 integration). ✓
- Both packages build via `tsc`. ✓
- hub-client still builds and tests cleanly.
- Manual: `npm run dev` in hub-client → preview pane renders →
  WASM init happens → q2-preview format displays.

### Phase 6 — Create `q2-preview-spa/` skeleton

- [ ] Create the directory + files per "skeleton" section above.
- [ ] Add `"q2-preview-spa"` to root `package.json` `workspaces`.
- [ ] `npm install` from root; confirm vite picks up the new
      workspace.
- [ ] `cd q2-preview-spa && npm run build`. Confirm `dist/`
      contains `index.html` and a bundled JS file. Open it
      in a browser; the placeholder text renders.
- [ ] Inspect the built bundle: `du -sh dist/` and `ls
      dist/assets/`. Confirm that none of hub-client's editor
      code (e.g. no Monaco, no `Editor`, no `FileSidebar`) is in
      the bundle. This validates the §invariant the easy way:
      because the SPA imports only from shared packages, editor
      code *cannot* be transitively pulled in.

**Acceptance:**
- `q2-preview-spa/dist/index.html` builds and renders in a
  browser.
- Bundle does not contain editor-only code (Monaco, Editor,
  auth, sidebar). Record `du`/`grep` evidence in the commit
  message.

### Phase 7 — `cargo xtask verify` integration, cleanup, docs

- [ ] Extend `cargo xtask verify` so it also runs `npm run
      typecheck && npm test` for the two new packages. Today it
      does `cd hub-client && npm run test:ci`; the simplest
      extension is to also run `npm test --workspaces
      --if-present` from repo root, which picks up the new
      packages automatically.
- [ ] Add a `cd q2-preview-spa && npm run build` step to
      `cargo xtask verify` (or to a follow-on `build-preview`
      task — bd-kw93 Phase A will introduce the cargo-xtask
      command formally; for now just ensure the SPA build runs
      in CI).
- [ ] Delete files left behind in hub-client. Audit
      `hub-client/src/` for any orphaned imports or empty dirs.
- [ ] Update `hub-client/changelog.md` per the project's
      hub-client commit convention.
- [ ] Optional: update `CLAUDE.md`'s "Workspace structure"
      section to reflect the two new packages and the SPA.

**Acceptance:**
- `cargo xtask verify` succeeds from a fresh clone (modulo the
  `npm install` step from `.claude/rules/worktrees.md`).
- Hub-client builds, tests, and runs the preview pane.
- `q2-preview-spa` builds and the placeholder renders.
- All paths reviewed.

## Invariant enforcement

The epic's §Crate / SPA layout invariant says:

> The components that render the preview pane inside hub-client
> and the components in the preview SPA must be the *same* React
> components — same source files, same imports, same tests.

After this sub-epic the invariant is enforced **at the
`Q2PreviewIframe + framework + q2-preview registry` layer**, which
is the layer that turns an AST into rendered DOM. Both surfaces
go through the same code to:

- dispatch on AST node types (`framework/dispatch.tsx`),
- render each block / inline / custom node
  (`q2-preview/blocks/`, `inlines/`, `custom/`),
- mount and morph the iframe (`iframe/Q2PreviewIframe.tsx`,
  `MorphIframe.tsx`),
- surface render errors (`overlays/PreviewErrorOverlay.tsx`).

The *shells* that wrap that core (hub-client's `PreviewRouter` +
`ReactRenderer` + scroll-sync hooks; the SPA's `main.tsx`) are
allowed to differ — they carry surface-specific concerns
(editor coupling vs none). The invariant is about content
rendering, not chrome.

If someone later adds a new block to the q2-preview format, they
have only one place to add it (`preview-renderer/src/q2-preview/
blocks/`), and both surfaces pick it up for free. If they want to
add a new variant *router* (e.g., a new format like q2-poster),
that's a hub-client concern unless and until the SPA needs it
too — at which point the variant moves up into the shared
package.

This is "feature parity by construction" in the sense the epic
described.

## Risks

1. **`parity.integration.test.tsx` ownership.** This test compares
   the HTML iframe path (`Preview.tsx` — staying in hub-client)
   against the React path (moving to shared). It can only live in
   a place that has both paths reachable. Plan: keep it in
   hub-client. Note this in the test's header comment.

2. **WASM test infrastructure in `preview-runtime`.** The
   `wasmRenderer` tests are currently colocated in hub-client and
   use hub-client's vitest configs (`vitest.config.ts` +
   `vitest.wasm.config.ts`). Moving them requires re-creating
   that WASM-init plumbing in the new package. Potential gotchas:
   - WASM module path resolution under jsdom (Vitest needs the
     alias and the `wasm()` plugin).
   - The shared `__mocks__/` and `test-utils/` directories
     contain helpers (`fake-indexeddb` shim, jsdom-wasm setup)
     that may need to be moved or duplicated.

   Mitigation: do Phase 5 last so the renderer is stable first;
   if WASM tests blow up, hub-client still has its preview pane
   working (because the runtime is still consumed via the new
   package and Vite's app-level alias works).

3. **Cross-package circular imports.** The proposed dep is
   `preview-renderer → preview-runtime` (for `vfsReadBinaryFile`
   used by `assetWalker`). One-way. If a future addition
   introduces `preview-runtime → preview-renderer` (e.g., the
   runtime wanting a React error type from the renderer), that's
   a circular dep. **Rule:** keep runtime React-free. If a type
   needs to be shared in both directions, it belongs in a third
   tiny package or in the renderer's `types/`.

4. **Tests-stay-green is a strong invariant.** The
   tree-shake-based bundling can occasionally hide a missing
   re-export until production build. Each phase's acceptance
   includes `npm run build:all`, not just typecheck — the prod
   build uses project references in `tsc -b` and is stricter,
   matching the CLAUDE.md note about hub-client builds.

5. **`hub-client/changelog.md` update cadence.** The CLAUDE.md
   policy is "two-commit workflow" — change, then changelog
   referencing the change's hash. Each phase here that touches
   hub-client triggers that. Easy to forget when phases are
   rapid-fire. Mitigation: include "update changelog.md" as the
   final checklist item in each hub-client-touching phase.

6. **Surprise editor-only fields in shared types.** `project.ts`,
   `diagnostic.ts`, etc. were authored for hub-client's full UI.
   Some fields may be editor-flavored (e.g. tab state, presence
   metadata). Audit during Phase 2; split or rename if needed.

7. **Discovery during the move.** It is likely that one or two
   files don't fit cleanly on either side and want a small refactor
   (split a function, lift a helper). When this happens: file a
   `discovered-from:bd-hfjj` beads issue, do the smallest split
   necessary to unblock the move, and continue. Do not absorb
   open-ended refactor into this sub-epic.

## Out of scope

- Anything in `crates/quarto-preview/` (Phase A of bd-kw93).
- The `build.rs` placeholder pattern (Phase A).
- The `__QUARTO_PREVIEW__` build-time flag (the epic v3 proposed
  this for a single-vite-entry approach; we don't need it because
  the SPA is its own workspace package).
- Moving `Preview.tsx` (HTML iframe path) to shared. It depends
  on Monaco-aware scroll sync. If the future SPA needs an HTML
  iframe path, that's a follow-up sub-epic.
- Moving slides (`ReactAstSlideRenderer.tsx`,
  `RevealjsReactAstSlideRenderer.tsx`) to shared. Same reasoning.
- Moving q2-debug to shared. Same.
- Decomposing `hub-client/components/render/` further (e.g. into
  per-format packages). The current split is "shared = q2-preview
  format only"; that's the load-bearing one.
- npm publish for the new packages. They are workspace-internal
  (`"private": true`).

## Open follow-ups (to file as related beads issues after plan approval)

1. **(`bd-hfjj-fu1` etc.)** Move slides + q2-debug to shared
   packages, so the SPA can render those formats too. Probably a
   single follow-up sub-epic.
2. **Refactor `useScrollSync` / `useSelectionSync`** to be
   Monaco-optional, so an SPA-flavored scroll sync becomes
   possible. Needed if `q2 preview` ever shows a side-by-side
   editor.
3. **Audit shared types** for editor-only fields and split as
   needed. May surface during Phase 2's audit step.
4. **(`bd-hfjj-fu-theme`)** Move `ThemeContext.tsx` to
   preview-renderer with DI'd preferences. Currently deferred
   because (a) no rendering-side component uses it and (b) the
   current implementation hard-codes localStorage-backed
   `services/preferences/`. Right shape: `ThemeProvider` takes
   `getColorScheme`/`setColorScheme` as props, the SPA passes
   no-op or sessionStorage variants. File when the SPA actually
   needs theme switching.

## Reference

- Epic: `claude-notes/plans/2026-05-11-q2-preview-epic.md`
- Beads: `bd-hfjj` (this sub-epic), `bd-kw93` (parent), `bd-56b0`
  (related: cross-doc dep audit)
- Workspace pattern model: `ts-packages/quarto-sync-client/`
- WASM consumption model: `hub-client/vite.config.ts:38-40` (the
  alias) and `hub-client/wasm-quarto-hub-client` (the symlink)
- Build-script precedent (used by Phase A, not this sub-epic):
  `crates/quarto-trace-server/build.rs`
