#!/usr/bin/env node
// Reserve a new @quarto/* package name on npm with a `0.0.0` placeholder
// (bd-korgbhew).
//
//   node scripts/npm-publish/bootstrap-placeholder.mjs [--dry-run] <workspace-package-dir>
//
// npm trusted publishing is configured per package, and only on a package
// that already exists on the registry. So the first version of every new
// name has to be published by a person, once. This script publishes the
// smallest such version: `0.0.0`, with a README saying what it is and no
// code. Real versions are then staged by `.github/workflows/publish-npm.yml`
// through trusted publishing. Once the first real version is approved,
// deprecate the placeholder:
//
//   npm deprecate <name>@0.0.0 "placeholder; use a later version"
//
// Run it from your own machine, logged in (`npm login`) with an account that
// can publish to the @quarto scope; npm prompts for 2FA. Refuses names that
// already exist on the registry and private packages.

import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { PUBLISH_REPOSITORY } from './manifest.mjs';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

const argv = process.argv.slice(2);
const dryRun = argv.includes('--dry-run');
const dirs = argv.filter((a) => a !== '--dry-run');
if (dirs.length !== 1) {
  console.error('Usage: node scripts/npm-publish/bootstrap-placeholder.mjs [--dry-run] <workspace-package-dir>');
  process.exit(1);
}

const dir = relative(repoRoot, resolve(dirs[0]));
const pkg = JSON.parse(readFileSync(join(repoRoot, dir, 'package.json'), 'utf8'));
if (pkg.private) {
  console.error(`${pkg.name} is private; refusing to reserve it`);
  process.exit(1);
}
if (!pkg.name?.startsWith('@quarto/')) {
  console.error(`${pkg.name} is not in the @quarto scope`);
  process.exit(1);
}

let exists = true;
try {
  execFileSync('npm', ['view', pkg.name, 'name'], { stdio: ['ignore', 'pipe', 'pipe'] });
} catch {
  exists = false; // E404
}
if (exists) {
  console.error(`${pkg.name} already exists on the registry; no placeholder needed`);
  process.exit(1);
}

const stage = mkdtempSync(join(tmpdir(), 'q2-npm-placeholder-'));
writeFileSync(
  join(stage, 'package.json'),
  JSON.stringify(
    {
      name: pkg.name,
      version: '0.0.0',
      description: `Placeholder for ${pkg.name}. Real releases are published from quarto-dev/q2.`,
      license: pkg.license ?? 'MIT',
      repository: { type: 'git', url: PUBLISH_REPOSITORY, directory: dir },
      files: ['README.md'],
      publishConfig: { access: 'public' },
    },
    null,
    2,
  ) + '\n',
);
writeFileSync(
  join(stage, 'README.md'),
  `# ${pkg.name} (placeholder)\n\n` +
    'This version reserves the package name so that later versions can be published\n' +
    'from GitHub Actions with npm trusted publishing. It contains no code; install a\n' +
    'later version. Source: https://github.com/quarto-dev/q2/tree/main/' +
    `${dir}\n`,
);

const publishArgs = ['publish', '--access', 'public', ...(dryRun ? ['--dry-run'] : [])];
console.log(`$ npm ${publishArgs.join(' ')}   (in ${stage})`);
try {
  execFileSync('npm', publishArgs, { cwd: stage, stdio: 'inherit' });
} finally {
  rmSync(stage, { recursive: true, force: true });
}
if (!dryRun) {
  console.log(`\nReserved ${pkg.name}@0.0.0. Next: configure its trusted publisher (see publish-npm.yml).`);
}
