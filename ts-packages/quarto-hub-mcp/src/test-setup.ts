/**
 * Per-worker test setup: point the qmd parser loader at the crate's
 * freshly built nodejs pkg (built once by the global setup). The package
 * is a build artifact, never an npm dependency, so tests resolve it by
 * file URL (CAP-11).
 *
 * When the global setup could not build the parser (toolchain-less CI
 * legs), it leaves a marker file instead: parser-tier test files gate on
 * PARSER_UNAVAILABLE and skip, like the network-gated live tests.
 */

import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const pkgRoot = join(here, '..');

/** True when the global setup could not build the parser on this machine. */
export const PARSER_UNAVAILABLE = existsSync(join(pkgRoot, '.qmd-parser-unavailable'));

if (!PARSER_UNAVAILABLE) {
  process.env['QUARTO_QMD_PARSER_SPEC'] = pathToFileURL(
    join(pkgRoot, '..', '..', 'crates', 'wasm-qmd-parser', 'pkg-nodejs', 'wasm_qmd_parser.js'),
  ).href;
}
