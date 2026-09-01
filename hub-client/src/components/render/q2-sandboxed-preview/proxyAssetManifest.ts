/**
 * Proxy-URL asset manifest for the sandboxed preview.
 *
 * The q2-preview parent (`Q2PreviewIframe`) walks the AST and mints
 * parent-origin **blob URLs** for each image (`assetWalker.ts`). Blob URLs
 * are scoped to the minting origin, so the cross-origin sandboxed iframe
 * could never fetch them. Here the manifest instead maps each image target
 * to a page-relative URL inside the service-worker proxy namespace
 * (`__q2_vfs__/<resolved path>`); the iframe's `<Image>` renders it as-is,
 * the browser fetches it on the iframe's own origin, and the service
 * worker round-trips the bytes from this parent's WASM VFS.
 *
 * Path resolution deliberately mirrors `assetWalker.buildAssetManifest`
 * (same `resolveRelativePath` + leading-slash strip), so the sandboxed
 * preview resolves exactly the paths q2-preview resolves. No VFS reads
 * happen at manifest time — bytes are read on demand when the service
 * worker asks.
 */
import { resolveRelativePath } from '@quarto/preview-renderer/utils/vfsPaths';
import { proxyUrlForVfsPath } from '../../../../quarto-hub-sandboxed-preview/src/assetPolicy';

export function buildProxyAssetManifest(
  astJson: string,
  currentFilePath: string,
): Record<string, string> {
  let ast: unknown;
  try {
    ast = JSON.parse(astJson);
  } catch {
    return {};
  }

  const manifest: Record<string, string> = {};
  for (const origPath of collectImagePaths(ast)) {
    const resolved = resolveRelativePath(currentFilePath, origPath).replace(/^\/+/, '');
    manifest[origPath] = proxyUrlForVfsPath(resolved);
  }
  return manifest;
}

/**
 * Collect Image-target URL strings from the AST, skipping external URLs —
 * copied from the private walker in
 * `@quarto/preview-renderer/q2-preview/assetWalker.ts` (not exported
 * there, and q2-preview is not modified by this port).
 */
function collectImagePaths(ast: unknown): string[] {
  const paths = new Set<string>();
  visit(ast, paths);
  return Array.from(paths);
}

function visit(value: unknown, out: Set<string>): void {
  if (!value || typeof value !== 'object') return;
  if (Array.isArray(value)) {
    for (const item of value) visit(item, out);
    return;
  }
  const obj = value as { t?: unknown; c?: unknown; blocks?: unknown };
  if (obj.t === 'Image' && Array.isArray(obj.c) && obj.c.length >= 3) {
    const target = obj.c[2];
    if (Array.isArray(target) && typeof target[0] === 'string') {
      const url = target[0];
      if (!isExternal(url)) out.add(url);
    }
  }
  if ('c' in obj) visit(obj.c, out);
  if ('blocks' in obj) visit(obj.blocks, out);
}

function isExternal(url: string): boolean {
  return (
    url.startsWith('http://') ||
    url.startsWith('https://') ||
    url.startsWith('data:') ||
    url.startsWith('blob:') ||
    url.startsWith('//')
  );
}
