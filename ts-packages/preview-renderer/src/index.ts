// Public API for @quarto/preview-renderer.
//
// Two surfaces:
//   - Top-level re-exports below — for SPA consumers who want a single
//     import path for the common preview-pane components.
//   - Sub-path exports (see package.json) — for deeper imports of types/
//     utils/ framework / q2-preview internals where the barrel would
//     produce name collisions (`Diagnostic` exists in both
//     `types/diagnostic` and `types/intelligence`).

// Iframe wrappers — host the rendered q2-preview format inside a
// sandboxed iframe and morph DOM on edits.
export { Q2PreviewIframe } from './iframe/Q2PreviewIframe';
export { default as MorphIframe, type MorphIframeHandle } from './iframe/MorphIframe';
export { default as DoubleBufferedIframe, type DoubleBufferedIframeHandle } from './iframe/DoubleBufferedIframe';

// Overlays — diagnostics and static error / fallback / placeholder
// views layered over the iframe.
export { PreviewErrorOverlay } from './overlays/PreviewErrorOverlay';
export { ErrorView, FallbackView, NonQmdPlaceholderView } from './overlays/PreviewStaticInfoViews';

// q2-preview format — the React components that render Quarto's
// q2-preview AST. The full surface (Block, Inline, PreviewDocument,
// previewRegistry, PreviewContext, AssetManifestContext) is in the
// sub-barrel at `@quarto/preview-renderer/q2-preview`.
export * from './q2-preview';
