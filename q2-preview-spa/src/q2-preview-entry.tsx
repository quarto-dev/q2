// q2-preview iframe entry — re-imports the shared entry from
// `@quarto/preview-renderer`. The script tag in `q2-preview.html`
// points here; the actual postMessage protocol lives in the shared
// package (used by both hub-client and the q2-preview SPA so the
// iframe surface stays identical).
import '@quarto/preview-renderer/q2-preview/entry';
