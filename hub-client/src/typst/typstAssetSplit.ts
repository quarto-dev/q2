import type { TypstFile } from '@quarto/typst-host';

/**
 * Split the Rust `get_typst_assets()` export into what a typst job takes: vendored packages
 * (`packages/preview/<name>/<version>/...`, passed relative to the cache root) and the
 * vendored fonts (`fonts/*.otf|ttf`, the Font Awesome files). Anything else (the font
 * license) is dropped.
 */
export function splitTypstAssets(files: { path: string; bytes: Uint8Array }[]): { vendoredPackages: TypstFile[]; fonts: Uint8Array[] } {
  const vendoredPackages: TypstFile[] = [];
  const fonts: Uint8Array[] = [];
  for (const f of files) {
    if (f.path.startsWith('packages/')) vendoredPackages.push({ path: f.path.slice('packages/'.length), bytes: f.bytes });
    else if (f.path.startsWith('fonts/') && /\.(otf|ttf)$/i.test(f.path)) fonts.push(f.bytes);
  }
  return { vendoredPackages, fonts };
}
