/**
 * Build the `.mcpb` one-click bundle for Claude Desktop (CAP-15,
 * bd-3tak0lyy).
 *
 *   node scripts/pack-mcpb.mjs --out <dir> [--platforms a,b,c]
 *
 * A `.mcpb` host extracts the bundle and runs it — there is no npm
 * install step — so unlike the npm package this payload vendors the
 * `@napi-rs/keyring` addon for EVERY supported platform (the same
 * co-staging the release tarball uses, via `stage-keyring.mjs`).
 * Platforms not installed locally are fetched with `npm pack` at the
 * loader's exact version (network; `--platforms` narrows the list for
 * offline development/test runs).
 *
 * Steps: clean bundle rebuild → stage `dist-bundle/` + all-platform
 * keyring + `mcpb/manifest.json` + LICENSE/README/NOTICE → validate the
 * manifest → `mcpb pack`. The workspace `dist-bundle/` keeps its dev
 * (host-only) state; only the staged copy gains all platforms.
 */

import { execFileSync } from 'node:child_process';
import { cpSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { noticeText } from './notice-text.mjs';
import { parsePlatformList, stageKeyring } from './stage-keyring.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const pkgRoot = join(here, '..');
const repoRoot = join(pkgRoot, '..', '..');
const require = createRequire(import.meta.url);

/** Every platform the release workflow ships (release-pipeline.yml). */
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

function parseArgs(argv) {
  let out;
  let platformsRaw;
  for (let i = 2; i < argv.length; i++) {
    if (argv[i] === '--out' && i + 1 < argv.length) {
      out = argv[++i];
    } else if (argv[i] === '--platforms' && i + 1 < argv.length) {
      platformsRaw = argv[++i];
    } else {
      console.error(`Unknown argument: ${argv[i]}`);
      process.exit(1);
    }
  }
  if (!out) {
    console.error('Usage: node scripts/pack-mcpb.mjs --out <dir> [--platforms a,b,c]');
    process.exit(1);
  }
  return { out, platforms: parsePlatformList(platformsRaw) ?? ALL_PLATFORMS };
}

const { out, platforms } = parseArgs(process.argv);

const sourcePkg = JSON.parse(readFileSync(join(pkgRoot, 'package.json'), 'utf8'));
const manifest = JSON.parse(readFileSync(join(pkgRoot, 'mcpb', 'manifest.json'), 'utf8'));
if (manifest.version !== sourcePkg.version) {
  console.error(
    `mcpb/manifest.json version ${manifest.version} != package.json ${sourcePkg.version} — ` +
      'bump them together (src/registry.test.ts pins this).',
  );
  process.exit(1);
}

console.log('building bundle (npm run bundle)…');
execFileSync('npm', ['run', 'bundle'], { cwd: pkgRoot, stdio: 'inherit' });

const stage = join(out, 'stage-mcpb');
rmSync(stage, { recursive: true, force: true });
mkdirSync(stage, { recursive: true });

cpSync(join(pkgRoot, 'dist-bundle'), join(stage, 'dist-bundle'), { recursive: true });

const napiSrcDir = join(dirname(require.resolve('@napi-rs/keyring/package.json')), '..');
const staged = stageKeyring({
  napiSrcDir,
  outNapiDir: join(stage, 'dist-bundle', 'node_modules', '@napi-rs'),
  platforms,
});
console.log(`keyring staged: ${staged.join(', ')}`);

cpSync(join(pkgRoot, 'mcpb', 'manifest.json'), join(stage, 'manifest.json'));
cpSync(join(repoRoot, 'LICENSE'), join(stage, 'LICENSE'));
cpSync(join(pkgRoot, 'README.md'), join(stage, 'README.md'));
writeFileSync(join(stage, 'NOTICE.md'), noticeText({ vendoredKeyring: true }));

mkdirSync(out, { recursive: true });
// NB: the `./cli` subpath export is the LIBRARY (dist/cli.js), not the
// commander entry — resolving it runs nothing and exits 0 silently. The
// executable is its sibling, the package bin at dist/cli/cli.js.
const mcpbCli = join(dirname(require.resolve('@anthropic-ai/mcpb/cli')), 'cli', 'cli.js');
execFileSync(process.execPath, [mcpbCli, 'validate', join(stage, 'manifest.json')], {
  stdio: 'inherit',
});
const outFile = join(out, `quarto-hub-mcp-${sourcePkg.version}.mcpb`);
execFileSync(process.execPath, [mcpbCli, 'pack', stage, outFile], { stdio: 'inherit' });

console.log(`packed ${outFile}`);
