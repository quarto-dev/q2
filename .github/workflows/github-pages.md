# GitHub Pages deployment

`deploy-sandboxed-preview.yml` builds `hub-client/quarto-hub-sandboxed-preview/`
(`npm ci` + `npm run build`) and publishes its `dist/` — a self-contained
`index.html` plus `serviceWorker.js` — to **https://quarto-dev.github.io/q2/**,
where hub-client's `Q2SandboxedPreviewIframe.tsx` loads it cross-origin as its
iframe `src` (override with `VITE_Q2_SANDBOXED_PREVIEW_URL`). It runs on any
push to `main` touching that package, or manually via
`gh workflow run deploy-sandboxed-preview.yml`; Pages is enabled on this repo
in "GitHub Actions" mode, and the `/q2/` path is fixed by GitHub (project
Pages sites always live at `https://<org>.github.io/<repo>/`). The page is
inert when visited directly — it only resolves content via `postMessage` to a
parent frame.
