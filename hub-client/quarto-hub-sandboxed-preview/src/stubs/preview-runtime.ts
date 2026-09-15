/**
 * Build-time stub for `@quarto/preview-runtime`.
 *
 * The sandboxed iframe bundle imports `@quarto/preview-renderer`'s
 * q2-preview barrel, which re-exports the parent-side `Q2PreviewIframe`
 * and `assetWalker` — both of which import `@quarto/preview-runtime`
 * (and, transitively, the WASM module). The iframe never executes those
 * code paths: the parent owns all WASM/VFS access, and assets reach the
 * iframe through the service-worker proxy. This stub satisfies module
 * resolution without pulling any of that machinery into the bundle.
 *
 * If one of these ever actually runs inside the iframe, that is a
 * porting bug — fail loudly rather than returning fabricated content.
 */

export interface VfsResponse {
  success: boolean;
  content?: string;
  error?: string;
}

export function vfsReadFile(path: string): VfsResponse {
  return {
    success: false,
    error: `vfsReadFile('${path}') called inside the sandboxed iframe; the iframe has no VFS access — assets must come from the parent via postMessage/service worker`,
  };
}

export function vfsReadBinaryFile(path: string): VfsResponse {
  return {
    success: false,
    error: `vfsReadBinaryFile('${path}') called inside the sandboxed iframe; the iframe has no VFS access — assets must come from the parent via postMessage/service worker`,
  };
}
