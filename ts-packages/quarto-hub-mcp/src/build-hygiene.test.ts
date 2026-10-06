/**
 * Build hygiene (HY-3, bd-zv8u2sxi).
 *
 * `tsc` only ever adds/overwrites outputs — it never deletes. A deleted
 * source file therefore lingers as an orphaned module in `dist/`
 * (gitignored, so nothing catches it), and consumers of `dist/` —
 * `McpTestClient` spawns `node dist/index.js` — can pick up the orphan
 * (the device-flow modules outlived their source for months).
 *
 * The behavioral version of this test (plant an orphan, run the build,
 * assert it vanished) races vitest's parallel file execution — sibling
 * files spawn `dist/index.js` while the wipe is in flight. So the pin
 * is structural: the build script itself must rebuild `dist/` from a
 * clean slate. The bundle is unaffected either way (esbuild compiles
 * from `src/` via the `source` condition and empties `dist-bundle/`
 * itself), so `dist/` has exactly one consumer class: tests and the
 * package `bin` — both served by a clean build.
 */

import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const pkgRoot = join(dirname(fileURLToPath(import.meta.url)), '..');

describe('build hygiene (HY-3)', () => {
  it('the build script rebuilds dist/ from a clean slate', () => {
    const pkg = JSON.parse(readFileSync(join(pkgRoot, 'package.json'), 'utf8')) as {
      scripts: Record<string, string>;
    };
    expect(pkg.scripts.build).toMatch(/^rm -rf dist && /);
  });
});
