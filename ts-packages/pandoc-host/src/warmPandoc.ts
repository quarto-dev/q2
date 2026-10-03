import { ConsoleStdout, File, OpenFile, PreopenDirectory, WASI, WASIProcExit } from '@bjorn3/browser_wasi_shim';
import { argvToDefaults, UnsupportedArgv, type ConvertOptions } from './argv.ts';
import { execute } from './execute.ts';
import { withPreamble } from './preamble.ts';
import { concat, failure, hostError, looksLikeOom, mount, readFile, validate, type Tree } from './shared.ts';
import type { Limits } from './limits.ts';
import type { ExecuteFailure, ExecuteResult, FailureKind, Fault, PandocRequest, RunStats, ShareTree } from './types.ts';

const enc = new TextEncoder();
const dec = new TextDecoder();

export interface WarmCreateOptions {
  /** RTS options given to `hs_init_with_rtsopts` (fault injection only; the production path passes none). */
  rts?: string[];
  /** Fault for this instance: `oom` becomes an RTS option, `crash` and `hang` fire once at the next `path_open`. */
  fault?: Fault;
  limits?: Limits;
  shareRoot?: string;
}

/** What the last `run` showed about the instance's health; `WarmSession` decides what to do with it. */
export interface RunSignals {
  /** A wasm trap or other non-exit exception came out of `convert`. */
  trapped: boolean;
  /** `os.exit(n)` or a heap-exhaustion exit: the thrown exit code. */
  thrownExit: number | null;
  /** fd 2 carried the RTS's out-of-memory text. */
  oomMessage: boolean;
}

const NO_SIGNALS: RunSignals = { trapped: false, thrownExit: null, oomMessage: false };
const OOM_TEXT = /out of memory|Heap exhausted/i;

/** True for writers the warm path runs: `typst` and `typst-<extensions>` (the `-citations` writer). */
export const isTypstWriter = (writer: string) => writer === 'typst' || writer.startsWith('typst-');

interface FaultHooks {
  pathOpen: (() => number) | undefined;
}

interface WarmExports {
  memory: WebAssembly.Memory;
  malloc: (n: number) => number;
  convert: (ptr: number, len: number) => void;
  hs_init_with_rtsopts: (argc: number, argv: number) => void;
  __wasm_call_ctors: () => void;
}

/**
 * One instantiation of pandoc.wasm that ran `hs_init_with_rtsopts` once and serves many `convert` calls (D2(b)).
 * `_start` traps on re-entry, so this drives the module's exported `convert` instead; the options are pandoc
 * defaults-file keys (`argvToDefaults` translates the request's argv). The WASI environment is empty: a warm
 * instance reads it once, so the per-request environment reaches Lua through the `init.lua` preamble. A request
 * the translator rejects, or whose writer is not typst, runs through the fresh `execute()` instead.
 */
export class WarmPandoc {
  private readonly module: WebAssembly.Module;
  private readonly x: WarmExports;
  private readonly options: WarmCreateOptions;
  /** The guest's `/`, shared with the WASI preopen; cleared and refilled by every run. */
  private readonly guest: Tree;
  private readonly fd1: Uint8Array[];
  private readonly fd2: Uint8Array[];
  private readonly hooks: FaultHooks;
  private fault: Fault | undefined;
  /** Signals of the last `run`. */
  signals: RunSignals = NO_SIGNALS;

  private constructor(module: WebAssembly.Module, x: WarmExports, options: WarmCreateOptions, guest: Tree, fd1: Uint8Array[], fd2: Uint8Array[], hooks: FaultHooks) {
    this.module = module;
    this.x = x;
    this.options = options;
    this.guest = guest;
    this.fd1 = fd1;
    this.fd2 = fd2;
    this.hooks = hooks;
    this.fault = options.fault;
  }

  static async create(module: WebAssembly.Module, options: WarmCreateOptions = {}): Promise<WarmPandoc> {
    const rts = [...(options.rts ?? [])];
    if (options.fault?.kind === 'oom') rts.push('+RTS', `-M${options.fault.limit ?? '5m'}`, '-RTS');
    const fd1: Uint8Array[] = [];
    const fd2: Uint8Array[] = [];
    const tree: Tree = new Map();
    const args = ['pandoc.wasm', ...rts];
    const fds = [
      new OpenFile(new File(new Uint8Array(0))),
      new ConsoleStdout((b) => fd1.push(b.slice())),
      new ConsoleStdout((b) => fd2.push(b.slice())),
      new PreopenDirectory('/', tree),
    ];
    const wasi = new WASI(args, [], fds, { debug: false });
    // Imports are bound at instantiation, so a fault cannot replace `path_open` later; route it through a hook instead.
    const hooks: FaultHooks = { pathOpen: undefined };
    const realPathOpen = wasi.wasiImport.path_open;
    wasi.wasiImport.path_open = (...a: Parameters<typeof realPathOpen>) => (hooks.pathOpen ? hooks.pathOpen() : realPathOpen.apply(wasi.wasiImport, a));
    const instance = await WebAssembly.instantiate(module, { wasi_snapshot_preview1: wasi.wasiImport });
    const x = instance.exports as unknown as WarmExports;
    wasi.initialize(instance as Parameters<WASI['initialize']>[0]);
    x.__wasm_call_ctors();
    // hs_init_with_rtsopts(&argc, &argv) with a hand-built argv.
    const view = () => new DataView(x.memory.buffer);
    const cstr = (s: string) => {
      const b = enc.encode(s);
      const p = x.malloc(b.length + 1);
      const m = new Uint8Array(x.memory.buffer, p, b.length + 1);
      m.set(b);
      m[b.length] = 0;
      return p;
    };
    const argc = x.malloc(4);
    view().setUint32(argc, args.length, true);
    const argvp = x.malloc(4 * (args.length + 1));
    args.forEach((s, i) => view().setUint32(argvp + 4 * i, cstr(s), true));
    view().setUint32(argvp + 4 * args.length, 0, true);
    const argvpp = x.malloc(4);
    view().setUint32(argvpp, argvp, true);
    x.hs_init_with_rtsopts(argc, argvpp);
    return new WarmPandoc(module, x, options, tree, fd1, fd2, hooks);
  }

  /** Linear memory size now. It never shrinks. */
  memoryBytes(): number {
    return this.x.memory.buffer.byteLength;
  }

  /** One `convert` over the current tree; returns what the guest wrote, and the thrown exit or trap. */
  private call(options: ConvertOptions) {
    this.fd1.length = 0;
    this.fd2.length = 0;
    const json = enc.encode(JSON.stringify(options));
    const ptr = this.x.malloc(json.length);
    new Uint8Array(this.x.memory.buffer, ptr, json.length).set(json);
    this.armFault();
    let trap: unknown = null;
    let exit: number | null = null;
    try {
      this.x.convert(ptr, json.length);
    } catch (e) {
      if (e instanceof WASIProcExit) exit = e.code;
      else trap = e;
    }
    this.disarmFault();
    return { trap, exit, fd1: dec.decode(concat(this.fd1)), fd2: dec.decode(concat(this.fd2)) };
  }

  /** `crash` and `hang` patch `path_open` for one run, as `execute()` does for a fresh instance; they fire once. */
  private armFault() {
    const fault = this.fault;
    if (fault?.kind !== 'crash' && fault?.kind !== 'hang') return;
    this.fault = undefined;
    this.hooks.pathOpen = () => {
      if (fault.kind === 'crash') throw new Error(fault.message ?? 'injected crash');
      // hang: only a terminated worker gets out. `performance.now()` keeps esbuild from deleting the loop.
      while (performance.now() >= 0) {
        /* spin */
      }
      return 0;
    };
  }

  private disarmFault() {
    this.hooks.pathOpen = undefined;
  }

  /** Replace the guest tree's contents: the run's mounted tree plus the files `convert` reads and writes at `/`. */
  private fill(mounted: Tree, stdin: Uint8Array) {
    const guest = this.guest;
    guest.clear();
    for (const [k, v] of mounted) guest.set(k, v);
    guest.set('stdin', new File(stdin));
    guest.set('stdout', new File(new Uint8Array(0)));
    guest.set('stderr', new File(new Uint8Array(0)));
    guest.set('warnings', new File(new Uint8Array(0)));
  }

  private text(name: string): string {
    const f = this.guest.get(name);
    return f instanceof File ? dec.decode(f.data) : '';
  }

  /**
   * The canary: `convert` on the one-line document `x` with no filters. A healthy instance answers `x`; after a heap
   * exhaustion every `convert` throws, so this is how a thrown exit is told from an instance that is gone.
   */
  canary(): boolean {
    this.fill(new Map(), enc.encode('x'));
    const r = this.call({ from: 'markdown', to: 'plain' });
    return r.trap === null && r.exit === null && this.text('stdout').trim() === 'x';
  }

  /** Run one request. A request the warm path does not take runs through the fresh `execute()`, with `stats.fallback` set. */
  async run(request: PandocRequest, shareTree: ShareTree, hooks: { onProgress?: (stage: 'mounting' | 'running') => void } = {}): Promise<ExecuteResult> {
    const problems = validate(request, shareTree, this.options);
    if (problems.length) return failure('invalid-request', null, '', '', problems);

    let options: ConvertOptions | undefined;
    if (isTypstWriter(request.writer)) {
      try {
        options = argvToDefaults(request.argv, request.files);
        if (options['output-file'] !== request.output_path) throw new UnsupportedArgv('the output file is not the request output path', '-o');
      } catch (e) {
        if (!(e instanceof UnsupportedArgv)) throw e;
      }
    }
    if (!options) {
      this.signals = NO_SIGNALS;
      const result = await execute(request, shareTree, { module: this.module, limits: this.options.limits, shareRoot: this.options.shareRoot, onProgress: hooks.onProgress });
      return withStats(result, { fallback: true });
    }

    hooks.onProgress?.('mounting');
    const mounted = mount(request, withPreamble(shareTree, request.env));
    this.fill(mounted.tree, new Uint8Array(0));
    hooks.onProgress?.('running');
    const t = performance.now();
    const r = this.call(options);
    const runMs = Math.round(performance.now() - t);

    const warnings = this.warningLines();
    const stderr = r.fd2 + this.text('stderr') + warnings;
    const stdout = r.fd1 + this.text('stdout');
    const stats: RunStats = { mountMs: mounted.mountMs, instanceMs: 0, runMs, memoryBytes: this.memoryBytes(), mountedBytes: mounted.mountedBytes, warm: true };
    const oomMessage = OOM_TEXT.test(r.fd2);
    this.signals = { trapped: r.trap !== null, thrownExit: r.exit, oomMessage };

    const fail = (kind: FailureKind, status: number | null, message?: string): ExecuteFailure =>
      failure(kind, status, stderr, stdout, message ? [hostError(kind === 'oom' ? 'pandoc-oom' : kind === 'crash' ? 'pandoc-crash' : 'no-output', message)] : [], stats);
    if (r.trap !== null) {
      const oom = looksLikeOom(r.trap);
      return fail(oom ? 'oom' : 'crash', null, `pandoc.wasm ${oom ? 'ran out of memory' : 'crashed'}: ${String(r.trap)}`);
    }
    // `convert` returns no status. A thrown exit is `os.exit(n)` or heap exhaustion, which look alike (exit 1);
    // only the RTS's message on fd 2 says the heap is gone.
    if (oomMessage) return fail('oom', r.exit, 'pandoc ran out of memory');
    if (r.exit !== null && r.exit !== 0) return fail('pandoc-exit', r.exit);
    const output = readFile(this.guest, request.output_path);
    if (!output) {
      if (r.exit === null && /(^|\n)ERROR:/.test(stderr)) return fail('pandoc-exit', 1);
      return fail('no-output', 0, `pandoc exited 0 but wrote no file at \`${request.output_path}\``);
    }
    return { ok: true, status: 0, output, outputPath: request.output_path, stderr, stdout, diagnostics: [], stats };
  }

  /** pandoc's own warnings: `convert` writes them as JSON to `/warnings`, where `_start` prints `[WARNING] ...` on stderr. */
  private warningLines(): string {
    const raw = this.text('warnings').trim();
    if (!raw) return '';
    try {
      const list = JSON.parse(raw) as { verbosity?: string; pretty?: string; message?: string }[];
      // `_start` runs at pandoc's default verbosity, which prints warnings (and errors) but not `[INFO]` lines.
      return list
        .filter((w) => (w.verbosity ?? 'WARNING') !== 'INFO')
        // The CLI indents the continuation lines of a message by two spaces; `pretty` does not.
        .map((w) => `[${w.verbosity ?? 'WARNING'}] ${(w.pretty ?? w.message ?? '').split('\n').join('\n  ')}\n`)
        .join('');
    } catch {
      return '';
    }
  }
}

function withStats(result: ExecuteResult, extra: Partial<RunStats>): ExecuteResult {
  const base: RunStats = result.stats ?? { mountMs: 0, instanceMs: 0, runMs: 0, memoryBytes: 0, mountedBytes: 0 };
  return { ...result, stats: { ...base, ...extra } } as ExecuteResult;
}
