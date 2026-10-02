import { ConsoleStdout, Directory, File, OpenFile, PreopenDirectory, WASI, WASIProcExit, type Inode } from '@bjorn3/browser_wasi_shim';
import { DEFAULT_LIMITS, DEFAULT_SHARE_ROOT } from './limits.ts';
import { components } from './paths.ts';
import { validateRequest } from './validate.ts';
import type {
  Diagnostic,
  ExecuteFailure,
  ExecuteOptions,
  ExecuteResult,
  FailureKind,
  HostDiagnostic,
  PandocRequest,
  RunStats,
  ShareTree,
} from './types.ts';

const enc = new TextEncoder();
const dec = new TextDecoder();

/** pandoc's exit code when `+RTS -M` is exceeded (H0, evidence §11). */
const EXIT_HEAP_EXHAUSTED = 251;

type Tree = Map<string, Inode>;

function mkdirp(root: Tree, parts: string[]): Tree {
  let cur = root;
  for (const p of parts) {
    let d = cur.get(p);
    if (!d) {
      d = new Directory(new Map());
      cur.set(p, d);
    }
    cur = (d as Directory).contents;
  }
  return cur;
}

/** Build the guest filesystem from the request alone: no host passthrough. Validated first. */
function buildTree(req: PandocRequest, shareTree: ShareTree): Tree {
  const root: Tree = new Map();
  const put = (path: string, bytes: Uint8Array) => {
    const parts = components(path);
    mkdirp(root, parts.slice(0, -1)).set(parts[parts.length - 1], new File(bytes));
  };
  for (const d of req.dirs) mkdirp(root, components(d));
  for (const f of shareTree.files) put(`${req.share_tree_path}/${f.path}`, f.bytes);
  for (const f of req.files) put(f.path, f.bytes);
  for (const f of req.resource_refs) put(f.path, f.bytes);
  return root;
}

function readFile(root: Tree, path: string): Uint8Array | null {
  let cur: Inode | undefined;
  let dir: Tree = root;
  for (const p of components(path)) {
    cur = dir.get(p);
    if (!cur) return null;
    if (cur instanceof Directory) dir = cur.contents;
  }
  return cur instanceof File ? cur.data : null;
}

const concat = (chunks: Uint8Array[]): Uint8Array => {
  const out = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
  let k = 0;
  for (const c of chunks) {
    out.set(c, k);
    k += c.length;
  }
  return out;
};

const hostError = (code: HostDiagnostic['code'], message: string): HostDiagnostic => ({ origin: 'host', kind: 'error', code, message });

function failure(kind: FailureKind, status: number | null, stderr: string, stdout: string, diagnostics: Diagnostic[], stats?: RunStats): ExecuteFailure {
  return { ok: false, kind, status, stderr, stdout, diagnostics, ...(stats ? { stats } : {}) };
}

const looksLikeOom = (e: unknown) => /out of memory|could not allocate|memory\.grow|Array buffer allocation failed|Invalid array length/i.test(String(e));

/**
 * Run one pandoc job: validate, mount, run `_start` on a fresh instance, collect the output.
 * `_start` traps on a second call, so every call instantiates anew; the compiled `Module` is reused.
 */
export async function execute(request: PandocRequest, shareTree: ShareTree, options: ExecuteOptions): Promise<ExecuteResult> {
  const limits = options.limits ?? DEFAULT_LIMITS;
  const shareRoot = options.shareRoot ?? DEFAULT_SHARE_ROOT;
  const problems = validateRequest(request, shareTree, { limits, shareRoot });
  if (problems.length) return failure('invalid-request', null, '', '', problems);

  options.onProgress?.('mounting');
  const tree = buildTree(request, shareTree);
  const mountedBytes =
    shareTree.files.reduce((n, f) => n + f.bytes.length, 0) +
    request.files.reduce((n, f) => n + f.bytes.length, 0) +
    request.resource_refs.reduce((n, f) => n + f.bytes.length, 0);

  const argv = [...request.argv];
  if (options.fault?.kind === 'oom') argv.splice(1, 0, '+RTS', `-M${options.fault.limit ?? '5m'}`, '-RTS');
  const env = Object.entries(request.env).map(([k, v]) => `${k}=${v}`);

  const out: Uint8Array[] = [];
  const err: Uint8Array[] = [];
  // Raw write callbacks, not `ConsoleStdout.lineBuffered`, which drops an unterminated last line.
  const fds = [
    new OpenFile(new File(new Uint8Array(0))),
    new ConsoleStdout((b) => out.push(b.slice())),
    new ConsoleStdout((b) => err.push(b.slice())),
    new PreopenDirectory('/', tree),
  ];
  const wasi = new WASI(argv, env, fds, { debug: false });

  // The shim counts UTF-16 units for args_sizes_get; a non-ASCII argv then overruns the buffer.
  const origSizes = wasi.wasiImport.args_sizes_get;
  wasi.wasiImport.args_sizes_get = (argcPtr: number, bufSizePtr: number) => {
    const r = origSizes.call(wasi.wasiImport, argcPtr, bufSizePtr);
    const memory = (wasi as unknown as { inst: { exports: { memory: WebAssembly.Memory } } }).inst.exports.memory;
    const dv = new DataView(memory.buffer);
    dv.setUint32(bufSizePtr, argv.reduce((n, a) => n + enc.encode(a).length + 1, 0), true);
    return r;
  };

  if (options.fault?.kind === 'crash' || options.fault?.kind === 'hang') {
    const fault = options.fault;
    wasi.wasiImport.path_open = () => {
      if (fault.kind === 'crash') throw new Error(fault.message ?? 'injected crash');
      // hang: only a terminated worker gets out. A bare `for (;;);` is deleted by esbuild's
      // minifier, so production bundles would not hang; calling `performance.now()` keeps it.
      while (performance.now() >= 0) {
        /* spin */
      }
      return 0;
    };
  }

  const t0 = performance.now();
  let instance: WebAssembly.Instance;
  try {
    instance = await WebAssembly.instantiate(options.module, { wasi_snapshot_preview1: wasi.wasiImport });
  } catch (e) {
    const kind: FailureKind = looksLikeOom(e) ? 'oom' : 'crash';
    return failure(kind, null, '', '', [hostError(kind === 'oom' ? 'pandoc-oom' : 'pandoc-crash', `pandoc.wasm could not be instantiated: ${String(e)}`)]);
  }
  const t1 = performance.now();

  options.onProgress?.('running');
  let status: number | null = null;
  let trap: unknown = null;
  try {
    status = wasi.start(instance as Parameters<WASI['start']>[0]);
  } catch (e) {
    if (e instanceof WASIProcExit) status = e.code;
    else trap = e;
  }
  const t2 = performance.now();

  const stderr = dec.decode(concat(err));
  const stdout = dec.decode(concat(out));
  const stats: RunStats = {
    instanceMs: Math.round(t1 - t0),
    runMs: Math.round(t2 - t1),
    memoryBytes: (instance.exports.memory as WebAssembly.Memory | undefined)?.buffer.byteLength ?? 0,
    mountedBytes,
  };

  if (trap !== null) {
    const oom = looksLikeOom(trap);
    return failure(oom ? 'oom' : 'crash', null, stderr, stdout, [hostError(oom ? 'pandoc-oom' : 'pandoc-crash', `pandoc.wasm ${oom ? 'ran out of memory' : 'crashed'}: ${String(trap)}`)], stats);
  }
  if (status === EXIT_HEAP_EXHAUSTED)
    return failure('oom', status, stderr, stdout, [hostError('pandoc-oom', 'pandoc ran out of memory (exit 251)')], stats);
  if (status !== 0) return failure('pandoc-exit', status, stderr, stdout, [], stats);

  const output = readFile(tree, request.output_path);
  if (!output) return failure('no-output', 0, stderr, stdout, [hostError('no-output', `pandoc exited 0 but wrote no file at \`${request.output_path}\``)], stats);
  return { ok: true, status: 0, output, outputPath: request.output_path, stderr, stdout, diagnostics: [], stats };
}
