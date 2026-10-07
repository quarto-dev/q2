/**
 * The page count of a PDF written by typst, from the page tree root's `/Count`. typst writes
 * the tree as a plain (unfiltered) object, so a text scan is enough; this is not a PDF parser.
 * Returns `undefined` when no root is found.
 */
export function countPdfPages(pdf: Uint8Array): number | undefined {
  const text = new TextDecoder('latin1').decode(pdf);
  for (const m of text.matchAll(/<<([^<>]*\/Type\s*\/Pages\b[^<>]*)>>/g)) {
    const dict = m[1];
    if (/\/Parent\b/.test(dict)) continue;
    const count = /\/Count\s+(\d+)/.exec(dict);
    if (count) return Number(count[1]);
  }
  return undefined;
}
