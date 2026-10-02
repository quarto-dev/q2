import { createTypstCompiler, type TypstCompiler } from '@myriaddreamin/typst.ts/compiler';
import { loadFonts, withAccessModel, withPackageRegistry } from '@myriaddreamin/typst.ts/options.init';
import { fontFamilies } from './fonts.ts';
import { moduleRef } from './moduleRef.ts';
import { DEFAULT_LIMITS, type Limits } from './limits.ts';
import { missingPackage, PackageStore, packageImports, specKey, specLabel } from './packages.ts';
import { countPdfPages } from './pdf.ts';
import type {
  CompileFailure,
  CompileInput,
  CompileResult,
  CompileStats,
  Diagnostic,
  HostDiagnostic,
  PackageFetcher,
  PackageSpec,
  TypstDiagnostic,
  TypstFile,
} from './types.ts';

export interface SessionInit {
  /** The compiled typst.ts wasm, held by the caller. */
  module: WebAssembly.Module;
  /** Font file bytes: typst-assets defaults, vendored Font Awesome, brand fonts. */
  fonts: Uint8Array[];
  /** Vendored packages, paths relative to the cache root (`preview/<name>/<version>/...`). */
  vendoredPackages?: TypstFile[];
  /** Where non-vendored packages come from; without one they are reported as not found. */
  fetchPackage?: PackageFetcher;
  limits?: Partial<Limits>;
}

export const looksLikeOom = (e: unknown) =>
  /out of memory|could not allocate|memory\.grow|maximum memory size|Array buffer allocation failed|Invalid array length/i.test(String(e));

const MEMORY_PREFIX = '/@memory/';

const host = (code: HostDiagnostic['code'], message: string, pkg?: string, kind: HostDiagnostic['kind'] = 'error'): HostDiagnostic => ({
  origin: 'host',
  kind,
  code,
  message,
  ...(pkg ? { package: pkg } : {}),
  stage: 'typst',
});

interface RawDiagnostic {
  package?: string;
  path?: string;
  severity?: string;
  range?: string;
  message?: string;
}

const toDiagnostic = (d: RawDiagnostic): TypstDiagnostic => ({
  origin: 'typst',
  kind: d.severity === 'warning' ? 'warning' : 'error',
  message: d.message ?? '',
  path: d.path ?? '',
  range: d.range ?? '',
  ...(d.package ? { package: d.package } : {}),
  stage: 'typst',
});

const invalid = (message: string): CompileFailure => ({ ok: false, kind: 'invalid-input', diagnostics: [host('invalid-input', message)] });

/** `input` problems, as one message; `undefined` when it is acceptable. */
export function checkInput(input: CompileInput, limits: Limits): string | undefined {
  const root = input.root ?? '/';
  const seen = new Set<string>();
  let total = 0;
  const bad = (p: string) =>
    !p.startsWith('/') || p.includes('\\') || p.includes('//') || p.split('/').some((s) => s === '.' || s === '..') || (p.length > 1 && p.endsWith('/'));
  if (bad(root)) return `compile root ${JSON.stringify(root)} is not a normalized absolute path`;
  for (const f of input.files) {
    if (bad(f.path)) return `file path ${JSON.stringify(f.path)} is not a normalized absolute path`;
    if (f.path.startsWith(MEMORY_PREFIX)) return `file path ${f.path} is reserved for packages`;
    if (seen.has(f.path)) return `file path ${f.path} appears twice`;
    seen.add(f.path);
    total += f.bytes.length;
  }
  if (!seen.has(input.main)) return `main file ${input.main} is not among the files`;
  const under = root === '/' ? input.main : input.main.startsWith(`${root}/`) ? input.main : undefined;
  if (!under) return `main file ${input.main} is outside the compile root ${root}`;
  if (total > limits.total_bytes) return `the files total ${total} bytes, over the ${limits.total_bytes} byte limit`;
  return undefined;
}

/**
 * One typst compiler instance with its fonts and packages. Create it in a worker (or any
 * thread), compile once or several times; a fresh worker per compile is the hub-client
 * pattern, so an abort never needs to unwind the wasm.
 */
export class TypstSession {
  private compiler: TypstCompiler;
  private readonly store: PackageStore;
  private readonly init: SessionInit;
  private readonly fetchPackage: PackageFetcher | undefined;
  private readonly limits: Limits;
  private readonly fontCount: number;
  /** Family names of the loaded fonts, for `typst-available-fonts`. */
  readonly fontFamilies: string[];

  private constructor(init: SessionInit, compiler: TypstCompiler, store: PackageStore, limits: Limits, fontFamilies: string[]) {
    this.init = init;
    this.compiler = compiler;
    this.store = store;
    this.fetchPackage = init.fetchPackage;
    this.limits = limits;
    this.fontFamilies = fontFamilies;
    this.fontCount = init.fonts.length;
  }

  /**
   * A fresh typst.ts compiler over the same store. The wasm remembers a package it failed to
   * resolve for the life of the instance (it does not ask `resolve` again, and `reset()` does
   * not clear that), so a retry after fetching a package needs a new instance.
   */
  private static async buildCompiler(init: SessionInit, store: PackageStore): Promise<TypstCompiler> {
    const compiler = createTypstCompiler();
    await compiler.init({
      getModule: () => moduleRef(init.module),
      beforeBuild: [
        // `assets: false` is load-bearing: by default typst.ts fetches 17 fonts from jsdelivr, and
        // with none loaded a compile succeeds with a text-less PDF and no diagnostic.
        loadFonts(init.fonts, { assets: false }),
        withAccessModel(store.access),
        withPackageRegistry({ resolve: store.resolve }),
      ],
    });
    return compiler;
  }

  static async create(init: SessionInit): Promise<TypstSession> {
    const store = new PackageStore();
    if (init.vendoredPackages) store.addVendored(init.vendoredPackages);
    const families = await fontFamilies(init.module, init.fonts);
    const compiler = await TypstSession.buildCompiler(init, store);
    return new TypstSession(init, compiler, store, { ...DEFAULT_LIMITS, ...init.limits }, families);
  }

  async compile(input: CompileInput, options: { signal?: AbortSignal } = {}): Promise<CompileResult> {
    const problem = checkInput(input, this.limits);
    if (problem) return invalid(problem);
    if (this.fontCount === 0)
      return { ok: false, kind: 'invalid-input', diagnostics: [host('no-fonts', 'No fonts are loaded, so the PDF would have no text. The host must supply the default fonts.')] };

    const t0 = performance.now();
    const stats: CompileStats = { prepareMs: 0, attempts: 0, compileMs: 0, packagesFetched: 0, packageBytes: 0 };
    this.compiler.resetShadow();
    for (const f of input.files) this.compiler.mapShadow(f.path, f.bytes);

    const deadline = t0 + this.limits.package_ms;
    const failedFetch = (diagnostics: Diagnostic[]): CompileFailure => ({ ok: false, kind: 'package-fetch', diagnostics, stats });
    const notFound = new Map<string, PackageSpec>();
    const tried = new Set<string>();

    // Prefetch what the sources name; a package those packages import is found by retrying.
    const named = new Map<string, PackageSpec>();
    const decoder = new TextDecoder();
    for (const f of input.files)
      if (f.path.endsWith('.typ')) for (const s of packageImports(decoder.decode(f.bytes))) named.set(specKey(s), s);

    const fetchAll = async (specs: PackageSpec[]): Promise<CompileFailure | undefined> => {
      const wanted = specs.filter((s) => !this.store.has(s) && !tried.has(specKey(s)));
      for (const s of wanted) tried.add(specKey(s));
      if (stats.packagesFetched + wanted.length > this.limits.max_packages)
        return failedFetch([host('limit-exceeded', `The document needs more than ${this.limits.max_packages} packages from the registry; stopped before fetching ${wanted.map(specLabel).join(', ')}.`)]);
      if (!this.fetchPackage) {
        for (const s of wanted) notFound.set(specKey(s), s);
        return undefined;
      }
      const controller = new AbortController();
      const onAbort = () => controller.abort(options.signal?.reason);
      options.signal?.addEventListener('abort', onAbort, { once: true });
      const timer = setTimeout(() => controller.abort(new Error('package fetch time limit')), Math.max(0, deadline - performance.now()));
      try {
        const results = await Promise.all(
          wanted.map(async (s) => {
            try {
              return { s, bytes: await (this.fetchPackage as PackageFetcher)(s, controller.signal) };
            } catch (e) {
              return { s, error: controller.signal.aborted && !options.signal?.aborted ? new Error(`the ${Math.round(this.limits.package_ms / 1000)} s package time limit passed`) : e };
            }
          }),
        );
        const errors: HostDiagnostic[] = [];
        for (const r of results) {
          if ('error' in r) {
            const why = r.error instanceof Error ? r.error.message : String(r.error);
            errors.push(host('package-fetch-failed', `Could not download Typst package ${specLabel(r.s)} (${why}). If you are offline, connect once so it can be downloaded and cached.`, specLabel(r.s)));
          } else if (!r.bytes) notFound.set(specKey(r.s), r.s);
          else {
            stats.packagesFetched++;
            stats.packageBytes += r.bytes.length;
            if (stats.packageBytes > this.limits.package_bytes) {
              errors.push(host('limit-exceeded', `Typst packages exceed the ${this.limits.package_bytes} byte download limit at ${specLabel(r.s)}.`, specLabel(r.s)));
              continue;
            }
            this.store.addTarball(r.s, r.bytes);
          }
        }
        return errors.length ? failedFetch(errors) : undefined;
      } finally {
        clearTimeout(timer);
        options.signal?.removeEventListener('abort', onAbort);
      }
    };

    const early = await fetchAll([...named.values()]);
    if (early) return early;
    stats.prepareMs = performance.now() - t0;

    const t1 = performance.now();
    for (;;) {
      if (options.signal?.aborted) return { ok: false, kind: 'crash', diagnostics: [host('typst-crash', 'The compile was cancelled.')], stats };
      stats.attempts++;
      let raw: { result?: Uint8Array; diagnostics?: RawDiagnostic[] };
      try {
        raw = (await this.compiler.compile({ mainFilePath: input.main, root: input.root ?? '/', format: 1, diagnostics: 'full', inputs: input.inputs })) as typeof raw;
      } catch (e) {
        stats.compileMs = performance.now() - t1;
        const oom = looksLikeOom(e);
        return { ok: false, kind: oom ? 'oom' : 'crash', diagnostics: [host(oom ? 'typst-oom' : 'typst-crash', `The Typst compiler ${oom ? 'ran out of memory' : 'crashed'}: ${String(e)}`)], stats };
      }
      const diagnostics = (raw.diagnostics ?? []).map(toDiagnostic);
      if (raw.result) {
        stats.compileMs = performance.now() - t1;
        return { ok: true, pdf: raw.result, pages: countPdfPages(raw.result) ?? 0, diagnostics, stats };
      }

      // A package typst could not find: fetch it (it may be a dependency of a package) and retry.
      const wantMore = diagnostics
        .map((d) => missingPackage(d.message))
        .filter((s): s is PackageSpec => !!s && !this.store.has(s) && !tried.has(specKey(s)) && !notFound.has(specKey(s)));
      if (wantMore.length && stats.attempts < this.limits.max_attempts && performance.now() < deadline) {
        const err = await fetchAll(wantMore);
        if (err) return err;
        if (wantMore.some((s) => this.store.has(s))) {
          this.compiler = await TypstSession.buildCompiler(this.init, this.store);
          for (const f of input.files) this.compiler.mapShadow(f.path, f.bytes);
          continue;
        }
      }
      stats.compileMs = performance.now() - t1;
      const out: Diagnostic[] = [...diagnostics];
      const named = new Set<string>();
      for (const d of diagnostics) {
        const s = missingPackage(d.message);
        if (s && !named.has(specKey(s))) {
          named.add(specKey(s));
          out.push(host('package-not-found', `Typst package ${specLabel(s)} was not found${s.namespace === 'preview' ? ' in the Typst package registry' : ''}.`, specLabel(s)));
        }
      }
      return { ok: false, kind: 'typst-error', diagnostics: out, stats };
    }
  }
}
