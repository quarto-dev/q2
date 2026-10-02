import { DEFAULT_LIMITS, DEFAULT_SHARE_ROOT, SUPPORTED_SCHEMA_VERSION, type Limits } from './limits.ts';
import { components, isNormalizedAbsolute, isUnder } from './paths.ts';
import type { HostDiagnostic, HostDiagnosticCode, PandocRequest, RequestFile, ShareTree } from './types.ts';

export interface ValidateOptions {
  limits?: Limits;
  shareRoot?: string;
}

const IMAGE_EXT = /\.(png|jpe?g|gif|svg|webp|bmp|tiff?|ico|avif|emf|wmf|eps|pdf)$/i;
const TMP = '/tmp';

const err = (code: HostDiagnosticCode, message: string, path?: string): HostDiagnostic => ({
  origin: 'host',
  kind: 'error',
  code,
  message,
  ...(path === undefined ? {} : { path }),
});

const isStr = (v: unknown): v is string => typeof v === 'string';
const isFileList = (v: unknown): v is RequestFile[] =>
  Array.isArray(v) &&
  v.every((f) => f && isStr((f as RequestFile).path) && (f as RequestFile).bytes instanceof Uint8Array);

/** Shape check of the wire object (the TS type is checked against the schema in request.test.ts). */
export function checkShape(r: unknown): HostDiagnostic[] {
  if (!r || typeof r !== 'object') return [err('malformed-request', 'request is not an object')];
  const q = r as Record<string, unknown>;
  if (q.schema_version !== SUPPORTED_SCHEMA_VERSION) {
    return [
      err(
        'unsupported-schema-version',
        `request schema_version ${String(q.schema_version)} is not supported (this host understands ${SUPPORTED_SCHEMA_VERSION})`,
      ),
    ];
  }
  const out: HostDiagnostic[] = [];
  const need = (ok: boolean, field: string) => {
    if (!ok) out.push(err('malformed-request', `request field \`${field}\` is missing or has the wrong type`));
  };
  for (const f of ['share_root', 'share_tree_path', 'doc_dir', 'project_root', 'output_path', 'share_tree_version'])
    need(isStr(q[f]), f);
  need(Array.isArray(q.argv) && q.argv.length > 0 && q.argv.every(isStr), 'argv');
  need(!!q.env && typeof q.env === 'object' && Object.values(q.env as object).every(isStr), 'env');
  need(isFileList(q.files), 'files');
  need(isFileList(q.resource_refs), 'resource_refs');
  need(Array.isArray(q.dirs) && q.dirs.every(isStr), 'dirs');
  if (out.length === 0 && (q.argv as string[])[0] !== 'pandoc')
    out.push(err('malformed-request', 'argv[0] must be the literal `pandoc`'));
  return out;
}

/** The `--reference-doc` value (either argv form), if any. */
export function referenceDocPath(argv: string[]): string | undefined {
  for (let i = 1; i < argv.length; i++) {
    if (argv[i] === '--reference-doc') return argv[i + 1];
    if (argv[i].startsWith('--reference-doc=')) return argv[i].slice('--reference-doc='.length);
  }
  return undefined;
}

/** Path-valued argv entries: a bare absolute path, or the value of `--flag=/abs`. */
export function argvPaths(argv: string[]): string[] {
  const out: string[] = [];
  for (const a of argv.slice(1)) {
    const eq = /^--[A-Za-z][\w-]*=(.*)$/s.exec(a);
    const v = eq ? eq[1] : a;
    if (/^(\/(?!\*)|[A-Za-z]:\/)/.test(v)) out.push(v);
  }
  return out;
}

/**
 * Enforces the mount rules (design: Contracts), the path rules and the limits.
 * Returns every violation; empty means the request may be mounted. Pure: no I/O.
 */
export function validateRequest(
  req: PandocRequest,
  shareTree: ShareTree,
  opts: ValidateOptions = {},
): HostDiagnostic[] {
  const shape = checkShape(req);
  if (shape.length) return shape;
  const limits = opts.limits ?? DEFAULT_LIMITS;
  const shareRoot = opts.shareRoot ?? DEFAULT_SHARE_ROOT;
  const out: HostDiagnostic[] = [];

  if (req.share_root !== shareRoot)
    out.push(err('share-root-mismatch', `request share_root \`${req.share_root}\` is not the reserved \`${shareRoot}\``, req.share_root));
  if (req.share_tree_path !== `${shareRoot}/pandoc-share`)
    out.push(err('share-root-mismatch', `share_tree_path must be \`${shareRoot}/pandoc-share\``, req.share_tree_path));
  if (req.share_tree_version !== shareTree.share_tree_version)
    out.push(
      err(
        'share-tree-mismatch',
        `request wants share tree ${req.share_tree_version}, host was given ${shareTree.share_tree_version}`,
      ),
    );

  // Every path must be absolute, `/`-normalized (so `..` cannot hide).
  const allPaths = [
    req.share_root, req.share_tree_path, req.doc_dir, req.project_root, req.output_path, req.json_path,
    ...req.dirs, ...req.files.map((f) => f.path), ...req.resource_refs.map((f) => f.path),
  ].filter((p) => typeof p === 'string');
  const bad = new Set<string>();
  for (const p of allPaths)
    if (!isNormalizedAbsolute(p) && !bad.has(p)) {
      bad.add(p);
      out.push(err('path-not-normalized', `path \`${p}\` is not an absolute, \`/\`-normalized path`, p));
    }
  if (bad.size) return out; // later checks assume normalized paths

  // argv paths stay under the share root or the project root.
  for (const p of argvPaths(req.argv)) {
    if (!isNormalizedAbsolute(p)) out.push(err('path-not-normalized', `argv path \`${p}\` is not normalized`, p));
    else if (!isUnder(p, shareRoot) && !isUnder(p, req.project_root))
      out.push(err('path-outside-root', `argv path \`${p}\` is outside the share root and the project root`, p));
  }

  // Reserved prefixes and the allowed root for resources.
  for (const f of req.resource_refs) {
    if (isUnder(f.path, shareRoot) || isUnder(f.path, TMP))
      out.push(err('reserved-path', `resource \`${f.path}\` is under a reserved root (\`${shareRoot}\`, \`${TMP}\`)`, f.path));
    else if (!isUnder(f.path, req.project_root))
      out.push(err('path-outside-root', `resource \`${f.path}\` is outside the project root \`${req.project_root}\``, f.path));
  }
  for (const f of req.files)
    if (isUnder(f.path, TMP)) out.push(err('reserved-path', `file \`${f.path}\` is under the reserved \`${TMP}\``, f.path));

  // Share tree entries: relative, normalized, no `..`.
  const treeFiles: RequestFile[] = [];
  for (const f of shareTree.files) {
    const rel = f.path;
    if (rel === '' || rel.startsWith('/') || rel.includes('\\') || rel.split('/').some((c) => c === '' || c === '.' || c === '..')) {
      out.push(err('path-not-normalized', `share tree path \`${rel}\` is not a clean relative path`, rel));
      continue;
    }
    treeFiles.push({ path: `${req.share_tree_path}/${rel}`, bytes: f.bytes });
  }

  // Mount conflicts: nothing is ever overwritten, so a path may appear once.
  const owner = new Map<string, { list: string; bytes: Uint8Array }>();
  const lists: [string, RequestFile[]][] = [['share tree', treeFiles], ['files', req.files], ['resource_refs', req.resource_refs]];
  for (const [list, entries] of lists) {
    for (const f of entries) {
      const prev = owner.get(f.path);
      if (!prev) owner.set(f.path, { list, bytes: f.bytes });
      else if (prev.list !== list)
        out.push(err('mount-conflict', `path \`${f.path}\` is in both ${prev.list} and ${list}`, f.path));
      else if (!sameBytes(prev.bytes, f.bytes))
        out.push(err('mount-conflict', `path \`${f.path}\` appears twice in ${list} with different bytes`, f.path));
    }
  }
  const dirSet = new Set(req.dirs);
  for (const d of dirSet) if (owner.has(d)) out.push(err('mount-conflict', `path \`${d}\` is both a file and a directory`, d));
  const asDirs = [...owner.keys(), ...dirSet];
  for (const p of asDirs) {
    const parts = components(p);
    for (let i = 1; i < parts.length; i++) {
      const anc = '/' + parts.slice(0, i).join('/');
      if (owner.has(anc)) {
        out.push(err('mount-conflict', `path \`${anc}\` is a file but \`${p}\` needs it to be a directory`, anc));
        break;
      }
    }
  }

  // Limits.
  const refDoc = referenceDocPath(req.argv);
  let total = 0;
  let biggest: RequestFile | undefined;
  for (const [, entries] of lists) {
    for (const f of entries) {
      total += f.bytes.length;
      if (!biggest || f.bytes.length > biggest.bytes.length) biggest = f;
    }
  }
  for (const f of req.resource_refs) {
    if (f.path === refDoc && f.bytes.length > limits.reference_doc_bytes)
      out.push(err('limit-exceeded', `reference-doc \`${f.path}\` is ${f.bytes.length} bytes; the limit is ${limits.reference_doc_bytes}`, f.path));
    else if (IMAGE_EXT.test(f.path) && f.path !== refDoc && f.bytes.length > limits.image_bytes)
      out.push(err('limit-exceeded', `image \`${f.path}\` is ${f.bytes.length} bytes; the limit is ${limits.image_bytes}`, f.path));
  }
  if (total > limits.total_bytes)
    out.push(err('limit-exceeded', `mounted payload is ${total} bytes; the limit is ${limits.total_bytes} (largest: \`${biggest?.path}\`)`, biggest?.path));

  return out;
}

function sameBytes(a: Uint8Array, b: Uint8Array): boolean {
  if (a === b) return true;
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
  return true;
}
