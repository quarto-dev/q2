/**
 * Placeholder entry point for the q2-preview SPA.
 *
 * The standalone SPA is the future host of the `q2 preview` CLI command
 * (bd-kw93). Today, its only job is to *prove the cross-package
 * boundary works*: it imports a real component from
 * `@quarto/preview-renderer` and renders something. That alone tells us
 * the workspace plumbing, the `source` exports condition, and the
 * extension resolution all line up.
 *
 * Phase A of bd-kw93 will replace this with the real wiring: samod sync
 * client, WASM init, document-doc mounting, and `<Q2PreviewIframe>`
 * driven off automerge state.
 */

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
// Import via the overlay sub-path rather than the top-level barrel.
// The barrel `@quarto/preview-renderer` re-exports the full q2-preview
// surface (which transitively pulls in `@quarto/preview-runtime` →
// `wasm-quarto-hub-client` + the `/src/wasm-js-bridge/*` glue). The SPA
// placeholder doesn't need any of that yet, and we don't yet host the
// bridge files at the SPA root. Phase A of bd-kw93 will revisit this
// when the SPA actually drives a render.
import { PreviewErrorOverlay } from '@quarto/preview-renderer/overlays/PreviewErrorOverlay';

const placeholder = {
  message:
    "q2 preview SPA — under construction. The CLI command `quarto preview` " +
    "will boot this SPA against an ephemeral local hub. Today it just " +
    "exercises the @quarto/preview-renderer boundary.",
};

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <PreviewErrorOverlay error={placeholder} visible={true} collapsed={false} />
  </StrictMode>,
);
