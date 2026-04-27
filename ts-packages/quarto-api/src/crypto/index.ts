/**
 * @quarto/api — crypto namespace (PURE)
 *
 * Ported from Q1:
 *   - md5Hash: core/hash.ts:10 (md5HashSync)
 *
 * Uses `blueimp-md5` (pure-JS, platform-neutral npm package).
 * No Deno.* / node:* used — only the blueimp-md5 library.
 */

import md5 from "blueimp-md5";

/**
 * Compute the MD5 hex digest of a UTF-8 string.
 * Mirrors Q1 `core/hash.ts:10` (`md5HashSync`), which uses `blueimpMd5`
 * from a vendored skypack bundle. This port uses the npm package directly.
 *
 * @example
 * md5Hash("")   // → "d41d8cd98f00b204e9800998ecf8427e"
 * md5Hash("abc") // → "900150983cd24fb0d6963f7d28e17f72"
 */
export function md5Hash(content: string): string {
  return md5(content);
}
