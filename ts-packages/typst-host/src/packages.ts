import { MemoryAccessModel } from '@myriaddreamin/typst.ts/fs/memory';
import type { PackageResolveContext } from '@myriaddreamin/typst.ts/internal.types';
import { PACKAGE_REGISTRY } from './limits.ts';
import type { PackageFetcher, PackageSpec, TypstFile } from './types.ts';

export const specKey = (s: PackageSpec) => `${s.namespace}/${s.name}/${s.version}`;
export const specLabel = (s: PackageSpec) => `@${s.namespace}/${s.name}:${s.version}`;

const IMPORT = /"@([a-z][a-z0-9_-]*)\/([a-z0-9_-]+):(\d+\.\d+\.\d+)"/g;

/** `@namespace/name:x.y.z` strings in typst source, deduplicated, in first-seen order. */
export function packageImports(source: string): PackageSpec[] {
  const seen = new Map<string, PackageSpec>();
  for (const m of source.matchAll(IMPORT)) {
    const spec = { namespace: m[1], name: m[2], version: m[3] };
    seen.set(specKey(spec), spec);
  }
  return [...seen.values()];
}

/** The spec named by typst's "package not found" diagnostic, if that is what the message is. */
export function missingPackage(message: string): PackageSpec | undefined {
  const m = /package not found \(searched for @([a-z][a-z0-9_-]*)\/([a-z0-9_-]+):(\d+\.\d+\.\d+)\)/.exec(message);
  return m ? { namespace: m[1], name: m[2], version: m[3] } : undefined;
}

/**
 * Tarball URL for the registry: `<base>/<namespace>/<name>-<version>.tar.gz`. Only the
 * `preview` namespace is hosted; any other namespace has no URL.
 */
export function tarballUrl(spec: PackageSpec, base = PACKAGE_REGISTRY): string | undefined {
  if (spec.namespace !== 'preview') return undefined;
  return `${base}/preview/${spec.name}-${spec.version}.tar.gz`;
}

/** Cache of fetched tarballs (the Cache API in the browser; the shell supplies it). */
export interface TarballCache {
  get(url: string): Promise<Uint8Array | undefined>;
  put(url: string, bytes: Uint8Array): Promise<void>;
}

export interface RegistryFetcherOptions {
  fetch: typeof fetch;
  cache?: TarballCache;
  base?: string;
}

/**
 * The default fetcher: the Cache API first, then `packages.typst.org`. A 404 resolves
 * `undefined` (no such package); any other failure throws with a message naming the cause.
 */
export function registryFetcher(options: RegistryFetcherOptions): PackageFetcher {
  return async (spec, signal) => {
    const url = tarballUrl(spec, options.base);
    if (!url) return undefined;
    const hit = await options.cache?.get(url).catch(() => undefined);
    if (hit) return hit;
    const res = await options.fetch(url, { signal, credentials: 'omit' });
    if (res.status === 404) return undefined;
    if (!res.ok) throw new Error(`HTTP ${res.status} from ${url}`);
    const bytes = new Uint8Array(await res.arrayBuffer());
    await options.cache?.put(url, bytes).catch(() => undefined);
    return bytes;
  };
}

const MEMORY_ROOT = '/@memory/packages';

/**
 * Typst resolves packages synchronously, so everything a compile may need is in memory
 * before it starts: the vendored packages (their files) and fetched tarballs (bytes,
 * unpacked lazily by typst.ts's own `untar` when typst asks).
 */
export class PackageStore {
  readonly access = new MemoryAccessModel();
  private readonly vendored = new Map<string, TypstFile[]>();
  private readonly tarballs = new Map<string, Uint8Array>();
  private readonly mounted = new Map<string, string>();

  /** `files[].path` is relative to the cache root: `preview/<name>/<version>/typst.toml`. */
  addVendored(files: TypstFile[]) {
    for (const f of files) {
      const parts = f.path.split('/');
      if (parts.length < 4) continue;
      const key = parts.slice(0, 3).join('/');
      const list = this.vendored.get(key) ?? [];
      list.push({ path: parts.slice(3).join('/'), bytes: f.bytes });
      this.vendored.set(key, list);
    }
  }

  has(spec: PackageSpec): boolean {
    const key = specKey(spec);
    return this.vendored.has(key) || this.tarballs.has(key);
  }

  addTarball(spec: PackageSpec, bytes: Uint8Array) {
    this.tarballs.set(specKey(spec), bytes);
  }

  /** The `resolve` callback for typst.ts's `withPackageRegistry`. */
  resolve = (spec: PackageSpec, context: PackageResolveContext): string | undefined => {
    const key = specKey(spec);
    const done = this.mounted.get(key);
    if (done) return done;
    const dir = `${MEMORY_ROOT}/${key}`;
    const now = new Date();
    const vendored = this.vendored.get(key);
    if (vendored) {
      for (const f of vendored) this.access.insertFile(`${dir}/${f.path}`, f.bytes, now);
    } else {
      const tarball = this.tarballs.get(key);
      if (!tarball) {
        // Returning undefined yields typst's "package not found" diagnostic naming the package;
        // a throw would lose the message.
        return undefined;
      }
      context.untar(tarball, (path, data, mtimeMs) => {
        // `tar -C dir .` writes `./` prefixes, which break path resolution.
        const clean = path.replace(/^(\.\/)+/, '');
        if (!clean || clean.endsWith('/')) return;
        this.access.insertFile(`${dir}/${clean}`, data, new Date(mtimeMs));
      });
    }
    this.mounted.set(key, dir);
    return dir;
  };
}
