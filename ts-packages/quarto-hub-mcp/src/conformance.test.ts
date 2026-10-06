/**
 * Protocol conformance harness (Phase 0, bd-f1dr7gs1;
 * claude-notes/plans/2026-10-05-elevate-quarto-hub-mcp.md).
 *
 * The regression net every later phase TDDs against. Drives the real
 * server through the real SDK `Client` over an in-memory linked
 * transport (see in-memory-fixture.ts) — no stdio process, no schema
 * validation skipped — against the in-process test-hub. Offline-only:
 * the only sockets involved are the test-hub's 127.0.0.1 listener, same
 * as the existing suite.
 *
 * Two nets are intentionally red-by-construction today and ship as
 * `it.fails` so the Phase 0 gate stays green; each flips to a normal
 * test when its Phase 1 fix lands and the flipped test starts failing:
 *   - cancellation hygiene (BP-3): the server ignores
 *     `notifications/cancelled`, so the cancelled long-poll's waiter
 *     leaks in ConnectionManager until its timeout fires;
 *   - authorization-URL validation (BP-18): the URL handed to the
 *     browser is built from fetched IdP metadata with no scheme/host
 *     check.
 */

import { describe, it, expect, vi } from 'vitest';
import { Ajv2020 } from 'ajv/dist/2020.js';
import { ToolSchema } from '@modelcontextprotocol/core';

import {
  ConnectionManager,
  InsecureTransportError,
} from './connection-manager.js';
import {
  CredentialStore,
  type CredentialBundle,
  type KeyringBackend,
} from './auth/credential-store.js';
import { ReauthRequired, RefreshManager } from './auth/refresh-manager.js';
import { AUTH_TOOL_DEFINITIONS, AuthToolsState } from './auth/auth-tools.js';
import { assertSafeAuthorizationEndpoint } from './auth/oauth-config.js';
import type { LoopbackListener } from './auth/loopback.js';
import {
  startInMemoryMcp,
  seedProject,
  callTool,
  type InMemoryMcpFixture,
  type SeededProject,
} from './in-memory-fixture.js';

/** Tool budget (ERG-5): the default listing never exceeds this. */
const TOOL_BUDGET = 24;

/** Read-write mode lists these today (10 tools; auth tools need OAuth env). */
const EXPECTED_RW_TOOLS = [
  'connect_project',
  'create_file',
  'create_project',
  'delete_file',
  'list_files',
  'patch_file',
  'read_file',
  'rename_file',
  'wait_for_change',
  'write_file',
];

// ============================================================================
// Fixture smoke — the Phase 0 e2e: a real SDK client driving the real server
// ============================================================================

describe('in-memory fixture smoke', () => {
  let f: InMemoryMcpFixture;

  it('boots, negotiates, and serves the golden path', async () => {
    f = await startInMemoryMcp();
    try {
      // initialize carried identity, capabilities, and instructions.
      expect(f.client.getServerVersion()?.name).toBe('quarto-hub');
      expect(f.client.getServerCapabilities()?.tools).toBeDefined();
      expect(f.client.getInstructions()).toBeTruthy();

      const tools = await f.client.listTools();
      expect(tools.tools.map((t) => t.name).sort()).toEqual(EXPECTED_RW_TOOLS);

      // Golden path: create → list → read through the real client.
      const seed = await seedProject(f, [
        { path: 'index.qmd', content: '---\ntitle: Conformance\n---\n\nHello\n' },
      ]);
      const listed = await callTool(f, 'list_files', { project: seed.indexDocId });
      expect(listed.isError).not.toBe(true);
      const listedBlock = listed.content[0];
      expect(listedBlock?.type).toBe('text');
      if (listedBlock?.type !== 'text') throw new Error('unreachable');
      const files = JSON.parse(listedBlock.text) as Array<{ path: string; type: string }>;
      expect(files).toEqual([{ path: 'index.qmd', type: 'text' }]);

      const read = await callTool(f, 'read_file', {
        project: seed.indexDocId,
        path: 'index.qmd',
      });
      expect(read.isError).not.toBe(true);
      const readBlock = read.content[0];
      expect(readBlock?.type).toBe('text');
      if (readBlock?.type !== 'text') throw new Error('unreachable');
      expect(readBlock.text).toContain('title: Conformance');
    } finally {
      await f.close();
    }
  }, 30000);

  it('serves read-only mode with the reduced surface', async () => {
    const ro = await startInMemoryMcp({ readOnly: true });
    try {
      const tools = await ro.client.listTools();
      expect(tools.tools.map((t) => t.name).sort()).toEqual([
        'connect_project',
        'list_files',
        'read_file',
        'wait_for_change',
      ]);
    } finally {
      await ro.close();
    }
  });
});

// ============================================================================
// Tool budget (ERG-5)
// ============================================================================

describe('tool budget (ERG-5)', () => {
  it('default listing stays within the 24-tool ceiling', async () => {
    const f = await startInMemoryMcp();
    try {
      const tools = await f.client.listTools();
      expect(tools.tools.length).toBeLessThanOrEqual(TOOL_BUDGET);
    } finally {
      await f.close();
    }
  });

  it('read-only listing also stays within the ceiling', async () => {
    const f = await startInMemoryMcp({ readOnly: true });
    try {
      const tools = await f.client.listTools();
      expect(tools.tools.length).toBeLessThanOrEqual(TOOL_BUDGET);
    } finally {
      await f.close();
    }
  });
});

// ============================================================================
// Schema conformance (BP-11, BP-14)
// ============================================================================

describe('schema conformance (BP-11, BP-14)', () => {
  it('every tools/list entry validates against the SDK ToolSchema', async () => {
    const f = await startInMemoryMcp();
    try {
      const { tools } = await f.client.listTools();
      expect(tools.length).toBeGreaterThan(0);
      for (const tool of tools) {
        const parsed = ToolSchema.safeParse(tool);
        if (!parsed.success) {
          throw new Error(
            `tool ${tool.name} fails ToolSchema: ${JSON.stringify(parsed.error.issues)}`,
          );
        }
      }
    } finally {
      await f.close();
    }
  });

  it('every inputSchema compiles as JSON Schema 2020-12', async () => {
    const f = await startInMemoryMcp();
    try {
      const ajv = new Ajv2020({ allErrors: true });
      const { tools } = await f.client.listTools();
      for (const tool of tools) {
        try {
          ajv.compile(tool.inputSchema);
        } catch (err) {
          throw new Error(
            `tool ${tool.name} inputSchema is not valid JSON Schema 2020-12: ` +
              `${err instanceof Error ? err.message : String(err)}`,
          );
        }
      }
    } finally {
      await f.close();
    }
  });
});

// ============================================================================
// Result conformance (BP-1's future net)
// ============================================================================

/**
 * Golden result-conformance cases (BP-1, landed Phase 1). Every tool
 * that declares an `outputSchema` must have a case here — the
 * `registered coverage` test fails otherwise — and each golden call's
 * `structuredContent` is validated against the declared schema.
 *
 * Cases run sequentially against one seeded project (`index.qmd`
 * present); later cases may depend on earlier ones (create → delete).
 */
const GOLDEN_RESULT_CASES: ReadonlyArray<{
  tool: string;
  args: (seed: SeededProject) => Record<string, unknown>;
}> = [
  { tool: 'connect_project', args: (seed) => ({ project: seed.indexDocId }) },
  { tool: 'list_files', args: (seed) => ({ project: seed.indexDocId }) },
  {
    tool: 'read_file',
    args: (seed) => ({ project: seed.indexDocId, path: 'index.qmd' }),
  },
  {
    tool: 'wait_for_change',
    args: (seed) => ({ project: seed.indexDocId, path: 'index.qmd', timeout_seconds: 1 }),
  },
  {
    tool: 'write_file',
    args: (seed) => ({ project: seed.indexDocId, path: 'index.qmd', content: 'v2\n' }),
  },
  {
    tool: 'patch_file',
    args: (seed) => ({
      project: seed.indexDocId,
      path: 'index.qmd',
      old_string: 'v2',
      new_string: 'v3',
    }),
  },
  {
    tool: 'create_file',
    args: (seed) => ({ project: seed.indexDocId, path: 'golden-new.qmd', content: 'new\n' }),
  },
  {
    tool: 'delete_file',
    args: (seed) => ({ project: seed.indexDocId, path: 'golden-new.qmd' }),
  },
  {
    tool: 'rename_file',
    args: (seed) => ({
      project: seed.indexDocId,
      old_path: 'index.qmd',
      new_path: 'golden-renamed.qmd',
    }),
  },
  {
    tool: 'create_project',
    args: () => ({ files: [{ path: 'p.qmd', content: 'x\n' }] }),
  },
];

describe('result conformance (BP-1 net)', () => {
  it('registers a golden case for every tool that declares an outputSchema', async () => {
    const f = await startInMemoryMcp();
    try {
      const { tools } = await f.client.listTools();
      const withOutputSchema = tools.filter((t) => t.outputSchema !== undefined).map((t) => t.name);
      const covered = GOLDEN_RESULT_CASES.map((c) => c.tool);
      for (const name of withOutputSchema) {
        expect(
          covered,
          `tool ${name} declares an outputSchema but has no golden result case`,
        ).toContain(name);
      }
      // And the converse: no stale cases for schema-less tools.
      for (const name of covered) {
        expect(
          withOutputSchema,
          `golden case for ${name} but the tool declares no outputSchema`,
        ).toContain(name);
      }
    } finally {
      await f.close();
    }
  });

  // The BP-1 contract, directly: structuredContent matching the
  // declared outputSchema, with the JSON text fallback retained.
  it('list_files carries structuredContent matching its outputSchema plus the JSON text fallback', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const { tools } = await f.client.listTools();
      const declared = tools.find((t) => t.name === 'list_files')?.outputSchema;
      expect(declared, 'list_files declares no outputSchema').toBeDefined();

      const result = await callTool(f, 'list_files', { project: seed.indexDocId });
      expect(result.isError).not.toBe(true);
      // Text fallback retained: the bare JSON array, as today.
      const block = result.content[0];
      expect(block?.type).toBe('text');
      if (block?.type !== 'text') throw new Error('unreachable');
      const fromText = JSON.parse(block.text) as Array<{ path: string; type: string }>;
      expect(fromText).toEqual([{ path: 'index.qmd', type: 'text' }]);
      // structuredContent: the same files, wrapped as an object.
      const structured = result.structuredContent as { files?: unknown } | undefined;
      expect(structured?.files).toEqual(fromText);

      const ajv = new Ajv2020({ allErrors: true });
      const validate = ajv.compile(declared!);
      expect(
        validate(structured),
        `structuredContent fails its outputSchema: ${JSON.stringify(validate.errors)}`,
      ).toBe(true);
    } finally {
      await f.close();
    }
  });

  it('validates each golden call\u2019s structuredContent against the declared outputSchema', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [
        { path: 'index.qmd', content: '---\ntitle: Golden\n---\n' },
      ]);
      const ajv = new Ajv2020({ allErrors: true });
      const { tools } = await f.client.listTools();
      for (const c of GOLDEN_RESULT_CASES) {
        const declared = tools.find((t) => t.name === c.tool)?.outputSchema;
        if (declared === undefined) {
          throw new Error(`golden case for ${c.tool}: tool declares no outputSchema`);
        }
        const result = await callTool(f, c.tool, c.args(seed));
        expect(result.isError, `golden call for ${c.tool} errored`).not.toBe(true);
        const validate = ajv.compile(declared);
        const ok = validate(result.structuredContent);
        expect(
          ok,
          `${c.tool} structuredContent fails its outputSchema: ${JSON.stringify(validate.errors)}`,
        ).toBe(true);
      }
    } finally {
      await f.close();
    }
  });

  // The net above is vacuous while the registry is empty; prove the
  // validation machinery itself catches a mismatch.
  it('the validation machinery rejects a schema mismatch', () => {
    const ajv = new Ajv2020();
    const validate = ajv.compile({
      type: 'object',
      properties: { path: { type: 'string' } },
      required: ['path'],
    });
    expect(validate({ path: 'a.qmd' })).toBe(true);
    expect(validate({ path: 42 })).toBe(false);
    expect(validate({})).toBe(false);
  });
});

// ============================================================================
// Security invariants
// ============================================================================

function inMemoryBackend(initial?: string | null): KeyringBackend {
  let v: string | null = initial ?? null;
  return {
    async read() {
      return v;
    },
    async write(value: string) {
      v = value;
    },
    async clear() {
      const had = v !== null;
      v = null;
      return had;
    },
  };
}

function seededStore(): CredentialStore {
  const issuer = 'https://accounts.google.com';
  const clientId = 'test-client.apps.googleusercontent.com';
  const bundle: CredentialBundle = {
    idToken: 'id-token',
    refreshToken: 'refresh-token',
    idTokenExpiresAt: new Date(Date.now() + 60 * 60 * 1000),
    scopes: ['openid', 'email', 'profile'],
  };
  const serialized = JSON.stringify({
    schema_version: 1,
    issuer,
    client_id: clientId,
    id_token: bundle.idToken,
    refresh_token: bundle.refreshToken,
    id_token_expires_at: bundle.idTokenExpiresAt.toISOString(),
    scopes: bundle.scopes,
  });
  return new CredentialStore({ issuer, clientId }, inMemoryBackend(serialized));
}

/** A RefreshManager shell — the insecure-transport gate fires before any token pull. */
function stubRefreshManager(): RefreshManager {
  const rm = Object.create(RefreshManager.prototype) as RefreshManager;
  Object.defineProperty(rm, 'getValidIdToken', { value: vi.fn().mockResolvedValue('id-token') });
  return rm;
}

describe('security invariants', () => {
  // Pinned, passes today: the gate exists (connection-manager.ts
  // assertSecureTransport); this names it as a conformance assertion so a
  // regression fails a security-labeled test, not a generic one.
  it('never sends a Bearer token over ws:// to a non-loopback peer', async () => {
    const manager = new ConnectionManager({
      serverUrl: 'ws://hub.example.com/ws',
      credentialStore: seededStore(),
      refreshManager: stubRefreshManager(),
      fetch: vi.fn() as unknown as typeof fetch,
      env: {},
    });
    await expect(manager.connect('some-index-doc-id')).rejects.toBeInstanceOf(
      InsecureTransportError,
    );
  });

  it('permits ws:// to loopback (the test-hub every other test relies on)', async () => {
    // The fixture itself is the proof: every test in this file connects a
    // Bearer-less client over ws://127.0.0.1. With credentials present the
    // gate's loopback exception must likewise let the connection proceed —
    // assert it gets *past* the transport gate (failure, if any, comes from
    // the unreachable example host, not InsecureTransportError).
    const manager = new ConnectionManager({
      serverUrl: 'ws://127.0.0.1:1/ws',
      credentialStore: seededStore(),
      refreshManager: stubRefreshManager(),
      fetch: vi.fn().mockRejectedValue(new Error('connection refused')) as unknown as typeof fetch,
      env: {},
    });
    await expect(manager.connect('some-index-doc-id')).rejects.not.toBeInstanceOf(
      InsecureTransportError,
    );
  });

  // BP-18: the authorization URL handed to the browser is built from
  // *fetched* authorization-server metadata — SSRF input. These pin the
  // invariant the Phase 1 fix landed: with the insecure-auth escape
  // hatch unset, nothing but a public https URL may reach the browser
  // (or the user). Landed red-by-construction as `it.fails` in Phase 0;
  // flipped when the fix landed in Phase 1 (bd-zv8u2sxi).
  describe('authorization URL validation (BP-18)', () => {
    const BAD_ENDPOINTS = [
      'http://169.254.169.254/latest/meta-data', // link-local cloud metadata
      'https://192.168.1.1/authorize', // private-range host
      'http://idp.example.com/authorize', // public host, plain http
    ];

    function harness(authorizationEndpoint: string): {
      state: AuthToolsState;
      browserUrls: string[];
    } {
      const browserUrls: string[] = [];
      const refreshManager = Object.create(RefreshManager.prototype) as RefreshManager;
      Object.defineProperty(refreshManager, 'getValidIdToken', {
        value: vi.fn().mockRejectedValue(new ReauthRequired()),
      });
      const listener: LoopbackListener = {
        port: 1,
        redirectUri: 'http://127.0.0.1:1/callback',
        // The flow blocks on the callback; reject promptly so the handler
        // settles. The invariant under test is what reached the browser.
        // Deferred rejection: an eagerly-rejected promise would race the
        // handler's await and surface as an unhandled rejection.
        result: new Promise((_, reject) =>
          setTimeout(() => reject(new Error('test: no callback coming')), 10),
        ),
        close() {},
      };
      const state = new AuthToolsState({
        credentialStore: seededStore(),
        refreshManager,
        connectionManager: { lastObservedAuthMode: () => 'unknown' },
        flowConfig: {
          clientId: 'test-client',
          clientSecret: 'test-secret',
          issuer: 'https://idp.example.com',
        },
        authServer: async () => ({
          issuer: 'https://idp.example.com',
          authorization_endpoint: authorizationEndpoint,
          token_endpoint: 'https://idp.example.com/token',
        }),
        startListener: async () => listener,
        openBrowser: (url: string) => {
          browserUrls.push(url);
          return undefined;
        },
        logger: () => {},
      });
      return { state, browserUrls };
    }

    it(
      'refuses to hand a non-https or private-host authorization URL to the browser',
      async () => {
        // The escape hatch for local dev IdPs must be off for the default
        // posture under test.
        vi.stubEnv('QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH', '');
        try {
          for (const endpoint of BAD_ENDPOINTS) {
            const { state, browserUrls } = harness(endpoint);
            const result = await state.handle('authenticate');
            expect(
              browserUrls,
              `browser was handed a URL derived from ${endpoint}`,
            ).toEqual([]);
            expect(result.isError).toBe(true);
          }
        } finally {
          vi.unstubAllEnvs();
        }
      },
    );

    it('hands a public https authorization URL to the browser', async () => {
      const { state, browserUrls } = harness('https://idp.example.com/authorize');
      await state.handle('authenticate');
      expect(browserUrls).toHaveLength(1);
      expect(browserUrls[0]).toMatch(/^https:\/\/idp\.example\.com\/authorize\?/);
    });

    // Unit-level edge cases for the validator itself (conformance covers
    // the integration path through `authenticate`).
    describe('assertSafeAuthorizationEndpoint edge cases', () => {
      const strictEnv = {} as NodeJS.ProcessEnv;
      const REJECT = [
        'javascript:alert(1)',
        'data:text/html,<script>',
        'file:///etc/passwd',
        'http://169.254.169.254/', // link-local
        'https://10.0.0.4/authorize', // private
        'https://172.16.8.1/authorize', // private 172.16/12
        'https://100.64.1.1/authorize', // CGNAT
        'https://127.0.0.1/authorize', // loopback without the hatch
        'https://[::1]/authorize', // v6 loopback
        'https://[fc00::1]/authorize', // v6 unique-local
        'https://[fe80::1]/authorize', // v6 link-local
        'https://[::ffff:192.168.0.1]/authorize', // v4-mapped private
        'https://localhost:8888/authorize', // named loopback without the hatch
        'not a url',
      ];
      for (const endpoint of REJECT) {
        it(`rejects ${endpoint}`, () => {
          expect(() => assertSafeAuthorizationEndpoint(endpoint, strictEnv)).toThrow();
        });
      }
      it('accepts a public https endpoint', () => {
        expect(() =>
          assertSafeAuthorizationEndpoint('https://idp.example.com/authorize', strictEnv),
        ).not.toThrow();
      });
      it('accepts a loopback http endpoint only with the escape hatch', () => {
        const hatchEnv = { QUARTO_HUB_MCP_ALLOW_INSECURE_AUTH: '1' } as NodeJS.ProcessEnv;
        expect(() =>
          assertSafeAuthorizationEndpoint('http://127.0.0.1:8888/authorize', hatchEnv),
        ).not.toThrow();
        expect(() =>
          assertSafeAuthorizationEndpoint('http://127.0.0.1:8888/authorize', strictEnv),
        ).toThrow();
      });
    });
  });
});

// ============================================================================
// Cancellation hygiene (BP-3)
// ============================================================================

describe('cancellation hygiene (BP-3)', () => {
  // Phase 1 threads ctx.mcpReq.signal through handleTool →
  // ConnectionManager.waitForChange: the cancelled call's waiter (and
  // its timeout timer) is unregistered promptly, not when the timeout
  // fires. Landed red-by-construction as `it.fails` in Phase 0; flipped
  // when the fix landed in Phase 1 (bd-zv8u2sxi).
  it(
    'cancelling wait_for_change mid-poll resolves as cancelled and frees the waiter',
    async () => {
      const f = await startInMemoryMcp();
      try {
        const seed = await seedProject(f, [{ path: 'live.qmd', content: 'v1\n' }]);

        const controller = new AbortController();
        const call = f.client.callTool(
          {
            name: 'wait_for_change',
            arguments: { project: seed.indexDocId, path: 'live.qmd', timeout_seconds: 3 },
          },
          { signal: controller.signal },
        );

        // Wait until the server has actually registered the waiter
        // (first-time connect pays a probe + WS join; poll instead of
        // sleeping a fixed beat).
        await vi.waitFor(
          () => {
            expect(f.manager.pendingWaiterCount(seed.indexDocId)).toBe(1);
          },
          { timeout: 10000, interval: 25 },
        );

        controller.abort();
        await expect(call).rejects.toThrow(/abort|cancel/i);

        // The cancelled call's waiter must be unregistered promptly —
        // not when its 3 s timeout eventually fires.
        await vi.waitFor(
          () => {
            expect(f.manager.pendingWaiterCount(seed.indexDocId)).toBe(0);
          },
          { timeout: 1000, interval: 25 },
        );
      } finally {
        await f.close();
      }
    },
    30000,
  );
});

// ============================================================================
// Tool titles (BP-9)
// ============================================================================

describe('tool titles (BP-9)', () => {
  it('every listed tool carries a human-friendly title', async () => {
    const f = await startInMemoryMcp();
    try {
      const { tools } = await f.client.listTools();
      const titles = Object.fromEntries(tools.map((t) => [t.name, t.title]));
      expect(titles).toEqual({
        connect_project: 'Connect to a project',
        list_files: 'List files',
        read_file: 'Read a file',
        wait_for_change: 'Watch for changes',
        write_file: 'Write a file',
        patch_file: 'Patch a file',
        create_file: 'Create a file',
        delete_file: 'Delete a file',
        rename_file: 'Rename a file',
        create_project: 'Create a project',
      });
    } finally {
      await f.close();
    }
  });

  it('auth tools carry titles too', async () => {
    // Auth tools register only with OAuth env configured; the definition
    // table is the wire source, so assert it directly.
    const titles = Object.fromEntries(
      AUTH_TOOL_DEFINITIONS.map((t) => [t.name, (t as { title?: string }).title]),
    );
    expect(titles).toEqual({
      authenticate: 'Sign in to Quarto Hub',
      authenticate_clear: 'Clear Quarto Hub credentials',
      authenticate_status: 'Check Quarto Hub sign-in status',
    });
  });
});
