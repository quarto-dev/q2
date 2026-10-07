/**
 * Embedded share URLs (CAP-1): `create_project` (with the new `name`),
 * `connect_project`, and `get_project_info` results carry a `shareUrl`
 * the agent can hand a human — a clickable link that round-trips
 * through `parseProjectRef`. No standalone get_share_url tool (ERG-5).
 *
 * Every embedded shareUrl must be *complete*: hub-client's share route
 * requires server=, file=, and name= and rejects links missing any of
 * them as "incomplete" (bd-jtl4o0pt). buildShareUrl's own unit tests
 * live in share-url.test.ts; this file covers the tool-level embeds.
 */

import { describe, it, expect } from 'vitest';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';
import { parseProjectRef } from './share-url.js';

function structuredOf(result: { structuredContent?: unknown }): Record<string, unknown> {
  const sc = result.structuredContent;
  if (sc === undefined || sc === null || typeof sc !== 'object') {
    throw new Error(`expected structuredContent, got: ${JSON.stringify(result)}`);
  }
  return sc as Record<string, unknown>;
}

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
      expect(ref.file).toBe('index.qmd');
      // The test hub is not the production hub: server= must be present.
      expect(ref.server).toBe(f.hub.url);
    } finally {
      await f.close();
    }
  });

  it('create_project without a name embeds a fallback name and default file', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, []);
      const created = structuredOf(await callTool(f, 'create_project', { files: [] }));
      const ref = parseProjectRef(created.shareUrl as string);
      expect(ref.project).toBe(created.indexDocId);
      // An empty project has no file to name; the link points at the
      // conventional entry point, with hub-client's display fallback name.
      expect(ref.name).toBe('Untitled project');
      expect(ref.file).toBe('index.qmd');
      expect(ref.server).toBe(f.hub.url);
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
      // Connected by bare id: no incoming name/file, so the link falls
      // back to the project's default file and the display fallback name.
      expect(ref.file).toBe('index.qmd');
      expect(ref.name).toBe('Untitled project');
    } finally {
      await f.close();
    }
  });

  it('connect_project via a share URL propagates its file= and name=', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const incoming =
        `https://quarto-hub.com/#/share/${seed.indexDocId}` +
        `?server=${encodeURIComponent(f.hub.url)}&file=index.qmd&name=Field+notes`;
      const result = await callTool(f, 'connect_project', { project: incoming });
      expect(result.isError).not.toBe(true);
      const ref = parseProjectRef(structuredOf(result).shareUrl as string);
      expect(ref.project).toBe(seed.indexDocId);
      expect(ref.file).toBe('index.qmd');
      expect(ref.name).toBe('Field notes');
    } finally {
      await f.close();
    }
  });

  it('get_project_info embeds a complete shareUrl', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'get_project_info', { project: seed.indexDocId });
      expect(result.isError).not.toBe(true);
      const ref = parseProjectRef(structuredOf(result).shareUrl as string);
      expect(ref.project).toBe(seed.indexDocId);
      expect(ref.server).toBe(f.hub.url);
      expect(ref.file).toBe('index.qmd');
      expect(ref.name).toBe('Untitled project');
    } finally {
      await f.close();
    }
  });
});
