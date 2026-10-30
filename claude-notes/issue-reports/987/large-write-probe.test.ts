// Temporary triage probe for issue #987; copied to claude-notes/issue-reports/987 afterwards.
import { describe, expect, it } from 'vitest';
import { randomBytes } from 'node:crypto';

import { startInMemoryMcp, seedProject, callTool } from './in-memory-fixture.js';

describe('issue 987 probe', () => {
  for (const mb of [1, 5, 10, 25]) {
    it(`server accepts ${mb} MB via encoding: base64`, async () => {
      const f = await startInMemoryMcp();
      try {
        const seed = await seedProject(f, []);
        const bytes = randomBytes(mb * 1024 * 1024);
        const content = bytes.toString('base64');
        const t0 = Date.now();
        const result = await callTool(f, 'edit', {
          action: 'write',
          project: seed.indexDocId,
          path: 'shots/screenshot.png',
          content,
          encoding: 'base64',
          wait_for_sync: true,
        });
        const sc = result.structuredContent as { size?: number; synced?: boolean };
        console.log(JSON.stringify(result.content).slice(0, 600));
        console.log(
          `PROBE ${mb} MB: base64 chars=${content.length} isError=${result.isError === true} ` +
            `size=${sc?.size} synced=${sc?.synced} ms=${Date.now() - t0}`,
        );
        expect(result.isError).not.toBe(true);
      } finally {
        await f.close();
      }
    }, 120000);
  }
});
