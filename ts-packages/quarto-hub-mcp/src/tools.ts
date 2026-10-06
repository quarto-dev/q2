/**
 * MCP Tool Definitions
 *
 * Registers all MCP tools on the server via the SDK v2 `McpServer`
 * registration API (BP-16). Each tool's zod v4 `inputSchema` is the
 * single source for both the advertised JSON Schema and runtime input
 * validation — a wrong-typed argument is a tool-execution error naming
 * the offending parameter (BP-2, SEP-1303), and there is no
 * hand-written JSON schema left to drift from the handlers. Unknown
 * tool names are the SDK's own `-32602` protocol error (BP-15).
 *
 * Handlers stay transport- and SDK-agnostic (see `handleTool`): the
 * registration layer is the only SDK-coupled code, so a revert of the
 * v2 migration is registration-only.
 */

import { z } from 'zod';
import type {
  CallToolResult,
  McpServer,
  ToolAnnotations,
} from '@modelcontextprotocol/server';
import {
  fileUnavailableMessage,
  inferMimeType,
  type FilePayload,
  type SyncClient,
} from '@quarto/quarto-sync-client';
import { ConnectionManager, hashPayload } from './connection-manager.js';
import {
  AUTH_TOOL_DEFINITIONS,
  AuthToolsState,
  extractAuthContext,
} from './auth/auth-tools.js';
import { redactTokens } from './auth/redact.js';
import { parseProjectRef, serversMatch } from './share-url.js';

function text(msg: string): CallToolResult {
  return { content: [{ type: 'text', text: msg }] };
}

function error(msg: string): CallToolResult {
  return { content: [{ type: 'text', text: msg }], isError: true };
}

/**
 * A result that is machine-readable both ways (BP-1):
 * `structuredContent` for hosts that validate against the tool's
 * declared `outputSchema`, plus the same payload as a JSON text block
 * for legacy clients and agents that read text. `textOverride` carries
 * the pre-BP-1 text shape when that differs from the structured
 * payload (e.g. list_files' bare array).
 */
function structured(
  payload: Record<string, unknown>,
  textOverride?: unknown,
): CallToolResult {
  return {
    content: [{ type: 'text', text: JSON.stringify(textOverride ?? payload, null, 2) }],
    structuredContent: payload,
  };
}

/**
 * Shared description for every `project` parameter. Tells the model that a
 * quarto-hub.com share URL is accepted in place of a bare id — the server
 * extracts the id (and a default `path`) from it. See {@link parseProjectRef}.
 */
const PROJECT_PARAM_DESC =
  "The project's automerge index document ID, OR a full quarto-hub.com share URL " +
  'such as `https://quarto-hub.com/#/share/<id>?file=…&name=…` (the link users share ' +
  'to grant access). Given a share URL, the `<id>` after `#/share/` is used as the ' +
  'project and the `file=` query parameter, if present, supplies a default `path`.';

const projectParam = z.string().describe(PROJECT_PARAM_DESC);
const pathParam = z.string().describe('The file path within the project');

/** Shared `encoding` parameter for the write verbs (CAP-5). */
const encodingParam = z
  .enum(['utf8', 'base64'])
  .optional()
  .describe(
    '"utf8" (default) writes text. "base64" writes binary: `content` is the file bytes ' +
      'base64-encoded — use it for images, PDFs, and other non-text files.',
  );

/** Shared `mime_type` parameter for the binary write arm (CAP-5). */
const mimeTypeParam = z
  .string()
  .optional()
  .describe(
    'MIME type for `encoding: "base64"` writes. Defaults to the type inferred from the ' +
      'path extension (e.g. .png → image/png).',
  );

const ANNOT_READ: ToolAnnotations = {
  readOnlyHint: true,
  destructiveHint: false,
  idempotentHint: true,
};

/**
 * The bounded delivery wait applied to every write (ERG-2): long enough
 * for a healthy hub's ack (tens of ms on a good link), short enough
 * that a stalled hub costs one noticeable pause, not a hang.
 */
const SYNC_WAIT_MS = 2000;

const waitForSyncParam = z
  .boolean()
  .optional()
  .describe(
    'Set false to skip the bounded delivery wait. By default the call returns after the hub ' +
      'acknowledges the write (up to ~2s) and the result carries `synced: true|false`; ' +
      '`synced: false` means "not yet confirmed", not "lost" — the change is queued locally ' +
      'and still syncs when the hub is reachable.',
  );

/**
 * Run the bounded delivery wait for a just-applied write, unless the
 * caller passed `wait_for_sync: false`. Returns the `synced` entry to
 * spread into the result JSON — absent when the wait was skipped,
 * because an absent field is honest: we did not check.
 */
async function syncField(
  args: ToolArgs,
  manager: ConnectionManager,
  project: string,
  paths: string[],
): Promise<{ synced?: boolean }> {
  if (args.wait_for_sync === false) return {};
  return {
    synced: await manager.awaitDelivery(project, paths, SYNC_WAIT_MS, {
      server: routedServer(args),
    }),
  };
}

// ============================================================================
// Tool handlers
// ============================================================================

type ToolArgs = Record<string, unknown>;

/**
 * Normalize tool arguments before dispatch. If `project` is a quarto-hub.com
 * share URL, replace it with the bare index doc id; and when the share URL
 * named a `file=` and the caller gave no explicit `path`, default `path` to it.
 * A bare id passes through unchanged, so existing callers are unaffected.
 *
 * A share URL whose `server=` names a different hub ROUTES the call there
 * (bd-qt7h8h5g): the normalized args carry `server`, and the connection
 * manager joins that hub authorless — the configured hub's Bearer is
 * audience-bound and never replayed to a foreign origin.
 */
function normalizeArgs(args: ToolArgs, configuredServer: string): ToolArgs {
  if (typeof args.project !== 'string') {
    return args;
  }
  const ref = parseProjectRef(args.project);
  const next: ToolArgs = { ...args, project: ref.project };
  if (ref.server && !serversMatch(ref.server, configuredServer)) {
    next.server = ref.server;
  }
  if (ref.file && (next.path === undefined || next.path === '')) {
    next.path = ref.file;
  }
  return next;
}

/** The foreign hub a share URL routed this call to, if any (bd-qt7h8h5g). */
function routedServer(args: ToolArgs): string | undefined {
  return typeof args.server === 'string' ? args.server : undefined;
}

/**
 * One listed file. `type` is present for loaded files; dangling index
 * entries (bd-vm5e5u10) instead carry `status: 'unavailable'` plus the
 * doc id the index references, so agents can see — and repair via
 * `delete_file` — entries whose documents never reached the hub.
 */
interface ListedFile {
  path: string;
  type?: string;
  status?: 'unavailable';
  docId?: string;
  /** ERG-3 listing metadata: byte size always; mimeType always; lines for text. */
  size?: number;
  mimeType?: string;
  lines?: number;
}

/** Project state as exposed by {@link ConnectionManager.connect}. */
type ProjectState = Awaited<ReturnType<ConnectionManager['connect']>>;

function buildFileList(state: ProjectState): ListedFile[] {
  const fileList: ListedFile[] = Array.from(state.files.entries()).map(([path, payload]) =>
    payload.type === 'binary'
      ? { path, type: 'binary', size: payload.data.byteLength, mimeType: payload.mimeType }
      : {
          path,
          type: 'text',
          size: Buffer.byteLength(payload.text, 'utf8'),
          mimeType: textMimeType(path),
          lines: splitLines(payload.text).length,
        },
  );
  for (const ghost of state.client.getUnavailableFiles()) {
    fileList.push({ path: ghost.path, status: 'unavailable', docId: ghost.docId });
  }
  fileList.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  return fileList;
}

/** The dangling-entry record for `path`, if the index references a document the hub cannot provide. */
function findUnavailable(client: SyncClient, path: string): { path: string; docId: string } | undefined {
  return client.getUnavailableFiles().find((f) => f.path === path);
}

/** Per-file error for tools that need the file's content (bd-vm5e5u10 requirement 5/6). */
function unavailableFileError(path: string, docId: string): CallToolResult {
  return error(
    `Error: ${fileUnavailableMessage(path, docId)}. ` +
      'Use delete_file to remove the dangling entry.',
  );
}

// ---------------------------------------------------------------------------
// ERG-4: actionable not-found errors (parameter, state, next tool, near paths)
// ---------------------------------------------------------------------------

/** Character bigrams of `s` — the similarity alphabet for near-path hints. */
function bigrams(s: string): Set<string> {
  const out = new Set<string>();
  for (let i = 0; i < s.length - 1; i++) out.add(s.slice(i, i + 2));
  return out;
}

/** Dice coefficient over character bigrams: cheap, deterministic typo ranking. */
function pathSimilarity(a: string, b: string): number {
  const A = bigrams(a);
  const B = bigrams(b);
  if (A.size === 0 || B.size === 0) return 0;
  let overlap = 0;
  for (const g of A) if (B.has(g)) overlap++;
  return (2 * overlap) / (A.size + B.size);
}

/** Up to `limit` closest existing paths to `target`, best first (ties alphabetical). */
function closestPaths(target: string, candidates: Iterable<string>, limit = 3): string[] {
  return [...candidates]
    .map((p) => ({ p, score: pathSimilarity(target, p) }))
    .filter((c) => c.score > 0)
    .sort((x, y) => y.score - x.score || (x.p < y.p ? -1 : 1))
    .slice(0, limit)
    .map((c) => c.p);
}

/** Every path the project knows about, loaded or dangling. */
function allPaths(state: ProjectState): string[] {
  return [...state.files.keys(), ...state.client.getUnavailableFiles().map((f) => f.path)];
}

/**
 * The ERG-4 not-found error: names the failing parameter and the current
 * state, points at `list_files`, and lists up to three closest existing
 * paths so a typo self-corrects in one call instead of three.
 */
function fileNotFoundError(path: string, state: ProjectState): CallToolResult {
  const paths = allPaths(state);
  const near = closestPaths(path, paths);
  let msg =
    `Error: File not found: "${path}". The project has ` +
    `${paths.length} file${paths.length === 1 ? '' : 's'}; call list_files to see them.`;
  if (near.length > 0) {
    msg += ` Closest existing paths: ${near.map((p) => `"${p}"`).join(', ')}.`;
  }
  return error(msg);
}

// ---------------------------------------------------------------------------
// ERG-3: read ranges and truncation
// ---------------------------------------------------------------------------

/** Default byte cap on a read_file response body (the "sensible default" truncation). */
const DEFAULT_MAX_BYTES = 65536;
/** Hard ceiling for `max_bytes` — large enough to refuse politely instead of paging forever. */
const MAX_BYTES_CAP = 1048576;

/**
 * Split text into lines, treating a trailing newline as the terminator
 * of the last line rather than the start of an empty one: "a\n" is one
 * line, "a\n\n" is two, "" is zero.
 */
function splitLines(text: string): string[] {
  if (text === '') return [];
  const lines = text.split('\n');
  if (lines[lines.length - 1] === '') lines.pop();
  return lines;
}

/** Cut `text` to at most `maxBytes` UTF-8 bytes without splitting a multi-byte character. */
function cutToBytes(text: string, maxBytes: number): string {
  const buf = Buffer.from(text, 'utf8');
  if (buf.byteLength <= maxBytes) return text;
  let end = maxBytes;
  // Back off a UTF-8 continuation byte (10xxxxxx) to the sequence start.
  while (end > 0 && (buf[end] & 0xc0) === 0x80) end--;
  return buf.subarray(0, end).toString('utf8');
}

interface LineWindow {
  content: string;
  totalLines: number;
  truncated: boolean;
  nextOffset: number | null;
  hint?: string;
}

/**
 * The line-window/byte-cap engine behind read_file (ERG-3). Windows are
 * line-aligned and byte-exact: concatenating the pages `next_offset`
 * walks through reassembles the original text. `truncated` means the
 * returned window does not extend to EOF; a single line that alone
 * exceeds the byte cap is byte-cut with no line-aligned continuation
 * (`nextOffset: null` + an explanatory hint).
 */
function windowLines(
  text: string,
  offset: number,
  limit: number | undefined,
  maxBytes: number,
): LineWindow {
  const lines = splitLines(text);
  const totalLines = lines.length;
  const start = offset - 1;
  let end = limit === undefined ? totalLines : Math.min(start + limit, totalLines);

  let windowed: string;
  if (start === 0 && end === totalLines) {
    windowed = text; // whole file: return verbatim, no rejoin loss
  } else {
    windowed = lines.slice(start, end).join('\n');
    // Lines in a window were terminated in the original (only the file's
    // very last line may lack a newline).
    if (end < totalLines || text.endsWith('\n')) windowed += '\n';
  }

  if (Buffer.byteLength(windowed, 'utf8') <= maxBytes) {
    const truncated = end < totalLines;
    return {
      content: windowed,
      totalLines,
      truncated,
      nextOffset: truncated ? end + 1 : null,
      ...(truncated
        ? {
            hint:
              `Returned lines ${offset}-${end} of ${totalLines}. ` +
              `Call read_file with offset=${end + 1} to continue.`,
          }
        : {}),
    };
  }

  // Byte cap bites: re-cut at a line boundary where possible.
  const pageLines: string[] = [];
  let bytes = 0;
  let cutAt = start;
  for (let i = start; i < end; i++) {
    const lineBytes = Buffer.byteLength(lines[i], 'utf8') + 1; // +1 for the terminator
    if (bytes + lineBytes > maxBytes) break;
    pageLines.push(lines[i]);
    bytes += lineBytes;
    cutAt = i + 1;
  }
  if (pageLines.length > 0) {
    end = cutAt;
    return {
      content: pageLines.join('\n') + '\n',
      totalLines,
      truncated: true,
      nextOffset: end + 1,
      hint:
        `Returned lines ${offset}-${end} of ${totalLines} (max_bytes=${maxBytes}). ` +
        `Call read_file with offset=${end + 1} to continue.`,
    };
  }
  // Pathological: a single line exceeds the byte cap (minified assets,
  // one-line data files). Byte-cut it; there is no line-aligned
  // continuation, so next_offset is null and the hint says why.
  const content = cutToBytes(lines[start], maxBytes);
  return {
    content,
    totalLines,
    truncated: true,
    nextOffset: null,
    hint:
      `Line ${offset} alone exceeds max_bytes=${maxBytes}; returned its first ` +
      `${Buffer.byteLength(content, 'utf8')} bytes. Raise max_bytes (up to ${MAX_BYTES_CAP}) ` +
      'to see more of it.',
  };
}

/** Display MIME type for a text file (listings only — inferMimeType covers binaries). */
function textMimeType(path: string): string {
  const ext = path.includes('.') ? path.slice(path.lastIndexOf('.') + 1).toLowerCase() : '';
  const textTypes: Record<string, string> = {
    qmd: 'text/markdown',
    md: 'text/markdown',
    html: 'text/html',
    css: 'text/css',
    js: 'text/javascript',
    mjs: 'text/javascript',
    ts: 'text/typescript',
    json: 'application/json',
    yml: 'application/yaml',
    yaml: 'application/yaml',
    csv: 'text/csv',
    txt: 'text/plain',
    xml: 'application/xml',
  };
  return textTypes[ext] ?? 'text/plain';
}

/** The data tools (everything except the auth tools). */
type DataToolName =
  | 'connect_project'
  | 'list_files'
  | 'read_file'
  | 'wait_for_change'
  | 'write_file'
  | 'patch_file'
  | 'create_file'
  | 'delete_file'
  | 'rename_file'
  | 'create_project';

/**
 * Tools whose `path` argument a share URL's `file=` parameter can
 * supply (see {@link normalizeArgs}). Their zod schemas declare `path`
 * optional — a schema-level `required` would reject the share-URL call
 * before normalization runs — so the requirement is enforced here,
 * after normalization.
 */
const PATH_DEFAULTABLE: ReadonlySet<DataToolName> = new Set([
  'read_file',
  'wait_for_change',
  'write_file',
  'patch_file',
  'create_file',
  'delete_file',
]);

/** Per-call extras threaded from the SDK request context (BP-3). */
interface ToolExtras {
  /** The MCP request's cancellation signal, when the caller can cancel. */
  readonly signal?: AbortSignal;
}

async function handleTool(
  name: DataToolName,
  rawArgs: ToolArgs,
  manager: ConnectionManager,
  extras: ToolExtras = {},
): Promise<CallToolResult> {
  const args = normalizeArgs(rawArgs, manager.configuredServerUrl);
  if (PATH_DEFAULTABLE.has(name) && (typeof args.path !== 'string' || args.path === '')) {
    return error(
      `Error: ${name} requires a \`path\` argument — the file path within the project ` +
        '(or pass a share URL whose `file=` parameter names it).',
    );
  }
  switch (name) {
    case 'connect_project':
      return handleConnectProject(args, manager);
    case 'list_files':
      return handleListFiles(args, manager);
    case 'read_file':
      return handleReadFile(args, manager);
    case 'wait_for_change':
      return handleWaitForChange(args, manager, extras);
    case 'write_file':
      return handleWriteFile(args, manager);
    case 'patch_file':
      return handlePatchFile(args, manager);
    case 'create_file':
      return handleCreateFile(args, manager);
    case 'delete_file':
      return handleDeleteFile(args, manager);
    case 'rename_file':
      return handleRenameFile(args, manager);
    case 'create_project':
      return handleCreateProject(args, manager);
  }
}

async function handleConnectProject(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const state = await manager.connect(project, { server: routedServer(args) });
  return structured({ project, files: buildFileList(state) });
}

async function handleListFiles(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const state = await manager.connect(project, { server: routedServer(args) });
  const files = buildFileList(state);
  // Text fallback stays the pre-BP-1 bare array; structuredContent is
  // the object-wrapped form outputSchema requires.
  return structured({ files }, files);
}

/**
 * ERG-1 compare-and-swap refusal: the file changed since the caller's
 * read, so the write is refused. The payload carries the CURRENT content
 * and its hash — the caller can merge and retry without an extra read.
 */
function staleHashError(
  tool: 'write_file' | 'patch_file',
  path: string,
  currentText: string,
): CallToolResult {
  return error(
    JSON.stringify(
      {
        error: 'stale_expected_hash',
        message:
          `${tool} refused: the file changed since you read it (expected_hash does not match ` +
          'the current content). The current content and its hash are included here — merge ' +
          'your changes against it and retry with the new expected_hash (or re-read with ' +
          'read_file first).',
        path,
        hash: hashPayload({ type: 'text', text: currentText }),
        content: currentText,
      },
      null,
      2,
    ),
  );
}

/**
 * The binary arm of the compare-and-swap refusal: same contract as
 * {@link staleHashError}, but the current content cannot ride the error
 * as text — the caller re-reads (bytes come back as an image/blob
 * block) and merges against the included hash.
 */
function staleHashErrorBinary(
  tool: 'write_file',
  path: string,
  current: FilePayload & { type: 'binary' },
): CallToolResult {
  return error(
    JSON.stringify(
      {
        error: 'stale_expected_hash',
        message:
          `${tool} refused: the file changed since you read it (expected_hash does not match ` +
          'the current content). Re-read with read_file for the current bytes and hash, merge ' +
          'your changes, and retry with the new expected_hash.',
        path,
        hash: hashPayload(current),
        type: 'binary',
        mimeType: current.mimeType,
        size: current.data.byteLength,
      },
      null,
      2,
    ),
  );
}

/** Strict base64 decode (Node's Buffer.from is lenient — it silently drops invalid characters). */
function decodeBase64(content: string): Uint8Array | CallToolResult {
  const compact = content.replace(/\s+/g, '');
  if (!/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(compact)) {
    return error(
      'Error: parameter `content` is not valid base64. Pass the file bytes base64-encoded, ' +
        'or drop `encoding: "base64"` to write UTF-8 text.',
    );
  }
  return new Uint8Array(Buffer.from(compact, 'base64'));
}

/** The structured metadata a binary read result carries (CAP-4). */
function binaryMeta(path: string, data: Uint8Array, mimeType: string): Record<string, unknown> {
  return {
    path,
    hash: hashPayload({ type: 'binary', data, mimeType }),
    type: 'binary',
    mimeType,
    size: data.byteLength,
  };
}

/**
 * The write-side twin of {@link binaryMeta}: no `type` — write results
 * distinguish by what they carry (`mimeType`/`size` here, `created`
 * below), `type` is read-side disambiguation (CAP-5).
 */
function binaryWriteMeta(
  path: string,
  data: Uint8Array,
  mimeType: string,
): Record<string, unknown> {
  const { type: _type, ...rest } = binaryMeta(path, data, mimeType);
  return rest;
}

async function handleReadFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const metadataOnly = args.metadata_only === true;
  const state = await manager.connect(project, { server: routedServer(args) });
  const payload = state.files.get(path);

  if (!payload) {
    const ghost = findUnavailable(state.client, path);
    if (ghost) {
      return unavailableFileError(path, ghost.docId);
    }
    return fileNotFoundError(path, state);
  }
  if (payload.type === 'binary') {
    // CAP-4: binary rides read_file (no sibling tool, ERG-5 rule (a)).
    // Image MIME types come back as an `image` block a multimodal host
    // can render; anything else as an embedded blob resource (base64).
    // The URI is this server's own ephemeral reference for the blob —
    // Phase 5's Q-2 decides the public hub:// resources contract.
    const meta = binaryMeta(path, payload.data, payload.mimeType);
    if (metadataOnly) {
      return structured(meta);
    }
    const data = Buffer.from(payload.data).toString('base64');
    const blob: CallToolResult['content'][number] = payload.mimeType.startsWith('image/')
      ? { type: 'image', data, mimeType: payload.mimeType }
      : {
          type: 'resource',
          resource: { uri: `hub://${project}/${path}`, mimeType: payload.mimeType, blob: data },
        };
    return {
      content: [blob, { type: 'text', text: JSON.stringify(meta, null, 2) }],
      structuredContent: meta,
    };
  }
  if (metadataOnly) {
    return error(
      `Error: metadata_only applies to binary files only; ${path} is a text file — ` +
        'drop metadata_only to read its content.',
    );
  }

  // ERG-3: line window + byte cap. offset is 1-based; the byte cap
  // guards context size even when no explicit window is given.
  const offset = typeof args.offset === 'number' ? args.offset : 1;
  const limit = typeof args.limit === 'number' ? args.limit : undefined;
  const maxBytes = typeof args.max_bytes === 'number' ? args.max_bytes : DEFAULT_MAX_BYTES;
  const totalLines = splitLines(payload.text).length;
  if (offset > Math.max(totalLines, 1)) {
    return error(
      `Error: offset ${offset} exceeds the file's length — ${path} has ${totalLines} ` +
        `line${totalLines === 1 ? '' : 's'}. Use offset=1 (the default) or a value ≤ ${totalLines}.`,
    );
  }
  const window = windowLines(payload.text, offset, limit, maxBytes);
  return structured({
    path,
    hash: hashPayload(payload),
    type: 'text',
    content: window.content,
    truncated: window.truncated,
    total_lines: window.totalLines,
    next_offset: window.nextOffset,
    ...(window.hint ? { hint: window.hint } : {}),
  });
}

async function handleWaitForChange(
  args: ToolArgs,
  manager: ConnectionManager,
  extras: ToolExtras,
): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const rawTimeout = typeof args.timeout_seconds === 'number' ? args.timeout_seconds : 25;
  const timeoutSec = Math.max(1, Math.min(55, rawTimeout));
  const sinceHash = typeof args.since_hash === 'string' ? args.since_hash : undefined;

  const result = await manager.waitForChange(project, path, timeoutSec * 1000, sinceHash, {
    signal: extras.signal,
    server: routedServer(args),
  });

  if (!result.changed) {
    return structured({
      changed: false,
      path,
      hash: result.hash,
      message: `No change within ${timeoutSec}s. Call wait_for_change again (pass this hash as since_hash) to keep watching.`,
    });
  }
  if (result.payload === null) {
    return structured({ changed: true, removed: true, path });
  }
  if (result.payload.type === 'binary') {
    return structured({
      changed: true,
      path,
      type: 'binary',
      mimeType: result.payload.mimeType,
      hash: result.hash,
    });
  }
  return structured({
    changed: true,
    path,
    hash: result.hash,
    content: result.payload.text,
  });
}

async function handleWriteFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const content = args.content as string;
  const expectedHash = typeof args.expected_hash === 'string' ? args.expected_hash : undefined;
  const state = await manager.connect(project, { server: routedServer(args) });
  const existing = state.files.get(path);

  if (args.encoding === 'base64') {
    return handleWriteFileBinary(args, manager, state, project, path, content, expectedHash, existing);
  }

  if (!existing) {
    // A dangling entry is not writable: silently re-creating the
    // document would repoint the index away from whatever the original
    // (never-synced) client still holds. Repair is delete_file.
    const ghost = findUnavailable(state.client, path);
    if (ghost) {
      return unavailableFileError(path, ghost.docId);
    }
    if (expectedHash !== undefined) {
      return error(
        `Error: write_file refused: expected_hash was given but ${path} does not exist in ` +
          'the project (it may have been deleted since you read it). Call list_files to see ' +
          'the current files; drop expected_hash to create a new file.',
      );
    }
    await state.client.createFile(path, content);
    return structured({
      path,
      hash: hashPayload({ type: 'text', text: content }),
      created: true,
      ...(await syncField(args, manager, project, [path])),
    });
  }
  if (existing.type === 'binary') {
    return error(
      `Error: ${path} is a binary file. Cannot write text content to it — ` +
        'pass `encoding: "base64"` with base64-encoded bytes to replace it.',
    );
  }
  if (expectedHash !== undefined && hashPayload(existing) !== expectedHash) {
    return staleHashError('write_file', path, existing.text);
  }

  state.client.updateFileContent(path, content);
  return structured({
    path,
    hash: hashPayload({ type: 'text', text: content }),
    ...(await syncField(args, manager, project, [path])),
  });
}

/**
 * The `encoding: "base64"` arm of write_file (CAP-5): create or replace
 * a binary file. Same contract as the text arm — dangling entries are
 * refused, `expected_hash` compare-and-swaps, the result carries the
 * new `hash` and `synced` — plus `mimeType`/`size`.
 */
async function handleWriteFileBinary(
  args: ToolArgs,
  manager: ConnectionManager,
  state: ProjectState,
  project: string,
  path: string,
  content: string,
  expectedHash: string | undefined,
  existing: FilePayload | undefined,
): Promise<CallToolResult> {
  const decoded = decodeBase64(content);
  if (!ArrayBuffer.isView(decoded)) return decoded; // the validation error result
  const mimeType =
    typeof args.mime_type === 'string' && args.mime_type !== ''
      ? args.mime_type
      : inferMimeType(path);

  if (!existing) {
    const ghost = findUnavailable(state.client, path);
    if (ghost) {
      return unavailableFileError(path, ghost.docId);
    }
    if (expectedHash !== undefined) {
      return error(
        `Error: write_file refused: expected_hash was given but ${path} does not exist in ` +
          'the project (it may have been deleted since you read it). Call list_files to see ' +
          'the current files; drop expected_hash to create a new file.',
      );
    }
    const created = await state.client.createBinaryFile(path, decoded, mimeType);
    return structured({
      ...binaryWriteMeta(created.path, decoded, mimeType),
      created: true,
      ...(await syncField(args, manager, project, [created.path])),
    });
  }
  if (existing.type === 'text') {
    return error(
      `Error: ${path} is a text file. Cannot write binary (base64) content to it — ` +
        'drop `encoding: "base64"` to write text, or delete_file first and re-create it as binary.',
    );
  }
  if (expectedHash !== undefined && hashPayload(existing) !== expectedHash) {
    return staleHashErrorBinary('write_file', path, existing);
  }

  await state.client.updateBinaryFileContent(path, decoded, mimeType);
  return structured({
    ...binaryWriteMeta(path, decoded, mimeType),
    ...(await syncField(args, manager, project, [path])),
  });
}

async function handlePatchFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const oldString = args.old_string as string;
  const newString = args.new_string as string;
  const expectedHash = typeof args.expected_hash === 'string' ? args.expected_hash : undefined;
  const state = await manager.connect(project, { server: routedServer(args) });
  const payload = state.files.get(path);

  if (!payload) {
    const ghost = findUnavailable(state.client, path);
    if (ghost) {
      return unavailableFileError(path, ghost.docId);
    }
    return fileNotFoundError(path, state);
  }
  if (payload.type === 'binary') {
    return error(`Error: ${path} is a binary file. Cannot patch.`);
  }
  if (expectedHash !== undefined && hashPayload(payload) !== expectedHash) {
    return staleHashError('patch_file', path, payload.text);
  }

  const currentContent = payload.text;
  const index = currentContent.indexOf(oldString);
  if (index === -1) {
    return error(
      `Error: old_string not found in ${path}. The file may have changed since you read it — ` +
        'call read_file for the current content (and its hash), then retry with an exact substring.',
    );
  }

  const secondIndex = currentContent.indexOf(oldString, index + 1);
  if (secondIndex !== -1) {
    return error(`Error: old_string appears multiple times in ${path}. Provide a longer, unique string to match.`);
  }

  const newContent =
    currentContent.slice(0, index) +
    newString +
    currentContent.slice(index + oldString.length);

  state.client.updateFileContent(path, newContent);
  return structured({
    path,
    hash: hashPayload({ type: 'text', text: newContent }),
    ...(await syncField(args, manager, project, [path])),
  });
}

async function handleCreateFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const content = (args.content as string) ?? '';
  const state = await manager.connect(project, { server: routedServer(args) });

  if (state.files.has(path)) {
    return error(`Error: File already exists: ${path}. Use write_file to update it.`);
  }
  // Same hazard as write_file: don't silently repoint a dangling entry.
  const ghost = findUnavailable(state.client, path);
  if (ghost) {
    return unavailableFileError(path, ghost.docId);
  }

  if (args.encoding === 'base64') {
    const decoded = decodeBase64(content);
    if (!ArrayBuffer.isView(decoded)) return decoded;
    const mimeType =
      typeof args.mime_type === 'string' && args.mime_type !== ''
        ? args.mime_type
        : inferMimeType(path);
    const created = await state.client.createBinaryFile(path, decoded, mimeType);
    return structured({
      ...binaryWriteMeta(created.path, decoded, mimeType),
      created: true,
      ...(await syncField(args, manager, project, [created.path])),
    });
  }

  await state.client.createFile(path, content);
  return structured({
    path,
    hash: hashPayload({ type: 'text', text: content }),
    created: true,
    ...(await syncField(args, manager, project, [path])),
  });
}

async function handleDeleteFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const state = await manager.connect(project, { server: routedServer(args) });

  // Dangling entries ARE deletable: delete only edits the index, no
  // document fetch involved — this is the self-service repair for a
  // ghost entry (bd-vm5e5u10; the 2026-06-12 incident needed manual
  // index surgery precisely because this path didn't exist).
  if (!state.files.has(path) && !findUnavailable(state.client, path)) {
    return fileNotFoundError(path, state);
  }

  state.client.deleteFile(path);
  return structured({
    path,
    deleted: true,
    ...(await syncField(args, manager, project, [])),
  });
}

async function handleRenameFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const oldPath = args.old_path as string;
  const newPath = args.new_path as string;
  const state = await manager.connect(project, { server: routedServer(args) });

  // Renaming only edits the index, so a dangling entry can be renamed.
  if (!state.files.has(oldPath) && !findUnavailable(state.client, oldPath)) {
    return fileNotFoundError(oldPath, state);
  }
  if (state.files.has(newPath) || findUnavailable(state.client, newPath)) {
    return error(
      `Error: Destination already exists: ${newPath} — rename_file does not overwrite. ` +
        `Choose a different new_path, or delete_file "${newPath}" first if replacing it is intended.`,
    );
  }

  state.client.renameFile(oldPath, newPath);
  return structured({
    old_path: oldPath,
    new_path: newPath,
    renamed: true,
    ...(await syncField(args, manager, project, [])),
  });
}

async function handleCreateProject(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const files = (args.files as Array<{ path: string; content: string }>) ?? [];
  const result = await manager.createProject(files);
  return structured({
    indexDocId: result.indexDocId,
    files: result.files,
    ...(await syncField(
      args,
      manager,
      result.indexDocId,
      result.files.map((f) => f.path),
    )),
  });
}

// ============================================================================
// Registration
// ============================================================================

/**
 * Run a data-tool handler, converting a throw into a tool-execution
 * error with token bytes redacted. The SDK wraps handler throws as
 * `isError` results too, but without redaction — this keeps the
 * defensive scrub on the error path.
 */
async function runDataTool(
  name: DataToolName,
  args: ToolArgs,
  manager: ConnectionManager,
  extras: ToolExtras = {},
): Promise<CallToolResult> {
  try {
    return await handleTool(name, args, manager, extras);
  } catch (err) {
    // Cancellation is not a tool error: let the abort propagate so the
    // SDK settles the (already cancelled) request without a bogus
    // isError payload.
    if (err instanceof DOMException && err.name === 'AbortError') throw err;
    const message = err instanceof Error ? err.message : String(err);
    return error(`Error in ${name}: ${redactTokens(message)}`);
  }
}

// ---------------------------------------------------------------------------
// Output schemas (BP-1)
//
// Every data tool declares the shape of its `structuredContent`. The
// Phase 0 harness keeps a golden case per declaring tool
// (conformance.test.ts GOLDEN_RESULT_CASES), so a schema added here
// without a golden case (or a drifted result shape) fails the suite.
// ---------------------------------------------------------------------------

const outListedFile = z.object({
  path: z.string(),
  type: z.string().optional(),
  status: z.literal('unavailable').optional(),
  docId: z.string().optional(),
  size: z.number().optional(),
  mimeType: z.string().optional(),
  lines: z.number().optional(),
});

const outConnectProject = z.object({
  project: z.string(),
  files: z.array(outListedFile),
});

const outListFiles = z.object({ files: z.array(outListedFile) });

const outReadFile = z.object({
  path: z.string(),
  hash: z.string(),
  type: z.enum(['text', 'binary']),
  content: z.string().optional(),
  mimeType: z.string().optional(),
  size: z.number().optional(),
  truncated: z.boolean().optional(),
  total_lines: z.number().optional(),
  next_offset: z.number().nullable().optional(),
  hint: z.string().optional(),
});

const outWaitForChange = z.object({
  changed: z.boolean(),
  path: z.string(),
  hash: z.string().nullable().optional(),
  removed: z.literal(true).optional(),
  type: z.string().optional(),
  mimeType: z.string().optional(),
  content: z.string().optional(),
  message: z.string().optional(),
});

const outWriteFile = z.object({
  path: z.string(),
  hash: z.string(),
  created: z.literal(true).optional(),
  synced: z.boolean().optional(),
  mimeType: z.string().optional(),
  size: z.number().optional(),
});

const outPatchFile = z.object({
  path: z.string(),
  hash: z.string(),
  synced: z.boolean().optional(),
});

const outCreateFile = z.object({
  path: z.string(),
  hash: z.string(),
  created: z.literal(true),
  synced: z.boolean().optional(),
  mimeType: z.string().optional(),
  size: z.number().optional(),
});

const outDeleteFile = z.object({
  path: z.string(),
  deleted: z.literal(true),
  synced: z.boolean().optional(),
});

const outRenameFile = z.object({
  old_path: z.string(),
  new_path: z.string(),
  renamed: z.literal(true),
  synced: z.boolean().optional(),
});

const outCreateProject = z.object({
  indexDocId: z.string(),
  files: z.array(z.object({ path: z.string(), docId: z.string() })),
  synced: z.boolean().optional(),
});

/**
 * Register all tool handlers on the MCP server. Auth tools register
 * first so they lead the `tools/list` order and the data tools'
 * "no credentials" errors can name them.
 */
export function registerTools(
  server: McpServer,
  manager: ConnectionManager,
  readOnly: boolean,
  authToolsState?: AuthToolsState,
): void {
  if (authToolsState) {
    for (const def of AUTH_TOOL_DEFINITIONS) {
      server.registerTool(
        def.name,
        {
          title: def.title,
          description: def.description,
          inputSchema: z.object({}),
          ...(def.outputSchema ? { outputSchema: def.outputSchema } : {}),
          annotations: def.annotations,
        },
        (_args, ctx) => authToolsState.handle(def.name, extractAuthContext(ctx)),
      );
    }
  }

  server.registerTool(
    'connect_project',
    {
      title: 'Connect to a project',
      description:
        'Connect to a Quarto Hub project by its automerge index document ID — ' +
        'or by a quarto-hub.com share URL (`https://quarto-hub.com/#/share/<id>?…`), ' +
        'from which the id is extracted automatically. ' +
        'Returns the list of files in the project. ' +
        'If the hub requires authentication and no valid credentials are cached, ' +
        'this throws an `AuthRequiredError` / `ReauthRequired` — call ' +
        '`authenticate` to sign in.',
      inputSchema: z.object({ project: projectParam }),
      outputSchema: outConnectProject,
      annotations: ANNOT_READ,
    },
    (args) => runDataTool('connect_project', args, manager),
  );

  server.registerTool(
    'list_files',
    {
      title: 'List files',
      description: 'List all files in a connected Quarto Hub project.',
      inputSchema: z.object({ project: projectParam }),
      outputSchema: outListFiles,
      annotations: ANNOT_READ,
    },
    (args) => runDataTool('list_files', args, manager),
  );

  server.registerTool(
    'read_file',
    {
      title: 'Read a file',
      description:
        'Read a file in a Quarto Hub project. Text files return `{ path, hash, type: "text", content }` — ' +
        'pass `hash` back as `expected_hash` on write_file/patch_file so an edit a collaborator made ' +
        'since this read is never silently overwritten. Large reads are capped by `max_bytes` ' +
        '(default 64 KB): a truncated result carries `next_offset` — call again with `offset` set ' +
        'to it to continue. Binary files return the bytes as an image ' +
        'block (image MIME types) or an embedded blob resource (anything else), with structured ' +
        '`{ path, hash, type: "binary", mimeType, size }`; pass `metadata_only: true` for just the ' +
        'metadata without the bytes.',
      inputSchema: z.object({
        project: projectParam,
        path: pathParam.optional(),
        offset: z
          .number()
          .int()
          .min(1)
          .optional()
          .describe('First line to return, 1-based (default 1).'),
        limit: z
          .number()
          .int()
          .min(1)
          .optional()
          .describe('Maximum number of lines to return (default: no line limit).'),
        max_bytes: z
          .number()
          .int()
          .min(16)
          .max(MAX_BYTES_CAP)
          .optional()
          .describe(
            `Byte cap on the returned content (default ${DEFAULT_MAX_BYTES}, max ${MAX_BYTES_CAP}). ` +
              'A read that does not reach EOF returns `truncated: true` with a `next_offset` to continue from.',
          ),
        metadata_only: z
          .boolean()
          .optional()
          .describe(
            'Binary files only: return just `{ path, hash, type, mimeType, size }` without ' +
              'the bytes. Errors on text files.',
          ),
      }),
      outputSchema: outReadFile,
      annotations: ANNOT_READ,
    },
    (args) => runDataTool('read_file', args, manager),
  );

  server.registerTool(
    'wait_for_change',
    {
      title: 'Watch for changes',
      description:
        'Long-poll: block until a file in the project is edited by any collaborator, then return its ' +
        'new content. Returns as soon as a change is observed, or after `timeout_seconds` with ' +
        '`changed: false` (re-call to keep watching). The result includes a `hash`; pass it back as ' +
        '`since_hash` on the next call so an edit landing between calls is never missed. Lets an agent ' +
        'react to a live collaborator without busy-polling read_file.',
      inputSchema: z.object({
        project: projectParam,
        path: z.string().describe('The file path within the project to watch').optional(),
        timeout_seconds: z
          .number()
          .describe('Max seconds to block before returning changed=false (default 25, clamped to 1-55)')
          .default(25),
        since_hash: z
          .string()
          .optional()
          .describe(
            'Optional hash from a prior result. If the file already differs from it, returns immediately ' +
              '(closes the gap between polls).',
          ),
      }),
      outputSchema: outWaitForChange,
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: false },
    },
    // The one blocking tool: thread the request's cancellation signal
    // (BP-3) so a client cancel unregisters the waiter promptly. ctx is
    // always present from the SDK; the optional chain keeps bare
    // handler-level test harnesses working.
    (args, ctx) => runDataTool('wait_for_change', args, manager, { signal: ctx?.mcpReq?.signal }),
  );

  if (readOnly) return;

  server.registerTool(
    'write_file',
    {
      title: 'Write a file',
      description:
        'Replace the entire content of a file in a Quarto Hub project. Creates the file if it ' +
        'does not exist. Returns `{ path, hash }` (plus `mimeType`/`size` for binary). ' +
        'Prefer patch_file for small changes to large text files. ' +
        'With `encoding: "base64"`, `content` is base64-encoded bytes and the file is binary.',
      inputSchema: z.object({
        project: projectParam,
        path: pathParam.optional(),
        content: z
          .string()
          .describe('The new file content (base64-encoded bytes when `encoding: "base64"`)'),
        encoding: encodingParam,
        mime_type: mimeTypeParam,
        expected_hash: z
          .string()
          .optional()
          .describe(
            'Optional `hash` from a prior read_file/write_file result. When given, the write is ' +
              'refused (returning the current content and its hash) if the file changed since — ' +
              'compare-and-swap against collaborator edits.',
          ),
        wait_for_sync: waitForSyncParam,
      }),
      outputSchema: outWriteFile,
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: true },
    },
    (args) => runDataTool('write_file', args, manager),
  );

  server.registerTool(
    'patch_file',
    {
      title: 'Patch a file',
      description:
        'Apply a targeted edit to a text file by replacing a specific string. More context-efficient ' +
        'than write_file for small changes to large files. Returns `{ path, hash }`.',
      inputSchema: z.object({
        project: projectParam,
        path: pathParam.optional(),
        old_string: z.string().describe('The exact string to find and replace'),
        new_string: z.string().describe('The replacement string'),
        expected_hash: z
          .string()
          .optional()
          .describe(
            'Optional `hash` from a prior read_file/patch_file result. When given, the patch is ' +
              'refused (returning the current content and its hash) if the file changed since — ' +
              'compare-and-swap against collaborator edits.',
          ),
        wait_for_sync: waitForSyncParam,
      }),
      outputSchema: outPatchFile,
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false },
    },
    (args) => runDataTool('patch_file', args, manager),
  );

  server.registerTool(
    'create_file',
    {
      title: 'Create a file',
      description:
        'Create a new file in a Quarto Hub project. Text by default; with ' +
        '`encoding: "base64"`, `content` is base64-encoded bytes and the file is binary.',
      inputSchema: z.object({
        project: projectParam,
        path: pathParam.optional(),
        content: z
          .string()
          .describe('Initial file content (defaults to empty; base64 bytes when `encoding: "base64"`)')
          .default(''),
        encoding: encodingParam,
        mime_type: mimeTypeParam,
        wait_for_sync: waitForSyncParam,
      }),
      outputSchema: outCreateFile,
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false },
    },
    (args) => runDataTool('create_file', args, manager),
  );

  server.registerTool(
    'delete_file',
    {
      title: 'Delete a file',
      description: 'Delete a file from a Quarto Hub project.',
      inputSchema: z.object({
        project: projectParam,
        path: z.string().describe('The file path to delete').optional(),
        wait_for_sync: waitForSyncParam,
      }),
      outputSchema: outDeleteFile,
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false },
    },
    (args) => runDataTool('delete_file', args, manager),
  );

  server.registerTool(
    'rename_file',
    {
      title: 'Rename a file',
      description: 'Rename or move a file within a Quarto Hub project.',
      inputSchema: z.object({
        project: projectParam,
        old_path: z.string().describe('The current file path'),
        new_path: z.string().describe('The new file path'),
        wait_for_sync: waitForSyncParam,
      }),
      outputSchema: outRenameFile,
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false },
    },
    (args) => runDataTool('rename_file', args, manager),
  );

  server.registerTool(
    'create_project',
    {
      title: 'Create a project',
      description: 'Create a new Quarto Hub project on the sync server with optional initial files.',
      inputSchema: z.object({
        files: z
          .array(
            z.object({
              path: z.string().describe('File path'),
              content: z.string().describe('File content'),
            }),
          )
          .describe('Initial files to create in the project')
          .default([]),
        wait_for_sync: waitForSyncParam,
      }),
      outputSchema: outCreateProject,
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false, openWorldHint: true },
    },
    (args) => runDataTool('create_project', args, manager),
  );
}
