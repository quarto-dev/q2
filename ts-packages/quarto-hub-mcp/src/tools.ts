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
import { fileUnavailableMessage, type SyncClient } from '@quarto/quarto-sync-client';
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
  return { synced: await manager.awaitDelivery(project, paths, SYNC_WAIT_MS) };
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
 * If the share URL's `server=` names a hub different from the one this MCP is
 * configured to use, returns an `error` instead: silently connecting to the
 * configured hub would read/write the wrong documents. `configuredServer` is
 * the manager's {@link ConnectionManager.configuredServerUrl}.
 */
function normalizeArgs(
  args: ToolArgs,
  configuredServer: string,
): { args: ToolArgs } | { error: string } {
  if (typeof args.project !== 'string') {
    return { args };
  }
  const ref = parseProjectRef(args.project);
  if (ref.server && !serversMatch(ref.server, configuredServer)) {
    return {
      error:
        `Error: this share URL targets Quarto Hub server ${ref.server}, but this MCP ` +
        `server is connected to ${configuredServer}. Reading or writing would hit the ` +
        `wrong hub. Restart quarto-hub-mcp with \`--server ${ref.server}\` (or set ` +
        `QUARTO_HUB_SERVER=${ref.server}) to use the project this link points to.`,
    };
  }
  const next: ToolArgs = { ...args, project: ref.project };
  if (ref.file && (next.path === undefined || next.path === '')) {
    next.path = ref.file;
  }
  return { args: next };
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
}

/** Project state as exposed by {@link ConnectionManager.connect}. */
type ProjectState = Awaited<ReturnType<ConnectionManager['connect']>>;

function buildFileList(state: ProjectState): ListedFile[] {
  const fileList: ListedFile[] = Array.from(state.files.keys()).map((path) => ({
    path,
    type: state.files.get(path)!.type,
  }));
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
  const normalized = normalizeArgs(rawArgs, manager.configuredServerUrl);
  if ('error' in normalized) {
    return error(normalized.error);
  }
  const args = normalized.args;
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
  const state = await manager.connect(project);
  return text(JSON.stringify({ project, files: buildFileList(state) }, null, 2));
}

async function handleListFiles(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const state = await manager.connect(project);
  return text(JSON.stringify(buildFileList(state), null, 2));
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

async function handleReadFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const state = await manager.connect(project);
  const payload = state.files.get(path);

  if (!payload) {
    const ghost = findUnavailable(state.client, path);
    if (ghost) {
      return unavailableFileError(path, ghost.docId);
    }
    return fileNotFoundError(path, state);
  }
  if (payload.type === 'binary') {
    // HY-1 interim: binary reads land in read_file in Phase 2 (CAP-4);
    // until then say so — never name a tool that does not exist.
    return error(`Error: ${path} is a binary file; read_file currently supports text files only.`);
  }
  return text(
    JSON.stringify({ path, hash: hashPayload(payload), content: payload.text }, null, 2),
  );
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
  });

  if (!result.changed) {
    return text(
      JSON.stringify(
        {
          changed: false,
          path,
          hash: result.hash,
          message: `No change within ${timeoutSec}s. Call wait_for_change again (pass this hash as since_hash) to keep watching.`,
        },
        null,
        2,
      ),
    );
  }
  if (result.payload === null) {
    return text(JSON.stringify({ changed: true, removed: true, path }, null, 2));
  }
  if (result.payload.type === 'binary') {
    return text(
      JSON.stringify(
        { changed: true, path, type: 'binary', mimeType: result.payload.mimeType, hash: result.hash },
        null,
        2,
      ),
    );
  }
  return text(
    JSON.stringify({ changed: true, path, hash: result.hash, content: result.payload.text }, null, 2),
  );
}

async function handleWriteFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const content = args.content as string;
  const expectedHash = typeof args.expected_hash === 'string' ? args.expected_hash : undefined;
  const state = await manager.connect(project);
  const existing = state.files.get(path);

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
    return text(
      JSON.stringify(
        {
          path,
          hash: hashPayload({ type: 'text', text: content }),
          created: true,
          ...(await syncField(args, manager, project, [path])),
        },
        null,
        2,
      ),
    );
  }
  if (existing.type === 'binary') {
    return error(`Error: ${path} is a binary file. Cannot write text content to it.`);
  }
  if (expectedHash !== undefined && hashPayload(existing) !== expectedHash) {
    return staleHashError('write_file', path, existing.text);
  }

  state.client.updateFileContent(path, content);
  return text(
    JSON.stringify(
      {
        path,
        hash: hashPayload({ type: 'text', text: content }),
        ...(await syncField(args, manager, project, [path])),
      },
      null,
      2,
    ),
  );
}

async function handlePatchFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const oldString = args.old_string as string;
  const newString = args.new_string as string;
  const expectedHash = typeof args.expected_hash === 'string' ? args.expected_hash : undefined;
  const state = await manager.connect(project);
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
  return text(
    JSON.stringify(
      {
        path,
        hash: hashPayload({ type: 'text', text: newContent }),
        ...(await syncField(args, manager, project, [path])),
      },
      null,
      2,
    ),
  );
}

async function handleCreateFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const content = (args.content as string) ?? '';
  const state = await manager.connect(project);

  if (state.files.has(path)) {
    return error(`Error: File already exists: ${path}. Use write_file to update it.`);
  }
  // Same hazard as write_file: don't silently repoint a dangling entry.
  const ghost = findUnavailable(state.client, path);
  if (ghost) {
    return unavailableFileError(path, ghost.docId);
  }

  await state.client.createFile(path, content);
  return text(
    JSON.stringify(
      {
        path,
        hash: hashPayload({ type: 'text', text: content }),
        created: true,
        ...(await syncField(args, manager, project, [path])),
      },
      null,
      2,
    ),
  );
}

async function handleDeleteFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const state = await manager.connect(project);

  // Dangling entries ARE deletable: delete only edits the index, no
  // document fetch involved — this is the self-service repair for a
  // ghost entry (bd-vm5e5u10; the 2026-06-12 incident needed manual
  // index surgery precisely because this path didn't exist).
  if (!state.files.has(path) && !findUnavailable(state.client, path)) {
    return fileNotFoundError(path, state);
  }

  state.client.deleteFile(path);
  return text(
    JSON.stringify(
      { path, deleted: true, ...(await syncField(args, manager, project, [])) },
      null,
      2,
    ),
  );
}

async function handleRenameFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const oldPath = args.old_path as string;
  const newPath = args.new_path as string;
  const state = await manager.connect(project);

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
  return text(
    JSON.stringify(
      {
        old_path: oldPath,
        new_path: newPath,
        renamed: true,
        ...(await syncField(args, manager, project, [])),
      },
      null,
      2,
    ),
  );
}

async function handleCreateProject(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const files = (args.files as Array<{ path: string; content: string }>) ?? [];
  const result = await manager.createProject(files);
  return text(JSON.stringify({
    indexDocId: result.indexDocId,
    files: result.files,
    ...(await syncField(
      args,
      manager,
      result.indexDocId,
      result.files.map((f) => f.path),
    )),
  }, null, 2));
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
          description: def.description,
          inputSchema: z.object({}),
          annotations: def.annotations,
        },
        (_args, ctx) => authToolsState.handle(def.name, extractAuthContext(ctx)),
      );
    }
  }

  server.registerTool(
    'connect_project',
    {
      description:
        'Connect to a Quarto Hub project by its automerge index document ID — ' +
        'or by a quarto-hub.com share URL (`https://quarto-hub.com/#/share/<id>?…`), ' +
        'from which the id is extracted automatically. ' +
        'Returns the list of files in the project. ' +
        'If the hub requires authentication and no valid credentials are cached, ' +
        'this throws an `AuthRequiredError` / `ReauthRequired` — call ' +
        '`authenticate` to sign in.',
      inputSchema: z.object({ project: projectParam }),
      annotations: ANNOT_READ,
    },
    (args) => runDataTool('connect_project', args, manager),
  );

  server.registerTool(
    'list_files',
    {
      description: 'List all files in a connected Quarto Hub project.',
      inputSchema: z.object({ project: projectParam }),
      annotations: ANNOT_READ,
    },
    (args) => runDataTool('list_files', args, manager),
  );

  server.registerTool(
    'read_file',
    {
      description:
        'Read the text content of a file in a Quarto Hub project. Returns `{ path, hash, content }` — ' +
        'pass `hash` back as `expected_hash` on write_file/patch_file so an edit a collaborator made ' +
        'since this read is never silently overwritten.',
      inputSchema: z.object({ project: projectParam, path: pathParam.optional() }),
      annotations: ANNOT_READ,
    },
    (args) => runDataTool('read_file', args, manager),
  );

  server.registerTool(
    'wait_for_change',
    {
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
      description:
        'Replace the entire content of a text file in a Quarto Hub project. Creates the file if it ' +
        'does not exist. Returns `{ path, hash }`. Prefer patch_file for small changes to large files.',
      inputSchema: z.object({
        project: projectParam,
        path: pathParam.optional(),
        content: z.string().describe('The new file content'),
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
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: true },
    },
    (args) => runDataTool('write_file', args, manager),
  );

  server.registerTool(
    'patch_file',
    {
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
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false },
    },
    (args) => runDataTool('patch_file', args, manager),
  );

  server.registerTool(
    'create_file',
    {
      description: 'Create a new text file in a Quarto Hub project.',
      inputSchema: z.object({
        project: projectParam,
        path: pathParam.optional(),
        content: z.string().describe('Initial file content (defaults to empty)').default(''),
        wait_for_sync: waitForSyncParam,
      }),
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false },
    },
    (args) => runDataTool('create_file', args, manager),
  );

  server.registerTool(
    'delete_file',
    {
      description: 'Delete a file from a Quarto Hub project.',
      inputSchema: z.object({
        project: projectParam,
        path: z.string().describe('The file path to delete').optional(),
        wait_for_sync: waitForSyncParam,
      }),
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false },
    },
    (args) => runDataTool('delete_file', args, manager),
  );

  server.registerTool(
    'rename_file',
    {
      description: 'Rename or move a file within a Quarto Hub project.',
      inputSchema: z.object({
        project: projectParam,
        old_path: z.string().describe('The current file path'),
        new_path: z.string().describe('The new file path'),
        wait_for_sync: waitForSyncParam,
      }),
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false },
    },
    (args) => runDataTool('rename_file', args, manager),
  );

  server.registerTool(
    'create_project',
    {
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
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false, openWorldHint: true },
    },
    (args) => runDataTool('create_project', args, manager),
  );
}
