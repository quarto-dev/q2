import { isWasmReady, resolvePandocFormats } from '@quarto/preview-runtime';
import { wasmRenderOptions, type RenderBlock } from './previewConfig';

/**
 * Whether `path` previews as a live PDF (P11-D1): its resolved format (the Rust resolver's `key`) is `typst-pdf`.
 * `render` is the session's `--to` / `-M` block; it decides the format like it does for `q2 render`.
 * A document whose `output-ext` is not `pdf` resolves to the `typst` (source) key, so it keeps the
 * "no live preview" overlay; the download route's E8 behaviour does not apply here.
 */
export function isTypstPdf(path: string, render?: RenderBlock): boolean {
  if (!isWasmReady()) return false;
  // The session's `--to` and `-M` (F8): `--to typst` makes every page a PDF, `-M format:typst` likewise.
  const resolved = resolvePandocFormats(path, wasmRenderOptions(render));
  if (!resolved.success) return false;
  return resolved.key === 'typst-pdf' && resolved.class === 'download';
}
