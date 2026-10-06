/**
 * Embedded share URLs (CAP-1): `create_project` (with the new `name`),
 * `connect_project`, and `get_project_info` results carry a `shareUrl`
 * the agent can hand a human — a clickable link that round-trips
 * through `parseProjectRef`. No standalone get_share_url tool (ERG-5).
 */

import { describe, it, expect } from 'vitest';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';
import { buildShareUrl, parseProjectRef } from './share-url.js';

function structuredOf(result: { structuredContent?: unknown }): Record<string, unknown> {
  const sc = result.structuredContent;
  if (sc === undefined || sc === null || typeof sc !== 'object') {
    throw new Error(`expected structuredContent, got: ${JSON.stringify(result)}`);
  }
  return sc as Record<string, unknown>;
}

describe('buildShareUrl (unit)', () => {
  it('omits server= for the production hub and round-trips', () => {
    const url = buildShareUrl({
      server: 'wss://quarto-hub.com/ws',
      indexDocId: 'abc123',
      name: 'My book',
    });
    expect(url).toBe('https://quarto-hub.com/#/share/abc123?name=My+book');
    const ref = parseProjectRef(url);
    expect(ref.project).toBe('abc123');
    expect(ref.name).toBe('My book');
    expect(ref.server).toBeUndefined();
  });

  it('adds server= for a non-production hub and round-trips', () => {
    const url = buildShareUrl({
      server: 'ws://127.0.0.1:4321/ws',
      indexDocId: 'abc123',
      name: 'Local',
      file: 'index.qmd',
    });
    const ref = parseProjectRef(url);
    expect(ref.project).toBe('abc123');
    expect(ref.name).toBe('Local');
    expect(ref.file).toBe('index.qmd');
    expect(ref.server).toBe('ws://127.0.0.1:4321/ws');
  });
});

describe('embedded shareUrl (CAP-1)', () => {
  it('create_project with a name embeds a shareUrl that round-trips', async () => {
    const f: InMemoryMcpFixture = await startInMemoryMcp();
    try {
      const result = await callTool(f, 'create_project', {
        files: [{ path: 'index.qmd', content: 'x\n' }],
        name: 'Field notes',
      });
      expect(result.isError).not.toBe(true);
      const sc = structuredOf(result);
      expect(typeof sc.shareUrl).toBe('string');
      const ref = parseProjectRef(sc.shareUrl as string);
      expect(ref.project).toBe(sc.indexDocId);
      expect(ref.name).toBe('Field notes');
      // The test hub is not the production hub: server= must be present.
      expect(ref.server).toBe(f.hub.url);
    } finally {
      await f.close();
    }
  });

  it('create_project without a name embeds a nameless shareUrl', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, []);
      const created = structuredOf(
        await callTool(f, 'create_project', { files: [] }),
      );
      const ref = parseProjectRef(created.shareUrl as string);
      expect(ref.project).toBe(created.indexDocId);
      expect(ref.name).toBeUndefined();
      expect(seed.indexDocId).not.toBe(created.indexDocId);
    } finally {
      await f.close();
    }
  });

  it('connect_project embeds a shareUrl that round-trips to the same project', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'connect_project', { project: seed.indexDocId });
      expect(result.isError).not.toBe(true);
      const ref = parseProjectRef(structuredOf(result).shareUrl as string);
      expect(ref.project).toBe(seed.indexDocId);
      expect(ref.server).toBe(f.hub.url);
    } finally {
      await f.close();
    }
  });
});
