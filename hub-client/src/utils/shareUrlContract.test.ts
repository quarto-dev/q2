/**
 * Cross-package contract test (bd-jtl4o0pt): every shareUrl the Quarto Hub
 * MCP server emits must parse through hub-client's own share route as
 * *complete* — App.tsx disables the invite CTA and shows "This share link
 * is incomplete" when syncServer, filePath, or name is empty.
 *
 * `buildShareUrl` is imported straight from the MCP package's TypeScript
 * source (vitest resolves the .js specifier to the .ts file); like every
 * *.test.ts, this file is excluded from `tsc -b`.
 */
import { describe, it, expect } from 'vitest';
import { buildShareUrl } from '../../../ts-packages/quarto-hub-mcp/src/share-url.js';
import { parseHashRoute } from './routing';

function parseShare(url: string) {
  const route = parseHashRoute(new URL(url).hash);
  if (route.type !== 'share') {
    throw new Error(`expected a share route, got ${route.type}`);
  }
  return route;
}

describe('MCP buildShareUrl ↔ hub-client share route', () => {
  it('a default-hub link parses with server, file, and name all set', () => {
    const route = parseShare(
      buildShareUrl({
        server: 'wss://quarto-hub.com/ws',
        indexDocId: '34uwVhmgDyuQv9WftjkvW9FcCarp',
        file: 'index.qmd',
        name: 'MCP Test',
      }),
    );
    // The App.tsx completeness predicate: all three must be non-empty.
    expect(route.syncServer).toBe('wss://quarto-hub.com/ws');
    expect(route.filePath).toBe('index.qmd');
    expect(route.name).toBe('MCP Test');
  });

  it('a non-default-hub link round-trips', () => {
    const route = parseShare(
      buildShareUrl({
        server: 'ws://127.0.0.1:4321/ws',
        indexDocId: 'abc123',
        file: 'docs/intro.qmd',
        name: 'Field notes',
      }),
    );
    expect(route.syncServer).toBe('ws://127.0.0.1:4321/ws');
    expect(route.filePath).toBe('docs/intro.qmd');
    expect(route.name).toBe('Field notes');
  });

  it('names and files needing encoding survive the round-trip', () => {
    const route = parseShare(
      buildShareUrl({
        server: 'wss://quarto-hub.com/ws',
        indexDocId: 'abc123',
        file: 'chapters/my chapter.qmd',
        name: 'A `quarto-hub` update',
      }),
    );
    expect(route.filePath).toBe('chapters/my chapter.qmd');
    expect(route.name).toBe('A `quarto-hub` update');
  });
});
