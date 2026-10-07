/**
 * Registry + one-click bundle manifests (CAP-15, bd-3tak0lyy).
 *
 * Two artifacts carry the server's public identity outside npm itself:
 *
 * - `server.json` (package root) — the MCP Registry listing. It must
 *   validate against the official registry schema (vendored at
 *   `schema/server.schema.json`; see `schema/README.md` for the source
 *   URL and refresh procedure — tests are offline, so the schema is a
 *   pinned copy, not a fetch) and stay in lockstep with `package.json`
 *   (name ↔ mcpName, version, npm identifier). `scripts/pack-npm.mjs`
 *   re-checks the same invariants at pack time; these tests are the
 *   always-on net.
 *
 * - `mcpb/manifest.json` — the one-click Claude Desktop bundle manifest.
 *   It must validate against the official mcpb v0.3 schema (read from
 *   the installed `@anthropic-ai/mcpb` package, no vendoring) and its
 *   one-click config must show the exact command a consent dialog
 *   presents (`node ${__dirname}/dist-bundle/index.mjs`) with
 *   credentials entering only through prompted `user_config` — the
 *   Security Best Practices consent rule for one-click installs.
 */

import { describe, it, expect } from 'vitest';
import { Ajv } from 'ajv';
import { existsSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const pkgRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(import.meta.url);

const sourcePkg = JSON.parse(readFileSync(join(pkgRoot, 'package.json'), 'utf8')) as {
  name: string;
  version: string;
  mcpName?: string;
};

describe('server.json (MCP Registry)', () => {
  const serverJsonPath = join(pkgRoot, 'server.json');

  it('exists and validates against the vendored registry schema', () => {
    expect(existsSync(serverJsonPath)).toBe(true);
    const schema = JSON.parse(
      readFileSync(join(pkgRoot, 'schema', 'server.schema.json'), 'utf8'),
    );
    const serverJson = JSON.parse(readFileSync(serverJsonPath, 'utf8'));
    const ajv = new Ajv({ strict: false, allErrors: true });
    const valid = ajv.validate(schema, serverJson);
    expect(ajv.errorsText(ajv.errors)).toBe('No errors');
    expect(valid).toBe(true);
  });

  it('stays in lockstep with package.json (name, version, npm entry)', () => {
    const serverJson = JSON.parse(readFileSync(serverJsonPath, 'utf8')) as {
      name: string;
      version: string;
      packages?: Array<{ registryType: string; identifier?: string; version?: string }>;
    };
    expect(serverJson.name).toBe(sourcePkg.mcpName);
    expect(serverJson.version).toBe(sourcePkg.version);
    const npmEntry = (serverJson.packages ?? []).find((p) => p.registryType === 'npm');
    expect(npmEntry?.identifier).toBe(sourcePkg.name);
    expect(npmEntry?.version).toBe(sourcePkg.version);
  });
});

describe('mcpb/manifest.json (one-click bundle)', () => {
  const manifestPath = join(pkgRoot, 'mcpb', 'manifest.json');

  it('exists and validates against the official mcpb v0.3 schema', () => {
    expect(existsSync(manifestPath)).toBe(true);
    const schemaPath = require.resolve(
      '@anthropic-ai/mcpb/mcpb-manifest-v0.3.schema.json',
    );
    const schema = JSON.parse(readFileSync(schemaPath, 'utf8'));
    const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'));
    const ajv = new Ajv({ strict: false, allErrors: true });
    const valid = ajv.validate(schema, manifest);
    expect(ajv.errorsText(ajv.errors)).toBe('No errors');
    expect(valid).toBe(true);
  });

  it('shows the exact command the one-click install runs', () => {
    const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as {
      version: string;
      server: {
        type: string;
        entry_point: string;
        mcp_config: { command: string; args?: string[]; env?: Record<string, string> };
      };
      compatibility?: { runtimes?: { node?: string } };
      user_config?: Record<
        string,
        { type: string; required?: boolean; sensitive?: boolean; default?: unknown }
      >;
    };
    // Nothing indirection can hide behind: the consent dialog shows
    // exactly this command line.
    expect(manifest.server.type).toBe('node');
    expect(manifest.server.mcp_config.command).toBe('node');
    expect(manifest.server.mcp_config.args).toEqual([
      '${__dirname}/dist-bundle/index.mjs',
    ]);
    expect(manifest.server.entry_point).toBe('dist-bundle/index.mjs');
    // The bundle targets node24; the manifest must say so up front.
    expect(manifest.compatibility?.runtimes?.node).toBe('>=24');
    // The bundle version tracks the package version.
    expect(manifest.version).toBe(sourcePkg.version);
  });

  it('routes credentials through prompted user_config, never baked defaults', () => {
    const manifest = JSON.parse(readFileSync(manifestPath, 'utf8')) as {
      server: { mcp_config: { env?: Record<string, string> } };
      user_config?: Record<
        string,
        { type: string; title: string; description: string; required?: boolean; sensitive?: boolean; default?: unknown }
      >;
    };
    const env = manifest.server.mcp_config.env ?? {};
    // Every env value must be a ${user_config.*} substitution — a baked
    // literal here would be a credential smuggled past the consent screen.
    for (const [key, value] of Object.entries(env)) {
      expect(value, `env ${key} must come from user_config`).toMatch(/^\$\{user_config\.\w+\}$/);
    }
    // The client secret is the sensitive one: prompted (consent), never
    // defaulted, masked by the host.
    const secretEntry = Object.values(manifest.user_config ?? {}).find(
      (c) => c.sensitive === true,
    );
    expect(secretEntry).toBeDefined();
    expect(secretEntry).not.toHaveProperty('default');
  });
});
