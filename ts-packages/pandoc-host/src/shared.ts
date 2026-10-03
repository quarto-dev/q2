import { Directory, File, type Inode } from '@bjorn3/browser_wasi_shim';
import { DEFAULT_LIMITS, DEFAULT_SHARE_ROOT } from './limits.ts';
import { components } from './paths.ts';
import { validateRequest } from './validate.ts';
import type { Diagnostic, ExecuteFailure, ExecuteOptions, FailureKind, HostDiagnostic, PandocRequest, RunStats, ShareTree } from './types.ts';

export type Tree = Map<string, Inode>;

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
export function buildTree(req: PandocRequest, shareTree: ShareTree): Tree {
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

export function readFile(root: Tree, path: string): Uint8Array | null {
  let cur: Inode | undefined;
  let dir: Tree = root;
  for (const p of components(path)) {
    cur = dir.get(p);
    if (!cur) return null;
    if (cur instanceof Directory) dir = cur.contents;
  }
  return cur instanceof File ? cur.data : null;
}

export const concat = (chunks: Uint8Array[]): Uint8Array => {
  const out = new Uint8Array(chunks.reduce((n, c) => n + c.length, 0));
  let k = 0;
  for (const c of chunks) {
    out.set(c, k);
    k += c.length;
  }
  return out;
};

export const hostError = (code: HostDiagnostic['code'], message: string): HostDiagnostic => ({ origin: 'host', kind: 'error', code, message });

export function failure(kind: FailureKind, status: number | null, stderr: string, stdout: string, diagnostics: Diagnostic[], stats?: RunStats): ExecuteFailure {
  return { ok: false, kind, status, stderr, stdout, diagnostics, ...(stats ? { stats } : {}) };
}

/** True for the errors browsers throw when a wasm memory or an ArrayBuffer cannot grow. */
export const looksLikeOom = (e: unknown) =>
  /out of memory|could not allocate|memory\.grow|maximum memory size|Array buffer allocation failed|Invalid array length/i.test(String(e));

/** The parts of a run that the fresh path and the warm executor share: validate, then mount. */
export interface Mounted {
  tree: Tree;
  mountMs: number;
  mountedBytes: number;
}

/** Validate the request against the limits and the mount rules; the problems, empty when it may run. */
export function validate(request: PandocRequest, shareTree: ShareTree, options: Pick<ExecuteOptions, 'limits' | 'shareRoot'>): Diagnostic[] {
  return validateRequest(request, shareTree, { limits: options.limits ?? DEFAULT_LIMITS, shareRoot: options.shareRoot ?? DEFAULT_SHARE_ROOT });
}

/** Build the guest tree and account for its size. */
export function mount(request: PandocRequest, shareTree: ShareTree): Mounted {
  const t = performance.now();
  const tree = buildTree(request, shareTree);
  const mountMs = Math.round(performance.now() - t);
  const mountedBytes =
    shareTree.files.reduce((n, f) => n + f.bytes.length, 0) +
    request.files.reduce((n, f) => n + f.bytes.length, 0) +
    request.resource_refs.reduce((n, f) => n + f.bytes.length, 0);
  return { tree, mountMs, mountedBytes };
}

/** The output file of a finished run, or the `no-output` failure. */
export function readOutput(tree: Tree, request: PandocRequest, status: number, stderr: string, stdout: string, stats: RunStats): { output: Uint8Array } | ExecuteFailure {
  const output = readFile(tree, request.output_path);
  if (!output) return failure('no-output', status, stderr, stdout, [hostError('no-output', `pandoc exited 0 but wrote no file at \`${request.output_path}\``)], stats);
  return { output };
}

