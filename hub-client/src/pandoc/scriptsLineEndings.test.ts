import { describe, expect, it } from 'vitest';
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';

// Guards the `scripts/**/*.mjs text eol=lf` pin in .gitattributes (GH #1344).
// Vite strips a shebang with /^#!.*\n/, which can't match a CRLF line, so a
// CRLF checkout of a shebang script makes any test that imports it fail to
// load (pruneEmbedDist.test.ts imports scripts/prune-embed-dist.mjs). This
// lives in its own file because a broken import would keep that test file
// from running at all. It only fails on a CRLF checkout (Windows).
const SCRIPTS_DIR = path.resolve(import.meta.dirname, '../../../scripts');

function mjsFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return mjsFiles(full);
    return entry.name.endsWith('.mjs') ? [full] : [];
  });
}

describe('scripts/**/*.mjs line endings', () => {
  it('checks out with LF line endings', () => {
    const files = mjsFiles(SCRIPTS_DIR);
    expect(files.length).toBeGreaterThan(0);
    const crlf = files
      .filter((f) => readFileSync(f, 'utf8').includes('\r'))
      .map((f) => path.relative(SCRIPTS_DIR, f));
    expect(crlf, 'these scripts have CRLF line endings; see .gitattributes').toEqual([]);
  });
});
