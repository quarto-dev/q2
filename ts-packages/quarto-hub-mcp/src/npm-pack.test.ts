/**
 * Packaging smoke test (CAP-15, bd-3tak0lyy): the npm tarball must build
 * from a clean tree, install into an empty prefix, and the installed
 * server must run — the `npx @quarto/hub-mcp` channel a user gets,
 * verified end to end before anything is published.
 *
 * The pack itself is `scripts/pack-npm.mjs`: it rebuilds the bundle from
 * source (HY-3 — esbuild compiles from src/, so a stale dist/ can never
 * leak in), stages a publish-shaped package (bin, minimal runtime deps,
 * LICENSE/NOTICE), and runs `npm pack` on the staging dir. The workspace
 * `dist-bundle/` keeps its dev state throughout: the publish copy strips
 * the vendored `@napi-rs` addons because npm delivers the right platform
 * package via the loader's own optionalDependencies. `wasm-qmd-parser`
 * stays vendored — it is ours and unpublished.
 *
 * Install-time network: the tarball itself is local; npm may fetch
 * `@napi-rs/keyring` from cache (`--prefer-offline`). After a workspace
 * `npm install` the loader + host addon are always cached, so this is
 * offline in practice on dev machines and in CI.
 */

import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { McpTestClient } from './mcp-test-client.js';
import { startTestHub } from './test-hub.js';

const pkgRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const sourcePkg = JSON.parse(readFileSync(join(pkgRoot, 'package.json'), 'utf8')) as {
  version: string;
};

describe('npm packaging (CAP-15)', () => {
  let tmp: string;
  let tarball: string;
  let installRoot: string;
  let installedPkg: string;
  let binPath: string;

  beforeAll(async () => {
    tmp = mkdtempSync(join(tmpdir(), 'hub-mcp-pack-'));
    execFileSync(
      process.execPath,
      [join(pkgRoot, 'scripts', 'pack-npm.mjs'), '--out', tmp],
      { stdio: ['ignore', 'pipe', 'pipe'] },
    );
    tarball = join(tmp, `quarto-hub-mcp-${sourcePkg.version}.tgz`);
    installRoot = join(tmp, 'install');
    execFileSync(
      'npm',
      [
        'install',
        '--prefix',
        installRoot,
        '--no-audit',
        '--no-fund',
        '--prefer-offline',
        tarball,
      ],
      { stdio: ['ignore', 'pipe', 'pipe'] },
    );
    installedPkg = join(installRoot, 'node_modules', '@quarto', 'hub-mcp');
    binPath = join(installRoot, 'node_modules', '.bin', 'hub-mcp');
  }, 300_000);

  afterAll(() => {
    rmSync(tmp, { recursive: true, force: true });
  });

  it('packs a tarball named for the package version', () => {
    expect(existsSync(tarball)).toBe(true);
  });

  it('installs a runnable bin and a self-contained payload', () => {
    expect(existsSync(binPath)).toBe(true);
    expect(existsSync(join(installedPkg, 'dist-bundle', 'index.mjs'))).toBe(true);
    // The qmd parser is ours and unpublished — it must ride inside the
    // bundle dir. The keyring addon must NOT: npm delivers the right
    // platform build via the loader's optionalDependencies, so the
    // installed package resolves it from its own node_modules.
    expect(
      existsSync(
        join(
          installedPkg,
          'dist-bundle',
          'node_modules',
          'wasm-qmd-parser',
          'wasm_qmd_parser_bg.wasm',
        ),
      ),
    ).toBe(true);
    expect(existsSync(join(installedPkg, 'dist-bundle', 'node_modules', '@napi-rs'))).toBe(
      false,
    );
    // npm hoists the loader to the prefix root; the bundle's upward
    // node_modules walk from dist-bundle/ finds it there.
    expect(existsSync(join(installRoot, 'node_modules', '@napi-rs', 'keyring'))).toBe(true);
    // Notices travel with the package (npm auto-includes LICENSE/README).
    expect(existsSync(join(installedPkg, 'LICENSE'))).toBe(true);
    expect(existsSync(join(installedPkg, 'NOTICE.md'))).toBe(true);
    expect(existsSync(join(installedPkg, 'README.md'))).toBe(true);
  });

  it('publishes a manifest shaped for the registry and npx', () => {
    const pkg = JSON.parse(readFileSync(join(installedPkg, 'package.json'), 'utf8')) as {
      name: string;
      version: string;
      mcpName?: string;
      bin: Record<string, string>;
      dependencies?: Record<string, string>;
      engines?: { node?: string };
    };
    expect(pkg.name).toBe('@quarto/hub-mcp');
    expect(pkg.version).toBe(sourcePkg.version);
    // The registry ownership link: server.json's name must match this.
    expect(pkg.mcpName).toBe('io.github.quarto-dev/hub-mcp');
    // The bin name matches the unscoped package name so bare
    // `npx @quarto/hub-mcp` resolves it unambiguously.
    expect(pkg.bin['hub-mcp']).toBe('./dist-bundle/index.mjs');
    // Everything else is bundled; the native keyring addon is the only
    // runtime dependency.
    expect(Object.keys(pkg.dependencies ?? {}).sort()).toEqual(['@napi-rs/keyring']);
    // The bundle targets node24 and is not version-guarded when run
    // standalone — the manifest must say so honestly.
    expect(pkg.engines?.node).toBe('>=24');
  });

  it('runs --help from the installed bin shim', () => {
    // The npx leg: the .bin shim must execute the bundle and exit 0 with
    // usage on stderr and a clean stdout (stdio-hygiene applies here too).
    const shim = process.platform === 'win32' ? `${binPath}.cmd` : binPath;
    const result = execFileSync(shim, ['--help'], {
      encoding: 'utf8',
      shell: process.platform === 'win32',
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    // execFileSync returns stdout; usage goes to stderr by convention.
    expect(result).toBe('');
  });

  it('serves tools/list against the in-process test-hub', async () => {
    const hub = await startTestHub();
    const client = new McpTestClient();
    try {
      await client.start(['--server', hub.url], {
        entry: join(installedPkg, 'dist-bundle', 'index.mjs'),
      });
      const response = await client.sendRequest('tools/list');
      const tools = (response.result as { tools: Array<{ name: string }> }).tools;
      const names = tools.map((t) => t.name);
      expect(names).toContain('connect_project');
      expect(names).toContain('read_file');
      expect(names).toContain('create_project');
    } finally {
      await client.stop();
      await hub.stop();
    }
  }, 60_000);

  it('reports the npm version on the MCP serverInfo record', async () => {
    const hub = await startTestHub();
    const client = new McpTestClient();
    try {
      await client.start(['--server', hub.url], {
        entry: join(installedPkg, 'dist-bundle', 'index.mjs'),
      });
      expect(client.serverInfo?.version).toBe(sourcePkg.version);
    } finally {
      await client.stop();
      await hub.stop();
    }
  }, 60_000);
});
