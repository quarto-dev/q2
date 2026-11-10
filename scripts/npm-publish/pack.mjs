#!/usr/bin/env node
// Build, test, stage and pack q2 workspace packages for npm (bd-korgbhew).
//
//   node scripts/npm-publish/pack.mjs --out <dir> \
//     [--already-published <name>]... [--check-unpublished] <package-dir>...
//
// For each package (in dependency order):
//
// 1. `npm run clean` (if present) and `npm run build` in the workspace, then
//    `npm test` (if present). A failing suite stops the run: we never pack a
//    package whose own tests fail.
// 2. Stage a publish-shaped copy under `<out>/stage/<dir-name>/`: the
//    package's `files` entries, its README, the repo LICENSE, and a manifest
//    derived by `publishManifest` (manifest.mjs). The workspace is never
//    modified.
// 3. `npm pack` the staged copy into `<out>/`.
//
// Then a smoke test installs every tarball into an empty project in a fresh
// temporary directory outside the repo (registry dependencies come from the registry, in-run
// siblings from the tarballs) and checks that each package both imports in
// Node and type-checks with `tsc` (no skipLibCheck), so a broken `exports`
// map or a declaration that names an unshipped file fails here, not in a
// consumer's build.
//
// `<out>/packages.json` lists `{name, version, tarball}` in publish order;
// `.github/workflows/publish-npm.yml` stages the tarballs in that order.
//
// `--check-unpublished` fails when a package's version is already on the
// registry (npm rejects republishing a version, staged or not).
// A workspace sibling that is a dependency but not part of the run is looked
// up on the registry and must already be published at its workspace version;
// `--already-published <name>` asserts that without the lookup.

import { execFileSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { outOfRunWorkspaceDeps, publishManifest, sortByDependencies } from './manifest.mjs';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');

function usage(message) {
  if (message) console.error(message);
  console.error(
    'Usage: node scripts/npm-publish/pack.mjs --out <dir> ' +
      '[--already-published <name>]... [--check-unpublished] <package-dir>...',
  );
  process.exit(1);
}

function parseArgs(argv) {
  const args = { out: undefined, alreadyPublished: new Set(), checkUnpublished: false, dirs: [] };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === '--out') args.out = argv[++i];
    else if (a === '--already-published') args.alreadyPublished.add(argv[++i]);
    else if (a === '--check-unpublished') args.checkUnpublished = true;
    else if (a.startsWith('--')) usage(`Unknown option ${a}`);
    else args.dirs.push(a);
  }
  if (!args.out || args.dirs.length === 0) usage();
  return args;
}

function run(cmd, cmdArgs, opts = {}) {
  console.log(`$ ${cmd} ${cmdArgs.join(' ')}`);
  return execFileSync(cmd, cmdArgs, { stdio: 'inherit', cwd: repoRoot, ...opts });
}

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

/** Every package matched by the root `workspaces` globs: dir -> {name, version}. */
function workspacePackages() {
  const found = new Map();
  for (const glob of readJson(join(repoRoot, 'package.json')).workspaces ?? []) {
    const dirs = glob.endsWith('/*')
      ? readdirSync(join(repoRoot, glob.slice(0, -2)), { withFileTypes: true })
          .filter((e) => e.isDirectory())
          .map((e) => join(glob.slice(0, -2), e.name))
      : [glob];
    for (const dir of dirs) {
      const pj = join(repoRoot, dir, 'package.json');
      if (existsSync(pj)) {
        const { name, version } = readJson(pj);
        if (name) found.set(dir, { name, version });
      }
    }
  }
  return found;
}

function isPublished(name, version) {
  try {
    const outText = execFileSync('npm', ['view', `${name}@${version}`, 'version'], {
      encoding: 'utf8',
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    return outText.trim() === version;
  } catch {
    return false; // E404: the package or the version does not exist
  }
}

const args = parseArgs(process.argv);
const out = resolve(args.out);
const workspace = workspacePackages();
const versions = new Map([...workspace.values()].map(({ name, version }) => [name, version]));

const requested = args.dirs.map((d) => {
  const dir = relative(repoRoot, resolve(repoRoot, d));
  if (!workspace.has(dir)) usage(`${d} is not a workspace package directory`);
  const pkg = readJson(join(repoRoot, dir, 'package.json'));
  if (pkg.private) usage(`${pkg.name} (${dir}) is private; refusing to publish it`);
  return { dir, pkg };
});
const byName = new Map(requested.map((r) => [r.pkg.name, r]));
const ordered = sortByDependencies(requested.map((r) => r.pkg)).map((p) => byName.get(p.name));
const publishSet = new Set(byName.keys());

// A workspace sibling left out of the run must already be on the registry at
// the version the published manifest will depend on (`^<its version>`).
// Checked against the registry, so the workflow needs no extra input for it;
// `--already-published` skips the lookup (offline use, tests).
for (const { name, version } of outOfRunWorkspaceDeps(
  requested.map((r) => r.pkg),
  publishSet,
  versions,
)) {
  if (args.alreadyPublished.has(name)) continue;
  if (!isPublished(name, version)) {
    console.error(
      `${name}@${version} is a dependency of this run but is neither in it nor on the registry; ` +
        'add its directory to the run',
    );
    process.exit(1);
  }
  console.log(`${name}@${version} is already on the registry; depending on ^${version}`);
  args.alreadyPublished.add(name);
}

if (args.checkUnpublished) {
  const taken = ordered.filter(({ pkg }) => isPublished(pkg.name, pkg.version));
  if (taken.length > 0) {
    console.error(
      'Already on the registry (bump the version first):\n' +
        taken.map(({ pkg }) => `  ${pkg.name}@${pkg.version}`).join('\n'),
    );
    process.exit(1);
  }
}

rmSync(out, { recursive: true, force: true });
mkdirSync(join(out, 'stage'), { recursive: true });

const packed = [];
for (const { dir, pkg } of ordered) {
  console.log(`\n=== ${pkg.name}@${pkg.version} (${dir})`);
  const scripts = pkg.scripts ?? {};
  if (scripts.clean) run('npm', ['run', 'clean', '--workspace', dir]);
  run('npm', ['run', 'build', '--workspace', dir]);
  if (scripts.test) run('npm', ['test', '--workspace', dir]);

  const manifest = publishManifest(pkg, {
    dir,
    workspaceVersions: versions,
    publishSet,
    alreadyPublished: args.alreadyPublished,
  });

  const stage = join(out, 'stage', basename(dir));
  mkdirSync(stage, { recursive: true });
  for (const entry of manifest.files ?? []) {
    const src = join(repoRoot, dir, entry);
    if (!existsSync(src)) throw new Error(`${pkg.name}: "files" entry ${entry} does not exist after build`);
    cpSync(src, join(stage, entry), { recursive: true });
  }
  for (const doc of ['README.md', 'LICENSE']) {
    const own = join(repoRoot, dir, doc);
    const fallback = join(repoRoot, doc);
    if (existsSync(own)) cpSync(own, join(stage, doc));
    else if (doc === 'LICENSE' && existsSync(fallback)) cpSync(fallback, join(stage, doc));
  }
  writeFileSync(join(stage, 'package.json'), JSON.stringify(manifest, null, 2) + '\n');

  // Every path the manifest promises must be in the staged copy.
  const promised = [manifest.main, manifest.types];
  for (const target of Object.values(manifest.exports ?? {})) {
    if (typeof target === 'string') promised.push(target);
    else promised.push(...Object.values(target));
  }
  for (const p of promised.filter(Boolean)) {
    if (!existsSync(join(stage, p))) throw new Error(`${pkg.name}: manifest points at missing ${p}`);
  }

  const tarball = execFileSync('npm', ['pack', stage, '--pack-destination', out], {
    encoding: 'utf8',
  })
    .trim()
    .split('\n')
    .pop();
  packed.push({ name: pkg.name, version: pkg.version, tarball });
}

// --- smoke test: install the tarballs into an empty project -------------
console.log('\n=== smoke test');
// Outside the repo on purpose: inside it, Node and tsc would fall back to the
// repo's own node_modules (workspace symlinks included) and hide a dependency
// the tarball forgot to declare.
const smoke = mkdtempSync(join(tmpdir(), 'q2-npm-smoke-'));
writeFileSync(
  join(smoke, 'package.json'),
  JSON.stringify({ name: 'npm-publish-smoke', private: true, type: 'module' }, null, 2) + '\n',
);
run(
  'npm',
  ['install', '--no-audit', '--no-fund', '--ignore-scripts', ...packed.map((p) => join(out, p.tarball))],
  { cwd: smoke },
);
const importLines = packed.map((p, i) => `import * as m${i} from '${p.name}';\nvoid m${i};`);
writeFileSync(join(smoke, 'smoke.ts'), importLines.join('\n') + '\nexport {};\n');
writeFileSync(
  join(smoke, 'tsconfig.json'),
  JSON.stringify(
    {
      compilerOptions: {
        target: 'ES2022',
        module: 'nodenext',
        moduleResolution: 'nodenext',
        strict: true,
        noEmit: true,
        skipLibCheck: false,
        types: [],
      },
      files: ['smoke.ts'],
    },
    null,
    2,
  ) + '\n',
);
run('node', [join(repoRoot, 'node_modules', 'typescript', 'bin', 'tsc'), '-p', smoke]);
for (const p of packed) {
  run('node', ['--input-type=module', '-e', `await import('${p.name}'); console.log('imported ${p.name}');`], {
    cwd: smoke,
  });
}

rmSync(smoke, { recursive: true, force: true });

writeFileSync(join(out, 'packages.json'), JSON.stringify(packed, null, 2) + '\n');
console.log('\npacked (publish order):');
for (const p of packed) console.log(`  ${p.name}@${p.version}  ${p.tarball}`);
