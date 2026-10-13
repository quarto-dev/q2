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
import { ToolSchema, ResourceSchema, PromptSchema } from '@modelcontextprotocol/core';
import type { ProjectSetDocument } from '@quarto/quarto-sync-client';

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
import { resetDocsCorpusForTests } from './docs-tool.js';
import { PARSER_UNAVAILABLE } from './test-setup.js';
import { realWasmHostAvailable } from './test-wasm-host.js';

/** Tool budget (ERG-5): the default listing never exceeds this. */
const TOOL_BUDGET = 10;

/**
 * Read-write mode lists these 7 data tools (bd-lj6f7z9s: the 21
 * same-subject verbs consolidated into four multi-action tools —
 * `project`, `read`, `edit` — plus the deliberately standalone
 * `wait_for_change`, `get_errors`, `docs`, `create_project`). The auth
 * tool adds to this when OAuth env is configured — see
 * EXPECTED_AUTH_TOOLS and the production-posture budget test.
 */
const EXPECTED_RW_TOOLS = [
  'create_project',
  'docs',
  'edit',
  'get_errors',
  'project',
  'read',
  'wait_for_change',
];

/**
 * The auth surface (bd-lj6f7z9s: `authenticate`/`authenticate_clear`
 * folded into one `auth` tool selected by `action`: `sign_in` no-ops
 * with "Already authenticated as <email>" when credentials are valid,
 * so a separate status probe spends a listing slot on nothing). It
 * registers only when OAuth env is configured; the fixture registers
 * it via a stubbed AuthToolsState.
 */
const EXPECTED_AUTH_TOOLS = ['auth'];

/**
 * The merged-tool arms (bd-lj6f7z9s), as advertised on the wire: each
 * arm of a merged tool's `oneOf` input schema carries
 * `properties.action.const`. The golden-case coverage net uses this to
 * require a golden call per (tool, action) pair.
 */
type AdvertisedArms = Record<string, string[]>;
function advertisedArms(
  tools: ReadonlyArray<{ name: string; inputSchema: unknown }>,
): AdvertisedArms {
  const out: AdvertisedArms = {};
  for (const t of tools) {
    const schema = t.inputSchema as { oneOf?: Array<{ properties?: { action?: { const?: string } } }> };
    if (!Array.isArray(schema.oneOf)) continue;
    out[t.name] = schema.oneOf
      .map((arm) => arm.properties?.action?.const)
      .filter((a): a is string => typeof a === 'string')
      .sort();
  }
  return out;
}

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
      const listed = await callTool(f, 'read', { action: 'list', project: seed.indexDocId });
      expect(listed.isError).not.toBe(true);
      const listedBlock = listed.content[0];
      expect(listedBlock?.type).toBe('text');
      if (listedBlock?.type !== 'text') throw new Error('unreachable');
      const files = JSON.parse(listedBlock.text) as Array<{ path: string; type: string }>;
      // ERG-3: entries carry size/mimeType/lines alongside path/type.
      expect(files).toEqual([
        {
          path: 'index.qmd',
          type: 'text',
          size: Buffer.byteLength('---\ntitle: Conformance\n---\n\nHello\n', 'utf8'),
          mimeType: 'text/markdown',
          lines: 5,
        },
      ]);

      const read = await callTool(f, 'read', {
        action: 'read',
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
        'docs',
        'get_errors',
        'project',
        'read',
        'wait_for_change',
      ]);
    } finally {
      await ro.close();
    }
  });
});

// ============================================================================
// Capability advertisement (BP-5, BP-6, BP-9, BP-17)
// ============================================================================

describe('capability advertisement (Phase 5)', () => {
  it('declares honest capabilities: resources subscribe+listChanged; static lists declare no listChanged', async () => {
    const f = await startInMemoryMcp();
    try {
      const caps = f.client.getServerCapabilities();
      // Resources genuinely list-change (collaborators add/remove files)
      // and are subscribable; the tool and prompt lists are fixed at
      // construction, so claiming listChanged for them would be a lie
      // (and contradict the long cache hints, BP-17).
      expect(caps?.resources).toEqual({ subscribe: true, listChanged: true });
      expect(caps?.prompts).toEqual({ listChanged: false });
      expect(caps?.tools).toEqual({ listChanged: false });
      // BP-9: the Implementation record carries the Quarto icon.
      expect(f.client.getServerVersion()?.icons?.length).toBe(1);
      expect(f.client.getServerVersion()?.icons?.[0]?.mimeType).toBe('image/svg+xml');
    } finally {
      await f.close();
    }
  });

  it('resources/list and prompts/list entries validate against the SDK wire schemas', async () => {
    const f = await startInMemoryMcp();
    try {
      await seedProject(f, [{ path: 'a.qmd', content: 'x\n' }]);
      const { resources } = await f.client.listResources();
      expect(resources.length).toBeGreaterThan(0);
      for (const r of resources) {
        const check = ResourceSchema.safeParse(r);
        expect(
          check.success,
          `resource ${String(r.uri)} fails ResourceSchema: ${check.success ? '' : JSON.stringify(check.error.issues)}`,
        ).toBe(true);
      }
      const { prompts } = await f.client.listPrompts();
      expect(prompts.length).toBeGreaterThan(0);
      for (const p of prompts) {
        const check = PromptSchema.safeParse(p);
        expect(
          check.success,
          `prompt ${String(p.name)} fails PromptSchema: ${check.success ? '' : JSON.stringify(check.error.issues)}`,
        ).toBe(true);
      }
    } finally {
      await f.close();
    }
  });
});

// ============================================================================
// Tool budget (ERG-5)
// ============================================================================

describe('tool budget (ERG-5)', () => {
  it('default listing stays within the 10-tool ceiling', async () => {
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

  // The production posture (Phase 7, bd-1ilystml): with OAuth env
  // configured the auth tools register too — that is the listing real
  // users see, and the one the ceiling must hold against. The fixture
  // without OAuth env undercounts by the auth tools' number, which is
  // how the listing quietly grew past the budget once (25 > 24 at the
  // Phase 6 tip while this file asserted against 22).
  it('production listing (auth tools registered) stays within the ceiling', async () => {
    const f = await startInMemoryMcp({ authToolsState: stubAuthToolsState() });
    try {
      const tools = await f.client.listTools();
      const names = tools.tools.map((t) => t.name).sort();
      expect(names).toEqual([...EXPECTED_RW_TOOLS, ...EXPECTED_AUTH_TOOLS].sort());
      expect(tools.tools.length).toBeLessThanOrEqual(TOOL_BUDGET);
    } finally {
      await f.close();
    }
  });

  it('read-only production listing also stays within the ceiling', async () => {
    const f = await startInMemoryMcp({ readOnly: true, authToolsState: stubAuthToolsState() });
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
 * `structuredContent` is validated against the declared schema. For the
 * merged tools (bd-lj6f7z9s) the net is per (tool, action) arm: every
 * advertised arm needs a golden call (dispatch-equivalence through the
 * wire).
 *
 * Cases run sequentially against one seeded project (`index.qmd`
 * present); later cases may depend on earlier ones (create → delete).
 */
const GOLDEN_RESULT_CASES: ReadonlyArray<{
  tool: string;
  args: (seed: SeededProject) => Record<string, unknown>;
  /** Extra args needing fixture/seed access (e.g. a doc minted on the hub). */
  setup?: (f: InMemoryMcpFixture, seed: SeededProject) => Promise<Record<string, unknown>>;
  /**
   * The call needs the wasm-qmd-parser (CAP-11). Toolchain-less CI legs
   * (workspace-ts-suites) leave the parser unbuilt and the tool errors by
   * design, so the execution loop skips these cases there — the same
   * gated-tier pattern as the qmd-* test files. The coverage test is
   * unaffected: the case stays registered either way.
   */
  requiresParser?: true;
  /**
   * The call needs the real prebundled WASM render host (get_errors).
   * Same gated-tier pattern as requiresParser, keyed on
   * realWasmHostAvailable().
   */
  requiresWasmHost?: true;
}> = [
  { tool: 'project', args: (seed) => ({ action: 'connect', project: seed.indexDocId }) },
  { tool: 'read', args: (seed) => ({ action: 'list', project: seed.indexDocId }) },
  {
    tool: 'read',
    args: (seed) => ({ action: 'read', project: seed.indexDocId, path: 'index.qmd' }),
  },
  {
    // Before edit/rename moves index.qmd: the golden calls run in order.
    tool: 'read',
    args: (seed) => ({ action: 'history', project: seed.indexDocId, path: 'index.qmd' }),
  },
  {
    // index.qmd has no headings — an empty outline is a valid result.
    tool: 'read',
    requiresParser: true,
    args: (seed) => ({ action: 'outline', project: seed.indexDocId, path: 'index.qmd' }),
  },
  {
    // The docs corpus comes from a fake q2 stub so the case is offline and
    // embed-state-independent (CAP-13).
    tool: 'docs',
    args: () => ({ query: 'alpha' }),
    setup: async () => {
      const { mkdtempSync, writeFileSync, chmodSync } = await import('node:fs');
      const { tmpdir } = await import('node:os');
      const { join } = await import('node:path');
      const dir = mkdtempSync(join(tmpdir(), 'conformance-fake-q2-'));
      const stub = join(dir, 'fake-q2.mjs');
      writeFileSync(
        stub,
        '#!/usr/bin/env node\n' +
          'if (process.argv.includes("--full")) {\n' +
          '  process.stdout.write("---\\ntitle: Alpha\\nurl: alpha.md\\n---\\n\\n# Alpha\\n\\nalpha body.\\n");\n' +
          '  process.exit(0);\n' +
          '}\nprocess.exit(2);\n',
      );
      chmodSync(stub, 0o755);
      process.env['QUARTO_Q2_PATH'] = stub;
      resetDocsCorpusForTests();
      return {};
    },
  },
  {
    tool: 'wait_for_change',
    args: (seed) => ({ project: seed.indexDocId, path: 'index.qmd', timeout_seconds: 1 }),
  },
  {
    tool: 'edit',
    args: (seed) => ({
      action: 'write',
      project: seed.indexDocId,
      path: 'index.qmd',
      content: 'v2\n',
    }),
  },
  {
    tool: 'edit',
    args: (seed) => ({
      action: 'patch',
      project: seed.indexDocId,
      path: 'index.qmd',
      old_string: 'v2',
      new_string: 'v3',
    }),
  },
  {
    // Renders index.qmd for real — before edit/rename moves it below.
    tool: 'get_errors',
    requiresWasmHost: true,
    args: (seed) => ({ project: seed.indexDocId, path: 'index.qmd' }),
  },
  {
    tool: 'edit',
    args: (seed) => ({ action: 'delete', project: seed.indexDocId, path: 'golden-new.qmd' }),
    setup: async (f, seed) => {
      // Phase 7: the file comes from the write arm's create arm
      // (create_file is gone — its refuse-if-exists semantics live on as
      // create_only, exercised in create-only.test.ts).
      await callTool(f, 'edit', {
        action: 'write',
        project: seed.indexDocId,
        path: 'golden-new.qmd',
        content: 'new\n',
      });
      return {};
    },
  },
  {
    tool: 'edit',
    args: (seed) => ({
      action: 'rename',
      project: seed.indexDocId,
      old_path: 'index.qmd',
      new_path: 'golden-renamed.qmd',
    }),
  },
  {
    tool: 'edit',
    args: (seed) => ({ action: 'create_folder', project: seed.indexDocId, path: 'golden-folder' }),
  },
  {
    tool: 'edit',
    args: (seed) => ({ action: 'delete_folder', project: seed.indexDocId, path: 'golden-folder' }),
  },
  {
    tool: 'read',
    args: (seed) => ({ action: 'search', project: seed.indexDocId, query: 'v3' }),
  },
  {
    // The include arm doubles as the presence golden case (Phase 7:
    // list_presence folded into the info arm). No peers broadcast
    // here, so the arm validates as an empty list + guidance message.
    tool: 'project',
    args: (seed) => ({ action: 'info', project: seed.indexDocId, include: ['presence'] }),
  },
  {
    tool: 'project',
    args: () => ({ action: 'list_sets' }),
    setup: async (f) => {
      // Mint a project-set document on the hub (CAP-3).
      const handle = f.hub.repo.create<ProjectSetDocument>();
      handle.change((d) => {
        d.version = 1;
        d.projects = {};
      });
      return { project_set: handle.documentId };
    },
  },
  {
    tool: 'project',
    args: () => ({ action: 'set_default_set' }),
    setup: async (f) => {
      // Mint a project-set document on the hub (bd-3h5hbfdv); the store
      // write lands in the fixture's in-memory keyring backend.
      const handle = f.hub.repo.create<ProjectSetDocument>();
      handle.change((d) => {
        d.version = 1;
        d.projects = {};
      });
      return { project_set: handle.documentId };
    },
  },
  {
    // Needs a real change head, discovered in setup against its own project.
    tool: 'edit',
    args: () => ({ action: 'restore' }),
    setup: async (f) => {
      const created = await callTool(f, 'create_project', {
        files: [{ path: 'r.qmd', content: 'r1\n' }],
      });
      const createdBlock = created.content[0];
      if (createdBlock?.type !== 'text') throw new Error('unreachable');
      const id = (JSON.parse(createdBlock.text) as { indexDocId: string }).indexDocId;
      await callTool(f, 'edit', { action: 'write', project: id, path: 'r.qmd', content: 'r2\n' });
      const hist = await callTool(f, 'read', { action: 'history', project: id, path: 'r.qmd' });
      const histBlock = hist.content[0];
      if (histBlock?.type !== 'text') throw new Error('unreachable');
      const entries = (JSON.parse(histBlock.text) as { entries: Array<{ head: string }> })
        .entries;
      return { project: id, path: 'r.qmd', hash: entries[entries.length - 1]!.head };
    },
  },
  // project/disconnect runs last: it drops the seed project's connection
  // (later golden calls would transparently reconnect anyway).
  {
    tool: 'project',
    args: (seed) => ({ action: 'disconnect', project: seed.indexDocId }),
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

  // bd-lj6f7z9s: the merged tools' dispatch net. Every advertised arm
  // (each `oneOf` branch's `action` const, read off the wire) needs a
  // golden call — that is what keeps the (tool, action) → handler
  // mapping honest per arm, not just per tool.
  it('registers a golden case for every advertised (tool, action) arm', async () => {
    const f = await startInMemoryMcp();
    try {
      const { tools } = await f.client.listTools();
      const arms = advertisedArms(tools);
      expect(
        Object.keys(arms).sort(),
        'the merged tools advertise oneOf action arms',
      ).toEqual(['edit', 'project', 'read']);
      const covered = new Set(
        GOLDEN_RESULT_CASES.map((c) => `${c.tool}:${String(c.args({ indexDocId: '', files: [] } as SeededProject)['action'] ?? '')}`),
      );
      for (const [tool, actions] of Object.entries(arms)) {
        for (const action of actions) {
          expect(
            covered.has(`${tool}:${action}`),
            `${tool} with action "${action}" is advertised but has no golden result case`,
          ).toBe(true);
        }
      }
    } finally {
      await f.close();
    }
  });

  // The BP-1 contract, directly: structuredContent matching the
  // declared outputSchema, with the JSON text fallback retained.
  it('read with action "list" carries structuredContent matching its outputSchema plus the JSON text fallback', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const { tools } = await f.client.listTools();
      const declared = tools.find((t) => t.name === 'read')?.outputSchema;
      expect(declared, 'read declares no outputSchema').toBeDefined();

      const result = await callTool(f, 'read', { action: 'list', project: seed.indexDocId });
      expect(result.isError).not.toBe(true);
      // Text fallback retained: the bare JSON array, as today.
      const block = result.content[0];
      expect(block?.type).toBe('text');
      if (block?.type !== 'text') throw new Error('unreachable');
      const fromText = JSON.parse(block.text) as Array<{ path: string; type: string }>;
      // ERG-3 shape: metadata rides every entry.
      expect(fromText).toEqual([
        { path: 'index.qmd', type: 'text', size: 2, mimeType: 'text/markdown', lines: 1 },
      ]);
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
        // Parser-tier cases cannot succeed without the parser build (the
        // tool errors by design) — skip them on toolchain-less legs.
        if (c.requiresParser && PARSER_UNAVAILABLE) continue;
        // Render-tier cases likewise need the real prebundled WASM host.
        if (c.requiresWasmHost && !realWasmHostAvailable()) continue;
        const declared = tools.find((t) => t.name === c.tool)?.outputSchema;
        if (declared === undefined) {
          throw new Error(`golden case for ${c.tool}: tool declares no outputSchema`);
        }
        const extra = c.setup ? await c.setup(f, seed) : {};
        const result = await callTool(f, c.tool, { ...c.args(seed), ...extra });
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

/**
 * An AuthToolsState with stubbed deps, for tests that need the auth
 * tools registered (the production listing) without an OAuth flow.
 * Construction has no side effects; calling the tools through this
 * state is not supported (the BP-18 harness below exercises the flow).
 */
function stubAuthToolsState(): AuthToolsState {
  return new AuthToolsState({
    credentialStore: seededStore(),
    refreshManager: stubRefreshManager(),
    connectionManager: { lastObservedAuthMode: () => 'unknown' },
    flowConfig: {
      clientId: 'test-client',
      clientSecret: 'test-secret',
      issuer: 'https://idp.example.com',
    },
    authServer: async () => ({
      issuer: 'https://idp.example.com',
      authorization_endpoint: 'https://idp.example.com/authorize',
      token_endpoint: 'https://idp.example.com/token',
    }),
    startListener: async () => {
      throw new Error('stub: no listener');
    },
    openBrowser: () => undefined,
    logger: () => {},
  });
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
            const result = await state.handle('sign_in');
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
      await state.handle('sign_in');
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
        project: 'Connect to and inspect projects',
        read: 'List, read, and search project files',
        edit: 'Edit project files and folders',
        docs: 'Search Quarto documentation',
        get_errors: 'Check a project for errors',
        wait_for_change: 'Watch for changes',
        create_project: 'Create a project',
      });
    } finally {
      await f.close();
    }
  });

  it('the auth tool carries a title too', async () => {
    // The auth tool registers only with OAuth env configured; the
    // definition table is the wire source, so assert it directly.
    const titles = Object.fromEntries(
      AUTH_TOOL_DEFINITIONS.map((t) => [t.name, (t as { title?: string }).title]),
    );
    expect(titles).toEqual({
      auth: 'Sign in or clear Quarto Hub credentials',
    });
  });
});

// ============================================================================
// Merged-tool schemas (bd-lj6f7z9s): oneOf arms keep per-arm required lists
// ============================================================================

describe('merged-tool input schemas (bd-lj6f7z9s)', () => {
  interface Arm {
    properties?: Record<string, { const?: string }>;
    required?: string[];
  }
  function armsOf(
    tools: ReadonlyArray<{ name: string; inputSchema: unknown }>,
    name: string,
  ): Record<string, Arm> {
    const schema = tools.find((t) => t.name === name)?.inputSchema as { oneOf?: Arm[] };
    expect(schema.oneOf, `${name} advertises a oneOf input schema`).toBeDefined();
    const out: Record<string, Arm> = {};
    for (const arm of schema.oneOf!) {
      const action = arm.properties?.['action']?.const;
      expect(typeof action).toBe('string');
      out[action as string] = arm;
    }
    return out;
  }

  it('read advertises its five arms with per-arm required lists', async () => {
    const f = await startInMemoryMcp();
    try {
      const { tools } = await f.client.listTools();
      const arms = armsOf(tools, 'read');
      expect(Object.keys(arms).sort()).toEqual(['history', 'list', 'outline', 'read', 'search']);
      expect(arms['list']!.required).toEqual(['action', 'project']);
      // `path` stays schema-optional: a share URL's `file=` fills it
      // post-validation (the PATH_DEFAULTABLE pattern).
      expect(arms['read']!.required).toEqual(['action', 'project']);
      expect(arms['outline']!.required).toEqual(['action', 'project']);
      expect(arms['search']!.required).toEqual(['action', 'project', 'query']);
      expect(arms['history']!.required).toEqual(['action', 'project']);
    } finally {
      await f.close();
    }
  });

  it('edit advertises its seven arms with per-arm required lists', async () => {
    const f = await startInMemoryMcp();
    try {
      const { tools } = await f.client.listTools();
      const arms = armsOf(tools, 'edit');
      expect(Object.keys(arms).sort()).toEqual([
        'create_folder',
        'delete',
        'delete_folder',
        'patch',
        'rename',
        'restore',
        'write',
      ]);
      expect(arms['write']!.required).toEqual(['action', 'project', 'content']);
      expect(arms['patch']!.required).toEqual(['action', 'project', 'new_string']);
      expect(arms['delete']!.required).toEqual(['action', 'project']);
      expect(arms['rename']!.required).toEqual(['action', 'project', 'old_path', 'new_path']);
      expect(arms['restore']!.required).toEqual(['action', 'project', 'hash']);
      expect(arms['create_folder']!.required).toEqual(['action', 'project', 'path']);
      expect(arms['delete_folder']!.required).toEqual(['action', 'project', 'path']);
    } finally {
      await f.close();
    }
  });

  it('project advertises its five arms with per-arm required lists', async () => {
    const f = await startInMemoryMcp();
    try {
      const { tools } = await f.client.listTools();
      const arms = armsOf(tools, 'project');
      expect(Object.keys(arms).sort()).toEqual([
        'connect',
        'disconnect',
        'info',
        'list_sets',
        'set_default_set',
      ]);
      expect(arms['connect']!.required).toEqual(['action', 'project']);
      expect(arms['disconnect']!.required).toEqual(['action', 'project']);
      expect(arms['info']!.required).toEqual(['action', 'project']);
      // list_sets' project_set is optional: omitted = remembered default.
      expect(arms['list_sets']!.required).toEqual(['action']);
      expect(arms['set_default_set']!.required).toEqual(['action', 'project_set']);
    } finally {
      await f.close();
    }
  });

  it('the merged input schemas compile as JSON Schema 2020-12 with top-level type object', async () => {
    const f = await startInMemoryMcp();
    try {
      const ajv = new Ajv2020({ allErrors: true });
      const { tools } = await f.client.listTools();
      for (const name of ['project', 'read', 'edit']) {
        const schema = tools.find((t) => t.name === name)?.inputSchema as Record<string, unknown>;
        // Hosts key on a top-level object; the arms ride `oneOf` beneath it.
        expect(schema['type'], `${name} inputSchema has top-level type`).toBe('object');
        expect(() => ajv.compile(schema), `${name} inputSchema compiles`).not.toThrow();
      }
    } finally {
      await f.close();
    }
  });
});

// ============================================================================
// Per-arm validation (bd-lj6f7z9s): a missing arm input errors actionably
// ============================================================================

describe('per-arm validation (bd-lj6f7z9s)', () => {
  it('read with action "read" and no path errors naming the parameter and the arm', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'read', { action: 'read', project: seed.indexDocId });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type !== 'text') throw new Error('unreachable');
      expect(block.text).toContain('`path`');
      expect(block.text).toContain('read');
      expect(block.text).toContain('action');
    } finally {
      await f.close();
    }
  });

  it('read with action "search" and no query fails input validation naming query', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'read', { action: 'search', project: seed.indexDocId });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type !== 'text') throw new Error('unreachable');
      expect(block.text).toContain('query');
    } finally {
      await f.close();
    }
  });

  it('edit with action "patch" and neither old_string nor section errors actionably', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'edit', {
        action: 'patch',
        project: seed.indexDocId,
        path: 'index.qmd',
        new_string: 'v2\n',
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type !== 'text') throw new Error('unreachable');
      expect(block.text).toContain('old_string');
      expect(block.text).toContain('section');
    } finally {
      await f.close();
    }
  });

  it('edit with action "write" and no content fails input validation naming content', async () => {
    const f = await startInMemoryMcp();
    try {
      const seed = await seedProject(f, [{ path: 'index.qmd', content: 'x\n' }]);
      const result = await callTool(f, 'edit', {
        action: 'write',
        project: seed.indexDocId,
        path: 'index.qmd',
      });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type !== 'text') throw new Error('unreachable');
      expect(block.text).toContain('content');
    } finally {
      await f.close();
    }
  });

  it('an unknown action fails input validation naming the action parameter', async () => {
    const f = await startInMemoryMcp();
    try {
      const result = await callTool(f, 'read', { action: 'frobnicate', project: 'x' });
      expect(result.isError).toBe(true);
      const block = result.content[0];
      if (block?.type !== 'text') throw new Error('unreachable');
      expect(block.text).toContain('action');
    } finally {
      await f.close();
    }
  });

  it('the auth tool dispatches its arms through the wire', async () => {
    const f = await startInMemoryMcp({ authToolsState: stubAuthToolsState() });
    try {
      // The stub's refresh manager resolves a token, so sign_in takes the
      // already-authenticated short-circuit — no browser, no listener.
      const signIn = await callTool(f, 'auth', { action: 'sign_in' });
      expect(signIn.isError).not.toBe(true);
      const signInBlock = signIn.content[0];
      if (signInBlock?.type !== 'text') throw new Error('unreachable');
      expect(signInBlock.text).toContain('Already authenticated');

      const cleared = await callTool(f, 'auth', { action: 'clear' });
      expect(cleared.isError).not.toBe(true);
      const clearedBlock = cleared.content[0];
      if (clearedBlock?.type !== 'text') throw new Error('unreachable');
      expect(clearedBlock.text).toContain('credentials cleared');
    } finally {
      await f.close();
    }
  });
});

// ============================================================================
// Merged-tool annotations (bd-lj6f7z9s): the read/write split is load-bearing
// ============================================================================

describe('merged-tool annotations (bd-lj6f7z9s)', () => {
  it('read keeps readOnlyHint: true; edit is destructive and non-idempotent', async () => {
    const f = await startInMemoryMcp();
    try {
      const { tools } = await f.client.listTools();
      const byName = Object.fromEntries(tools.map((t) => [t.name, t.annotations]));
      expect(byName['read']).toEqual({
        readOnlyHint: true,
        destructiveHint: false,
        idempotentHint: true,
      });
      expect(byName['edit']).toEqual({
        readOnlyHint: false,
        destructiveHint: true,
        idempotentHint: false,
      });
      expect(byName['project']).toEqual({
        readOnlyHint: false,
        destructiveHint: false,
        idempotentHint: true,
      });
    } finally {
      await f.close();
    }
  });

  it('the merged auth tool advertises destructiveHint: true (the clear arm revokes)', async () => {
    expect(AUTH_TOOL_DEFINITIONS).toHaveLength(1);
    expect(AUTH_TOOL_DEFINITIONS[0]!.annotations).toEqual({
      readOnlyHint: false,
      destructiveHint: true,
      idempotentHint: false,
    });
  });

  it('read-only mode registers no write tool', async () => {
    const f = await startInMemoryMcp({ readOnly: true });
    try {
      const { tools } = await f.client.listTools();
      const names = tools.map((t) => t.name);
      expect(names).not.toContain('edit');
      expect(names).not.toContain('create_project');
      // Every remaining tool is either hint-readOnly or a
      // local-state-only arm carrier (project: disconnect/set_default_set
      // mutate at most the keyring or connection state).
      for (const t of tools) {
        if (t.name === 'project') continue;
        expect(
          t.annotations?.readOnlyHint,
          `${t.name} in read-only mode should be hint-readOnly`,
        ).toBe(true);
      }
    } finally {
      await f.close();
    }
  });
});
