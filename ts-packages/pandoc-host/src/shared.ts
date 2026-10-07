import { Directory, File, type Inode } from '@bjorn3/browser_wasi_shim';
import { DEFAULT_LIMITS, DEFAULT_SHARE_ROOT } from './limits.ts';
import { components } from './paths.ts';
import { validateRequest } from './validate.ts';
import type { Diagnostic, ExecuteFailure, ExecuteOptions, FailureKind, HostDiagnostic, PandocRequest, RequestFile, RunStats, ShareTree } from './types.ts';
import type { Limits } from './limits.ts';

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
export function buildTree(req: PandocRequest, shareTree: ShareTree, inputs: Record<string, Uint8Array> = {}): Tree {
  const root: Tree = new Map();
  const put = (path: string, bytes: Uint8Array) => {
    const parts = components(path);
    mkdirp(root, parts.slice(0, -1)).set(parts[parts.length - 1], new File(bytes));
  };
  for (const d of req.dirs) mkdirp(root, components(d));
  for (const f of shareTree.files) put(`${req.share_tree_path}/${f.path}`, f.bytes);
  for (const f of req.files) put(f.path, f.bytes);
  for (const f of req.resource_refs) put(f.path, f.bytes);
  for (const i of req.host_inputs ?? []) put(i.path, inputs[i.path]);
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

/** The regular files under `dir` in the guest tree, with absolute paths. A missing `dir` (or a file at `dir`) has none. */
function filesUnder(root: Tree, dir: string): RequestFile[] {
  let cur: Tree = root;
  for (const p of components(dir)) {
    const next = cur.get(p);
    if (!(next instanceof Directory)) return [];
    cur = next.contents;
  }
  const out: RequestFile[] = [];
  const walk = (tree: Tree, prefix: string) => {
    for (const [name, inode] of tree) {
      const path = `${prefix}/${name}`;
      if (inode instanceof Directory) walk(inode.contents, path);
      else if (inode instanceof File) out.push({ path, bytes: inode.data });
    }
  };
  walk(cur, dir === '/' ? '' : dir);
  return out;
}

/**
 * The files a successful run left under `collect_dirs`, sorted by path. In path order, a file over
 * `collected_file_bytes`, or one that would take the running total over `collected_total_bytes`, is
 * dropped with a `collect-limit` warning (its `path` and `size`); later files are still checked.
 */
export function collectFiles(root: Tree, dirs: string[], limits: Limits): { collected: RequestFile[]; diagnostics: HostDiagnostic[] } {
  const byPath = new Map<string, RequestFile>();
  for (const d of dirs) for (const f of filesUnder(root, d)) byPath.set(f.path, f);
  const sorted = [...byPath.values()].sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  const collected: RequestFile[] = [];
  const diagnostics: HostDiagnostic[] = [];
  let total = 0;
  for (const f of sorted) {
    const size = f.bytes.length;
    const why =
      size > limits.collected_file_bytes
        ? `is ${size} bytes; the per-file limit is ${limits.collected_file_bytes}`
        : total + size > limits.collected_total_bytes
          ? `would take the collected total to ${total + size} bytes; the limit is ${limits.collected_total_bytes}`
          : undefined;
    if (why) diagnostics.push({ origin: 'host', kind: 'warning', code: 'collect-limit', message: `collected file \`${f.path}\` ${why} and was dropped`, path: f.path, size });
    else {
      collected.push(f);
      total += size;
    }
  }
  return { collected, diagnostics };
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

const hex = (b: ArrayBuffer) => [...new Uint8Array(b)].map((x) => x.toString(16).padStart(2, '0')).join('');

/**
 * Checks each `host_inputs` entry against the supplied bytes (present, declared size, sha256).
 * The problems, empty when the inputs may be mounted. Needs `crypto.subtle`, a host prerequisite.
 */
export async function checkInputs(request: PandocRequest, inputs: Record<string, Uint8Array> = {}): Promise<HostDiagnostic[]> {
  const out: HostDiagnostic[] = [];
  const bad = (path: string, message: string) => out.push({ origin: 'host', kind: 'error', code: 'input-mismatch', message, path });
  for (const i of request.host_inputs ?? []) {
    const bytes = Object.hasOwn(inputs, i.path) ? inputs[i.path] : undefined;
    if (!bytes) bad(i.path, `host input \`${i.path}\` was not supplied`);
    else if (bytes.length !== i.size) bad(i.path, `host input \`${i.path}\` is ${bytes.length} bytes; the request declares ${i.size}`);
    else if (hex(await crypto.subtle.digest('SHA-256', bytes as Uint8Array<ArrayBuffer>)) !== i.sha256)
      bad(i.path, `host input \`${i.path}\` does not match the declared sha256`);
  }
  return out;
}

/** Build the guest tree and account for its size. */
export function mount(request: PandocRequest, shareTree: ShareTree, inputs: Record<string, Uint8Array> = {}): Mounted {
  const t = performance.now();
  const tree = buildTree(request, shareTree, inputs);
  const mountMs = Math.round(performance.now() - t);
  const mountedBytes =
    shareTree.files.reduce((n, f) => n + f.bytes.length, 0) +
    request.files.reduce((n, f) => n + f.bytes.length, 0) +
    request.resource_refs.reduce((n, f) => n + f.bytes.length, 0) +
    (request.host_inputs ?? []).reduce((n, i) => n + (inputs[i.path]?.length ?? 0), 0);
  return { tree, mountMs, mountedBytes };
}

/** The output file of a finished run, or the `no-output` failure. */
export function readOutput(tree: Tree, request: PandocRequest, status: number, stderr: string, stdout: string, stats: RunStats): { output: Uint8Array } | ExecuteFailure {
  const output = readFile(tree, request.output_path);
  if (!output) return failure('no-output', status, stderr, stdout, [hostError('no-output', `pandoc exited 0 but wrote no file at \`${request.output_path}\``)], stats);
  return { output };
}

