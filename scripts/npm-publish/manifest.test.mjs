// Unit tests for the publish-manifest transform (bd-korgbhew).
//
//   node --test scripts/npm-publish/*.test.mjs
//
// The workflow `.github/workflows/publish-npm.yml` runs these before it packs.

import { test } from 'node:test';
import assert from 'node:assert/strict';

import { publishManifest, sortByDependencies, PUBLISH_REPOSITORY } from './manifest.mjs';

const workspacePkg = {
  name: '@quarto/annotated-qmd',
  version: '0.2.0',
  description: 'Convert JSON',
  license: 'MIT',
  author: { name: 'Posit PBC' },
  publishConfig: { access: 'public', registry: 'https://registry.npmjs.org/' },
  repository: { type: 'git', url: 'git+https://github.com/quarto-dev/quarto-markdown.git' },
  type: 'module',
  main: 'dist/index.js',
  types: 'src/index.ts',
  exports: {
    '.': { types: './src/index.ts', source: './src/index.ts', import: './dist/index.js' },
  },
  files: ['dist'],
  scripts: { build: 'tsc', prepublishOnly: 'npm run build' },
  dependencies: { '@quarto/mapped-string': '^0.1.8', '@quarto/pandoc-types': '*' },
  devDependencies: { typescript: '^5.4.2' },
};

const ctx = {
  dir: 'ts-packages/annotated-qmd',
  // name -> version of every workspace package
  workspaceVersions: new Map([
    ['@quarto/annotated-qmd', '0.2.0'],
    ['@quarto/pandoc-types', '0.1.0'],
  ]),
  // names published in the same run
  publishSet: new Set(['@quarto/annotated-qmd', '@quarto/pandoc-types']),
};

test('points types at emitted declarations and drops the source condition', () => {
  const m = publishManifest(workspacePkg, ctx);
  assert.equal(m.types, './dist/index.d.ts');
  assert.deepEqual(m.exports, {
    '.': { types: './dist/index.d.ts', import: './dist/index.js' },
  });
});

test('types come first in each export condition object', () => {
  // TypeScript resolves conditions in object order; `types` must precede `import`.
  const m = publishManifest(workspacePkg, ctx);
  assert.equal(Object.keys(m.exports['.'])[0], 'types');
});

test('repository points at quarto-dev/q2 with the package directory (provenance checks it)', () => {
  const m = publishManifest(workspacePkg, ctx);
  assert.deepEqual(m.repository, {
    type: 'git',
    url: PUBLISH_REPOSITORY,
    directory: 'ts-packages/annotated-qmd',
  });
});

test('workspace dependencies become caret ranges on the sibling version', () => {
  const m = publishManifest(workspacePkg, ctx);
  assert.deepEqual(m.dependencies, {
    '@quarto/mapped-string': '^0.1.8',
    '@quarto/pandoc-types': '^0.1.0',
  });
});

test('a workspace dependency outside the publish set is an error', () => {
  const narrow = { ...ctx, publishSet: new Set(['@quarto/annotated-qmd']) };
  assert.throws(() => publishManifest(workspacePkg, narrow), /@quarto\/pandoc-types.*not in this publish run/);
});

test('a workspace dependency may stay out of the run when allowed as already published', () => {
  const narrow = {
    ...ctx,
    publishSet: new Set(['@quarto/annotated-qmd']),
    alreadyPublished: new Set(['@quarto/pandoc-types']),
  };
  const m = publishManifest(workspacePkg, narrow);
  assert.equal(m.dependencies['@quarto/pandoc-types'], '^0.1.0');
});

test('drops scripts, devDependencies and the source-only fields', () => {
  const m = publishManifest(workspacePkg, ctx);
  assert.equal(m.scripts, undefined);
  assert.equal(m.devDependencies, undefined);
  assert.deepEqual(m.publishConfig, { access: 'public' });
  assert.deepEqual(m.files, ['dist']);
});

test('refuses private packages', () => {
  assert.throws(() => publishManifest({ ...workspacePkg, private: true }, ctx), /private/);
});

test('refuses an export whose types cannot be derived from a dist import', () => {
  const bad = { ...workspacePkg, exports: { '.': { source: './src/index.ts' } } };
  assert.throws(() => publishManifest(bad, ctx), /no "import"/);
});

test('handles subpath exports', () => {
  const pkg = {
    ...workspacePkg,
    exports: {
      '.': { types: './src/index.ts', source: './src/index.ts', import: './dist/index.js' },
      './types': { types: './src/types.ts', source: './src/types.ts', import: './dist/types.js' },
    },
  };
  const m = publishManifest(pkg, ctx);
  assert.deepEqual(m.exports['./types'], { types: './dist/types.d.ts', import: './dist/types.js' });
});

test('sortByDependencies puts dependencies first', () => {
  const pkgs = [
    { name: '@quarto/annotated-qmd', dependencies: { '@quarto/pandoc-types': '*' } },
    { name: '@quarto/pandoc-types' },
  ];
  assert.deepEqual(
    sortByDependencies(pkgs).map((p) => p.name),
    ['@quarto/pandoc-types', '@quarto/annotated-qmd'],
  );
});

test('sortByDependencies rejects cycles', () => {
  const pkgs = [
    { name: 'a', dependencies: { b: '*' } },
    { name: 'b', dependencies: { a: '*' } },
  ];
  assert.throws(() => sortByDependencies(pkgs), /cycle/);
});
