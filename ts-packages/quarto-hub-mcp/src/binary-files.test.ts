/**
 * Binary file I/O through the existing read/write verbs (CAP-4/CAP-5,
 * ERG-5 rule (a) — same verb, varied by parameter, no `read_binary_file`
 * sibling; HY-1 closed for real):
 *
 * - `write_file`/`create_file` with `encoding: "base64"` store binary
 *   bytes (`mime_type` optional; inferred from the path extension).
 * - `read_file` on a binary returns an `image` content block for image
 *   MIME types, an embedded blob resource otherwise, plus structured
 *   `{path, hash, type: 'binary', mimeType, size}`. `metadata_only`
 *   returns the metadata without the bytes.
 * - Binary participates in the compare-and-swap contract: `hash` is the
 *   sha256 of the raw bytes, and `expected_hash` gates binary writes.
 */

import { describe, it, expect } from 'vitest';
import { createHash } from 'node:crypto';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';

// A minimal but real PNG (8-byte signature + IHDR stub) — the MCP layer
// never parses image bytes, but tests keep them plausible.
const PNG = new Uint8Array([
  0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
]);
const PDF = new Uint8Array([0x25, 0x50, 0x44, 0x46, 0x2d, 0x31, 0x2e, 0x37, 0x0a, 0x25, 0xe2]);

const b64 = (bytes: Uint8Array) => Buffer.from(bytes).toString('base64');
const sha256 = (bytes: Uint8Array) => `sha256:${createHash('sha256').update(bytes).digest('hex')}`;

interface BinaryMeta {
  path: string;
  hash: string;
  type: 'binary';
  mimeType: string;
  size: number;
}

function structuredOf(result: { structuredContent?: unknown }): Record<string, unknown> {
  const sc = result.structuredContent;
  if (sc === undefined || sc === null || typeof sc !== 'object') {
    throw new Error(`expected structuredContent, got: ${JSON.stringify(result)}`);
  }
  return sc as Record<string, unknown>;
}

describe('binary write (CAP-5)', () => {
  it('write_file with encoding base64 creates a binary file and returns its metadata', async () => {
    const f: InMemoryMcpFixture = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, []);
      const result = await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'assets/logo.png',
        content: b64(PNG),
        encoding: 'base64',
      });
      expect(result.isError).not.toBe(true);
      const meta = structuredOf(result) as unknown as Partial<BinaryMeta> & {
        hash: string;
        created?: boolean;
        synced?: boolean;
      };
      expect(meta.path).toBe('assets/logo.png');
      expect(meta.type).toBeUndefined(); // write results carry hash, not type
      expect(meta.hash).toBe(sha256(PNG));
      expect(meta.mimeType).toBe('image/png'); // inferred from the extension
      expect(meta.size).toBe(PNG.byteLength);
      expect(meta.created).toBe(true);
      expect(meta.synced).toBe(true);
    } finally {
      await f.close();
    }
  });

  it('explicit mime_type overrides the extension inference', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, []);
      const result = await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'data/blob.bin',
        content: b64(PDF),
        encoding: 'base64',
        mime_type: 'application/pdf',
      });
      expect(result.isError).not.toBe(true);
      const meta = structuredOf(result);
      expect(meta.mimeType).toBe('application/pdf');
    } finally {
      await f.close();
    }
  });

  it('rejects invalid base64 with an actionable error', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, []);
      const result = await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'x.png',
        content: 'not valid base64 !!!',
        encoding: 'base64',
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      expect(block?.type).toBe('text');
      if (block?.type === 'text') {
        expect(block.text).toMatch(/base64/i);
      }
    } finally {
      await f.close();
    }
  });

  it('write_file base64 over an existing TEXT file is refused', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'hello\n' }]);
      const result = await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'index.qmd',
        content: b64(PNG),
        encoding: 'base64',
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type === 'text') {
        expect(block.text).toMatch(/text file/i);
      }
    } finally {
      await f.close();
    }
  });

  it('create_file with encoding base64 creates a binary file', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, []);
      const result = await callTool(f, 'create_file', {
        project: seed.indexDocId,
        path: 'paper.pdf',
        content: b64(PDF),
        encoding: 'base64',
      });
      expect(result.isError).not.toBe(true);
      const meta = structuredOf(result);
      expect(meta.hash).toBe(sha256(PDF));
      expect(meta.mimeType).toBe('application/pdf');
      expect(meta.created).toBe(true);
    } finally {
      await f.close();
    }
  });
});

describe('binary read (CAP-4)', () => {
  async function seedBinary(
    f: InMemoryMcpFixture,
    path: string,
    bytes: Uint8Array,
  ): Promise<{ indexDocId: string }> {
    const seed = await seedProject(f, []);
    const write = await callTool(f, 'write_file', {
      project: seed.indexDocId,
      path,
      content: b64(bytes),
      encoding: 'base64',
    });
    expect(write.isError).not.toBe(true);
    return seed;
  }

  it('read_file returns an image block for an image MIME type, bytes identical', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedBinary(f, 'logo.png', PNG);
      const result = await callTool(f, 'read_file', {
        project: seed.indexDocId,
        path: 'logo.png',
      });
      expect(result.isError).not.toBe(true);

      const imageBlock = result.content.find((b) => b.type === 'image');
      expect(imageBlock).toBeDefined();
      if (imageBlock?.type === 'image') {
        expect(imageBlock.mimeType).toBe('image/png');
        expect(Buffer.from(imageBlock.data, 'base64')).toEqual(Buffer.from(PNG));
      }
      const meta = structuredOf(result) as unknown as BinaryMeta;
      expect(meta).toMatchObject({
        path: 'logo.png',
        type: 'binary',
        mimeType: 'image/png',
        size: PNG.byteLength,
        hash: sha256(PNG),
      });
    } finally {
      await f.close();
    }
  });

  it('read_file returns an embedded blob resource for a non-image binary', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedBinary(f, 'paper.pdf', PDF);
      const result = await callTool(f, 'read_file', {
        project: seed.indexDocId,
        path: 'paper.pdf',
      });
      expect(result.isError).not.toBe(true);

      const resourceBlock = result.content.find((b) => b.type === 'resource');
      expect(resourceBlock).toBeDefined();
      if (resourceBlock?.type === 'resource') {
        expect(resourceBlock.resource.mimeType).toBe('application/pdf');
        expect('blob' in resourceBlock.resource && resourceBlock.resource.blob).toBe(b64(PDF));
      }
      expect(structuredOf(result)).toMatchObject({
        type: 'binary',
        mimeType: 'application/pdf',
        size: PDF.byteLength,
      });
    } finally {
      await f.close();
    }
  });

  it('metadata_only returns the metadata without the bytes', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedBinary(f, 'logo.png', PNG);
      const result = await callTool(f, 'read_file', {
        project: seed.indexDocId,
        path: 'logo.png',
        metadata_only: true,
      });
      expect(result.isError).not.toBe(true);
      expect(result.content.find((b) => b.type === 'image')).toBeUndefined();
      expect(result.content.find((b) => b.type === 'resource')).toBeUndefined();
      expect(structuredOf(result)).toMatchObject({
        path: 'logo.png',
        type: 'binary',
        mimeType: 'image/png',
        size: PNG.byteLength,
        hash: sha256(PNG),
      });
    } finally {
      await f.close();
    }
  });

  it('metadata_only on a text file is an actionable error', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'hello\n' }]);
      const result = await callTool(f, 'read_file', {
        project: seed.indexDocId,
        path: 'index.qmd',
        metadata_only: true,
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type === 'text') {
        expect(block.text).toMatch(/metadata_only.*binar/i);
      }
    } finally {
      await f.close();
    }
  });
});

describe('binary compare-and-swap (CAP-4/5 + ERG-1)', () => {
  it('write_file base64 replaces an existing binary in place; expected_hash gates it', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, []);
      await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'logo.png',
        content: b64(PNG),
        encoding: 'base64',
      });
      const read = structuredOf(
        await callTool(f, 'read_file', { project: seed.indexDocId, path: 'logo.png' }),
      );

      // Stale expected_hash → refused, names the current hash.
      const stale = await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'logo.png',
        content: b64(PDF),
        encoding: 'base64',
        mime_type: 'image/png',
        expected_hash: sha256(PDF),
      });
      expect(stale.isError).toBe(true);
      const staleBlock = stale.content[0];
      if (staleBlock?.type === 'text') {
        expect(staleBlock.text).toMatch(/stale_expected_hash/);
      }

      // Correct expected_hash → applied, same document (path listing stable).
      const PNG2 = new Uint8Array([...PNG, 0xff]);
      const updated = await callTool(f, 'write_file', {
        project: seed.indexDocId,
        path: 'logo.png',
        content: b64(PNG2),
        encoding: 'base64',
        expected_hash: read.hash as string,
      });
      expect(updated.isError).not.toBe(true);
      expect(structuredOf(updated).hash).toBe(sha256(PNG2));

      const reread = await callTool(f, 'read_file', {
        project: seed.indexDocId,
        path: 'logo.png',
      });
      const imageBlock = reread.content.find((b) => b.type === 'image');
      if (imageBlock?.type === 'image') {
        expect(Buffer.from(imageBlock.data, 'base64')).toEqual(Buffer.from(PNG2));
      }
    } finally {
      await f.close();
    }
  });

  it('default tool count stays within budget after binary support (no new tools)', async () => {
    const f = await startInMemoryMcp();
    try {
      const tools = await f.client.listTools();
      expect(tools.tools.length).toBeLessThanOrEqual(24);
    } finally {
      await f.close();
    }
  });
});
