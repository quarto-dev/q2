/**
 * Vitest global setup: build the wasm-qmd-parser nodejs pkg when missing or
 * stale, then point QUARTO_QMD_PARSER_SPEC at it so src/qmd-parser.ts loads
 * the real parser in tests (CAP-11). The package is a build artifact, never
 * an npm dependency, so tests resolve it by file URL.
 */

import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

import { ensureQmdParserPkg } from './qmd-parser-pkg.mjs';

export default function setup() {
  const pkgDir = ensureQmdParserPkg({ quiet: true });
  process.env['QUARTO_QMD_PARSER_SPEC'] = pathToFileURL(
    join(pkgDir, 'wasm_qmd_parser.js'),
  ).href;
}
