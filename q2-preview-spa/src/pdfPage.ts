import { isWasmReady, resolvePandocFormats } from '@quarto/preview-runtime';

/**
 * Whether `path` previews as a live PDF (P11-D1): its resolved format (the Rust resolver's `key`) is `typst-pdf`.
 * A document whose `output-ext` is not `pdf` resolves to the `typst` (source) key, so it keeps the
 * "no live preview" overlay; the download route's E8 behaviour does not apply here.
 */
export function isTypstPdf(path: string): boolean {
  if (!isWasmReady()) return false;
  const resolved = resolvePandocFormats(path);
  if (!resolved.success) return false;
  return resolved.key === 'typst-pdf' && resolved.class === 'download';
}
