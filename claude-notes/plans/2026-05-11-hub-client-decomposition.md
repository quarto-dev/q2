---
date: 2026-05-11
updated: 2026-05-13
branch: beads/bd-hfjj-hub-client-decomposition-shared
beads: bd-hfjj (sub-epic of bd-kw93)
status: approved 2026-05-13; Phases 0–1 complete; Phase 2 next
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

**contexts/**
- `hub-client/src/components/ThemeContext.tsx` → `contexts/ThemeContext.tsx`

`ViewModeContext.tsx` stays in hub-client (it controls editor
layout — meaningless to the SPA).

**utils/** — the utils used by the moving components
- `hub-client/src/utils/vfsPaths.ts` (+ `.test.ts`) →
  `utils/vfsPaths.ts`
- `hub-client/src/utils/iframeLinkHandlers.ts`
  (+ `.integration.test.ts`) → `utils/iframeLinkHandlers.ts`
- `hub-client/src/utils/iframePostProcessor.ts`
  (+ `.test.ts`, `.integration.test.ts`) →
  `utils/iframePostProcessor.ts`
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
- `hub-client/src/components/render/q2-preview/assetWalker.ts` →
  `assetWalker.ts` (the *implementation*; the test moves alongside
  it here as well, since it tests the runtime function. Update the
  preview-renderer cross-ref note above.)
- `hub-client/src/services/userGrammarDiscovery.ts` →
  `userGrammar/Discovery.ts`
- `hub-client/src/services/userGrammarCache.ts` →
  `userGrammar/Cache.ts`
- `hub-client/src/services/userGrammarHighlight.ts` →
  `userGrammar/Highlight.ts`

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

- [ ] Move the five `types/` files to
      `ts-packages/preview-renderer/src/types/`.
- [ ] Move `ThemeContext.tsx` to
      `ts-packages/preview-renderer/src/contexts/`.
- [ ] Move the five `utils/` files to
      `ts-packages/preview-renderer/src/utils/`.
- [ ] Add re-exports to `ts-packages/preview-renderer/src/index.ts`.
- [ ] Update every importer in hub-client (likely ~30-50 files).
      `find hub-client/src -name '*.tsx' -o -name '*.ts' | xargs
      grep -l "from '\.\./.*types/project'"` etc., rewrite to
      `from '@quarto/preview-renderer'`.
- [ ] Audit: do any of these types/utils carry editor-specific
      fields that the preview pane never uses? If yes, split. If
      not, the import-path rewrite is the whole change.

**Acceptance:**
- `cd hub-client && npm run typecheck && npm run test:ci &&
   npm run build:all` passes.
- `cargo xtask verify --skip-rust-tests` passes.
- `npm test --workspace @quarto/preview-renderer` passes.

### Phase 3 — Move framework/

- [ ] Move `components/render/framework/` entire subtree to
      `ts-packages/preview-renderer/src/framework/`.
- [ ] Re-export through `src/index.ts`.
- [ ] Update import paths in everything that imports from
      `components/render/framework/...`. Confirm that the framework
      tests (`framework/*.test.ts`) run via
      `npm test --workspace @quarto/preview-renderer`.

**Acceptance:** same as Phase 2.

### Phase 4 — Move q2-preview/, iframe wrappers, overlays

The biggest single move (~50 files including tests). Do it in one
phase so the q2-preview registry, dispatchers, and components stay
internally consistent.

- [ ] Move `components/render/q2-preview/` (entire subtree).
- [ ] Move `Q2PreviewIframe.tsx`, `MorphIframe.tsx`,
      `DoubleBufferedIframe.tsx` to
      `preview-renderer/src/iframe/`.
- [ ] Move `PreviewErrorOverlay.tsx`,
      `PreviewStaticInfoViews.tsx` to
      `preview-renderer/src/overlays/`.
- [ ] Move colocated tests including the integration tests:
      `Q2PreviewIframe.integration.test.tsx`,
      `q2-preview.integration.test.tsx`,
      `PreviewDocument.integration.test.tsx`,
      `custom-components.integration.test.tsx`,
      `entry.integration.test.tsx`,
      `PreviewErrorOverlay.integration.test.tsx`.
- [ ] Configure `vitest.integration.config.ts` in
      preview-renderer to run these. May need jsdom setup
      borrowed from hub-client's integration config.
- [ ] Update hub-client imports: `Preview.tsx`,
      `PreviewRouter.tsx`, `ReactPreview.tsx`,
      `ReactRenderer.tsx` rewrite to import from
      `@quarto/preview-renderer`.
- [ ] **Decide where `parity.integration.test.tsx` lives.**
      Recommendation: stays in hub-client (it compares the HTML
      iframe path — owned by hub-client — against the React
      path — owned by the shared package — so it's a hub-client
      consumer-level test). Document the call.

**Acceptance:**
- Same as Phase 2.
- All q2-preview integration tests run from the new package and
  pass.
- hub-client's preview pane still renders correctly in
  `npm run dev`. Manual browser check: open a Quarto project,
  see q2-preview format render. (Per CLAUDE.md §End-to-end
  verification — record what you saw.)

### Phase 5 — Move services to preview-runtime

- [ ] Move `wasmRenderer.ts` → `preview-runtime/src/wasmRenderer.ts`.
- [ ] Move `automergeSync.ts` → `preview-runtime/src/automergeSync.ts`.
- [ ] Move `assetWalker.ts` (from `q2-preview/`) →
      `preview-runtime/src/assetWalker.ts`.
- [ ] Move the three `userGrammar*` files →
      `preview-runtime/src/userGrammar/` (renamed: `Discovery.ts`,
      `Cache.ts`, `Highlight.ts`).
- [ ] Move colocated tests.
- [ ] `preview-renderer`'s `assetWalker.ts` consumers (only the
      Q2PreviewIframe boot path) now import from
      `@quarto/preview-runtime`. This is a renderer→runtime
      dependency — declare it in `preview-renderer/package.json`'s
      `dependencies` (workspace `*`).

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

- [ ] Update every hub-client import of these services to
      `@quarto/preview-runtime`.
- [ ] Configure `preview-runtime/vitest.config.ts` (and
      `vitest.wasm.config.ts` if needed) with the WASM alias.
      Confirm WASM-using tests run.

**Acceptance:**
- Same as Phase 4, plus:
- `npm test --workspace @quarto/preview-runtime` passes,
  including any WASM-using tests.
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

## Reference

- Epic: `claude-notes/plans/2026-05-11-q2-preview-epic.md`
- Beads: `bd-hfjj` (this sub-epic), `bd-kw93` (parent), `bd-56b0`
  (related: cross-doc dep audit)
- Workspace pattern model: `ts-packages/quarto-sync-client/`
- WASM consumption model: `hub-client/vite.config.ts:38-40` (the
  alias) and `hub-client/wasm-quarto-hub-client` (the symlink)
- Build-script precedent (used by Phase A, not this sub-epic):
  `crates/quarto-trace-server/build.rs`
