# quarto-hub-sandboxed-preview

A standalone Vite project that builds a self-contained, sandboxed iframe renderer for Quarto Hub.

## Purpose

This project bundles React, KaTeX, and other dependencies into a single HTML file (`q2-sandboxed-preview.html`) that can run in a sandboxed iframe **without** `allow-same-origin`. This provides better security isolation.

## Architecture

- **Separate build process**: Has its own `package.json`, `vite.config.ts`, and dependencies
- **Single-file output**: Uses `vite-plugin-singlefile` to inline all JS/CSS into one HTML file
- **Output location**: Builds to `../q2-sandboxed-preview.html` (hub-client root)
- **Sandboxed**: The output HTML runs with only `sandbox="allow-scripts"` (no `allow-same-origin`)

## Development

It is recommended to not work directly in this directory, but instead use `npm run local-prod:fresh:nginx` to test hub client with the `q2-sandboxed-preview` format.

This approach is the closest thing we have so far to simulating prod locally. The main issue
with it is that it does not provide HMR, so its harder to develop with. It would be nice to 
figure out how to get simulataneous HMR for both the sandboxed preview and the main
app in a way that we don't have to think too much about service worker registration, 
but we don't have that for now.

## Build Output

The build produces `../q2-sandboxed-preview.html` which is consumed by `Q2SandboxedPreviewIframe.tsx` in the parent hub-client project.

## Adding Dependencies

Since this is a separate project, you can freely add dependencies without affecting hub-client:

```bash
npm install <package>
```

All dependencies will be bundled into the single HTML output file.
