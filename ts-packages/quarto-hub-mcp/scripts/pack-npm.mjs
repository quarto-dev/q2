/**
 * Build the npm publish tarball for `@quarto/hub-mcp` (CAP-15,
 * bd-3tak0lyy).
 *
 *   node scripts/pack-npm.mjs --out <dir>
 *
 * Steps:
 *
 * 1. Rebuild `dist-bundle/` from source (`npm run bundle` — esbuild
 *    compiles from src/ via the `source` condition and empties the
 *    output dir first, so the payload can never embed a stale dist/ —
 *    HY-3's clean-build rule applied to packaging).
 * 2. Stage a publish-shaped package under `<out>/stage/`:
 *    - `dist-bundle/` copied MINUS the vendored `@napi-rs` addons. The
 *      published package declares `@napi-rs/keyring` as its only runtime
 *      dependency and npm delivers the right platform build via the
 *      loader's own optionalDependencies; shipping all platforms inside
 *      the tarball would bloat it for no benefit. `wasm-qmd-parser`
 *      stays vendored — it is ours and unpublished.
 *    - a generated `package.json` (the workspace one is `private` and
 *      carries the dev dependency surface; the publish manifest is
 *      minimal: bin, files, engines, mcpName, one dependency),
 *    - `LICENSE` (repo root), `README.md` (package), `NOTICE.md`
 *      (generated third-party notices),
 *    - `build-info.json` gains `npmVersion` so the server reports the
 *      package version — not a git stamp — on its MCP Implementation
 *      record when installed from npm.
 * 3. `npm pack` the staging dir into `<out>`.
 *
 * The workspace `dist-bundle/` is never touched: the bundle builds
 * straight into the staging dir (`HUB_MCP_BUNDLE_OUT`), so packaging
 * can run concurrently with anything that spawns or embeds the
 * workspace bundle.
 *
 * When `server.json` exists next to this script (registry listing,
 * CAP-15), its `version` and npm-package entry are checked against the
 * package version — a drifted registry manifest fails the pack.
 */

import { execFileSync } from 'node:child_process';
import {
  cpSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { noticeText } from './notice-text.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const pkgRoot = join(here, '..');
const repoRoot = join(pkgRoot, '..', '..');

function parseArgs(argv) {
  let out;
  for (let i = 2; i < argv.length; i++) {
    if (argv[i] === '--out' && i + 1 < argv.length) {
      out = argv[++i];
    } else {
      console.error(`Unknown argument: ${argv[i]}`);
      process.exit(1);
    }
  }
  if (!out) {
    console.error('Usage: node scripts/pack-npm.mjs --out <dir>');
    process.exit(1);
  }
  return { out };
}

const { out } = parseArgs(process.argv);

const sourcePkg = JSON.parse(readFileSync(join(pkgRoot, 'package.json'), 'utf8'));

// --- registry manifest consistency (when present) ----------------------
const serverJsonPath = join(pkgRoot, 'server.json');
if (existsSync(serverJsonPath)) {
  const serverJson = JSON.parse(readFileSync(serverJsonPath, 'utf8'));
  const problems = [];
  if (serverJson.version !== sourcePkg.version) {
    problems.push(`version ${serverJson.version} != package.json ${sourcePkg.version}`);
  }
  const npmEntry = (serverJson.packages ?? []).find((p) => p.registryType === 'npm');
  if (!npmEntry) {
    problems.push('no npm package entry');
  } else {
    if (npmEntry.identifier !== sourcePkg.name) {
      problems.push(`npm identifier ${npmEntry.identifier} != ${sourcePkg.name}`);
    }
    if (npmEntry.version !== undefined && npmEntry.version !== sourcePkg.version) {
      problems.push(`npm entry version ${npmEntry.version} != ${sourcePkg.version}`);
    }
  }
  if (sourcePkg.mcpName === undefined || serverJson.name !== sourcePkg.mcpName) {
    problems.push(
      `server.json name ${serverJson.name} != package.json mcpName ${sourcePkg.mcpName}`,
    );
  }
  if (problems.length > 0) {
    console.error(`server.json is out of sync with package.json:\n  - ${problems.join('\n  - ')}`);
    process.exit(1);
  }
}

// --- 1. clean rebuild, straight into the staging dir -------------------
// Building into a private dir (not the workspace dist-bundle) is what
// makes this safe to run concurrently with tests that spawn the
// workspace bundle (see HUB_MCP_BUNDLE_OUT in scripts/bundle.mjs).
const stage = join(out, 'stage');
rmSync(stage, { recursive: true, force: true });
mkdirSync(stage, { recursive: true });

console.log('building bundle (npm run bundle)…');
execFileSync('npm', ['run', 'bundle'], {
  cwd: pkgRoot,
  stdio: 'inherit',
  env: { ...process.env, HUB_MCP_BUNDLE_OUT: join(stage, 'dist-bundle') },
});

// --- 2. stage -----------------------------------------------------------
// npm delivers the keyring addon per platform; the vendored copy in a
// dev bundle would only bloat the tarball.
rmSync(join(stage, 'dist-bundle', 'node_modules', '@napi-rs'), {
  recursive: true,
  force: true,
});

// The server reports the npm version (not a git stamp) when installed
// from the registry — see resolveServerVersion in src/index.ts.
const buildInfoPath = join(stage, 'dist-bundle', 'build-info.json');
const buildInfo = JSON.parse(readFileSync(buildInfoPath, 'utf8'));
buildInfo.npmVersion = sourcePkg.version;
writeFileSync(buildInfoPath, JSON.stringify(buildInfo, null, 2) + '\n');

const publishPkg = {
  name: sourcePkg.name,
  version: sourcePkg.version,
  description: sourcePkg.description,
  license: sourcePkg.license,
  author: sourcePkg.author,
  type: 'module',
  bin: { 'hub-mcp': './dist-bundle/index.mjs' },
  files: ['dist-bundle', 'NOTICE.md'],
  engines: { node: '>=24' },
  mcpName: sourcePkg.mcpName,
  repository: {
    type: 'git',
    url: 'https://github.com/quarto-dev/q2.git',
    directory: 'ts-packages/quarto-hub-mcp',
  },
  keywords: ['mcp', 'model-context-protocol', 'quarto', 'automerge'],
  dependencies: {
    '@napi-rs/keyring': sourcePkg.dependencies['@napi-rs/keyring'],
  },
};
if (publishPkg.mcpName === undefined) {
  console.error('package.json lacks mcpName — the registry ownership link is required (CAP-15)');
  process.exit(1);
}
writeFileSync(join(stage, 'package.json'), JSON.stringify(publishPkg, null, 2) + '\n');

cpSync(join(repoRoot, 'LICENSE'), join(stage, 'LICENSE'));
cpSync(join(pkgRoot, 'README.md'), join(stage, 'README.md'));

writeFileSync(join(stage, 'NOTICE.md'), noticeText({ vendoredKeyring: false }));

// --- 3. pack ------------------------------------------------------------
mkdirSync(out, { recursive: true });
execFileSync('npm', ['pack', '--pack-destination', out, stage], { stdio: 'inherit' });

console.log(`packed ${sourcePkg.name}@${sourcePkg.version} into ${out}`);
