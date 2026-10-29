/**
 * Drift guard for the two perfCounters copies (see the files' own header
 * comments for why two exist, bd-k8ptcrd9). Asserts the implementations
 * are byte-identical after stripping each file's leading doc comment —
 * the only intended difference is the header naming each copy's location.
 */

import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

function stripHeaderComment(source: string): string {
  const idx = source.indexOf('interface Gauge');
  if (idx < 0) throw new Error('perfCounters: "interface Gauge" not found');
  return source.slice(idx);
}

describe('perfCounters copies', () => {
  it('hub-client and preview-runtime implementations stay byte-identical', () => {
    const here = dirname(fileURLToPath(import.meta.url));
    const hubCopy = readFileSync(join(here, 'perfCounters.ts'), 'utf8');
    const packageCopy = readFileSync(
      join(here, '../../../ts-packages/preview-runtime/src/perfCounters.ts'),
      'utf8',
    );
    expect(stripHeaderComment(packageCopy)).toBe(stripHeaderComment(hubCopy));
  });
});
