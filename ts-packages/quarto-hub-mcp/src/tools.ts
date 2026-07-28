/**
 * MCP Tool Definitions
 *
 * Registers all MCP tools on the server. Each tool operates on a project
 * identified by its automerge index document ID.
 *
 * Uses the lower-level Server API with explicit JSON schemas to avoid
 * Zod v4 type inference issues with the McpServer high-level API.
 */

import type { Server } from '@modelcontextprotocol/sdk/server/index.js';
import {
  CallToolRequestSchema,
  ListToolsRequestSchema,
} from '@modelcontextprotocol/sdk/types.js';
import type { Tool, CallToolResult } from '@modelcontextprotocol/sdk/types.js';
import { fileUnavailableMessage, type SyncClient } from '@quarto/quarto-sync-client';
import { ConnectionManager } from './connection-manager.js';
import { renderDiagnostics, type RenderedDiagnostic } from './local-render.js';
import {
  AUTH_TOOL_DEFINITIONS,
  AuthToolsState,
  extractAuthContext,
  type AuthToolName,
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

// ============================================================================
// Tool definitions
// ============================================================================

function getReadTools(): Tool[] {
  return [
    {
      name: 'connect_project',
      description:
        'Connect to a Quarto Hub project by its automerge index document ID — ' +
        'or by a quarto-hub.com share URL (`https://quarto-hub.com/#/share/<id>?…`), ' +
        'from which the id is extracted automatically. ' +
        'Returns the list of files in the project. ' +
        'If the hub requires authentication and no valid credentials are cached, ' +
        'this throws an `AuthRequiredError` / `ReauthRequired` — call ' +
        '`authenticate` to sign in.',
      inputSchema: {
        type: 'object',
        properties: {
          project: { type: 'string', description: PROJECT_PARAM_DESC },
        },
        required: ['project'],
      },
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
    },
    {
      name: 'list_files',
      description: 'List all files in a connected Quarto Hub project.',
      inputSchema: {
        type: 'object',
        properties: {
          project: { type: 'string', description: PROJECT_PARAM_DESC },
        },
        required: ['project'],
      },
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
    },
    {
      name: 'read_file',
      description: 'Read the text content of a file in a Quarto Hub project.',
      inputSchema: {
        type: 'object',
        properties: {
          project: { type: 'string', description: PROJECT_PARAM_DESC },
          path: { type: 'string', description: 'The file path within the project' },
        },
        required: ['project', 'path'],
      },
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
    },
    {
      name: 'wait_for_change',
      description:
        'Long-poll: block until a file in the project is edited by any collaborator, then return its ' +
        'new content. Returns as soon as a change is observed, or after `timeout_seconds` with ' +
        '`changed: false` (re-call to keep watching). The result includes a `hash`; pass it back as ' +
        '`since_hash` on the next call so an edit landing between calls is never missed. Lets an agent ' +
        'react to a live collaborator without busy-polling read_file.',
      inputSchema: {
        type: 'object',
        properties: {
          project: { type: 'string', description: PROJECT_PARAM_DESC },
          path: { type: 'string', description: 'The file path within the project to watch' },
          timeout_seconds: {
            type: 'number',
            description: 'Max seconds to block before returning changed=false (default 25, clamped to 1-55)',
            default: 25,
          },
          since_hash: {
            type: 'string',
            description:
              'Optional hash from a prior result. If the file already differs from it, returns immediately ' +
              '(closes the gap between polls).',
          },
        },
        required: ['project', 'path'],
      },
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: false },
    },
    {
      name: 'get_errors',
      description:
        'Check a Quarto Hub project for errors by rendering it with the same pipeline ' +
        'the browser preview uses, locally and on demand. Reports structured render ' +
        'errors and warnings (with line/column and hints) for exactly the file content ' +
        'the tool read (`checkedContentSha256` names it), plus engine execution errors ' +
        'recorded by executors. After you edit a file, just call get_errors again — it ' +
        'validates the new content immediately; there is nothing to wait for. Pass ' +
        '`path` to check one document; omit it to check every .qmd in the project.',
      inputSchema: {
        type: 'object',
        properties: {
          project: { type: 'string', description: PROJECT_PARAM_DESC },
          path: {
            type: 'string',
            description: 'Optional: check only this file path',
          },
        },
        required: ['project'],
      },
      annotations: { readOnlyHint: true, destructiveHint: false, idempotentHint: true },
    },
  ];
}

function getWriteTools(): Tool[] {
  return [
    {
      name: 'write_file',
      description: 'Replace the entire content of a text file in a Quarto Hub project. Creates the file if it does not exist.',
      inputSchema: {
        type: 'object',
        properties: {
          project: { type: 'string', description: PROJECT_PARAM_DESC },
          path: { type: 'string', description: 'The file path within the project' },
          content: { type: 'string', description: 'The new file content' },
        },
        required: ['project', 'path', 'content'],
      },
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: true },
    },
    {
      name: 'patch_file',
      description: 'Apply a targeted edit to a text file by replacing a specific string. More context-efficient than write_file for small changes to large files.',
      inputSchema: {
        type: 'object',
        properties: {
          project: { type: 'string', description: PROJECT_PARAM_DESC },
          path: { type: 'string', description: 'The file path within the project' },
          old_string: { type: 'string', description: 'The exact string to find and replace' },
          new_string: { type: 'string', description: 'The replacement string' },
        },
        required: ['project', 'path', 'old_string', 'new_string'],
      },
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false },
    },
    {
      name: 'create_file',
      description: 'Create a new text file in a Quarto Hub project.',
      inputSchema: {
        type: 'object',
        properties: {
          project: { type: 'string', description: PROJECT_PARAM_DESC },
          path: { type: 'string', description: 'The file path within the project' },
          content: { type: 'string', description: 'Initial file content (defaults to empty)', default: '' },
        },
        required: ['project', 'path'],
      },
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false },
    },
    {
      name: 'delete_file',
      description: 'Delete a file from a Quarto Hub project.',
      inputSchema: {
        type: 'object',
        properties: {
          project: { type: 'string', description: PROJECT_PARAM_DESC },
          path: { type: 'string', description: 'The file path to delete' },
        },
        required: ['project', 'path'],
      },
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false },
    },
    {
      name: 'rename_file',
      description: 'Rename or move a file within a Quarto Hub project.',
      inputSchema: {
        type: 'object',
        properties: {
          project: { type: 'string', description: PROJECT_PARAM_DESC },
          old_path: { type: 'string', description: 'The current file path' },
          new_path: { type: 'string', description: 'The new file path' },
        },
        required: ['project', 'old_path', 'new_path'],
      },
      annotations: { readOnlyHint: false, destructiveHint: true, idempotentHint: false },
    },
    {
      name: 'create_project',
      description: 'Create a new Quarto Hub project on the sync server with optional initial files.',
      inputSchema: {
        type: 'object',
        properties: {
          files: {
            type: 'array',
            description: 'Initial files to create in the project',
            items: {
              type: 'object',
              properties: {
                path: { type: 'string', description: 'File path' },
                content: { type: 'string', description: 'File content' },
              },
              required: ['path', 'content'],
            },
            default: [],
          },
        },
      },
      annotations: { readOnlyHint: false, destructiveHint: false, idempotentHint: false, openWorldHint: true },
    },
  ];
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

async function handleTool(
  name: string,
  rawArgs: ToolArgs,
  manager: ConnectionManager
): Promise<CallToolResult> {
  const normalized = normalizeArgs(rawArgs, manager.configuredServerUrl);
  if ('error' in normalized) {
    return error(normalized.error);
  }
  const args = normalized.args;
  switch (name) {
    case 'connect_project':
      return handleConnectProject(args, manager);
    case 'list_files':
      return handleListFiles(args, manager);
    case 'read_file':
      return handleReadFile(args, manager);
    case 'wait_for_change':
      return handleWaitForChange(args, manager);
    case 'get_errors':
      return handleGetErrors(args, manager);
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
    default:
      return error(`Unknown tool: ${name}`);
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
    return error(`Error: File not found: ${path}`);
  }
  if (payload.type === 'binary') {
    return error(`Error: ${path} is a binary file. Use read_binary_file_metadata instead.`);
  }
  return text(payload.text);
}

async function handleWaitForChange(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const rawTimeout = typeof args.timeout_seconds === 'number' ? args.timeout_seconds : 25;
  const timeoutSec = Math.max(1, Math.min(55, rawTimeout));
  const sinceHash = typeof args.since_hash === 'string' ? args.since_hash : undefined;

  const result = await manager.waitForChange(project, path, timeoutSec * 1000, sinceHash);

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
    const result = await renderDiagnostics(state.files, path);
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
  return text(JSON.stringify(report, null, 2));
}

async function handleWriteFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const content = args.content as string;
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
    await state.client.createFile(path, content);
    return text(`Created ${path}`);
  }
  if (existing.type === 'binary') {
    return error(`Error: ${path} is a binary file. Cannot write text content to it.`);
  }

  state.client.updateFileContent(path, content);
  return text(`Updated ${path}`);
}

async function handlePatchFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const path = args.path as string;
  const oldString = args.old_string as string;
  const newString = args.new_string as string;
  const state = await manager.connect(project);
  const payload = state.files.get(path);

  if (!payload) {
    const ghost = findUnavailable(state.client, path);
    if (ghost) {
      return unavailableFileError(path, ghost.docId);
    }
    return error(`Error: File not found: ${path}`);
  }
  if (payload.type === 'binary') {
    return error(`Error: ${path} is a binary file. Cannot patch.`);
  }

  const currentContent = payload.text;
  const index = currentContent.indexOf(oldString);
  if (index === -1) {
    return error(`Error: old_string not found in ${path}`);
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
  return text(`Patched ${path}`);
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
  return text(`Created ${path}`);
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
    return error(`Error: File not found: ${path}`);
  }

  state.client.deleteFile(path);
  return text(`Deleted ${path}`);
}

async function handleRenameFile(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const project = args.project as string;
  const oldPath = args.old_path as string;
  const newPath = args.new_path as string;
  const state = await manager.connect(project);

  // Renaming only edits the index, so a dangling entry can be renamed.
  if (!state.files.has(oldPath) && !findUnavailable(state.client, oldPath)) {
    return error(`Error: File not found: ${oldPath}`);
  }
  if (state.files.has(newPath) || findUnavailable(state.client, newPath)) {
    return error(`Error: Destination already exists: ${newPath}`);
  }

  state.client.renameFile(oldPath, newPath);
  return text(`Renamed ${oldPath} → ${newPath}`);
}

async function handleCreateProject(args: ToolArgs, manager: ConnectionManager): Promise<CallToolResult> {
  const files = (args.files as Array<{ path: string; content: string }>) ?? [];
  const result = await manager.createProject(files);
  return text(JSON.stringify({
    indexDocId: result.indexDocId,
    files: result.files,
  }, null, 2));
}

// ============================================================================
// Registration
// ============================================================================

/**
 * Register all tool handlers on the MCP server.
 */
export function registerTools(
  server: Server,
  manager: ConnectionManager,
  readOnly: boolean,
  authToolsState?: AuthToolsState,
): void {
  const dataTools = [...getReadTools(), ...(readOnly ? [] : getWriteTools())];
  const allTools = authToolsState
    ? [...AUTH_TOOL_DEFINITIONS, ...dataTools]
    : dataTools;

  server.setRequestHandler(ListToolsRequestSchema, async () => {
    return { tools: allTools };
  });

  server.setRequestHandler(CallToolRequestSchema, async (request, extra) => {
    const { name, arguments: args } = request.params;

    if (
      authToolsState &&
      (name === 'authenticate' || name === 'authenticate_clear')
    ) {
      return authToolsState.handle(name as AuthToolName, extractAuthContext(extra));
    }

    const tool = dataTools.find(t => t.name === name);
    if (!tool) {
      return error(`Unknown tool: ${name}`);
    }

    try {
      return await handleTool(name, args ?? {}, manager);
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      // Redact in case the error message carries token bytes (defensive).
      return error(`Error in ${name}: ${redactTokens(message)}`);
    }
  });
}
