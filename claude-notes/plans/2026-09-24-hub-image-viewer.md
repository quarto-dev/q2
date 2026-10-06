# Hub-client image viewer

## Overview

Selecting an image file in the Quarto Hub project sidebar now shows the
image in the editor pane. Previously `handleSelectFile` in `Editor.tsx`
ignored every binary extension, so clicking an image did nothing.

Design:

- Image bytes live in a binary Automerge document, not in the React
  `fileContents` map (text only). `ImageViewer` reads them through
  `getBinaryFileContent` from `@quarto/preview-runtime` and shows them via
  a data URL. (A first cut used a blob URL revoked from an effect cleanup;
  under StrictMode the double-invoked effect revoked it before the `<img>`
  fetched, so freshly uploaded images showed "could not be displayed"
  until a refresh.)
- `App` wires `onBinaryContent` into a `binaryFileVersions` map
  (path -> counter) passed to `Editor`, so the viewer re-reads when the
  binary doc syncs or changes. Until then it shows "Loading image…".
- `Editor` keeps `currentFile` as the image (so the sidebar highlights it
  and the URL updates) and renders the viewer in place of *both* the
  editor and preview panes, but binds the text-oriented hooks (Automerge sync,
  presence, intelligence, replay, branches) to `textFile`, which is null
  for binaries. Monaco stays mounted but hidden.
- `isImageExtension` was added to `@quarto/quarto-automerge-schema`
  (subset of `BINARY_EXTENSIONS`; the Rust mirror in
  `crates/quarto-hub/src/resource.rs` is unchanged, so `avif` was left
  out) and re-exported from `@quarto/preview-renderer/types/project`.
  `FileSidebar` uses it instead of its private list.
- Text files that are not `.qmd`/`.md` (yml, css, json, ...) get the same
  full-width treatment: the editor pane fills the main area with
  `flex: 1 1 0%`, stays visible in every view mode (including fullscreen
  preview), and the divider and preview pane are not rendered
  (`currentFileIsPlainText` in `Editor.tsx`).
- Other binaries (pdf, fonts, media) still have no viewer; selecting them
  remains a no-op and they keep the dimmed sidebar styling.

## Checklist

- [x] `isImageExtension` in schema + re-export.
- [x] `ImageViewer.tsx` / `ImageViewer.css` (tokens only, `lint:css` clean).
- [x] `Editor.tsx`: allow image selection (click + back/forward), render
      viewer, bind hooks to `textFile`.
- [x] `App.tsx`: `binaryFileVersions` from `onBinaryContent`.
- [x] `FileSidebar.tsx`: shared helper; images no longer dimmed.
- [x] `tsc -b`, eslint on touched files, `npm run build` green.
- [x] Spec/test files removed at the user's request (prototype; verified
      in-browser by hand).
- [ ] Commit (two-commit workflow: code, then `hub-client/changelog.md`).

## End-to-end verification (2026-09-24)

Verified once in Chromium via a throwaway Playwright run (since deleted):
clicking `figs/dot.png` in the sidebar showed the image centered in the
editor pane with the status line `figs/dot.png` / `1 × 1 · 67 B`, Monaco
hidden, the sidebar row active, and the preview pane reading "This .png
image is shown in the editor pane". Switching back to `index.qmd`
restored Monaco.

## Follow-ups (not done here)

- Viewers for other binaries (pdf, media).
- `avif` support requires adding it to the Rust `BINARY_EXTENSIONS` mirror
  first.
