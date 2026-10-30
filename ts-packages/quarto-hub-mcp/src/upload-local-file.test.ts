/**
 * `edit` with `action: "upload"` (bd-llklgl70, issue #987): the server
 * reads an image from the local filesystem itself, so full-size images
 * never pass through the conversation as base64 text. Project text can
 * carry prompt injection, so the source is limited to images verified
 * by content, under a size cap.
 */
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { createHash } from 'node:crypto';
import { mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
} from './in-memory-fixture.js';

const PNG = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d]);
const JPEG = Buffer.from([0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10]);
const GIF = Buffer.from('GIF89a-----');
const WEBP = Buffer.from('RIFF\x00\x00\x00\x00WEBPVP8 ', 'latin1');
const MAX_UPLOAD_BYTES = 10 * 1024 * 1024;

function textOf(result: { content: Array<{ type: string; text?: string }> }): string {
  return result.content.map((b) => b.text ?? '').join('\n');
}

function structuredOf(result: { structuredContent?: unknown }): Record<string, unknown> {
  return result.structuredContent as Record<string, unknown>;
}

describe('edit upload', () => {
  let f: InMemoryMcpFixture;
  let project: string;
  let dir: string;

  beforeEach(async () => {
    f = await startInMemoryMcp();
    project = (await seedProject(f, [{ path: 'index.qmd', content: 'hello\n' }])).indexDocId;
    dir = mkdtempSync(join(tmpdir(), 'hub-mcp-upload-'));
  });

  afterEach(async () => {
    await f.close();
    rmSync(dir, { recursive: true, force: true });
  });

  const upload = (localPath: string, extra: Record<string, unknown> = {}) =>
    callTool(f, 'edit', {
      action: 'upload',
      project,
      path: 'shots/screenshot.png',
      local_path: localPath,
      ...extra,
    });

  const writeSource = (name: string, bytes: Uint8Array): string => {
    const p = join(dir, name);
    writeFileSync(p, bytes);
    return p;
  };

  it.each([
    ['PNG', 'a.png', PNG, 'image/png'],
    ['JPEG', 'a.jpg', JPEG, 'image/jpeg'],
    ['GIF', 'a.gif', GIF, 'image/gif'],
    ['WebP', 'a.webp', WEBP, 'image/webp'],
  ])('stores a %s and reads it back byte-identical', async (_label, name, bytes, mimeType) => {
    const result = await upload(writeSource(name, bytes));
    expect(result.isError).not.toBe(true);
    const meta = structuredOf(result);
    expect(meta['path']).toBe('shots/screenshot.png');
    expect(meta['created']).toBe(true);
    expect(meta['mimeType']).toBe(mimeType);
    expect(meta['size']).toBe(bytes.byteLength);

    const read = await callTool(f, 'read', {
      action: 'read',
      project,
      path: 'shots/screenshot.png',
      metadata_only: true,
    });
    const stored = structuredOf(read);
    expect(stored['type']).toBe('binary');
    expect(stored['hash']).toBe(meta['hash']);
    expect(stored['size']).toBe(bytes.byteLength);
  });

  it('takes the MIME type from the content, not either file name', async () => {
    const result = await upload(writeSource('misnamed.gif', PNG));
    expect(structuredOf(result)['mimeType']).toBe('image/png');
  });

  it('handles a full-size image without a conversation-sized payload', async () => {
    const big = Buffer.concat([PNG, Buffer.alloc(8 * 1024 * 1024, 7)]);
    const result = await upload(writeSource('big.png', big));
    expect(result.isError).not.toBe(true);
    expect(structuredOf(result)['size']).toBe(big.byteLength);
    // The result reports metadata, never the bytes.
    expect(JSON.stringify(result).length).toBeLessThan(4096);
  });

  it('accepts a file at exactly the size cap and refuses one byte more', async () => {
    const atCap = Buffer.concat([PNG, Buffer.alloc(MAX_UPLOAD_BYTES - PNG.byteLength)]);
    expect((await upload(writeSource('cap.png', atCap))).isError).not.toBe(true);

    const over = Buffer.concat([atCap, Buffer.alloc(1)]);
    const refused = await upload(writeSource('over.png', over), { path: 'shots/over.png' });
    expect(refused.isError).toBe(true);
    expect(textOf(refused)).toMatch(/10 MB/);
  });

  it.each([
    ['a private key named .png', 'id_rsa.png', Buffer.from('-----BEGIN OPENSSH PRIVATE KEY-----\nsecret')],
    ['text', 'notes.png', Buffer.from('secret notes')],
    ['an SVG', 'a.svg', Buffer.from('<svg xmlns="http://www.w3.org/2000/svg"/>')],
    ['a PDF', 'a.pdf', Buffer.from('%PDF-1.7\nsecret')],
    ['an empty file', 'empty.png', Buffer.alloc(0)],
    ['a RIFF file that is not WebP', 'a.wav', Buffer.from('RIFF\x00\x00\x00\x00WAVEfmt ', 'latin1')],
  ])('refuses %s without echoing its content or creating a file', async (_label, name, bytes) => {
    const result = await upload(writeSource(name, bytes));
    expect(result.isError).toBe(true);
    expect(textOf(result)).toMatch(/PNG, JPEG, GIF, or WebP/);
    expect(textOf(result)).not.toMatch(/secret/);

    const list = await callTool(f, 'read', { action: 'list', project });
    expect(JSON.stringify(list.structuredContent)).not.toContain('screenshot.png');
  });

  it('refuses a relative local_path', async () => {
    const result = await upload('screenshot.png');
    expect(result.isError).toBe(true);
    expect(textOf(result)).toMatch(/absolute/);
  });

  it('explains a missing file, including the separate-filesystem case', async () => {
    const result = await upload(join(dir, 'nope.png'));
    expect(result.isError).toBe(true);
    expect(textOf(result)).toMatch(/not found/i);
    expect(textOf(result)).toMatch(/same filesystem|this machine/i);
  });

  it('refuses a directory', async () => {
    const sub = join(dir, 'sub.png');
    mkdirSync(sub);
    const result = await upload(sub);
    expect(result.isError).toBe(true);
    expect(textOf(result)).toMatch(/regular file/);
  });

  it.skipIf(process.platform === 'win32')('applies the content check through a symlink', async () => {
    const secret = writeSource('secret.txt', Buffer.from('secret'));
    const link = join(dir, 'link.png');
    symlinkSync(secret, link);
    const refused = await upload(link);
    expect(refused.isError).toBe(true);
    expect(textOf(refused)).toMatch(/PNG, JPEG, GIF, or WebP/);

    const real = writeSource('real.png', PNG);
    const goodLink = join(dir, 'good.png');
    symlinkSync(real, goodLink);
    expect((await upload(goodLink)).isError).not.toBe(true);
  });

  // A FIFO would block open() forever, hanging the call.
  it.skipIf(process.platform === 'win32')('refuses a FIFO instead of blocking on it', async () => {
    const fifo = join(dir, 'pipe.png');
    execFileSync('mkfifo', [fifo]);
    const result = await upload(fifo);
    expect(result.isError).toBe(true);
    expect(textOf(result)).toMatch(/regular file/);
  });

  it('honors create_only', async () => {
    const source = writeSource('a.png', PNG);
    expect((await upload(source)).isError).not.toBe(true);
    const refused = await upload(source, { create_only: true });
    expect(refused.isError).toBe(true);
    expect(textOf(refused)).toMatch(/`action: "upload"` refused.*already exists/s);
  });

  it('honors expected_hash on replace, and rejects it on a missing destination', async () => {
    const first = await upload(writeSource('a.png', PNG));
    const firstHash = structuredOf(first)['hash'] as string;
    const second = writeSource('b.png', Buffer.concat([PNG, Buffer.from([1])]));

    const stale = await upload(second, {
      expected_hash: `sha256:${createHash('sha256').update('other').digest('hex')}`,
    });
    expect(stale.isError).toBe(true);
    expect(textOf(stale)).toContain('stale_expected_hash');

    const replaced = await upload(second, { expected_hash: firstHash });
    expect(replaced.isError).not.toBe(true);
    expect(structuredOf(replaced)['created']).toBeUndefined();

    const missing = await upload(second, { path: 'shots/absent.png', expected_hash: firstHash });
    expect(missing.isError).toBe(true);
    expect(textOf(missing)).toMatch(/`action: "upload"` refused/);
  });

  it('refuses to replace a text file', async () => {
    const result = await upload(writeSource('a.png', PNG), { path: 'index.qmd' });
    expect(result.isError).toBe(true);
    expect(textOf(result)).toMatch(/text file/);
  });

  it('reports sync state when wait_for_sync is set', async () => {
    const result = await upload(writeSource('a.png', PNG), { wait_for_sync: true });
    expect(structuredOf(result)['synced']).toBe(true);
  });

  it('requires an explicit destination even with a share URL default', async () => {
    const result = await callTool(f, 'edit', {
      action: 'upload',
      project,
      local_path: writeSource('a.png', PNG),
    });
    expect(result.isError).toBe(true);
  });

  it('is not available in read-only mode', async () => {
    const ro = await startInMemoryMcp({ readOnly: true });
    try {
      const { tools } = await ro.client.listTools();
      expect(tools.map((t) => t.name)).not.toContain('edit');
    } finally {
      await ro.close();
    }
  });
});
