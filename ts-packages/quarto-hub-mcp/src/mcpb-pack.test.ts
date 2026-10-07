/**
 * `.mcpb` one-click bundle packaging (CAP-15, bd-3tak0lyy): the Claude
 * Desktop channel. `scripts/pack-mcpb.mjs` stages the bundle with the
 * keyring addon vendored for every supported platform (a `.mcpb` host
 * runs the payload as-is — no npm install) and packs it with the
 * official `@anthropic-ai/mcpb` tooling.
 *
 * Two legs:
 *
 * - host-only (`--platforms <host>`): offline; proves the staging,
 *   manifest, and payload shape.
 * - all-platforms: needs network for the `npm pack` fallback that
 *   fetches the non-host addons (the same mechanism the release
 *   workflow uses); skipped loudly when the registry is unreachable.
 *
 * The pack rebuilds the bundle, which stages the wasm-qmd-parser pkg —
 * so on toolchain-less legs (PARSER_UNAVAILABLE, e.g. CI's
 * workspace-ts-suites) the whole file skips, like bundle.test.ts, and
 * runs in the test-suite job's parser tier instead.
 */

import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { PARSER_UNAVAILABLE } from './test-setup.js';

const pkgRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
const require = createRequire(import.meta.url);
// The commander executable (bin), not the `./cli` library export —
// see scripts/pack-mcpb.mjs.
const mcpbCli = join(dirname(require.resolve('@anthropic-ai/mcpb/cli')), 'cli', 'cli.js');

const sourcePkg = JSON.parse(readFileSync(join(pkgRoot, 'package.json'), 'utf8')) as {
  version: string;
};

/** The host's `@napi-rs/keyring-*` platform suffix (musl/glibc and
 * exotic hosts fall back to skipping — the pack script's own fail-closed
 * checks cover staging correctness there). */
function hostKeyringPlatform(): string | undefined {
  const key = `${process.platform}-${process.arch}`;
  const map: Record<string, string> = {
    'darwin-x64': 'darwin-x64',
    'darwin-arm64': 'darwin-arm64',
    'linux-x64': 'linux-x64-gnu',
    'linux-arm64': 'linux-arm64-gnu',
    'win32-x64': 'win32-x64-msvc',
    'win32-arm64': 'win32-arm64-msvc',
  };
  return map[key];
}

const ALL_PLATFORMS = [
  'darwin-x64',
  'darwin-arm64',
  'linux-x64-gnu',
  'linux-x64-musl',
  'linux-arm64-gnu',
  'linux-arm64-musl',
  'win32-x64-msvc',
  'win32-arm64-msvc',
];

async function registryReachable(): Promise<boolean> {
  try {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 3000);
    const res = await fetch('https://registry.npmjs.org/@napi-rs/keyring/latest', {
      signal: controller.signal,
    });
    clearTimeout(timer);
    return res.ok;
  } catch {
    return false;
  }
}
const online = await registryReachable();
if (!online) {
  // eslint-disable-next-line no-console
  console.error('[mcpb-pack] SKIPPING all-platforms leg: registry.npmjs.org unreachable');
}

function hostPlatformKeyringDir(unpacked: string): string {
  return join(unpacked, 'dist-bundle', 'node_modules', '@napi-rs');
}

describe.skipIf(PARSER_UNAVAILABLE)('mcpb packaging (CAP-15)', () => {
  let tmp: string;
  const host = hostKeyringPlatform();

  beforeAll(() => {
    tmp = mkdtempSync(join(tmpdir(), 'hub-mcp-mcpb-'));
  }, 10_000);

  afterAll(() => {
    rmSync(tmp, { recursive: true, force: true });
  });

  it.skipIf(host === undefined)('packs a valid one-click bundle (host-platform leg)', () => {
    const out = join(tmp, 'host');
    execFileSync(
      process.execPath,
      [join(pkgRoot, 'scripts', 'pack-mcpb.mjs'), '--out', out, '--platforms', host!],
      { stdio: ['ignore', 'pipe', 'pipe'] },
    );
    const mcpbFile = join(out, `quarto-hub-mcp-${sourcePkg.version}.mcpb`);
    expect(existsSync(mcpbFile)).toBe(true);

    const unpacked = join(out, 'unpacked');
    execFileSync(process.execPath, [mcpbCli, 'unpack', mcpbFile, unpacked], {
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    // The manifest the host reads, identical to the reviewed source.
    const manifest = JSON.parse(readFileSync(join(unpacked, 'manifest.json'), 'utf8')) as {
      version: string;
      server: { mcp_config: { command: string; args: string[] } };
    };
    expect(manifest.version).toBe(sourcePkg.version);
    expect(manifest.server.mcp_config.command).toBe('node');
    expect(manifest.server.mcp_config.args).toEqual(['${__dirname}/dist-bundle/index.mjs']);

    // The payload runs as-is: bundle, host keyring addon, qmd parser.
    const entry = join(unpacked, 'dist-bundle', 'index.mjs');
    expect(existsSync(entry)).toBe(true);
    expect(readFileSync(entry, 'utf8').startsWith('#!/usr/bin/env node')).toBe(true);
    const keyringDir = join(hostPlatformKeyringDir(unpacked), `keyring-${host}`);
    expect(
      readdirSync(keyringDir).some((f) => f.endsWith('.node')),
      `expected a .node addon in ${keyringDir}`,
    ).toBe(true);
    expect(
      existsSync(
        join(
          unpacked,
          'dist-bundle',
          'node_modules',
          'wasm-qmd-parser',
          'wasm_qmd_parser_bg.wasm',
        ),
      ),
    ).toBe(true);

    // Notices describe the vendored layout (not the npm one).
    const notice = readFileSync(join(unpacked, 'NOTICE.md'), 'utf8');
    expect(notice).toContain('every supported platform');
    expect(existsSync(join(unpacked, 'LICENSE'))).toBe(true);
  }, 300_000);

  it.skipIf(!online || host === undefined)(
    'vendors the keyring addon for every release platform',
    () => {
      const out = join(tmp, 'all');
      execFileSync(
        process.execPath,
        [join(pkgRoot, 'scripts', 'pack-mcpb.mjs'), '--out', out],
        { stdio: ['ignore', 'pipe', 'pipe'] },
      );
      const unpacked = join(out, 'unpacked');
      execFileSync(
        process.execPath,
        [mcpbCli, 'unpack', join(out, `quarto-hub-mcp-${sourcePkg.version}.mcpb`), unpacked],
        { stdio: ['ignore', 'pipe', 'pipe'] },
      );
      for (const platform of ALL_PLATFORMS) {
        const dir = join(hostPlatformKeyringDir(unpacked), `keyring-${platform}`);
        expect(
          existsSync(dir) && readdirSync(dir).some((f) => f.endsWith('.node')),
          `missing keyring addon for ${platform}`,
        ).toBe(true);
      }
    },
    600_000,
  );
});
