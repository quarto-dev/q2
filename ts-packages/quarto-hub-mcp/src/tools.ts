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
  type ProjectSetDocument,
  type SyncClient,
} from '@quarto/quarto-sync-client';
import { ConnectionManager, hashPayload } from './connection-manager.js';
import {
  renderDiagnostics,
  type LocalRenderResult,
  type RenderedDiagnostic,
} from './local-render.js';
import { GET_ERRORS_TIMEOUT_MS, RENDER_CHECK_TIMEOUT_MS, withTimeout } from './timeout.js';
import {
  currentHeads,
  diffFileHistory,
  formatUnifiedDiff,
  listFileHistory,
  textAtHead,
  UnknownChangeHashError,
} from './file-history.js';
import {
  AUTH_TOOL_DEFINITIONS,
  AuthToolsState,
  extractAuthContext,
  type AuthAction,
  type ProgressNotification,
} from './auth/auth-tools.js';
import { redactTokens } from './auth/redact.js';
import {
  buildShareUrl,
  parseProjectRef,
  serversMatch,
  type ProjectRef,
} from './share-url.js';
import { buildFileResourceUri } from './resources.js';
import { loadQmdParser } from './qmd-parser.js';
import {
  extractOutline,
  findSection,
  replaceSection,
  type OutlineEntry,
} from './qmd-ast.js';
import {
  collectOutputs,
  makeRenderDir,
  materializeProject,
  outRender,
  parseDiagnostics,
  removeRenderDir,
  runRender,
  runRenderBatch,
} from './render.js';
import {
  DocsUnavailableError,
  findPage,
  loadDocsPages,
  outDocsPage,
  outDocsQuery,
  searchDocs,
} from './docs-tool.js';

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
 * Append a `resource_link` block pointing at the file's `hub://`
 * resource URI (BP-5): hosts that understand resources can then
 * attach, subscribe, or navigate to the file a write just touched.
 * The link supplements — never replaces — the text/structured payload.
 */
function withResourceLink(
  result: CallToolResult,
  project: string,
  path: string,
  args: ToolArgs,
): CallToolResult {
  result.content.push({
    type: 'resource_link',
    uri: buildFileResourceUri(project, path, routedServer(args)),
    name: path,
  });
  return result;
}

/**
 * Shared description for every `project` parameter. Tells the model that a
 * quarto-hub.com share URL is accepted in place of a bare id — the server
 * extracts the id (and a default `path`) from it. See {@link parseProjectRef}.
 *
 * Deliberately one line (Phase 7, bd-1ilystml): this rides every arm of
 * the merged tools' input schemas (~14× across 4 tools, bd-lj6f7z9s),
 * and the full share-URL semantics (`server=` routing, where the id
 * sits) live in the server instructions.
 */
const PROJECT_PARAM_DESC =
  "The project's automerge index document ID, or a quarto-hub.com share URL " +
  '(`https://quarto-hub.com/#/share/<id>?…`); a `file=` parameter supplies a default `path`.';

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
  // The project documents record no display name, so an incoming share
  // URL's `name=` is the only source — stash it for the handlers that
  // re-emit a shareUrl (connect_project, get_project_info).
  if (ref.name) {
    next.shareName = ref.name;
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
 * `edit` with `action: "delete"` — entries whose documents never reached
 * the hub.
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
      'Use `edit` with `action: "delete"` to remove the dangling entry.',
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
 * state, points at `read` with `action: "list"`, and lists up to three
 * closest existing paths so a typo self-corrects in one call instead of
 * three.
 */
function fileNotFoundError(path: string, state: ProjectState): CallToolResult {
  const paths = allPaths(state);
  const near = closestPaths(path, paths);
  let msg =
    `Error: File not found: "${path}". The project has ` +
    `${paths.length} file${paths.length === 1 ? '' : 's'}; call \`read\` with \`action: "list"\` to see them.`;
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
  /** 1-based line the window ends at (inclusive). */
  end: number;
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
      end,
      truncated,
      nextOffset: truncated ? end + 1 : null,
      ...(truncated
        ? {
            hint:
              `Returned lines ${offset}-${end} of ${totalLines}. ` +
              `Call \`read\` with \`action: "read"\` and offset=${end + 1} to continue.`,
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
      end,
      truncated: true,
      nextOffset: end + 1,
      hint:
        `Returned lines ${offset}-${end} of ${totalLines} (max_bytes=${maxBytes}). ` +
        `Call \`read\` with \`action: "read"\` and offset=${end + 1} to continue.`,
    };
  }
  // Pathological: a single line exceeds the byte cap (minified assets,
  // one-line data files). Byte-cut it; there is no line-aligned
  // continuation, so next_offset is null and the hint says why.
  const content = cutToBytes(lines[start], maxBytes);
  return {
    content,
    totalLines,
    end: start + 1,
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

/**
 * The data tools (everything except the auth tool). bd-lj6f7z9s: the 17
 * same-subject verbs consolidated into three multi-action tools —
 * `project` (5 arms), `read` (5 arms), `edit` (7 arms) — selected by the
 * `action` argument; the rest of the surface stayed standalone.
 */
type DataToolName =
  | 'project'
  | 'read'
  | 'edit'
  | 'docs'
  | 'wait_for_change'
  | 'get_errors'
  | 'render'
  | 'create_project';

/** The `project` tool's arms. */
type ProjectAction = 'connect' | 'disconnect' | 'info' | 'list_sets' | 'set_default_set';
/** The `read` tool's arms. */
type ReadAction = 'list' | 'read' | 'outline' | 'search' | 'history';
/** The `edit` tool's arms. */
type EditAction =
  | 'write'
  | 'patch'
  | 'delete'
  | 'rename'
  | 'restore'
  | 'create_folder'
  | 'delete_folder';

/**
 * Tool arms whose `path` argument a share URL's `file=` parameter can
 * supply (see {@link normalizeArgs}), keyed `"<tool>:<action>"`. Their
 * zod arm schemas declare `path` optional — a schema-level `required`
 * would reject the share-URL call before normalization runs — so the
 * requirement is enforced here, after normalization.
 *
 * `wait_for_change` is deliberately absent: its `path` is genuinely
 * optional since CAP-18 (omitted = project-wide watch). A share URL's
 * `file=` still fills it as a default via normalizeArgs.
 */
const PATH_DEFAULTABLE: ReadonlySet<string> = new Set([
  'read:read',
  'read:outline',
  'read:history',
  'edit:write',
  'edit:patch',
  'edit:delete',
  'edit:restore',
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

/**
 * Dispatch a data-tool call to its handler. The merged tools arrive with
 * an `action` selector (their zod schemas validate it before this runs);
 * the (tool, action) pair maps onto the same handlers the pre-merge
 * per-verb tools used (bd-lj6f7z9s).
 */
async function handleTool(
  name: DataToolName,
  rawArgs: ToolArgs,
  manager: ConnectionManager,
  extras: ToolExtras = {},
): Promise<CallToolResult> {
  const args = normalizeArgs(rawArgs, manager.configuredServerUrl);
  const action = typeof args.action === 'string' ? args.action : undefined;
  if (
    action !== undefined &&
    PATH_DEFAULTABLE.has(`${name}:${action}`) &&
    (typeof args.path !== 'string' || args.path === '')
  ) {
    return error(
      `Error: \`${name}\` with \`action: "${action}"\` requires a \`path\` argument — the ` +
        'file path within the project (or pass a share URL whose `file=` parameter names it).',
    );
  }
  switch (name) {
    case 'project':
      switch (action as ProjectAction) {
        case 'connect':
          return handleConnectProject(args, manager);
        case 'disconnect':
          return handleDisconnectProject(args, manager);
        case 'info':
          return handleGetProjectInfo(args, manager);
        case 'list_sets':
          return handleListProjects(args, manager);
        case 'set_default_set':
          return handleSetDefaultProjectSet(args, manager);
      }
      break;
    case 'read':
      switch (action as ReadAction) {
        case 'list':
          return handleListFiles(args, manager);
        case 'read':
          return handleReadFile(args, manager);
        case 'outline':
          return handleGetOutline(args, manager);
        case 'search':
          return handleSearchFiles(args, manager);
        case 'history':
          return handleGetFileHistory(args, manager);
      }
      break;
    case 'edit':
      switch (action as EditAction) {
        case 'write':
          return handleWriteFile(args, manager);
        case 'patch':
          return handlePatchFile(args, manager);
        case 'delete':
          return handleDeleteFile(args, manager);
        case 'rename':
          return handleRenameFile(args, manager);
        case 'restore':
          return handleRestoreFileVersion(args, manager);
        case 'create_folder':
          return handleCreateFolder(args, manager);
        case 'delete_folder':
          return handleDeleteFolder(args, manager);
      }
      break;
    case 'docs':
      return handleDocs(args);
    case 'wait_for_change':
      return handleWaitForChange(args, manager, extras);
    case 'get_errors':
      return handleGetErrors(args, manager);
    case 'render':
      return handleRender(args, manager, extras);
    case 'create_project':
      return handleCreateProject(args, manager);
  }
  return error(
    `Error: unknown action ${JSON.stringify(action)} for \`${name}\` — ` +
      'check the tool\'s input schema for the valid `action` values.',
  );
}

/** hub-client's own display fallback for a nameless project. */
const FALLBACK_PROJECT_NAME = 'Untitled project';

/**
 * The file a share link opens when the caller didn't name one: the
 * project's `index.qmd`, else its first `.qmd`, else its first file —
 * and plain `index.qmd` for an empty project (hub-client requires
 * `file=`; bd-jtl4o0pt).
 */
function defaultShareFile(paths: string[]): string {
  const sorted = [...paths].sort();
  if (sorted.includes('index.qmd')) return 'index.qmd';
  return sorted.find((p) => p.endsWith('.qmd')) ?? sorted[0] ?? 'index.qmd';
}

/** The `file=` for an emitted shareUrl: an incoming share URL's `file=`
 * (normalizeArgs stashes it in `path`), else the project default. */
function shareFileFor(args: ToolArgs, state: ProjectState): string {
  if (typeof args.path === 'string' && args.path !== '') return args.path;
  return defaultShareFile(Array.from(state.files.keys()));
}

/** The `name=` for an emitted shareUrl: an incoming share URL's `name=`
 * (normalizeArgs stashes it in `shareName`), else the display fallback. */
function shareNameFor(args: ToolArgs): string {
  return typeof args.shareName === 'string' && args.shareName !== ''
    ? args.shareName
    : FALLBACK_PROJECT_NAME;
}

async function handleConnectProject(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const state = await manager.connect(project, { server: routedServer(args) });
  return structured({
    project,
    files: buildFileList(state),
    shareUrl: buildShareUrl({
      server: state.serverUrl,
      indexDocId: project,
      file: shareFileFor(args, state),
      name: shareNameFor(args),
    }),
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

/**
 * The empty-presence guidance (CAP-8): observation is passive, so an
 * empty list is weak evidence. Carried as `presence_message` on
 * get_project_info's presence arm (Phase 7).
 */
const PRESENCE_EMPTY_MESSAGE =
  'No collaborators heard from recently. Presence is observed passively: a peer ' +
  'appears here only after their editor broadcasts (on cursor activity), and drops ' +
  'out about a minute after their last broadcast. Absence of an entry does not prove ' +
  'nobody has the project open.';

async function handleGetProjectInfo(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const include = Array.isArray(args.include) ? (args.include as readonly string[]) : [];
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
  // Presence arm (Phase 7, bd-1ilystml: the standalone list_presence
  // tool folded into the doctor — ERG-5 rule (d)). Passive observation;
  // this server never announces itself.
  const presences = include.includes('presence')
    ? await manager.observePresence(project, { server: routedServer(args) })
    : undefined;
  return structured({
    project,
    server: state.serverUrl,
    shareUrl: buildShareUrl({
      server: state.serverUrl,
      indexDocId: project,
      file: shareFileFor(args, state),
      name: shareNameFor(args),
    }),
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
    ...(presences !== undefined
      ? {
          presences,
          ...(presences.length === 0 ? { presence_message: PRESENCE_EMPTY_MESSAGE } : {}),
        }
      : {}),
  });
}

/**
 * Shared read of a project-set argument through `parseProjectRef`. On
 * failure returns the bare message (`could not read project-set …`); each
 * caller wraps it with its own advice.
 */
async function readProjectSetArg(
  projectSetArg: string,
  manager: ConnectionManager,
): Promise<
  | { doc: ProjectSetDocument; ref: ProjectRef; errorMessage?: undefined }
  | { doc?: undefined; ref: ProjectRef; errorMessage: string }
> {
  const ref = parseProjectRef(projectSetArg);
  const server =
    ref.server && !serversMatch(ref.server, manager.configuredServerUrl)
      ? ref.server
      : undefined;
  try {
    return { doc: await manager.readProjectSet(ref.project, { server }), ref };
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    return {
      ref,
      errorMessage: `could not read project-set ${ref.project}: ${redactTokens(msg)}`,
    };
  }
}

async function handleListProjects(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  // `project_set` takes the same id-or-share-URL forms as `project`
  // (normalizeArgs only rewrites `project`, so parse explicitly here).
  // Omitted (bd-3h5hbfdv): fall back to the remembered default — the
  // first explicit call that reads successfully persists its argument to
  // the OS keyring, and the set_default_set arm overwrites it.
  const store = manager.defaultProjectSetStore;
  const explicitArg =
    typeof args.project_set === 'string' && args.project_set.trim() !== ''
      ? args.project_set.trim()
      : undefined;
  const projectSetArg =
    explicitArg ?? (store !== undefined ? ((await store.read()) ?? undefined) : undefined);
  if (projectSetArg === undefined) {
    return error(
      'Error: no `project_set` given and no default stored yet. Provide the set\'s doc ' +
        'id or its `https://quarto-hub.com/#/link-project-set/<id>?server=…` link — ask ' +
        'the user if you don\'t have it. The first successful call stores it as the ' +
        'default for subsequent calls; change it later with `project` and `action: "set_default_set"`.',
    );
  }
  const { doc, errorMessage } = await readProjectSetArg(projectSetArg, manager);
  if (errorMessage !== undefined) {
    if (explicitArg === undefined) {
      // The failing value came from the keyring, not from this call — say
      // so, or the user cannot tell which id is being complained about.
      return error(
        `Error: ${errorMessage}. This value is the remembered default (OS keyring) — the ` +
          'set may have been deleted or belong to another hub. Pass an explicit ' +
          '`project_set`, or update the remembered default with `project` and `action: "set_default_set"`.',
      );
    }
    return error(
      `Error: ${errorMessage}. Check the id with whoever shared it — and if it names a ` +
        'project rather than a project set, use `project` with `action: "connect"` instead.',
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
        // topFiles is a per-user cache (ProjectSetEntrySummary); absent
        // one, the link points at the conventional entry point.
        file: defaultShareFile(entry.summary?.topFiles ?? []),
        name: entry.description || FALLBACK_PROJECT_NAME,
      }),
    }))
    .sort((a, b) => (a.lastAccessed < b.lastAccessed ? 1 : a.lastAccessed > b.lastAccessed ? -1 : 0));
  const result = structured({
    ...(doc.name !== undefined ? { name: doc.name } : {}),
    projects,
  });
  // First-time persist (bd-3h5hbfdv): only when nothing is stored, so
  // exploring someone else's set link never switches the user's default.
  // A keyring failure degrades to a warning — the listing still stands.
  if (explicitArg !== undefined && store !== undefined && (await store.read()) === null) {
    try {
      await store.write(explicitArg);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      result.content.push({
        type: 'text',
        text: `Warning: could not remember this project set as the default: ${redactTokens(msg)}`,
      });
    }
  }
  return result;
}

async function handleSetDefaultProjectSet(
  args: ToolArgs,
  manager: ConnectionManager,
): Promise<CallToolResult> {
  const store = manager.defaultProjectSetStore;
  if (store === undefined) {
    return error('Error: this server was started without a default-project-set store.');
  }
  if (typeof args.project_set !== 'string' || args.project_set.trim() === '') {
    return error(
      'Error: `project` with `action: "set_default_set"` requires a `project_set` argument — the set\'s doc ' +
        'id or its `https://quarto-hub.com/#/link-project-set/<id>?server=…` link.',
    );
  }
  const projectSetArg = args.project_set.trim();
  // Validate before storing: a bad link must fail fast, not poison the default.
  const { doc, ref, errorMessage } = await readProjectSetArg(projectSetArg, manager);
  if (errorMessage !== undefined) {
    return error(
      `Error: not stored — ${errorMessage}. The stored default was left unchanged. ` +
        'Check the link with whoever shared it — and note it must name a project set, ' +
        'not a single project.',
    );
  }
  try {
    await store.write(projectSetArg);
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    return error(`Error: could not store the default project set: ${redactTokens(msg)}`);
  }
  return structured({
    default_project_set: ref.project,
    ...(ref.server !== undefined ? { server: ref.server } : {}),
    ...(doc.name !== undefined ? { name: doc.name } : {}),
    project_count: Object.keys(doc.projects).length,
    note: 'Remembered in the OS keyring; bare `project` / `action: "list_sets"` calls now use this set.',
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
      `Error: \`read\` with \`action: "history"\` only supports text files; "${path}" is binary ` +
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
          'Pass both (from history list mode), or pass `from_hash` alone to diff ' +
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
          `Error: ${err.message} in "${path}". Call \`read\` with \`action: "history"\` without ` +
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
/** The `create_only` refusal (Phase 7): identical on both write arms. */
function createOnlyRefusal(path: string): CallToolResult {
  return error(
    `Error: \`edit\` with \`action: "write"\` refused: ${path} already exists (create_only). ` +
      'Drop create_only to overwrite it, or pass expected_hash for a compare-and-swap update.',
  );
}

function staleHashError(
  action: 'write' | 'patch' | 'restore',
  path: string,
  currentText: string,
): CallToolResult {
  return error(
    JSON.stringify(
      {
        error: 'stale_expected_hash',
        message:
          `\`edit\` with \`action: "${action}"\` refused: the file changed since you read it ` +
          '(expected_hash does not match the current content). The current content and its ' +
          'hash are included here — merge your changes against it and retry with the new ' +
          'expected_hash (or re-read with `read` and `action: "read"` first).',
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
  action: 'write',
  path: string,
  current: FilePayload & { type: 'binary' },
): CallToolResult {
  return error(
    JSON.stringify(
      {
        error: 'stale_expected_hash',
        message:
          `\`edit\` with \`action: "${action}"\` refused: the file changed since you read it ` +
          '(expected_hash does not match the current content). Re-read with `read` and ' +
          '`action: "read"` for the current bytes and hash, merge your changes, and retry ' +
          'with the new expected_hash.',
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

// ---------------------------------------------------------------------------
// CAP-11: qmd structure (get_outline + `section` selectors)
// ---------------------------------------------------------------------------

/** Structured section metadata carried in read_file/patch_file results. */
function sectionInfo(s: OutlineEntry): Record<string, unknown> {
  return { name: s.title, level: s.level, heading_line: s.line, end_line: s.endLine };
}

/**
 * Resolve a `section` selector against the file's current text. Shared by
 * the read_file and patch_file section arms so both compute identical
 * ranges — what read_file shows is exactly what patch_file replaces.
 * Errors follow the ERG-4 convention (failing parameter, current state,
 * next tool to call).
 */
async function resolveSection(
  path: string,
  text: string,
  selector: string,
): Promise<{ ok: true; section: OutlineEntry } | { ok: false; result: CallToolResult }> {
  if (!path.endsWith('.qmd')) {
    return {
      ok: false,
      result: error(
        `Error: the \`section\` selector is supported for .qmd files only; ${path} is not ` +
          'a qmd file. Read or patch it with offset/limit or old_string instead.',
      ),
    };
  }
  let parser;
  try {
    parser = await loadQmdParser();
  } catch (err) {
    const detail = err instanceof Error ? err.message : String(err);
    return {
      ok: false,
      result: error(`Error: the qmd parser is unavailable — ${detail}`),
    };
  }
  const ast = parser.parse(text);
  if (ast === null) {
    return {
      ok: false,
      result: error(
        `Error: ${path} could not be parsed as qmd, so \`section\` cannot be resolved. ` +
          'Read or patch it with offset/limit or old_string instead.',
      ),
    };
  }
  const found = findSection(ast, selector, splitLines(text).length);
  if (found.ok) return { ok: true, section: found.section };
  if (found.reason === 'ambiguous') {
    return {
      ok: false,
      result: error(
        `Error: section "${selector}" is ambiguous in ${path} — matches: ${found.matches.join(', ')}. ` +
          'Section titles must match exactly one heading; patch with old_string for one of these, ' +
          'or restructure the document.',
      ),
    };
  }
  const available =
    found.available.length === 0
      ? 'The document has no headings.'
      : `Available sections: ${found.available
          .slice(0, 10)
          .map((t) => `"${t}"`)
          .join(', ')}${found.available.length > 10 ? ', …' : ''}.`;
  return {
    ok: false,
    result: error(
      `Error: no section titled "${selector}" in ${path} (matching is exact and case-sensitive). ` +
        `${available} Call \`read\` with \`action: "outline"\` for the full heading tree with line numbers.`,
    ),
  };
}

async function handleGetOutline(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
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
    return error(`Error: ${path} is a binary file; the outline arm applies to .qmd text files only.`);
  }
  if (!path.endsWith('.qmd')) {
    return error(
      `Error: \`read\` with \`action: "outline"\` is supported for .qmd files only; ${path} is not a qmd file. ` +
        'Use `read` with `action: "read"` to read it.',
    );
  }
  let parser;
  try {
    parser = await loadQmdParser();
  } catch (err) {
    const detail = err instanceof Error ? err.message : String(err);
    return error(`Error: the qmd parser is unavailable — ${detail}`);
  }
  const ast = parser.parse(payload.text);
  if (ast === null) {
    return error(
      `Error: ${path} could not be parsed as qmd. The file may contain syntax the parser ` +
        'rejects; read it with `read` and `action: "read"` to inspect.',
    );
  }
  const outline = extractOutline(ast, splitLines(payload.text).length);
  return structured({
    path,
    hash: hashPayload(payload),
    outline: outline.map((e) => ({
      level: e.level,
      title: e.title,
      ...(e.id !== undefined ? { id: e.id } : {}),
      line: e.line,
      end_line: e.endLine,
    })),
  });
}

// ---------------------------------------------------------------------------
// CAP-12: render (opt-in via --allow-render; code execution gate)
// ---------------------------------------------------------------------------

/** Renderable-file cap for the loose-files fallback (argv safety on Windows). */
const RENDER_FILES_CAP = 500;

async function handleRender(
  args: ToolArgs,
  manager: ConnectionManager,
  extras: ToolExtras = {},
): Promise<CallToolResult> {
  const project = args.project as string;
  const pathArg = typeof args.path === 'string' && args.path !== '' ? args.path : undefined;
  const rawTimeout = typeof args.timeout_seconds === 'number' ? args.timeout_seconds : 120;
  const timeoutSeconds = Math.min(Math.max(rawTimeout, 1), 600);
  const state = await manager.connect(project, { server: routedServer(args) });

  if (pathArg !== undefined && !state.files.has(pathArg)) {
    const ghost = findUnavailable(state.client, pathArg);
    if (ghost) {
      return unavailableFileError(pathArg, ghost.docId);
    }
    return fileNotFoundError(pathArg, state);
  }

  // Choose the render mode. A directory render (`q2 render .`) needs a
  // `_quarto.yml`; without one q2 answers Q-7-7/Q-7-3 without touching a
  // file, so loose projects render as an explicit .qmd list instead.
  let mode: 'file' | 'project' | 'files';
  let targets: string[];
  if (pathArg !== undefined) {
    mode = 'file';
    targets = [pathArg];
  } else if (state.files.has('_quarto.yml')) {
    mode = 'project';
    targets = ['.'];
  } else {
    const qmds = [...state.files.keys()].filter((p) => p.endsWith('.qmd')).sort();
    if (qmds.length === 0) {
      return error(
        'Error: nothing to render — the project has no .qmd files and no _quarto.yml. ' +
          'Add a document, or pass `path` naming a specific file to render.',
      );
    }
    if (qmds.length > RENDER_FILES_CAP) {
      return error(
        `Error: the project has ${qmds.length} .qmd files and no _quarto.yml — the loose-files ` +
          `render is capped at ${RENDER_FILES_CAP}. Add a _quarto.yml for project renders, ` +
          'or pass `path` to render one file.',
      );
    }
    mode = 'files';
    targets = qmds;
  }

  const dir = makeRenderDir();
  try {
    const { written, skipped } = materializeProject(state.files, dir);
    const q2Path = process.env['QUARTO_Q2_PATH'] ?? 'q2';
    const timeoutMs = timeoutSeconds * 1000;

    // One spawn for project/file modes; per-file spawns (shared deadline)
    // for the loose-files fallback — q2 refuses multiple paths without a
    // project (Q-7-4).
    let ok: boolean;
    let exitCode: number | null;
    let timedOut = false;
    let durationMs: number;
    const diagnostics: ReturnType<typeof parseDiagnostics> = [];
    if (mode === 'files') {
      const started = Date.now();
      const batch = await runRenderBatch({
        q2Path,
        cwd: dir,
        files: targets,
        timeoutMs,
        signal: extras.signal,
      });
      durationMs = Date.now() - started;
      const spawnErr = batch.find((r) => r.outcome.spawnError !== undefined);
      if (spawnErr) {
        return error(
          `Error: could not launch q2 ("${q2Path}"): ${spawnErr.outcome.spawnError}. Under \`q2 mcp\` the ` +
            'launcher injects QUARTO_Q2_PATH pointing at the hosting binary; running standalone, ' +
            'set QUARTO_Q2_PATH or put q2 on PATH.',
        );
      }
      exitCode = batch.every((r) => r.outcome.exitCode === 0)
        ? 0
        : (batch.find((r) => (r.outcome.exitCode ?? 1) !== 0)?.outcome.exitCode ?? null);
      timedOut = batch.some((r) => r.outcome.timedOut);
      for (const r of batch) diagnostics.push(...parseDiagnostics(r.outcome.stderr, dir));
      ok = exitCode === 0 && !timedOut;
    } else {
      const outcome = await runRender({
        q2Path,
        cwd: dir,
        targets,
        timeoutMs,
        signal: extras.signal,
      });
      if (outcome.spawnError !== undefined) {
        return error(
          `Error: could not launch q2 ("${q2Path}"): ${outcome.spawnError}. Under \`q2 mcp\` the ` +
            'launcher injects QUARTO_Q2_PATH pointing at the hosting binary; running standalone, ' +
            'set QUARTO_Q2_PATH or put q2 on PATH.',
        );
      }
      diagnostics.push(...parseDiagnostics(outcome.stderr, dir));
      exitCode = outcome.exitCode;
      timedOut = outcome.timedOut;
      durationMs = outcome.durationMs;
      ok = outcome.exitCode === 0 && !timedOut;
    }

    const outputs = collectOutputs(dir, new Set(written));
    return structured({
      ok,
      exit_code: exitCode,
      target: pathArg ?? '.',
      mode,
      ...(mode === 'files' ? { files: targets } : {}),
      diagnostics,
      outputs,
      duration_ms: durationMs,
      ...(timedOut
        ? {
            timed_out: true,
            hint:
              `Render exceeded timeout_seconds=${timeoutSeconds} and was killed. Raise the ` +
              'timeout, or render a smaller target by passing `path`.',
          }
        : {}),
      ...(skipped.length > 0 ? { skipped_inputs: skipped } : {}),
    });
  } finally {
    removeRenderDir(dir);
  }
}

// ---------------------------------------------------------------------------
// CAP-13: docs (embedded Quarto 2 documentation, no project needed)
// ---------------------------------------------------------------------------

async function handleDocs(args: ToolArgs): Promise<CallToolResult> {
  const query = typeof args.query === 'string' && args.query.trim() !== '' ? args.query : undefined;
  const page = typeof args.page === 'string' && args.page.trim() !== '' ? args.page : undefined;
  if (query !== undefined && page !== undefined) {
    return error('Error: `query` and `page` are mutually exclusive — search OR fetch per call.');
  }
  if (query === undefined && page === undefined) {
    return error(
      'Error: docs requires one of `query` (search the documentation) or `page` (fetch one page).',
    );
  }
  let pages;
  try {
    pages = await loadDocsPages();
  } catch (err) {
    if (err instanceof DocsUnavailableError) {
      return error(`Error: the documentation corpus is unavailable — ${err.message}`);
    }
    throw err;
  }
  if (query !== undefined) {
    const maxResults =
      typeof args.max_results === 'number' ? Math.min(Math.max(args.max_results, 1), 20) : 5;
    const hits = searchDocs(query, pages);
    return structured({
      results: hits.slice(0, maxResults),
      total_matches: hits.length,
      truncated: hits.length > maxResults,
    });
  }
  const found = findPage(pages, page!);
  if (!found.ok) {
    const suffix =
      found.suggestions.length > 0
        ? ` Closest pages: ${found.suggestions.map((s) => `"${s}"`).join(', ')}.`
        : ' Call docs with `query` to search.';
    return error(`Error: no documentation page matches "${page}".${suffix}`);
  }
  return structured({
    href: found.page.href,
    title: found.page.title,
    markdown: found.page.content,
  });
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
    // can render; anything else as an embedded blob resource (base64)
    // whose URI is the file's hub:// resource (the Q-2 contract, BP-5).
    const meta = binaryMeta(path, payload.data, payload.mimeType);
    if (metadataOnly) {
      return structured(meta);
    }
    const data = Buffer.from(payload.data).toString('base64');
    const blob: CallToolResult['content'][number] = payload.mimeType.startsWith('image/')
      ? { type: 'image', data, mimeType: payload.mimeType }
      : {
          type: 'resource',
          resource: {
            uri: buildFileResourceUri(project, path, routedServer(args)),
            mimeType: payload.mimeType,
            blob: data,
          },
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

  // CAP-11: the `section` arm returns exactly one qmd section (heading
  // line included), computed with the same range math patch_file uses.
  if (typeof args.section === 'string') {
    if (args.offset !== undefined || args.limit !== undefined) {
      return error(
        'Error: `section` and `offset`/`limit` are mutually exclusive — a section read already ' +
          'selects its lines. Drop the window arguments, or drop `section` to page by line number.',
      );
    }
    const maxBytes = typeof args.max_bytes === 'number' ? args.max_bytes : DEFAULT_MAX_BYTES;
    const resolved = await resolveSection(path, payload.text, args.section);
    if (!resolved.ok) return resolved.result;
    const s = resolved.section;
    const window = windowLines(payload.text, s.line, s.endLine - s.line + 1, maxBytes);
    // Truncation is reported against the SECTION, not the file: a read
    // that reached the section's last line is complete (truncated: false)
    // even though the file continues; one cut short by max_bytes points
    // its continuation at the next section line. (The byte-cut
    // pathological case is truncated with a null nextOffset.)
    const sectionComplete =
      window.end >= s.endLine && !(window.truncated && window.nextOffset === null);
    return structured({
      path,
      hash: hashPayload(payload),
      type: 'text',
      content: window.content,
      truncated: !sectionComplete,
      total_lines: window.totalLines,
      next_offset: sectionComplete ? null : window.nextOffset,
      ...(sectionComplete
        ? {}
        : {
            hint: `Section "${s.title}" continues past max_bytes=${maxBytes} — ` +
              (window.nextOffset !== null
                ? `call \`read\` with \`action: "read"\` and offset=${window.nextOffset} to continue.`
                : 'raise max_bytes to see more of it.'),
          }),
      section: sectionInfo(s),
    });
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

/** One file's entry in the `get_errors` report. */
interface FileErrorsEntry {
  path: string;
  /** `sha256:<hex>` of the text this entry's render checked. */
  checkedContentSha256?: string;
  errors?: RenderedDiagnostic[];
  warnings?: RenderedDiagnostic[];
  /** Present on entries derived from a sibling's pass-1 failure. */
  note?: string;
  execution?: {
    state: string;
    lastError?: string;
  };
}

/** Cap on how many documents a no-path call renders (each is a full render). */
const MAX_CHECKED_DOCUMENTS = 25;

async function handleGetErrors(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const pathFilter = typeof args.path === 'string' && args.path !== '' ? args.path : undefined;
  const state = await manager.connect(project);

  let targets: string[];
  let capped = false;
  if (pathFilter) {
    const payload = state.files.get(pathFilter);
    if (!payload) {
      const ghost = findUnavailable(state.client, pathFilter);
      if (ghost) {
        return unavailableFileError(pathFilter, ghost.docId);
      }
      return error(`Error: File not found: ${pathFilter}`);
    }
    if (payload.type !== 'text') {
      return error(`Error: ${pathFilter} is a binary file; only text documents can be checked.`);
    }
    targets = [pathFilter];
  } else {
    targets = [...state.files.keys()]
      .filter((p) => p.endsWith('.qmd') && state.files.get(p)!.type === 'text')
      .sort();
    if (targets.length > MAX_CHECKED_DOCUMENTS) {
      targets = targets.slice(0, MAX_CHECKED_DOCUMENTS);
      capped = true;
    }
  }

  const entries = new Map<string, FileErrorsEntry>();
  const entryFor = (p: string): FileErrorsEntry => {
    let e = entries.get(p);
    if (!e) {
      e = { path: p };
      entries.set(p, e);
    }
    return e;
  };

  for (const path of targets) {
    // A render that cannot run (host failed to load, render hung past
    // the timeout) fails the tool with a clean error — unlike the write
    // tools, which degrade instead because their writes already landed.
    let result: LocalRenderResult;
    try {
      result = await withTimeout(
        renderDiagnostics(state.files, path),
        GET_ERRORS_TIMEOUT_MS,
        `render of ${path}`,
      );
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      return error(`Error: could not render ${path}: ${msg}`);
    }
    const entry = entryFor(path);
    entry.checkedContentSha256 = result.checkedContentSha256;
    entry.errors = result.errors;
    entry.warnings = result.warnings;
    // Sibling pass-1 failures surface under the failing file's own path
    // (only when that file wasn't/won't be rendered directly).
    for (const sibling of result.pass1Failures) {
      if (targets.includes(sibling.path)) continue;
      const se = entryFor(sibling.path);
      if (!se.errors?.length) {
        se.errors = sibling.errors;
        se.note = `pass-1 failure observed while rendering ${path}`;
      }
    }
  }

  // Execution errors come from the captures sidecar — they happen on
  // executors elsewhere and cannot be recomputed locally.
  for (const [p, cap] of Object.entries(state.sidecars.captures)) {
    if (cap.state === 'error' || cap.state === 'running') {
      entryFor(p).execution = {
        state: cap.state,
        ...(cap.lastError !== undefined ? { lastError: cap.lastError } : {}),
      };
    }
  }

  const files = [...entries.values()].sort((a, b) => (a.path < b.path ? -1 : 1));
  const report: { project: string; files: FileErrorsEntry[]; note?: string } = { project, files };
  if (capped) {
    report.note = `Checked the first ${MAX_CHECKED_DOCUMENTS} .qmd documents; pass a path to check a specific other file.`;
  }
  // BP-1, made true in Phase 7 (bd-1ilystml): structuredContent like
  // every sibling — this was the last unstructured tool result.
  return structured(report);
}

/**
 * Render-check the content a write tool just committed and return a
 * suffix for the tool response. Validity is a function of content, so
 * the check stages the new text over the current file map rather than
 * waiting for the CRDT callback to land. Never fails the write: a
 * check that cannot run degrades to a pointer at get_errors.
 */
async function renderCheckSuffix(
  files: Map<string, FilePayload>,
  path: string,
  newText: string,
): Promise<string> {
  if (!path.endsWith('.qmd')) return '';
  try {
    const staged = new Map(files);
    staged.set(path, { type: 'text', text: newText });
    const result = await withTimeout(
      renderDiagnostics(staged, path),
      RENDER_CHECK_TIMEOUT_MS,
      `render of ${path}`,
    );
    if (result.errors.length > 0) {
      const n = result.errors.length;
      return (
        `\nRender check: ${n} error${n === 1 ? '' : 's'} in ${path}:\n` +
        JSON.stringify(result.errors, null, 2)
      );
    }
    const w = result.warnings.length;
    return w > 0
      ? `\nRender check: clean (${w} warning${w === 1 ? '' : 's'}; call get_errors to see them).`
      : '\nRender check: clean.';
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return `\nRender check unavailable (${msg}); call get_errors to verify.`;
  }
}

async function handleWriteFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const content = args.content as string;
  const expectedHash = typeof args.expected_hash === 'string' ? args.expected_hash : undefined;
  const createOnly = args.create_only === true;
  if (createOnly && expectedHash !== undefined) {
    return error(
      'Error: `create_only` and `expected_hash` cannot be combined — create_only means ' +
        '"the file must not exist", while expected_hash guards an existing file. Pick one.',
    );
  }
  const state = await manager.connect(project, { server: routedServer(args) });
  const existing = state.files.get(path);

  if (args.encoding === 'base64') {
    return handleWriteFileBinary(
      args,
      manager,
      state,
      project,
      path,
      content,
      expectedHash,
      existing,
      createOnly,
    );
  }

  if (!existing) {
    // A dangling entry is not writable: silently re-creating the
    // document would repoint the index away from whatever the original
    // (never-synced) client still holds. Repair is the edit/delete arm.
    const ghost = findUnavailable(state.client, path);
    if (ghost) {
      return unavailableFileError(path, ghost.docId);
    }
    if (expectedHash !== undefined) {
      return error(
        `Error: \`edit\` with \`action: "write"\` refused: expected_hash was given but ${path} ` +
          'does not exist in the project (it may have been deleted since you read it). Call ' +
          '`read` with `action: "list"` to see the current files; drop expected_hash to create ' +
          'a new file.',
      );
    }
    await state.client.createFile(path, content);
    return withResourceLink(
      structured({
        path,
        hash: hashPayload({ type: 'text', text: content }),
        created: true,
        ...(await syncField(args, manager, project, [path])),
        renderCheck: (await renderCheckSuffix(state.files, path, content)).trim() || undefined,
      }),
      project,
      path,
      args,
    );
  }
  if (createOnly) {
    return createOnlyRefusal(path);
  }
  if (existing.type === 'binary') {
    return error(
      `Error: ${path} is a binary file. Cannot write text content to it — ` +
        'pass `encoding: "base64"` with base64-encoded bytes to replace it.',
    );
  }
  if (expectedHash !== undefined && hashPayload(existing) !== expectedHash) {
    return staleHashError('write', path, existing.text);
  }

  state.client.updateFileContent(path, content);
  return withResourceLink(
    structured({
      path,
      hash: hashPayload({ type: 'text', text: content }),
      ...(await syncField(args, manager, project, [path])),
      renderCheck: (await renderCheckSuffix(state.files, path, content)).trim() || undefined,
    }),
    project,
    path,
    args,
  );
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
  createOnly: boolean,
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
        `Error: \`edit\` with \`action: "write"\` refused: expected_hash was given but ${path} ` +
          'does not exist in the project (it may have been deleted since you read it). Call ' +
          '`read` with `action: "list"` to see the current files; drop expected_hash to create ' +
          'a new file.',
      );
    }
    const created = await state.client.createBinaryFile(path, decoded, mimeType);
    return withResourceLink(
      structured({
        ...binaryWriteMeta(created.path, decoded, mimeType),
        created: true,
        ...(await syncField(args, manager, project, [created.path])),
      }),
      project,
      created.path,
      args,
    );
  }
  if (createOnly) {
    return createOnlyRefusal(path);
  }
  if (existing.type === 'text') {
    return error(
      `Error: ${path} is a text file. Cannot write binary (base64) content to it — ` +
        'drop `encoding: "base64"` to write text, or `edit` with `action: "delete"` first and re-create it as binary.',
    );
  }
  if (expectedHash !== undefined && hashPayload(existing) !== expectedHash) {
    return staleHashErrorBinary('write', path, existing);
  }

  await state.client.updateBinaryFileContent(path, decoded, mimeType);
  return withResourceLink(
    structured({
      ...binaryWriteMeta(path, decoded, mimeType),
      ...(await syncField(args, manager, project, [path])),
    }),
    project,
    path,
    args,
  );
}

async function handlePatchFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const oldString = args.old_string as string | undefined;
  const newString = args.new_string as string;
  const expectedHash = typeof args.expected_hash === 'string' ? args.expected_hash : undefined;
  const sectionSel = typeof args.section === 'string' ? args.section : undefined;
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
  if (sectionSel !== undefined && oldString !== undefined) {
    return error(
      'Error: `section` and `old_string` are mutually exclusive — pick one edit style. ' +
        'With `section`, new_string replaces the entire named section (heading line included).',
    );
  }
  if (sectionSel === undefined && oldString === undefined) {
    return error(
      'Error: `edit` with `action: "patch"` requires one of `old_string` (replace an exact ' +
        'substring) or `section` (replace a named qmd section, heading included).',
    );
  }
  if (expectedHash !== undefined && hashPayload(payload) !== expectedHash) {
    return staleHashError('patch', path, payload.text);
  }

  const currentContent = payload.text;

  // CAP-11: replace one qmd section wholesale (heading line included).
  // The splice is computed against the *current* text, so a collaborator's
  // edit outside the section survives the merge.
  if (sectionSel !== undefined) {
    const resolved = await resolveSection(path, currentContent, sectionSel);
    if (!resolved.ok) return resolved.result;
    const newContent = replaceSection(currentContent, resolved.section, newString);
    state.client.updateFileContent(path, newContent);
    return withResourceLink(
      structured({
        path,
        hash: hashPayload({ type: 'text', text: newContent }),
        section: sectionInfo(resolved.section),
        ...(await syncField(args, manager, project, [path])),
      }),
      project,
      path,
      args,
    );
  }

  // The guards above establish old_string is present whenever section is
  // absent (same post-validation assertion pattern as `path`).
  const old = oldString as string;
  const index = currentContent.indexOf(old);
  if (index === -1) {
    return error(
      `Error: old_string not found in ${path}. The file may have changed since you read it — ` +
        'call `read` with `action: "read"` for the current content (and its hash), then retry with an exact substring.',
    );
  }

  const secondIndex = currentContent.indexOf(old, index + 1);
  if (secondIndex !== -1) {
    return error(`Error: old_string appears multiple times in ${path}. Provide a longer, unique string to match.`);
  }

  const newContent =
    currentContent.slice(0, index) +
    newString +
    currentContent.slice(index + old.length);

  state.client.updateFileContent(path, newContent);
  return withResourceLink(
    structured({
      path,
      hash: hashPayload({ type: 'text', text: newContent }),
      ...(await syncField(args, manager, project, [path])),
      renderCheck: (await renderCheckSuffix(state.files, path, newContent)).trim() || undefined,
    }),
    project,
    path,
    args,
  );
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
      `Error: Destination already exists: ${newPath} — the rename arm does not overwrite. ` +
        `Choose a different new_path, or \`edit\` with \`action: "delete"\` on "${newPath}" first if replacing it is intended.`,
    );
  }

  state.client.renameFile(oldPath, newPath);
  return withResourceLink(
    structured({
      old_path: oldPath,
      new_path: newPath,
      renamed: true,
      ...(await syncField(args, manager, project, [])),
    }),
    project,
    newPath,
    args,
  );
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
      'Error: `edit` with `action: "create_folder"` requires a non-empty `path` — the folder to create within the project.',
    );
  }
  const state = await manager.connect(project, { server: routedServer(args) });
  if (state.files.has(path)) {
    return error(
      `Error: ${path} is a file, not a folder. Choose a different folder name, ` +
        `or \`edit\` with \`action: "rename"\` on "${path}" first if it should move.`,
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
      `Error: Folder not found: "${path}". Call \`read\` with \`action: "list"\` to see the project's files and folders.`;
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
        'them together with the folder, or `edit` with `action: "delete"` on them individually first.',
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

async function handleRestoreFileVersion(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const targetHead = args.hash as string;
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
    return error(
      `Error: \`edit\` with \`action: "restore"\` only supports text files; "${path}" is binary ` +
        `(${payload.mimeType}). To roll a binary back, \`edit\` with \`action: "write"\` its previous bytes with ` +
        '`encoding: "base64"`.',
    );
  }
  const handle = state.client.getFileHandle(path);
  const doc = handle?.doc();
  if (!handle || !doc) {
    return fileNotFoundError(path, state);
  }

  let historicalText: string;
  try {
    historicalText = textAtHead(doc, targetHead);
  } catch (err) {
    if (err instanceof UnknownChangeHashError) {
      return error(
        `Error: ${err.message} in "${path}". Call \`read\` with \`action: "history"\` to list the change ` +
          'hashes this file knows, then pass one as `hash`.',
      );
    }
    throw err;
  }

  // ERG-1 consistency: the optional compare-and-swap refuses to restore
  // over a collaborator's racing edit.
  const preRestoreHash = hashPayload(payload);
  if (expectedHash !== undefined && preRestoreHash !== expectedHash) {
    return staleHashError('restore', path, payload.text);
  }
  const preRestoreHeads = currentHeads(doc);

  // Restoring the current state is an honest no-op — writing would mint
  // an empty change that litters the history.
  if (payload.text === historicalText) {
    return withResourceLink(
      structured({
        path,
        restored_from: targetHead,
        already_current: true,
        pre_restore_hash: preRestoreHash,
        pre_restore_heads: preRestoreHeads,
        hash: preRestoreHash,
      }),
      project,
      path,
      args,
    );
  }

  state.client.updateFileContent(path, historicalText);
  return withResourceLink(
    structured({
      path,
      restored_from: targetHead,
      pre_restore_hash: preRestoreHash,
      pre_restore_heads: preRestoreHeads,
      hash: hashPayload({ type: 'text', text: historicalText }),
      ...(await syncField(args, manager, project, [path])),
    }),
    project,
    path,
    args,
  );
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
      file: defaultShareFile(result.files.map((f) => f.path)),
      name: name ?? FALLBACK_PROJECT_NAME,
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
    const label =
      typeof args.action === 'string' ? `${name} (action: ${args.action})` : name;
    return error(`Error in ${label}: ${redactTokens(message)}`);
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
  // Present only with `include: ["presence"]` (Phase 7).
  presences: z.array(outPresenceEntry).optional(),
  presence_message: z.string().optional(),
});

// BP-1: get_errors' report shape (Phase 7 — it was the last tool
// without a declared outputSchema). Mirrors RenderedDiagnostic in
// local-render.ts.
const outDiagnostic = z.object({
  kind: z.enum(['error', 'warning', 'info', 'note']),
  title: z.string(),
  code: z.string().optional(),
  problem: z.string().optional(),
  hints: z.array(z.string()),
  start_line: z.number().optional(),
  start_column: z.number().optional(),
  end_line: z.number().optional(),
  end_column: z.number().optional(),
  details: z.array(z.unknown()),
});

const outGetErrors = z.object({
  project: z.string(),
  files: z.array(
    z.object({
      path: z.string(),
      checkedContentSha256: z.string().optional(),
      errors: z.array(outDiagnostic).optional(),
      warnings: z.array(outDiagnostic).optional(),
      note: z.string().optional(),
      execution: z
        .object({
          state: z.string(),
          lastError: z.string().optional(),
        })
        .optional(),
    }),
  ),
  note: z.string().optional(),
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

const outSetDefaultProjectSet = z.object({
  default_project_set: z.string(),
  server: z.string().optional(),
  name: z.string().optional(),
  project_count: z.number(),
  note: z.string(),
});

const outListFiles = z.object({ files: z.array(outListedFile) });

/** Section metadata for the CAP-11 `section` arms of read_file/patch_file. */
const outSectionInfo = z.object({
  name: z.string(),
  level: z.number(),
  heading_line: z.number(),
  end_line: z.number(),
});

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
  section: outSectionInfo.optional(),
});

const outGetOutline = z.object({
  path: z.string(),
  hash: z.string(),
  outline: z.array(
    z.object({
      level: z.number(),
      title: z.string(),
      id: z.string().optional(),
      line: z.number(),
      end_line: z.number(),
    }),
  ),
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
  renderCheck: z.string().optional(),
  mimeType: z.string().optional(),
  size: z.number().optional(),
});

const outPatchFile = z.object({
  path: z.string(),
  hash: z.string(),
  synced: z.boolean().optional(),
  renderCheck: z.string().optional(),
  section: outSectionInfo.optional(),
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

const outRestoreFileVersion = z.object({
  path: z.string(),
  restored_from: z.string(),
  pre_restore_hash: z.string(),
  pre_restore_heads: z.array(z.string()),
  hash: z.string(),
  already_current: z.literal(true).optional(),
  synced: z.boolean().optional(),
});


// ---------------------------------------------------------------------------
// Merged-tool schemas (bd-lj6f7z9s)
//
// The 17 same-subject verbs consolidated into three multi-action tools
// selected by a discriminated `action` argument: `project` (5 arms),
// `read` (5 arms), `edit` (7 arms). The discriminated union keeps each
// arm's required fields in the advertised JSON Schema (the SDK's
// converter emits top-level `type: "object"` + one `oneOf` branch per
// arm) and gives free input validation for arm-required parameters.
// `path` stays schema-optional where a share URL's `file=` can supply it
// — PATH_DEFAULTABLE enforces it post-normalization, per arm.
// ---------------------------------------------------------------------------

const inProject = z.discriminatedUnion('action', [
  z.object({
    action: z
      .literal('connect')
      .describe('Connect to a project by id or share URL; returns the file list and shareUrl.'),
    project: projectParam,
  }),
  z.object({
    action: z
      .literal('disconnect')
      .describe('Drop this project\'s connection (bounded sync drain first); the next call reconnects.'),
    project: projectParam,
    wait_for_sync: waitForSyncParam,
  }),
  z.object({
    action: z
      .literal('info')
      .describe(
        'Project health and shape in one call (the "doctor" arm); `include: ["presence"]` adds who is in the project now.',
      ),
    project: projectParam,
    include: z
      .array(z.enum(['presence']))
      .optional()
      .describe(
        'Optional extras to include. "presence": who is in the project right now ' +
          '(adds `presences`, and `presence_message` when empty).',
      ),
  }),
  z.object({
    action: z
      .literal('list_sets')
      .describe('Enumerate a project-set (a user\'s collection of projects), most-recently-used first.'),
    project_set: z
      .string()
      .optional()
      .describe(
        'The project-set document id, OR its quarto-hub.com link: ' +
          '`https://quarto-hub.com/#/link-project-set/<id>?server=…` ' +
          '(a `#/join-collection/<id>` invite link also names a set; the `server=` ' +
          'parameter, if present, routes the read to that hub). Omit to use the ' +
          'remembered default — an actionable error results when none is stored yet.',
      ),
  }),
  z.object({
    action: z
      .literal('set_default_set')
      .describe('Remember a project set (OS keyring) as the default bare list_sets calls use.'),
    project_set: z.string().describe(
      'The project-set document id, OR its quarto-hub.com link: ' +
        '`https://quarto-hub.com/#/link-project-set/<id>?server=…` ' +
        '(a `#/join-collection/<id>` invite link also names a set). The string is ' +
        'stored verbatim, so a link\'s `server=` keeps routing the reads.',
    ),
  }),
]);

const outProject = z.union([
  outConnectProject,
  outDisconnectProject,
  outGetProjectInfo,
  outListProjects,
  outSetDefaultProjectSet,
]);

const inRead = z.discriminatedUnion('action', [
  z.object({
    action: z
      .literal('list')
      .describe('List all files (size/mimeType/lines per entry; folders and dangling entries marked).'),
    project: projectParam,
  }),
  z.object({
    action: z
      .literal('read')
      .describe('Read a file (text content, or binary bytes as image/blob blocks).'),
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
    section: z
      .string()
      .optional()
      .describe(
        'qmd files only: return just the named section — the heading plus its content up ' +
          'to the next same-or-higher heading (subsections included), exactly the range ' +
          'the outline arm reports and the edit patch arm would replace. Mutually exclusive with ' +
          'offset/limit. Call `action: "outline"` first to discover section titles.',
      ),
  }),
  z.object({
    action: z
      .literal('outline')
      .describe('The heading tree of a .qmd file (level, title, id, 1-based line ranges).'),
    project: projectParam,
    path: pathParam.optional(),
  }),
  z.object({
    action: z
      .literal('search')
      .describe('Search the content of every text file (substring or regex).'),
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
  z.object({
    action: z
      .literal('history')
      .describe('History of one text file: recent changes, or a unified diff with from_hash/to_hash.'),
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
]);

const outRead = z.union([
  outListFiles,
  outReadFile,
  outGetOutline,
  outSearchFiles,
  outGetFileHistory,
]);

const inEdit = z.discriminatedUnion('action', [
  z.object({
    action: z
      .literal('write')
      .describe('Replace a file\'s entire content (creates if absent); base64 encoding writes binary.'),
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
        'Optional `hash` from a prior read/write result. When given, the write is ' +
          'refused (returning the current content and its hash) if the file changed since — ' +
          'compare-and-swap against collaborator edits.',
      ),
    create_only: z
      .boolean()
      .optional()
      .describe(
        'Refuse to overwrite: the write fails if the file already exists (default false). ' +
          'Cannot be combined with expected_hash.',
      ),
    wait_for_sync: waitForSyncParam,
  }),
  z.object({
    action: z
      .literal('patch')
      .describe('Targeted text edit: replace an exact old_string, or a whole qmd section.'),
    project: projectParam,
    path: pathParam.optional(),
    old_string: z
      .string()
      .optional()
      .describe('The exact string to find and replace. Required unless `section` is given.'),
    new_string: z
      .string()
      .describe(
        'The replacement string. With `section`, replaces the whole section — heading ' +
          'line included — so repeat the heading (edit it to rename); an empty string ' +
          'deletes the section. A read with the same `section` value shows exactly ' +
          'the range this replaces.',
      ),
    section: z
      .string()
      .optional()
      .describe(
        'qmd files only: replace the entire named section with `new_string` (the read ' +
          'outline arm shows the candidates). The splice touches only the section\'s lines, so a ' +
          'collaborator\'s edit elsewhere survives. Mutually exclusive with `old_string`.',
      ),
    expected_hash: z
      .string()
      .optional()
      .describe(
        'Optional `hash` from a prior read/patch result. When given, the patch is ' +
          'refused (returning the current content and its hash) if the file changed since — ' +
          'compare-and-swap against collaborator edits.',
      ),
    wait_for_sync: waitForSyncParam,
  }),
  z.object({
    action: z.literal('delete').describe('Delete a file (dangling index entries included).'),
    project: projectParam,
    path: z.string().describe('The file path to delete').optional(),
    wait_for_sync: waitForSyncParam,
  }),
  z.object({
    action: z.literal('rename').describe('Rename or move a file (old_path → new_path).'),
    project: projectParam,
    old_path: z.string().describe('The current file path'),
    new_path: z.string().describe('The new file path'),
    wait_for_sync: waitForSyncParam,
  }),
  z.object({
    action: z
      .literal('restore')
      .describe('Revert a text file to a prior version (the agent\'s undo) as a new change.'),
    project: projectParam,
    path: pathParam.optional(),
    hash: z
      .string()
      .describe('The change `head` to restore to, from the read history arm\'s list mode.'),
    expected_hash: z
      .string()
      .optional()
      .describe(
        'Compare-and-swap: refuse if the file\'s current content hash differs (a ' +
          'collaborator edited since your read).',
      ),
    wait_for_sync: waitForSyncParam,
  }),
  z.object({
    action: z
      .literal('create_folder')
      .describe('Create a folder (explicit marker; empty folders exist only once created).'),
    project: projectParam,
    path: z.string().describe('The folder path to create (e.g. `assets/images`)'),
    wait_for_sync: waitForSyncParam,
  }),
  z.object({
    action: z
      .literal('delete_folder')
      .describe('Delete a folder (recursive: true deletes contained files first).'),
    project: projectParam,
    path: z.string().describe('The folder path to delete'),
    recursive: z
      .boolean()
      .optional()
      .describe('Delete the files under the folder too (default false — refuse if non-empty).'),
    wait_for_sync: waitForSyncParam,
  }),
]);

const outEdit = z.union([
  outWriteFile,
  outPatchFile,
  outDeleteFile,
  outRenameFile,
  outRestoreFileVersion,
  outCreateFolder,
  outDeleteFolder,
]);

/**
 * Register all tool handlers on the MCP server. The auth tool registers
 * first so it leads the `tools/list` order and the data tools'
 * "no credentials" errors can name it.
 */
export function registerTools(
  server: McpServer,
  manager: ConnectionManager,
  readOnly: boolean,
  authToolsState?: AuthToolsState,
  allowRender = false,
): void {
  if (authToolsState) {
    for (const def of AUTH_TOOL_DEFINITIONS) {
      server.registerTool(
        def.name,
        {
          title: def.title,
          description: def.description,
          inputSchema: def.inputSchema,
          ...(def.outputSchema ? { outputSchema: def.outputSchema } : {}),
          annotations: def.annotations,
        },
        (args, ctx) =>
          authToolsState.handle((args as { action: AuthAction }).action, extractAuthContext(ctx)),
      );
    }
  }

  server.registerTool(
    'project',
    {
      title: 'Connect to and inspect projects',
      description:
        'Work with Quarto Hub projects and connections. `project` parameters accept a bare ' +
        'automerge index document ID or a quarto-hub.com share URL ' +
        '(`https://quarto-hub.com/#/share/<id>?…` — the id is extracted automatically, and a ' +
        '`file=` parameter supplies a default `path`). Every tool auto-connects on first use.\n\n' +
        '`action: "connect"` — connect to a project; returns the list of files and the ' +
        'project\'s `shareUrl`. If the hub requires authentication and no valid credentials are ' +
        'cached, this throws an `AuthRequiredError` / `ReauthRequired` — call `auth` with ' +
        '`action: "sign_in"`.\n\n' +
        '`action: "disconnect"` — drop the connection to one project (websocket + in-memory ' +
        'documents) without affecting others — the release valve for long sessions touching ' +
        'many projects. Outbound sync is drained first (bounded); the next tool call to the ' +
        'project transparently reconnects.\n\n' +
        '`action: "info"` — project health and shape in one call: file/folder/binary counts, ' +
        'contributor identities, engine-capture state (idle/running/error + lastError), the ' +
        'index document id, sync server, observed auth mode, sync diagnostics (peers, stranded ' +
        'files), and the share URL. The "doctor" arm — call it to understand an unfamiliar ' +
        'project or diagnose a connection. Pass `include: ["presence"]` to also see who is in ' +
        'the project right now (name, file, cursor offsets, last-heard-from). Presence is ' +
        'observed passively — this server never announces itself — so an empty list does not ' +
        'prove nobody is watching; check presence before editing a file a teammate may have ' +
        'open, and combine with wait_for_change to react to their edits.\n\n' +
        '`action: "list_sets"` — enumerate a project-set (a user\'s collection of projects). ' +
        'Omit `project_set` to use the remembered default: the first call that names a set ' +
        'successfully stores it in the OS keyring. The web client\'s copy-link form for a set ' +
        'is `https://quarto-hub.com/#/link-project-set/<id>?server=…` (note: not the `#/share/` ' +
        'route, which names a single project; a `#/join-collection/<id>` invite link also names ' +
        'a set). Returns each project\'s id, sync server, description, and a share URL you can ' +
        'pass to `action: "connect"`, most-recently-used first.\n\n' +
        '`action: "set_default_set"` — remember a project set as the default that bare ' +
        '`list_sets` calls use, stored in the OS keyring (the same secure storage as the ' +
        'sign-in credentials). The link or id is validated against the hub before storing and ' +
        'any previous default is overwritten. Local state only: nothing in any project is ' +
        'modified, so this arm is available in read-only mode.',
      inputSchema: inProject,
      outputSchema: outProject,
      // The merge of the arms' annotations: disconnect/set_default_set
      // mutate local (keyring/connection) state; every arm is idempotent.
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: true },
    },
    (args) => runDataTool('project', args, manager),
  );

  server.registerTool(
    'read',
    {
      title: 'List, read, and search project files',
      description:
        'Read from a Quarto Hub project. All arms take `project` (index doc id or share URL).\n\n' +
        '`action: "list"` — list all files. Entries carry size/mimeType/lines; folders list as ' +
        '`type: "folder"`; dangling index entries carry `status: "unavailable"` plus the doc ' +
        'id — repair them with `edit` and `action: "delete"`.\n\n' +
        '`action: "read"` — read a file. Text files return `{ path, hash, type: "text", content }` ' +
        '— pass `hash` back as `expected_hash` on `edit` writes so an edit a collaborator made ' +
        'since this read is never silently overwritten. Large reads are capped by `max_bytes` ' +
        '(default 64 KB): a truncated result carries `next_offset` — call again with `offset` ' +
        'set to it to continue. Binary files return the bytes as an image block (image MIME ' +
        'types) or an embedded blob resource (anything else), with structured ' +
        '`{ path, hash, type: "binary", mimeType, size }`; pass `metadata_only: true` for just ' +
        'the metadata without the bytes. qmd files only: `section` returns just the named ' +
        'section (the heading plus content up to the next same-or-higher heading), exactly the ' +
        'range `action: "outline"` reports and the `edit` patch arm would replace; mutually ' +
        'exclusive with `offset`/`limit`.\n\n' +
        '`action: "outline"` — the heading tree of a .qmd file: every heading with its level, ' +
        'title, id, and 1-based line range (`line`..`end_line`, where the section ends just ' +
        'before the next same-or-higher heading). The structural map of a document — use it ' +
        'before a section read or patch, or to aim `offset`/`limit` at a section without ' +
        'reading the whole file.\n\n' +
        '`action: "search"` — search the content of every text file (binaries are skipped): a ' +
        'plain substring (case-insensitive unless `case_sensitive`), or a regular expression ' +
        'with `regex: true`. Returns matching lines as `{ path, line, snippet }` ranked by ' +
        'per-file match count, capped by `max_results` (`truncated: true` when capped). Follow ' +
        'a hit with `action: "read"` and `offset` around the line for full context.\n\n' +
        '`action: "history"` — history of one text file. List mode (default): the most recent ' +
        'changes, newest first — each entry carries the change `head` (pass to ' +
        '`from_hash`/`to_hash`, or to `edit`\'s restore arm), the content `hash` after it, its ' +
        'author (with display name/color when the project records them), timestamp, and ' +
        'added/removed character counts. Diff mode: pass `from_hash` and `to_hash` for a ' +
        'unified diff between those two versions; `from_hash` alone diffs that version against ' +
        'the current content. Use it to answer "what changed since yesterday", to review a ' +
        'collaborator\'s edit before building on it, or to pick a restore point.',
      inputSchema: inRead,
      outputSchema: outRead,
      annotations: ANNOT_READ,
    },
    (args) => runDataTool('read', args, manager),
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
        'busy-polling `read` calls.',
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
    'get_errors',
    {
      title: 'Check a project for errors',
      description:
        'Check a Quarto Hub project for errors by rendering it with the same pipeline ' +
        'the browser preview uses, locally and on demand. Reports structured render ' +
        'errors and warnings (with line/column and hints) for exactly the file content ' +
        'the tool read (`checkedContentSha256` names it), plus engine execution errors ' +
        'recorded by executors. After you edit a file, just call get_errors again — it ' +
        'validates the new content immediately; there is nothing to wait for. Pass ' +
        '`path` to check one document; omit it to check every .qmd in the project. ' +
        'A full-project check renders each document in turn, so on a large project it ' +
        'can exceed your client timeout — prefer checking one path at a time there.',
      inputSchema: z.object({
        project: projectParam,
        path: z.string().describe('Optional: check only this file path').optional(),
      }),
      outputSchema: outGetErrors,
      annotations: ANNOT_READ,
    },
    (args) => runDataTool('get_errors', args, manager),
  );

  server.registerTool(
    'docs',
    {
      title: 'Search Quarto documentation',
      description:
        'Answer Quarto 2 usage questions from the documentation embedded in the q2 binary ' +
        '(the same corpus `q2 docs llms` serves — no network, no project connection). ' +
        '`query` searches page titles and full text and returns ranked `{href, title, snippet}` ' +
        'hits; `page` fetches one page\'s markdown by href (extensionless and .qmd spellings ' +
        'work). Typical flow: query, then page the best hit.',
      inputSchema: z.object({
        query: z
          .string()
          .optional()
          .describe('Search the documentation (titles + full text). Mutually exclusive with `page`.'),
        page: z
          .string()
          .optional()
          .describe(
            'Fetch one page by its href (e.g. `guides/authoring/figures.md`). ' +
              'Mutually exclusive with `query`.',
          ),
        max_results: z
          .number()
          .int()
          .min(1)
          .max(20)
          .optional()
          .describe('Maximum search hits to return (default 5, max 20).'),
      }),
      outputSchema: z.union([outDocsQuery, outDocsPage]),
      annotations: ANNOT_READ,
    },
    (args) => runDataTool('docs', args, manager),
  );

  if (readOnly) return;

  server.registerTool(
    'edit',
    {
      title: 'Edit project files and folders',
      description:
        'Edit a Quarto Hub project. All arms take `project` (index doc id or share URL) and ' +
        'honor `wait_for_sync`; the write/patch/restore arms accept `expected_hash` (from your ' +
        'last read) for compare-and-swap against collaborator edits.\n\n' +
        '`action: "write"` — replace the entire content of a file. Creates the file if it does ' +
        'not exist (`create_only: true` makes the write refuse instead of overwriting when it ' +
        'does — a create-with-guard). Prefer `action: "patch"` for small changes to large text ' +
        'files. With `encoding: "base64"`, `content` is base64-encoded bytes and the file is ' +
        'binary (`mime_type` overrides the extension-inferred type).\n\n' +
        '`action: "patch"` — apply a targeted edit to a text file: replace an exact ' +
        '`old_string`, or replace an entire qmd `section` (heading line included) with ' +
        '`new_string`; the two are mutually exclusive. More context-efficient than `action: ' +
        '"write"` for small changes to large files.\n\n' +
        '`action: "delete"` — delete a file. Dangling index entries (`status: "unavailable"`) ' +
        'are deletable too — the self-service repair.\n\n' +
        '`action: "rename"` — rename or move a file (`old_path` → `new_path`); refuses to ' +
        'overwrite an existing path.\n\n' +
        '`action: "restore"` — revert a text file to a prior version — the agent\'s undo ' +
        '(humans have Ctrl+Z and the web client\'s history view). `hash` is a change `head` ' +
        'from the `read` history arm. The restore is a NEW change on top of history (never a ' +
        'rewrite), attributed to you, so it syncs like any edit and collaborators keep their ' +
        'concurrent work. The result carries `pre_restore_hash` and `pre_restore_heads` — pass ' +
        '`pre_restore_heads[0]` back as `hash` to undo the restore itself. Restoring the ' +
        'current version is a no-op (`already_current: true`).\n\n' +
        '`action: "create_folder"` — create a folder. Folders are explicit markers: a file at ' +
        '`a/b/c.qmd` needs no folder to exist, but an EMPTY folder is listed only once created. ' +
        'Idempotent — re-creating an existing folder reports `created: false`.\n\n' +
        '`action: "delete_folder"` — delete a folder. Refuses while files remain under the ' +
        'path unless `recursive: true`, which deletes the contained files first (reported as ' +
        '`files_deleted`).',
      inputSchema: inEdit,
      outputSchema: outEdit,
      // Annotation homogenization (bd-lj6f7z9s, accepted on `edit`): the
      // merged tool is destructive + non-idempotent, so create_folder
      // looks scarier than it is.
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false },
    },
    (args) => runDataTool('edit', args, manager),
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

  // CAP-12: render is opt-in (--allow-render) and excluded from --read-only
  // even when both flags pass — read-only is the stricter gate. It is also
  // outside the ERG-5 default-listing ceiling precisely because it is
  // opt-in.
  if (!allowRender || readOnly) return;

  server.registerTool(
    'render',
    {
      title: 'Render the project',
      description:
        'Materialize the project to a temp dir and run `q2 render --json-errors` on it, ' +
        'returning `{ ok, exit_code, diagnostics, outputs, duration_ms }` — diagnostics carry ' +
        'Q- codes and source locations, so the edit → render → fix loop closes without a ' +
        'shell. Omit `path` to render everything; pass it for one file. SECURITY: rendering ' +
        'executes project code (computations, filters, engines) on this machine — only render ' +
        'projects you trust. The temp dir is removed afterwards; rendered files are not ' +
        'retained (`outputs` lists what was produced).',
      inputSchema: z.object({
        project: projectParam,
        path: z
          .string()
          .optional()
          .describe('Render just this file (project-relative). Omit to render the whole project.'),
        timeout_seconds: z
          .number()
          .min(1)
          .max(600)
          .default(120)
          .describe('Kill the render after this many seconds (default 120, max 600).'),
      }),
      outputSchema: outRender,
      annotations: {
        readOnlyHint: false,
        destructiveHint: false,
        idempotentHint: false,
        openWorldHint: true,
      },
    },
    (args, ctx) =>
      runDataTool('render', args, manager, {
        signal: ctx?.mcpReq?.signal,
      }),
  );
}
