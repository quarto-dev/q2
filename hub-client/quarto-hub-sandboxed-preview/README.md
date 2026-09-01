# quarto-hub-sandboxed-preview

A standalone Vite project that builds the sandboxed iframe renderer for
Quarto Hub — the full `q2-preview` renderer (via `@quarto/preview-renderer`)
packaged to run cross-origin.

## Purpose

This project bundles the real preview renderer (PreviewRoot, registry,
KaTeX, Bootstrap chrome JS) into a self-contained multi-file `dist/` that is
served from a **separate origin** (GitHub Pages in production), giving the
document content real origin isolation from the hub-client app: no cookie
access, no localStorage access, no reach into the parent DOM or the WASM VFS.
All communication with the parent is postMessage; document assets are proxied
through a service worker (see `src/serviceWorker.ts`).

Note on the sandbox attribute: the iframe is loaded with
`sandbox="allow-scripts allow-same-origin"`. `allow-same-origin` is
load-bearing — without it the frame gets an opaque origin and the service
worker cannot register. The isolation comes from the *separate origin*, not
from the sandbox attribute.

## Architecture

- **Separate build process**: own `package.json` + lockfile (deliberately NOT
  part of the root npm workspaces), own `vite.config.ts`
- **Renderer from source**: `@quarto/preview-renderer` is aliased to
  `../../ts-packages/preview-renderer/src` (its deps resolve from the
  repo-root `node_modules`, so run `npm install` at the repo root first);
  `@quarto/preview-runtime` is stubbed (`src/stubs/preview-runtime.ts`) —
  the iframe never touches WASM
- **Multi-file output**: normal Vite build with `base: './'` —
  `dist/index.html` + `dist/assets/*` + `dist/serviceWorker.js`
- **Output locations**: `dist/` (deployed to GitHub Pages by
  `.github/workflows/deploy-sandboxed-preview.yml`) and a copy at
  `../public/q2-sandboxed-preview/` (gitignored; same-origin fallback for
  hub-client dev via `VITE_Q2_SANDBOXED_PREVIEW_URL=q2-sandboxed-preview/index.html`)

## Development

It is recommended to not work directly in this directory, but instead use
`npm run local-prod:fresh:nginx` to test hub client with the
`q2-sandboxed-preview` format.

This approach is the closest thing we have so far to simulating prod locally.
The main issue with it is that it does not provide HMR, so its harder to
develop with. It would be nice to figure out how to get simultaneous HMR for
both the sandboxed preview and the main app in a way that we don't have to
think too much about service worker registration, but we don't have that for
now.

## Build Output

The build produces `dist/`, consumed by `Q2SandboxedPreviewIframe.tsx` in the
parent hub-client project (default iframe src is the GitHub Pages deployment;
see `.github/workflows/github-pages.md`).

## Adding Dependencies

Since this is a separate project, you can freely add dependencies without
affecting hub-client:

```bash
npm install <package>
```

Keep `react`/`react-dom`/`katex` versions aligned with
`@quarto/preview-renderer`'s — they are deduped to this project's copies at
build time (`resolve.dedupe` in vite.config.ts).
