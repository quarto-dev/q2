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

```bash
# Install dependencies (first time only)
npm install

# Development mode with HMR (not recommended)
npm run dev

# Build production bundle
npm run build
```

## Build Output

The build produces `../q2-sandboxed-preview.html` which is consumed by `Q2SandboxedPreviewIframe.tsx` in the parent hub-client project.

## Adding Dependencies

Since this is a separate project, you can freely add dependencies without affecting hub-client:

```bash
npm install <package>
```

All dependencies will be bundled into the single HTML output file.
