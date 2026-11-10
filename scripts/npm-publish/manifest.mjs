// Publish-manifest transform for q2's npm packages (bd-korgbhew).
//
// Workspace package.json files are shaped for in-repo development: `types`
// and the `source` export condition point at `src/*.ts` so hub-client and
// q2-preview-spa can bundle from source without a build, and workspace
// siblings are depended on as `"*"`. None of that works from a registry
// tarball, which ships only `dist/`. `publishManifest` derives the manifest
// that goes into the tarball; the workspace file is never modified.

/** The repository every q2 package is published from. npm provenance
 * verification rejects a publish whose `repository.url` does not match the
 * GitHub repository the workflow ran in. */
export const PUBLISH_REPOSITORY = 'git+https://github.com/quarto-dev/q2.git';

// Fields copied verbatim when present. Anything not listed (scripts,
// devDependencies, workspaces, private tooling keys) is dropped.
const COPIED_FIELDS = [
  'name',
  'version',
  'description',
  'keywords',
  'license',
  'author',
  'contributors',
  'homepage',
  'bugs',
  'type',
  'main',
  'bin',
  'files',
  'sideEffects',
  'engines',
  'peerDependencies',
  'peerDependenciesMeta',
  'optionalDependencies',
];

/** `./dist/x.js` -> `./dist/x.d.ts` (also `.mjs` -> `.d.mts`). */
function declarationFor(jsPath) {
  if (jsPath.endsWith('.mjs')) return jsPath.slice(0, -4) + '.d.mts';
  if (jsPath.endsWith('.js')) return jsPath.slice(0, -3) + '.d.ts';
  throw new Error(`cannot derive a declaration file for "${jsPath}"`);
}

function withDotSlash(p) {
  return p.startsWith('./') ? p : `./${p}`;
}

function publishExports(exportsField, pkgName) {
  if (exportsField === undefined) return undefined;
  const out = {};
  for (const [subpath, target] of Object.entries(exportsField)) {
    if (typeof target === 'string') {
      out[subpath] = target;
      continue;
    }
    const { import: importPath, types: _types, source: _source, ...rest } = target;
    if (typeof importPath !== 'string') {
      throw new Error(`${pkgName}: export "${subpath}" has no "import" condition to publish`);
    }
    // `types` must come first: TypeScript takes the first matching condition.
    out[subpath] = { types: declarationFor(importPath), import: importPath, ...rest };
  }
  return out;
}

function publishDependencies(deps, pkgName, ctx) {
  if (deps === undefined) return undefined;
  const out = {};
  for (const [dep, range] of Object.entries(deps)) {
    const siblingVersion = ctx.workspaceVersions.get(dep);
    if (siblingVersion === undefined) {
      out[dep] = range;
      continue;
    }
    const covered = ctx.publishSet.has(dep) || (ctx.alreadyPublished?.has(dep) ?? false);
    if (!covered) {
      throw new Error(
        `${pkgName}: depends on workspace package ${dep}, which is not in this publish run ` +
          `(add it, or pass it as already published)`,
      );
    }
    out[dep] = `^${siblingVersion}`;
  }
  return out;
}

/**
 * Derive the registry manifest from a workspace package.json.
 *
 * @param {object} pkg  the workspace package.json
 * @param {object} ctx
 * @param {string} ctx.dir  repo-relative package directory (for `repository.directory`)
 * @param {Map<string,string>} ctx.workspaceVersions  name -> version of every workspace package
 * @param {Set<string>} ctx.publishSet  names published in the same run
 * @param {Set<string>} [ctx.alreadyPublished]  workspace names known to be on the registry
 */
export function publishManifest(pkg, ctx) {
  if (pkg.private) {
    throw new Error(`${pkg.name} is private; refusing to publish it`);
  }
  const out = {};
  for (const field of COPIED_FIELDS) {
    if (pkg[field] !== undefined) out[field] = pkg[field];
  }
  if (typeof pkg.main === 'string') {
    out.types = declarationFor(withDotSlash(pkg.main));
  }
  const exportsField = publishExports(pkg.exports, pkg.name);
  if (exportsField !== undefined) out.exports = exportsField;
  const deps = publishDependencies(pkg.dependencies, pkg.name, ctx);
  if (deps !== undefined) out.dependencies = deps;
  out.repository = { type: 'git', url: PUBLISH_REPOSITORY, directory: ctx.dir };
  out.publishConfig = { access: 'public' };
  return out;
}

/** Order packages so each comes after the in-set packages it depends on. */
export function sortByDependencies(pkgs) {
  const byName = new Map(pkgs.map((p) => [p.name, p]));
  const done = new Set();
  const visiting = new Set();
  const order = [];
  const visit = (p) => {
    if (done.has(p.name)) return;
    if (visiting.has(p.name)) throw new Error(`dependency cycle through ${p.name}`);
    visiting.add(p.name);
    for (const dep of Object.keys(p.dependencies ?? {})) {
      const sibling = byName.get(dep);
      if (sibling) visit(sibling);
    }
    visiting.delete(p.name);
    done.add(p.name);
    order.push(p);
  };
  for (const p of pkgs) visit(p);
  return order;
}
