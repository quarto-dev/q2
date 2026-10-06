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
  getCapturesFromIndex,
  getIdentitiesFromIndex,
  inferMimeType,
  normalizeProjectPath,
  type FilePayload,
  type SyncClient,
} from '@quarto/quarto-sync-client';
import { ConnectionManager, hashPayload } from './connection-manager.js';
import {
  diffFileHistory,
  formatUnifiedDiff,
  listFileHistory,
  UnknownChangeHashError,
} from './file-history.js';
import {
  AUTH_TOOL_DEFINITIONS,
  AuthToolsState,
  extractAuthContext,
  type ProgressNotification,
} from './auth/auth-tools.js';
import { redactTokens } from './auth/redact.js';
import { buildShareUrl, parseProjectRef, serversMatch } from './share-url.js';

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
  // Explicit folder markers (CAP-6). Folders file paths merely imply are
  // not listed — an empty folder exists only once created.
  for (const folder of state.client.getFolderPaths()) {
    fileList.push({ path: folder, type: 'folder' });
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
  | 'disconnect_project'
  | 'get_project_info'
  | 'list_projects'
  | 'list_files'
  | 'list_presence'
  | 'get_file_history'
  | 'read_file'
  | 'search_files'
  | 'wait_for_change'
  | 'write_file'
  | 'patch_file'
  | 'create_file'
  | 'delete_file'
  | 'rename_file'
  | 'create_folder'
  | 'delete_folder'
  | 'create_project';

/**
 * Tools whose `path` argument a share URL's `file=` parameter can
 * supply (see {@link normalizeArgs}). Their zod schemas declare `path`
 * optional — a schema-level `required` would reject the share-URL call
 * before normalization runs — so the requirement is enforced here,
 * after normalization.
 *
 * `wait_for_change` is deliberately absent: its `path` is genuinely
 * optional since CAP-18 (omitted = project-wide watch). A share URL's
 * `file=` still fills it as a default via normalizeArgs.
 */
const PATH_DEFAULTABLE: ReadonlySet<DataToolName> = new Set([
  'read_file',
  'write_file',
  'patch_file',
  'create_file',
  'delete_file',
  'get_file_history',
]);

/** Per-call extras threaded from the SDK request context (BP-3, BP-4). */
interface ToolExtras {
  /** The MCP request's cancellation signal, when the caller can cancel. */
  readonly signal?: AbortSignal;
  /** Present only when the caller requested progress notifications (BP-4). */
  readonly progressToken?: string | number;
  /** Notification sender bound to the request (BP-4); token-gated by the caller. */
  readonly sendNotification?: (n: ProgressNotification) => Promise<void>;
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
    case 'disconnect_project':
      return handleDisconnectProject(args, manager);
    case 'get_project_info':
      return handleGetProjectInfo(args, manager);
    case 'list_projects':
      return handleListProjects(args, manager);
    case 'list_files':
      return handleListFiles(args, manager);
    case 'list_presence':
      return handleListPresence(args, manager);
    case 'get_file_history':
      return handleGetFileHistory(args, manager);
    case 'read_file':
      return handleReadFile(args, manager);
    case 'search_files':
      return handleSearchFiles(args, manager);
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
    case 'create_folder':
      return handleCreateFolder(args, manager);
    case 'delete_folder':
      return handleDeleteFolder(args, manager);
    case 'create_project':
      return handleCreateProject(args, manager);
  }
}

async function handleConnectProject(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const state = await manager.connect(project, { server: routedServer(args) });
  return structured({
    project,
    files: buildFileList(state),
    shareUrl: buildShareUrl({ server: state.serverUrl, indexDocId: project }),
  });
}

async function handleDisconnectProject(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const waitForSync = args.wait_for_sync !== false;
  const result = await manager.disconnect(project, {
    server: routedServer(args),
    drainMs: waitForSync ? SYNC_WAIT_MS : 0,
  });
  if (!result.wasConnected) {
    const connected = manager.connectedProjects();
    const suffix =
      connected.length > 0
        ? ` Connected projects: ${connected.map((c) => c.indexDocId).join(', ')}.`
        : ' No projects are currently connected.';
    return error(`Error: not connected to project ${project} — nothing to disconnect.${suffix}`);
  }
  return structured({
    project,
    disconnected: true,
    ...(waitForSync ? { synced: result.drained } : {}),
  });
}

async function handleGetProjectInfo(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const state = await manager.connect(project, { server: routedServer(args) });
  // The index doc is the source of truth for identities/captures; both
  // getters tolerate a V1 doc (absent maps → {}).
  const doc = state.client.getIndexHandle()?.doc();
  const identities = doc ? getIdentitiesFromIndex(doc) : {};
  const captures = doc ? getCapturesFromIndex(doc) : {};
  const diag = state.client.getSyncDiagnostics();
  let binary = 0;
  for (const payload of state.files.values()) {
    if (payload.type === 'binary') binary++;
  }
  return structured({
    project,
    server: state.serverUrl,
    shareUrl: buildShareUrl({ server: state.serverUrl, indexDocId: project }),
    // A foreign project (share-URL `server=` routing) is always joined
    // authorless, so its effective mode is no-auth regardless of the
    // configured hub's observation.
    auth_mode: serversMatch(state.serverUrl, manager.configuredServerUrl)
      ? manager.lastObservedAuthMode()
      : 'no-auth',
    counts: {
      files: state.files.size,
      binary,
      folders: state.client.getFolderPaths().length,
      unavailable: state.client.getUnavailableFiles().length,
    },
    identities,
    captures,
    sync: {
      connected_peers: diag.connectedPeers,
      retry_timer_active: diag.retryTimerActive,
      unavailable_retry_ticks: diag.unavailableRetryTicks,
      stranded: diag.stranded,
    },
  });
}

async function handleListProjects(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  // `project_set` takes the same id-or-share-URL forms as `project`
  // (normalizeArgs only rewrites `project`, so parse explicitly here).
  const ref = parseProjectRef(args.project_set as string);
  const server =
    ref.server && !serversMatch(ref.server, manager.configuredServerUrl)
      ? ref.server
      : undefined;
  let doc;
  try {
    doc = await manager.readProjectSet(ref.project, { server });
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    return error(
      `Error: could not read project-set ${ref.project}: ${redactTokens(msg)}. ` +
        'Check the id with whoever shared it — and if it names a project rather than a ' +
        'project set, use connect_project instead.',
    );
  }
  const projects = Object.entries(doc.projects)
    .map(([indexDocId, entry]) => ({
      indexDocId,
      syncServer: entry.syncServer,
      description: entry.description,
      addedAt: entry.addedAt,
      lastAccessed: entry.lastAccessed,
      ...(entry.summary ? { summary: entry.summary } : {}),
      shareUrl: buildShareUrl({
        server: entry.syncServer,
        indexDocId,
        name: entry.description,
      }),
    }))
    .sort((a, b) => (a.lastAccessed < b.lastAccessed ? 1 : a.lastAccessed > b.lastAccessed ? -1 : 0));
  return structured({
    ...(doc.name !== undefined ? { name: doc.name } : {}),
    projects,
  });
}

async function handleGetFileHistory(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
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
    return error(
      `Error: get_file_history only supports text files; "${path}" is binary ` +
        `(${payload.mimeType}). Binary content is all-or-nothing — there is no per-line ` +
        'history or diff to show.',
    );
  }
  const handle = state.client.getFileHandle(path);
  const doc = handle?.doc();
  if (!handle || !doc) {
    return fileNotFoundError(path, state);
  }

  const fromHash = typeof args.from_hash === 'string' ? args.from_hash : undefined;
  const toHash = typeof args.to_hash === 'string' ? args.to_hash : undefined;

  // Diff mode (CAP-9 rule (a): the diff is a mode of this tool, not a sibling).
  if (fromHash !== undefined || toHash !== undefined) {
    if (fromHash === undefined) {
      return error(
        'Error: `to_hash` requires `from_hash` — the diff direction would be ambiguous. ' +
          'Pass both (from `get_file_history` list mode), or pass `from_hash` alone to diff ' +
          'against the current content.',
      );
    }
    try {
      const { fromText, toText, resolvedTo } = diffFileHistory(doc, fromHash, toHash);
      const { diff, addedLines, removedLines } = formatUnifiedDiff(fromText, toText, path);
      return structured({
        path,
        from_hash: fromHash,
        to_hash: resolvedTo,
        diff,
        added_lines: addedLines,
        removed_lines: removedLines,
      });
    } catch (err) {
      if (err instanceof UnknownChangeHashError) {
        return error(
          `Error: ${err.message} in "${path}". Call get_file_history without ` +
            '`from_hash`/`to_hash` to list the change hashes this file knows.',
        );
      }
      throw err;
    }
  }

  // List mode.
  const rawLimit = typeof args.limit === 'number' ? args.limit : 20;
  const limit = Math.max(1, Math.min(100, rawLimit));
  const indexDoc = state.client.getIndexHandle()?.doc();
  const identities = indexDoc ? getIdentitiesFromIndex(indexDoc) : {};
  const result = listFileHistory(doc, identities, limit);
  return structured({
    path,
    heads: result.heads,
    entries: result.entries,
    total_changes: result.totalChanges,
    truncated: result.truncated,
  });
}

async function handleListPresence(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const presences = await manager.observePresence(project, { server: routedServer(args) });
  return structured({
    project,
    presences,
    ...(presences.length === 0
      ? {
          message:
            'No collaborators heard from recently. Presence is observed passively: a peer ' +
            'appears here only after their editor broadcasts (on cursor activity), and drops ' +
            'out about a minute after their last broadcast. Absence of an entry does not prove ' +
            'nobody has the project open.',
        }
      : {}),
  });
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

// ---------------------------------------------------------------------------
// CAP-7: search_files
// ---------------------------------------------------------------------------

/** Snippet budget per match line. Long lines are centered on the match. */
const SNIPPET_MAX_CHARS = 200;

function makeSnippet(line: string, matchIndex: number, matchLength: number): string {
  const trimmed = line.trim();
  if (trimmed.length <= SNIPPET_MAX_CHARS) return trimmed;
  // Center a SNIPPET_MAX_CHARS window on the match (in raw-line coords).
  let start = Math.max(0, matchIndex - Math.floor((SNIPPET_MAX_CHARS - matchLength) / 2));
  const end = Math.min(line.length, start + SNIPPET_MAX_CHARS);
  start = Math.max(0, end - SNIPPET_MAX_CHARS);
  const prefix = start > 0 ? '…' : '';
  const suffix = end < line.length ? '…' : '';
  return `${prefix}${line.slice(start, end).trim()}${suffix}`;
}

async function handleSearchFiles(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const query = args.query as string;
  const useRegex = args.regex === true;
  const caseSensitive = args.case_sensitive === true;
  const maxResults = typeof args.max_results === 'number' ? args.max_results : 20;
  if (query === '') {
    return error(
      'Error: parameter `query` must not be empty — pass a substring (or a pattern with `regex: true`) to search for.',
    );
  }
  const state = await manager.connect(project, { server: routedServer(args) });

  // A line matcher: the match's start index in the line, or -1.
  let matcher: (line: string) => number;
  if (useRegex) {
    let re: RegExp;
    try {
      re = new RegExp(query, caseSensitive ? '' : 'i');
    } catch (err) {
      const detail = err instanceof Error ? err.message : String(err);
      return error(
        `Error: parameter \`query\` is not a valid regular expression (${detail}). ` +
          'Fix the pattern, or pass `regex: false` (the default) for a plain substring search.',
      );
    }
    matcher = (line) => {
      const m = re.exec(line);
      return m === null ? -1 : m.index;
    };
  } else {
    matcher = (line) =>
      caseSensitive
        ? line.indexOf(query)
        : line.toLowerCase().indexOf(query.toLowerCase());
  }

  // Scan text files in path order; binaries and dangling entries have
  // no searchable text. One snippet per matching line.
  const perFile = new Map<string, Array<{ line: number; snippet: string }>>();
  let totalMatches = 0;
  let filesSearched = 0;
  const paths = [...state.files.keys()].sort();
  for (const path of paths) {
    const payload = state.files.get(path)!;
    if (payload.type !== 'text') continue;
    filesSearched++;
    const lines = splitLines(payload.text);
    for (let i = 0; i < lines.length; i++) {
      const idx = matcher(lines[i]);
      if (idx === -1) continue;
      totalMatches++;
      const found = perFile.get(path) ?? [];
      found.push({ line: i + 1, snippet: makeSnippet(lines[i], idx, query.length) });
      perFile.set(path, found);
    }
  }

  // Rank: per-file match count desc, then path asc (deterministic).
  const ranked = [...perFile.entries()].sort(
    (a, b) => b[1].length - a[1].length || (a[0] < b[0] ? -1 : 1),
  );
  const matches: Array<{ path: string; line: number; snippet: string }> = [];
  for (const [path, fileMatches] of ranked) {
    for (const m of fileMatches) {
      if (matches.length >= maxResults) break;
      matches.push({ path, line: m.line, snippet: m.snippet });
    }
    if (matches.length >= maxResults) break;
  }

  return structured({
    matches,
    total_matches: totalMatches,
    files_searched: filesSearched,
    truncated: totalMatches > matches.length,
  });
}

/**
 * Progress cadence for the blocking watch (BP-4): an immediate
 * `progress: 0` so the host shows life, then one notification per 5 s.
 * Hosts with `resetTimeoutOnProgress` treat each as a liveness proof,
 * which is what lets a 55 s poll outlive a short per-request timeout.
 */
const PROGRESS_INTERVAL_MS = 5000;

/**
 * Run `wait` under a progress ticker: emits `progress: 0` immediately
 * and `elapsed / total` every {@link PROGRESS_INTERVAL_MS} until the
 * wait settles. No-op without a progressToken (BP-4 is opt-in per
 * request). The ticker is always torn down before the result returns,
 * so no notification ever races the response.
 */
async function withWaitProgress<T>(
  extras: ToolExtras,
  totalMs: number,
  watching: string,
  wait: () => Promise<T>,
): Promise<T> {
  if (extras.progressToken === undefined || !extras.sendNotification) {
    return wait();
  }
  const send = extras.sendNotification;
  const token = extras.progressToken;
  const startedAt = Date.now();
  const emit = (progress: number) =>
    send({
      method: 'notifications/progress',
      params: {
        progressToken: token,
        progress,
        total: totalMs,
        message: `watching ${watching} (${Math.round(progress / 1000)}s of ${Math.round(totalMs / 1000)}s)`,
      },
    }).catch(() => {
      // A dead notification channel must not fail the wait itself.
    });
  const ticker = setInterval(() => {
    void emit(Math.min(Date.now() - startedAt, totalMs));
  }, PROGRESS_INTERVAL_MS);
  try {
    await emit(0);
    return await wait();
  } finally {
    clearInterval(ticker);
  }
}

async function handleWaitForChange(
  args: ToolArgs,
  manager: ConnectionManager,
  extras: ToolExtras,
): Promise<CallToolResult> {
  const project = args.project as string;
  const path = typeof args.path === 'string' && args.path !== '' ? args.path : undefined;
  const rawTimeout = typeof args.timeout_seconds === 'number' ? args.timeout_seconds : 25;
  const timeoutSec = Math.max(1, Math.min(55, rawTimeout));
  const sinceHash = typeof args.since_hash === 'string' ? args.since_hash : undefined;

  // Project-wide arm (CAP-18): no path → watch every file. `since_hash`
  // becomes an exclusion filter for the caller's own write echo (ERG-8).
  if (path === undefined) {
    const result = await withWaitProgress(extras, timeoutSec * 1000, 'the whole project', () =>
      manager.waitForAnyChange(project, timeoutSec * 1000, {
        sinceHash,
        signal: extras.signal,
        server: routedServer(args),
      }),
    );
    if (!result.changed) {
      return structured({
        changed: false,
        changes: [],
        message: `No change within ${timeoutSec}s. Call wait_for_change again to keep watching.`,
      });
    }
    return structured({ changed: true, changes: result.changes });
  }

  const result = await withWaitProgress(extras, timeoutSec * 1000, path, () =>
    manager.waitForChange(project, path, timeoutSec * 1000, sinceHash, {
      signal: extras.signal,
      server: routedServer(args),
    }),
  );

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

/**
 * Every folder an agent can mean: explicit markers plus the folders
 * file paths imply (a/b/c.qmd implies `a` and `a/b`). Used for
 * not-found suggestions — listings show explicit markers only (CAP-6).
 */
function allFolderCandidates(state: ProjectState): Set<string> {
  const out = new Set<string>(state.client.getFolderPaths());
  for (const p of allPaths(state)) {
    const segs = p.split('/');
    for (let i = 1; i < segs.length; i++) {
      out.add(segs.slice(0, i).join('/'));
    }
  }
  return out;
}

async function handleCreateFolder(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = normalizeProjectPath(args.path as string);
  if (path === '') {
    return error(
      'Error: create_folder requires a non-empty `path` — the folder to create within the project.',
    );
  }
  const state = await manager.connect(project, { server: routedServer(args) });
  if (state.files.has(path)) {
    return error(
      `Error: ${path} is a file, not a folder. Choose a different folder name, ` +
        `or rename_file "${path}" first if it should move.`,
    );
  }
  // Idempotent: the marker is a set member, not a creation event.
  const existed = state.client.getFolderPaths().includes(path);
  if (!existed) {
    state.client.createFolder(path);
  }
  return structured({
    path,
    created: !existed,
    ...(await syncField(args, manager, project, [])),
  });
}

async function handleDeleteFolder(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = normalizeProjectPath(args.path as string);
  const recursive = args.recursive === true;
  const state = await manager.connect(project, { server: routedServer(args) });

  const markerExists = state.client.getFolderPaths().includes(path);
  const contained = allPaths(state).filter((p) => p.startsWith(`${path}/`));
  if (!markerExists && contained.length === 0) {
    const near = closestPaths(path, allFolderCandidates(state));
    let msg =
      `Error: Folder not found: "${path}". Call list_files to see the project's files and folders.`;
    if (near.length > 0) {
      msg += ` Closest existing folders: ${near.map((p) => `"${p}"`).join(', ')}.`;
    }
    return error(msg);
  }
  if (contained.length > 0 && !recursive) {
    const sample = contained.slice(0, 3).map((p) => `"${p}"`).join(', ');
    return error(
      `Error: Folder "${path}" is not empty — ${contained.length} file` +
        `${contained.length === 1 ? '' : 's'} remain${contained.length === 1 ? 's' : ''} under it ` +
        `(${sample}${contained.length > 3 ? ', …' : ''}). Pass \`recursive: true\` to delete ` +
        'them together with the folder, or delete_file them individually first.',
    );
  }
  for (const p of contained) {
    state.client.deleteFile(p);
  }
  if (markerExists) {
    state.client.deleteFolder(path);
  }
  return structured({
    path,
    deleted: true,
    ...(contained.length > 0 ? { files_deleted: contained.length } : {}),
    ...(await syncField(args, manager, project, [])),
  });
}

async function handleCreateProject(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const files = (args.files as Array<{ path: string; content: string }>) ?? [];
  const name = typeof args.name === 'string' && args.name !== '' ? args.name : undefined;
  const result = await manager.createProject(files);
  return structured({
    indexDocId: result.indexDocId,
    files: result.files,
    shareUrl: buildShareUrl({
      server: manager.configuredServerUrl,
      indexDocId: result.indexDocId,
      ...(name ? { name } : {}),
    }),
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

const outPresenceEntry = z.object({
  peer_id: z.string(),
  user_id: z.string(),
  user_name: z.string(),
  user_color: z.string(),
  file_path: z.string().nullable(),
  cursor_offset: z.number().nullable(),
  selection: z
    .object({ start_offset: z.number(), end_offset: z.number() })
    .nullable(),
  last_seen_ms_ago: z.number(),
  active: z.boolean(),
});

const outListPresence = z.object({
  project: z.string(),
  presences: z.array(outPresenceEntry),
  message: z.string().optional(),
});

const outHistoryEntry = z.object({
  head: z.string(),
  hash: z.string(),
  seq: z.number(),
  time: z.number(),
  author: z.string(),
  name: z.string().nullable(),
  color: z.string().nullable(),
  added_chars: z.number(),
  removed_chars: z.number(),
});

// CAP-9: one tool, two modes (ERG-5 rule (a)) — a bounded change
// listing, or a unified diff between two heads.
const outGetFileHistory = z.union([
  z.object({
    path: z.string(),
    heads: z.array(z.string()),
    entries: z.array(outHistoryEntry),
    total_changes: z.number(),
    truncated: z.boolean(),
  }),
  z.object({
    path: z.string(),
    from_hash: z.string(),
    to_hash: z.string(),
    diff: z.string(),
    added_lines: z.number(),
    removed_lines: z.number(),
  }),
]);

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
  shareUrl: z.string(),
});

const outDisconnectProject = z.object({
  project: z.string(),
  disconnected: z.literal(true),
  synced: z.boolean().optional(),
});

const outIdentity = z.object({ name: z.string(), color: z.string() });

const outGetProjectInfo = z.object({
  project: z.string(),
  server: z.string(),
  shareUrl: z.string(),
  auth_mode: z.enum(['no-auth', 'requires-auth', 'unknown']),
  counts: z.object({
    files: z.number(),
    binary: z.number(),
    folders: z.number(),
    unavailable: z.number(),
  }),
  identities: z.record(z.string(), outIdentity),
  captures: z.record(
    z.string(),
    z.object({
      captureDocId: z.string(),
      staleness: z.boolean().optional(),
      state: z.string().optional(),
      lastError: z.string().optional(),
    }),
  ),
  sync: z.object({
    connected_peers: z.number(),
    retry_timer_active: z.boolean(),
    unavailable_retry_ticks: z.number(),
    stranded: z.array(
      z.object({
        path: z.string(),
        docId: z.string(),
        handleState: z.string().nullable(),
        unavailableMarker: z.boolean(),
      }),
    ),
  }),
});

const outListProjects = z.object({
  name: z.string().optional(),
  projects: z.array(
    z.object({
      indexDocId: z.string(),
      syncServer: z.string(),
      description: z.string(),
      addedAt: z.string(),
      lastAccessed: z.string(),
      summary: z
        .object({
          fileCount: z.number(),
          topFiles: z.array(z.string()),
          contributors: z.array(outIdentity),
          asOf: z.string(),
        })
        .optional(),
      shareUrl: z.string(),
    }),
  ),
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
  // Per-path arm: the watched path. Absent on the project-wide arm
  // (CAP-18), which reports `changes` instead.
  path: z.string().optional(),
  hash: z.string().nullable().optional(),
  removed: z.literal(true).optional(),
  type: z.string().optional(),
  mimeType: z.string().optional(),
  content: z.string().optional(),
  message: z.string().optional(),
  // Project-wide arm (CAP-18): every file added, edited, or removed
  // during the wait, with its post-change content hash (null on
  // removal).
  changes: z
    .array(
      z.object({
        path: z.string(),
        hash: z.string().nullable(),
        kind: z.enum(['added', 'edited', 'removed']),
      }),
    )
    .optional(),
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

const outSearchFiles = z.object({
  matches: z.array(
    z.object({
      path: z.string(),
      line: z.number(),
      snippet: z.string(),
    }),
  ),
  total_matches: z.number(),
  files_searched: z.number(),
  truncated: z.boolean(),
});

const outCreateFolder = z.object({
  path: z.string(),
  created: z.boolean(),
  synced: z.boolean().optional(),
});

const outDeleteFolder = z.object({
  path: z.string(),
  deleted: z.literal(true),
  files_deleted: z.number().optional(),
  synced: z.boolean().optional(),
});

const outCreateProject = z.object({
  indexDocId: z.string(),
  files: z.array(z.object({ path: z.string(), docId: z.string() })),
  shareUrl: z.string(),
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
        'Returns the list of files in the project and its `shareUrl`. ' +
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
    'search_files',
    {
      title: 'Search files',
      description:
        'Search the content of every text file in a Quarto Hub project (binaries are skipped). ' +
        'Plain substring by default (case-insensitive unless `case_sensitive`), or a regular ' +
        'expression with `regex: true`. Returns matching lines as `{ path, line, snippet }` ' +
        'ranked by per-file match count, capped by `max_results` (`truncated: true` when capped). ' +
        'Use read_file with `offset` around a hit to see full context.',
      inputSchema: z.object({
        project: projectParam,
        query: z.string().describe('The substring (or regex pattern with `regex: true`) to search for'),
        regex: z.boolean().optional().describe('Treat `query` as a regular expression (default false)'),
        case_sensitive: z.boolean().optional().describe('Case-sensitive matching (default false)'),
        max_results: z
          .number()
          .int()
          .min(1)
          .max(100)
          .optional()
          .describe('Maximum matches to return (default 20, max 100)'),
      }),
      outputSchema: outSearchFiles,
      annotations: ANNOT_READ,
    },
    (args) => runDataTool('search_files', args, manager),
  );

  server.registerTool(
    'wait_for_change',
    {
      title: 'Watch for changes',
      description:
        'Long-poll: block until something changes, then return it. With `path`, watches that file and ' +
        'returns its new content on edit. Omit `path` to watch the whole project: returns ' +
        '`changes: [{path, hash, kind}]` (kind is added/edited/removed) for any collaborator\'s ' +
        'activity — pass your own just-written `hash` as `since_hash` so your own write\'s echo is ' +
        'not reported back to you. Returns as soon as a change is observed, or after ' +
        '`timeout_seconds` with `changed: false` (re-call to keep watching). On the per-file arm ' +
        'the result includes a `hash`; pass it back as `since_hash` on the next call so an edit ' +
        'landing between calls is never missed. Lets an agent react to a live collaborator without ' +
        'busy-polling read_file.',
      inputSchema: z.object({
        project: projectParam,
        path: z
          .string()
          .describe(
            'The file path within the project to watch. Omit to watch the whole project ' +
              '(result carries `changes` instead of content).',
          )
          .optional(),
        timeout_seconds: z
          .number()
          .describe('Max seconds to block before returning changed=false (default 25, clamped to 1-55)')
          .default(25),
        since_hash: z
          .string()
          .optional()
          .describe(
            'Per-file arm: hash from a prior result — if the file already differs, returns ' +
              'immediately (closes the gap between polls). Project-wide arm: your own post-write ' +
              'hash — its echo is excluded from the result.',
          ),
      }),
      outputSchema: outWaitForChange,
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: false },
    },
    // The one blocking tool: thread the request's cancellation signal
    // (BP-3) so a client cancel unregisters the waiter promptly, and
    // the progressToken + notification channel (BP-4) so a host that
    // asked for progress gets it. ctx is always present from the SDK;
    // the optional chains keep bare handler-level test harnesses working.
    (args, ctx) =>
      runDataTool('wait_for_change', args, manager, {
        signal: ctx?.mcpReq?.signal,
        progressToken: ctx?.mcpReq?._meta?.progressToken,
        sendNotification: ctx?.mcpReq
          ? (n) => ctx.mcpReq.notify(n as unknown as Parameters<typeof ctx.mcpReq.notify>[0])
          : undefined,
      }),
  );

  server.registerTool(
    'get_project_info',
    {
      title: 'Get project info',
      description:
        'Project health and shape in one call: file/folder/binary counts, contributor ' +
        'identities, engine-capture state (idle/running/error + lastError), the index ' +
        'document id, sync server, observed auth mode, sync diagnostics (peers, stranded ' +
        'files), and the project\'s share URL. The "doctor" tool — call it to understand ' +
        'an unfamiliar project or diagnose a connection.',
      inputSchema: z.object({ project: projectParam }),
      outputSchema: outGetProjectInfo,
      annotations: ANNOT_READ,
    },
    (args) => runDataTool('get_project_info', args, manager),
  );

  server.registerTool(
    'list_projects',
    {
      title: 'List projects in a collection',
      description:
        'Enumerate a Quarto Hub project-set (a user\'s collection of projects), given its ' +
        'document id or a share URL to it — a human gets this link from the web client. ' +
        'Returns each project\'s id, sync server, description, and a share URL you can pass ' +
        'to connect_project, most-recently-used first.',
      inputSchema: z.object({
        project_set: z
          .string()
          .describe(
            'The project-set document id, OR a quarto-hub.com share URL to the set ' +
              '(the `server=` parameter, if present, routes the read to that hub).',
          ),
      }),
      outputSchema: outListProjects,
      annotations: ANNOT_READ,
    },
    (args) => runDataTool('list_projects', args, manager),
  );

  server.registerTool(
    'list_presence',
    {
      title: 'List collaborators present',
      description:
        'Who is in the project right now: each collaborator\'s name, the file they are ' +
        'editing, cursor/selection offsets when resolvable, and how long ago they were last ' +
        'heard from (`active` = within the last 5s, same threshold the web client shows). ' +
        'Observation is passive — this server never announces itself, and a peer only appears ' +
        'after their editor broadcasts (on cursor activity), so an empty list does not prove ' +
        'nobody is watching. Check before editing a file a teammate may have open; combine ' +
        'with wait_for_change to react to their edits.',
      inputSchema: z.object({
        project: projectParam,
      }),
      outputSchema: outListPresence,
      annotations: ANNOT_READ,
    },
    (args) => runDataTool('list_presence', args, manager),
  );

  server.registerTool(
    'get_file_history',
    {
      title: 'Get file history',
      description:
        'History of one text file. List mode (default): the most recent changes, newest ' +
        'first — each entry carries the change `head` (pass to `from_hash`/`to_hash` or ' +
        'restore_file_version), the content `hash` after it, its author (with display ' +
        'name/color when the project records them), timestamp, and added/removed character ' +
        'counts. Diff mode: pass `from_hash` and `to_hash` (from a prior listing) for a ' +
        'unified diff between those two versions; `from_hash` alone diffs that version ' +
        'against the current content. Use it to answer "what changed since yesterday", to ' +
        'review a collaborator\'s edit before building on it, or to pick a restore point ' +
        'for restore_file_version.',
      inputSchema: z.object({
        project: projectParam,
        path: pathParam.optional(),
        limit: z
          .number()
          .int()
          .min(1)
          .max(100)
          .optional()
          .describe('Max changes to return in list mode (default 20, cap 100).'),
        from_hash: z
          .string()
          .optional()
          .describe('Diff mode: the older change `head`. Alone, diffs against the current content.'),
        to_hash: z
          .string()
          .optional()
          .describe('Diff mode: the newer change `head` (requires `from_hash`).'),
      }),
      outputSchema: outGetFileHistory,
      annotations: ANNOT_READ,
    },
    (args) => runDataTool('get_file_history', args, manager),
  );

  server.registerTool(
    'disconnect_project',
    {
      title: 'Disconnect from a project',
      description:
        'Drop the connection to one project (websocket + in-memory documents) without ' +
        'affecting others — the release valve for long sessions touching many projects. ' +
        'Outbound sync is drained first (bounded); the next tool call to the project ' +
        'transparently reconnects.',
      inputSchema: z.object({
        project: projectParam,
        wait_for_sync: waitForSyncParam,
      }),
      outputSchema: outDisconnectProject,
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true },
    },
    (args) => runDataTool('disconnect_project', args, manager),
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
    'create_folder',
    {
      title: 'Create a folder',
      description:
        'Create a folder in a Quarto Hub project. Folders are explicit markers: a file at ' +
        '`a/b/c.qmd` needs no folder to exist, but an EMPTY folder is listed only once created. ' +
        'Idempotent — re-creating an existing folder reports `created: false`.',
      inputSchema: z.object({
        project: projectParam,
        path: z.string().describe('The folder path to create (e.g. `assets/images`)'),
        wait_for_sync: waitForSyncParam,
      }),
      outputSchema: outCreateFolder,
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true },
    },
    (args) => runDataTool('create_folder', args, manager),
  );

  server.registerTool(
    'delete_folder',
    {
      title: 'Delete a folder',
      description:
        'Delete a folder from a Quarto Hub project. Refuses while files remain under the path ' +
        'unless `recursive: true`, which deletes the contained files first (reported as ' +
        '`files_deleted`).',
      inputSchema: z.object({
        project: projectParam,
        path: z.string().describe('The folder path to delete'),
        recursive: z
          .boolean()
          .optional()
          .describe('Delete the files under the folder too (default false — refuse if non-empty).'),
        wait_for_sync: waitForSyncParam,
      }),
      outputSchema: outDeleteFolder,
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false },
    },
    (args) => runDataTool('delete_folder', args, manager),
  );

  server.registerTool(
    'create_project',
    {
      title: 'Create a project',
      description:
        'Create a new Quarto Hub project on the sync server with optional initial files. ' +
        'The result includes a `shareUrl` you can hand a human to open the project.',
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
        name: z
          .string()
          .optional()
          .describe('Human-readable project name — carried on the result\'s `shareUrl`.'),
        wait_for_sync: waitForSyncParam,
      }),
      outputSchema: outCreateProject,
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false, openWorldHint: true },
    },
    (args) => runDataTool('create_project', args, manager),
  );
}
