/**
 * Tests for URL routing utilities.
 */
import { describe, it, expect, beforeAll, afterAll, beforeEach, afterEach, vi } from 'vitest';
import {
  parseHashRoute,
  buildHashRoute,
  buildShareableUrl,
  buildProjectSetLinkUrl,
  routesEqual,
  sameFile,
  savePreAuthHash,
  restorePreAuthHash,
  resolveSyncServerUrl,
  hubPath,
  type Route,
  type ShareRoute,
  type LinkProjectSetRoute,
} from './routing';

describe('parseHashRoute', () => {
  describe('project selector routes', () => {
    it('parses empty string as project selector', () => {
      expect(parseHashRoute('')).toEqual({ type: 'project-selector' });
    });

    it('parses bare # as project selector', () => {
      expect(parseHashRoute('#')).toEqual({ type: 'project-selector' });
    });

    it('parses #/ as project selector', () => {
      expect(parseHashRoute('#/')).toEqual({ type: 'project-selector' });
    });

    it('parses unknown routes as project selector', () => {
      expect(parseHashRoute('#/unknown/path')).toEqual({ type: 'project-selector' });
      expect(parseHashRoute('#/foo')).toEqual({ type: 'project-selector' });
    });
  });

  describe('project routes', () => {
    it('parses project route with UUID', () => {
      expect(parseHashRoute('#/p/abc-123-def')).toEqual({
        type: 'project',
        projectId: 'abc-123-def',
      });
    });

    it('parses project route with full UUID', () => {
      const uuid = '550e8400-e29b-41d4-a716-446655440000';
      expect(parseHashRoute(`#/p/${uuid}`)).toEqual({
        type: 'project',
        projectId: uuid,
      });
    });

    it('handles project route without leading #', () => {
      expect(parseHashRoute('/p/abc-123')).toEqual({
        type: 'project',
        projectId: 'abc-123',
      });
    });
  });

  describe('file routes', () => {
    it('parses simple file path', () => {
      expect(parseHashRoute('#/p/abc-123/file/index.qmd')).toEqual({
        type: 'file',
        projectId: 'abc-123',
        filePath: 'index.qmd',
      });
    });

    it('parses encoded nested file path', () => {
      // docs/intro.qmd encoded as docs%2Fintro.qmd
      expect(parseHashRoute('#/p/abc-123/file/docs%2Fintro.qmd')).toEqual({
        type: 'file',
        projectId: 'abc-123',
        filePath: 'docs/intro.qmd',
      });
    });

    it('parses file path with anchor', () => {
      expect(parseHashRoute('#/p/abc-123/file/index.qmd#section-1')).toEqual({
        type: 'file',
        projectId: 'abc-123',
        filePath: 'index.qmd',
        anchor: 'section-1',
      });
    });

    it('parses encoded path with anchor', () => {
      expect(parseHashRoute('#/p/abc-123/file/docs%2Fchapter1.qmd#intro')).toEqual({
        type: 'file',
        projectId: 'abc-123',
        filePath: 'docs/chapter1.qmd',
        anchor: 'intro',
      });
    });

    it('handles file path with special characters', () => {
      // Path with spaces: "my file.qmd" -> "my%20file.qmd"
      expect(parseHashRoute('#/p/abc-123/file/my%20file.qmd')).toEqual({
        type: 'file',
        projectId: 'abc-123',
        filePath: 'my file.qmd',
      });
    });

    it('handles deeply nested paths', () => {
      expect(parseHashRoute('#/p/abc/file/a%2Fb%2Fc%2Fd.qmd')).toEqual({
        type: 'file',
        projectId: 'abc',
        filePath: 'a/b/c/d.qmd',
      });
    });

    it('returns project route when file segment is empty', () => {
      expect(parseHashRoute('#/p/abc-123/file/')).toEqual({
        type: 'project',
        projectId: 'abc-123',
      });
    });
  });

  describe('share routes', () => {
    it('parses share route with all required params', () => {
      const result = parseHashRoute(
        '#/share/4XyZabc123?server=wss%3A%2F%2Fsync.automerge.org&file=docs%2Fintro.qmd&name=My+Project'
      );
      expect(result).toEqual({
        type: 'share',
        indexDocId: '4XyZabc123',
        syncServer: 'wss://sync.automerge.org',
        filePath: 'docs/intro.qmd',
        name: 'My Project',
      });
    });

    it('parses share route with missing params as empty strings', () => {
      const result = parseHashRoute('#/share/4XyZabc123');
      expect(result).toEqual({
        type: 'share',
        indexDocId: '4XyZabc123',
        syncServer: '',
        filePath: '',
        name: '',
      });
    });

    it('parses share route with only server param', () => {
      const result = parseHashRoute('#/share/4XyZabc123?server=wss%3A%2F%2Fmy-server.com');
      expect(result).toEqual({
        type: 'share',
        indexDocId: '4XyZabc123',
        syncServer: 'wss://my-server.com',
        filePath: '',
        name: '',
      });
    });

    it('decodes URL-encoded indexDocId', () => {
      const result = parseHashRoute(
        '#/share/abc%2B123?server=wss%3A%2F%2Fa.com&file=index.qmd&name=Test'
      );
      expect(result).toEqual({
        type: 'share',
        indexDocId: 'abc+123',
        syncServer: 'wss://a.com',
        filePath: 'index.qmd',
        name: 'Test',
      });
    });

    it('returns project-selector when share route has no indexDocId', () => {
      expect(parseHashRoute('#/share/')).toEqual({ type: 'project-selector' });
      expect(parseHashRoute('#/share')).toEqual({ type: 'project-selector' });
    });
  });

  describe('link-project-set routes', () => {
    it('parses link-project-set route with server param', () => {
      const result = parseHashRoute(
        '#/link-project-set/abc123?server=wss%3A%2F%2Fsync.example.com'
      );
      expect(result).toEqual({
        type: 'link-project-set',
        projectSetDocId: 'abc123',
        syncServer: 'wss://sync.example.com',
      });
    });

    it('parses link-project-set route with missing server as empty string', () => {
      const result = parseHashRoute('#/link-project-set/abc123');
      expect(result).toEqual({
        type: 'link-project-set',
        projectSetDocId: 'abc123',
        syncServer: '',
      });
    });

    it('decodes URL-encoded projectSetDocId', () => {
      const result = parseHashRoute(
        '#/link-project-set/abc%2B123?server=wss%3A%2F%2Fa.com'
      );
      expect(result).toEqual({
        type: 'link-project-set',
        projectSetDocId: 'abc+123',
        syncServer: 'wss://a.com',
      });
    });

    it('returns project-selector when no docId provided', () => {
      expect(parseHashRoute('#/link-project-set/')).toEqual({ type: 'project-selector' });
      expect(parseHashRoute('#/link-project-set')).toEqual({ type: 'project-selector' });
    });
  });
});

describe('buildHashRoute', () => {
  describe('project selector routes', () => {
    it('builds project selector route', () => {
      expect(buildHashRoute({ type: 'project-selector' })).toBe('#/');
    });
  });

  describe('project routes', () => {
    it('builds project route', () => {
      expect(buildHashRoute({ type: 'project', projectId: 'abc-123' })).toBe(
        '#/p/abc-123'
      );
    });
  });

  describe('file routes', () => {
    it('builds simple file route', () => {
      expect(
        buildHashRoute({
          type: 'file',
          projectId: 'abc-123',
          filePath: 'index.qmd',
        })
      ).toBe('#/p/abc-123/file/index.qmd');
    });

    it('encodes nested file paths', () => {
      expect(
        buildHashRoute({
          type: 'file',
          projectId: 'abc-123',
          filePath: 'docs/intro.qmd',
        })
      ).toBe('#/p/abc-123/file/docs%2Fintro.qmd');
    });

    it('builds file route with anchor', () => {
      expect(
        buildHashRoute({
          type: 'file',
          projectId: 'abc-123',
          filePath: 'index.qmd',
          anchor: 'section-1',
        })
      ).toBe('#/p/abc-123/file/index.qmd#section-1');
    });

    it('encodes special characters in path', () => {
      expect(
        buildHashRoute({
          type: 'file',
          projectId: 'abc',
          filePath: 'my file.qmd',
        })
      ).toBe('#/p/abc/file/my%20file.qmd');
    });
  });

  describe('share routes', () => {
    it('builds share route with all params', () => {
      const route: ShareRoute = {
        type: 'share',
        indexDocId: '4XyZabc123',
        syncServer: 'wss://sync.automerge.org',
        filePath: 'docs/intro.qmd',
        name: 'My Project',
      };
      const result = buildHashRoute(route);
      expect(result).toBe(
        '#/share/4XyZabc123?server=wss%3A%2F%2Fsync.automerge.org&file=docs%2Fintro.qmd&name=My+Project'
      );
    });

    it('encodes indexDocId with special characters', () => {
      const route: ShareRoute = {
        type: 'share',
        indexDocId: 'abc+123',
        syncServer: 'wss://sync.automerge.org',
        filePath: 'index.qmd',
        name: 'Test',
      };
      const result = buildHashRoute(route);
      expect(result).toBe(
        '#/share/abc%2B123?server=wss%3A%2F%2Fsync.automerge.org&file=index.qmd&name=Test'
      );
    });
  });

  describe('link-project-set routes', () => {
    it('builds link-project-set route with server param', () => {
      const route: LinkProjectSetRoute = {
        type: 'link-project-set',
        projectSetDocId: 'abc123',
        syncServer: 'wss://sync.example.com',
      };
      const result = buildHashRoute(route);
      expect(result).toBe(
        '#/link-project-set/abc123?server=wss%3A%2F%2Fsync.example.com'
      );
    });

    it('encodes special characters in docId', () => {
      const route: LinkProjectSetRoute = {
        type: 'link-project-set',
        projectSetDocId: 'abc+123',
        syncServer: 'wss://sync.example.com',
      };
      const result = buildHashRoute(route);
      expect(result).toBe(
        '#/link-project-set/abc%2B123?server=wss%3A%2F%2Fsync.example.com'
      );
    });
  });
});

describe('buildProjectSetLinkUrl', () => {
  const originalWindow = globalThis.window;

  beforeAll(() => {
    // @ts-expect-error - mocking window in node environment
    globalThis.window = {
      location: {
        origin: 'https://example.com',
        pathname: '/hub/',
      },
    };
  });

  afterAll(() => {
    // @ts-expect-error - restoring window
    globalThis.window = originalWindow;
  });

  it('builds a full link-project-set URL', () => {
    const url = buildProjectSetLinkUrl(
      'automerge:abc123',
      'wss://sync.example.com',
    );
    expect(url).toBe(
      'https://example.com/hub/#/link-project-set/abc123?server=wss%3A%2F%2Fsync.example.com'
    );
  });

  it('strips automerge: prefix from docId', () => {
    const url = buildProjectSetLinkUrl(
      'automerge:xyz789',
      'wss://sync.example.com',
    );
    expect(url).toContain('#/link-project-set/xyz789');
    expect(url).not.toContain('automerge%3A');
  });

  it('handles docId without prefix', () => {
    const url = buildProjectSetLinkUrl(
      'abc123',
      'wss://sync.example.com',
    );
    expect(url).toContain('#/link-project-set/abc123');
  });
});

describe('round-trip parsing', () => {
  const testCases: Route[] = [
    { type: 'project-selector' },
    { type: 'project', projectId: 'abc-123' },
    { type: 'project', projectId: '550e8400-e29b-41d4-a716-446655440000' },
    { type: 'file', projectId: 'abc', filePath: 'index.qmd' },
    { type: 'file', projectId: 'abc', filePath: 'docs/chapter1.qmd' },
    { type: 'file', projectId: 'abc', filePath: 'a/b/c/d.qmd' },
    { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'intro' },
    { type: 'file', projectId: 'abc', filePath: 'docs/api.qmd', anchor: 'methods' },
    { type: 'file', projectId: 'abc', filePath: 'my file.qmd' },
    // Share routes
    { type: 'share', indexDocId: '4XyZabc123', syncServer: 'wss://sync.automerge.org', filePath: 'index.qmd', name: 'My Project' },
    { type: 'share', indexDocId: '4XyZabc123', syncServer: 'wss://my-server.com', filePath: 'docs/intro.qmd', name: 'Another Project' },
    { type: 'share', indexDocId: 'abc+123', syncServer: 'wss://sync.automerge.org', filePath: 'test.qmd', name: 'Test' },
    // Link project set routes
    { type: 'link-project-set', projectSetDocId: 'abc123', syncServer: 'wss://sync.example.com' },
    { type: 'link-project-set', projectSetDocId: 'xyz+789', syncServer: 'wss://other.com' },
  ];

  for (const route of testCases) {
    it(`round-trips ${JSON.stringify(route)}`, () => {
      const hash = buildHashRoute(route);
      const parsed = parseHashRoute(hash);
      expect(parsed).toEqual(route);
    });
  }
});

describe('buildShareableUrl', () => {
  // Mock window for node environment
  const originalWindow = globalThis.window;

  beforeAll(() => {
    // @ts-expect-error - mocking window in node environment
    globalThis.window = {
      location: {
        origin: 'https://example.com',
        pathname: '/hub/',
      },
    };
  });

  afterAll(() => {
    // @ts-expect-error - restoring window
    globalThis.window = originalWindow;
  });

  it('builds shareable URL with all params', () => {
    const url = buildShareableUrl('4XyZabc123', 'wss://sync.automerge.org', 'My Project', 'docs/intro.qmd');
    expect(url).toBe(
      'https://example.com/hub/#/share/4XyZabc123?server=wss%3A%2F%2Fsync.automerge.org&file=docs%2Fintro.qmd&name=My+Project'
    );
  });

  it('strips automerge: prefix from indexDocId', () => {
    const url = buildShareableUrl('automerge:4XyZabc123', 'wss://sync.automerge.org', 'Test', 'index.qmd');
    expect(url).toBe(
      'https://example.com/hub/#/share/4XyZabc123?server=wss%3A%2F%2Fsync.automerge.org&file=index.qmd&name=Test'
    );
  });
});

describe('routesEqual', () => {
  it('returns true for equal project selector routes', () => {
    expect(
      routesEqual({ type: 'project-selector' }, { type: 'project-selector' })
    ).toBe(true);
  });

  it('returns true for equal project routes', () => {
    expect(
      routesEqual(
        { type: 'project', projectId: 'abc' },
        { type: 'project', projectId: 'abc' }
      )
    ).toBe(true);
  });

  it('returns false for different project IDs', () => {
    expect(
      routesEqual(
        { type: 'project', projectId: 'abc' },
        { type: 'project', projectId: 'def' }
      )
    ).toBe(false);
  });

  it('returns true for equal file routes', () => {
    expect(
      routesEqual(
        { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'intro' },
        { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'intro' }
      )
    ).toBe(true);
  });

  it('returns false for different anchors', () => {
    expect(
      routesEqual(
        { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'intro' },
        { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'outro' }
      )
    ).toBe(false);
  });

  it('returns false for different types', () => {
    expect(
      routesEqual({ type: 'project-selector' }, { type: 'project', projectId: 'abc' })
    ).toBe(false);
  });

  it('returns true for equal share routes', () => {
    expect(
      routesEqual(
        { type: 'share', indexDocId: 'abc', syncServer: 'wss://a.com', filePath: 'x.qmd', name: 'P' },
        { type: 'share', indexDocId: 'abc', syncServer: 'wss://a.com', filePath: 'x.qmd', name: 'P' }
      )
    ).toBe(true);
  });

  it('returns false for different share route indexDocIds', () => {
    expect(
      routesEqual(
        { type: 'share', indexDocId: 'abc', syncServer: 'wss://a.com', filePath: 'x.qmd', name: 'P' },
        { type: 'share', indexDocId: 'xyz', syncServer: 'wss://a.com', filePath: 'x.qmd', name: 'P' }
      )
    ).toBe(false);
  });

  it('returns false for different share route servers', () => {
    expect(
      routesEqual(
        { type: 'share', indexDocId: 'abc', syncServer: 'wss://a.com', filePath: 'x.qmd', name: 'P' },
        { type: 'share', indexDocId: 'abc', syncServer: 'wss://b.com', filePath: 'x.qmd', name: 'P' }
      )
    ).toBe(false);
  });

  it('returns false for different share route file paths', () => {
    expect(
      routesEqual(
        { type: 'share', indexDocId: 'abc', syncServer: 'wss://a.com', filePath: 'x.qmd', name: 'P' },
        { type: 'share', indexDocId: 'abc', syncServer: 'wss://a.com', filePath: 'y.qmd', name: 'P' }
      )
    ).toBe(false);
  });

  it('returns false for share route vs project route', () => {
    expect(
      routesEqual(
        { type: 'share', indexDocId: 'abc', syncServer: 'wss://a.com', filePath: 'x.qmd', name: 'P' },
        { type: 'project', projectId: 'abc' }
      )
    ).toBe(false);
  });
});

describe('sameFile', () => {
  it('returns true for same file with different anchors', () => {
    expect(
      sameFile(
        { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'intro' },
        { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'outro' }
      )
    ).toBe(true);
  });

  it('returns true for same file, one without anchor', () => {
    expect(
      sameFile(
        { type: 'file', projectId: 'abc', filePath: 'index.qmd' },
        { type: 'file', projectId: 'abc', filePath: 'index.qmd', anchor: 'section' }
      )
    ).toBe(true);
  });

  it('returns false for different files', () => {
    expect(
      sameFile(
        { type: 'file', projectId: 'abc', filePath: 'index.qmd' },
        { type: 'file', projectId: 'abc', filePath: 'about.qmd' }
      )
    ).toBe(false);
  });

  it('returns false for non-file routes', () => {
    expect(
      sameFile({ type: 'project-selector' }, { type: 'project-selector' })
    ).toBe(false);
    expect(
      sameFile(
        { type: 'project', projectId: 'abc' },
        { type: 'file', projectId: 'abc', filePath: 'index.qmd' }
      )
    ).toBe(false);
  });
});

// ── resolveSyncServerUrl ─────────────────────────────────────────

describe('resolveSyncServerUrl', () => {
  const originalWindow = globalThis.window;

  afterEach(() => {
    // @ts-expect-error - restoring window
    globalThis.window = originalWindow;
  });

  function mockLocation(protocol: string, host: string) {
    // @ts-expect-error - mocking window in node environment
    globalThis.window = {
      location: { protocol, host },
    };
  }

  it('resolves a relative path to wss:// on an https origin', () => {
    mockLocation('https:', 'hub.example.com');
    expect(resolveSyncServerUrl('/subpath/ws')).toBe(
      'wss://hub.example.com/subpath/ws'
    );
  });

  it('resolves a relative path to ws:// on an http origin', () => {
    mockLocation('http:', 'localhost:3939');
    expect(resolveSyncServerUrl('/subpath/ws')).toBe(
      'ws://localhost:3939/subpath/ws'
    );
  });

  it('leaves an absolute wss:// URL unchanged', () => {
    mockLocation('https:', 'hub.example.com');
    expect(resolveSyncServerUrl('wss://sync.automerge.org')).toBe(
      'wss://sync.automerge.org'
    );
  });

  it('leaves an absolute ws:// URL unchanged', () => {
    mockLocation('http:', 'localhost:3939');
    expect(resolveSyncServerUrl('ws://localhost:3000')).toBe(
      'ws://localhost:3000'
    );
  });

  it('leaves an absolute https:// URL unchanged', () => {
    mockLocation('https:', 'hub.example.com');
    expect(resolveSyncServerUrl('https://sync.example.com')).toBe(
      'https://sync.example.com'
    );
  });
});

// ── hubPath ──────────────────────────────────────────────────────
//
// Single source of truth for the subpath mount: prefixes auth REST
// calls and derives the sync-server default (`hubPath('/ws')`).

describe('hubPath', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it('is a no-op when no base path is set (served from the hub origin)', () => {
    expect(hubPath('/auth/me')).toBe('/auth/me');
  });

  it('prefixes the configured mount base (subpath deployment)', () => {
    vi.stubEnv('VITE_HUB_BASE_PATH', '/subpath');
    expect(hubPath('/auth/me')).toBe('/subpath/auth/me');
    expect(hubPath('/ws')).toBe('/subpath/ws');
  });
});

// ── Pre-Auth Hash Preservation ──────────────────────────────────

describe('savePreAuthHash / restorePreAuthHash', () => {
  const originalWindow = globalThis.window;
  let mockHash: string;
  let mockStorage: Map<string, string>;

  beforeEach(() => {
    mockHash = '';
    mockStorage = new Map();

    // @ts-expect-error - mocking window in node environment
    globalThis.window = {
      location: {
        get hash() { return mockHash; },
        set hash(v: string) { mockHash = v; },
      },
    };

    // @ts-expect-error - mocking sessionStorage in node environment
    globalThis.sessionStorage = {
      getItem: (key: string) => mockStorage.get(key) ?? null,
      setItem: (key: string, value: string) => { mockStorage.set(key, value); },
      removeItem: (key: string) => { mockStorage.delete(key); },
      clear: () => { mockStorage.clear(); },
    };
  });

  afterEach(() => {
    // @ts-expect-error - restoring window
    globalThis.window = originalWindow;
  });

  it('saves a share link hash and restores it on empty hash', () => {
    mockHash = '#/share/4XyZabc123?server=wss%3A%2F%2Fsync.automerge.org';
    savePreAuthHash();

    // Simulate post-auth redirect to "/"
    mockHash = '';
    const restored = restorePreAuthHash();

    expect(restored).toBe('#/share/4XyZabc123?server=wss%3A%2F%2Fsync.automerge.org');
    expect(mockHash).toBe('#/share/4XyZabc123?server=wss%3A%2F%2Fsync.automerge.org');
  });

  it('restores when current hash is #/', () => {
    mockHash = '#/share/abc123';
    savePreAuthHash();

    mockHash = '#/';
    const restored = restorePreAuthHash();

    expect(restored).toBe('#/share/abc123');
  });

  it('does not save empty hash', () => {
    mockHash = '';
    savePreAuthHash();

    expect(mockStorage.has('quarto-hub-pre-auth-hash')).toBe(false);
  });

  it('does not save #/ hash', () => {
    mockHash = '#/';
    savePreAuthHash();

    expect(mockStorage.has('quarto-hub-pre-auth-hash')).toBe(false);
  });

  it('does not restore if current hash is already meaningful', () => {
    mockHash = '#/share/original';
    savePreAuthHash();

    mockHash = '#/p/some-id';
    const restored = restorePreAuthHash();

    expect(restored).toBeNull();
    expect(mockHash).toBe('#/p/some-id');
  });

  it('clears sessionStorage after restore', () => {
    mockHash = '#/share/abc123';
    savePreAuthHash();
    expect(mockStorage.has('quarto-hub-pre-auth-hash')).toBe(true);

    mockHash = '';
    restorePreAuthHash();

    expect(mockStorage.has('quarto-hub-pre-auth-hash')).toBe(false);
  });

  it('clears sessionStorage even when not restored (hash already set)', () => {
    mockHash = '#/share/abc123';
    savePreAuthHash();

    mockHash = '#/p/other';
    restorePreAuthHash();

    expect(mockStorage.has('quarto-hub-pre-auth-hash')).toBe(false);
  });

  it('returns null when nothing was saved', () => {
    mockHash = '';
    const restored = restorePreAuthHash();
    expect(restored).toBeNull();
  });

  it('matches main.tsx usage: restore-or-save short-circuit', () => {
    // Phase 1: first visit with share link — restore returns null, save captures hash
    mockHash = '#/share/4XyZabc123?server=wss%3A%2F%2Fsync.automerge.org';
    restorePreAuthHash() || savePreAuthHash();

    expect(mockStorage.get('quarto-hub-pre-auth-hash'))
      .toBe('#/share/4XyZabc123?server=wss%3A%2F%2Fsync.automerge.org');

    // Phase 2: return from auth redirect — restore succeeds, save is skipped
    mockHash = '';
    restorePreAuthHash() || savePreAuthHash();

    expect(mockHash).toBe('#/share/4XyZabc123?server=wss%3A%2F%2Fsync.automerge.org');
    // sessionStorage should be clean and not re-saved
    expect(mockStorage.has('quarto-hub-pre-auth-hash')).toBe(false);
  });
});
