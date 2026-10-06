/**
 * Per-worker test setup: point the qmd parser loader at the crate's
 * freshly built nodejs pkg (built once by the global setup). The package
 * is a build artifact, never an npm dependency, so tests resolve it by
 * file URL (CAP-11).
 */

import { dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));

process.env['QUARTO_QMD_PARSER_SPEC'] = pathToFileURL(
  join(here, '..', '..', '..', 'crates', 'wasm-qmd-parser', 'pkg-nodejs', 'wasm_qmd_parser.js'),
).href;
