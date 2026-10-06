/**
 * Vitest global setup: build the wasm-qmd-parser nodejs pkg when missing or
 * stale. Best-effort: environments without the wasm toolchain (wasm-pack +
 * a wasm32-capable clang) get a marker file instead, and the parser-tier
 * tests skip themselves (see src/test-setup.ts) — the same gated-tier
 * pattern as the network-gated live tests. Failing the whole suite on a
 * toolchain-less CI leg would hide every non-parser test result.
 */

import { writeFileSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { ensureQmdParserPkg } from './qmd-parser-pkg.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const pkgRoot = join(here, '..');
const marker = join(pkgRoot, '.qmd-parser-unavailable');

export default function setup() {
  try {
    ensureQmdParserPkg({ quiet: true });
    rmSync(marker, { force: true });
  } catch (err) {
    const reason = err instanceof Error ? err.message.split('\n')[0] : String(err);
    writeFileSync(marker, `${reason}\n`);
    console.warn(
      `[qmd-parser] unavailable (${reason}) — parser-tier tests will skip. ` +
        'Install wasm-pack and a wasm32-capable clang to run them.',
    );
  }
}
