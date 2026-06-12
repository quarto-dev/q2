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
import {
  AUTH_TOOL_DEFINITIONS,
  AuthToolsState,
  extractAuthContext,
  type AuthToolName,
} from './auth/auth-tools.js';
import { redactTokens } from './auth/redact.js';

function text(msg: string): CallToolResult {
  return { content: [{ type: 'text', text: msg }] };
}

function error(msg: string): CallToolResult {
  return { content: [{ type: 'text', text: msg }], isError: true };
}

// ============================================================================
// Tool definitions
// ============================================================================

function getReadTools(): Tool[] {
  return [
    {
      name: 'connect_project',
      description:
        'Connect to a Quarto Hub project by its automerge index document ID. ' +
        'Returns the list of files in the project. ' +
        'If the hub requires authentication and no valid credentials are cached, ' +
        'this throws an `AuthRequiredError` / `ReauthRequired` — call ' +
        '`authenticate` to sign in.',
      inputSchema: {
        type: 'object',
        properties: {
          project: { type: 'string', description: 'The automerge index document ID of the project' },
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
          project: { type: 'string', description: 'The automerge index document ID of the project' },
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
          project: { type: 'string', description: 'The automerge index document ID of the project' },
          path: { type: 'string', description: 'The file path within the project' },
        },
        required: ['project', 'path'],
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
          project: { type: 'string', description: 'The automerge index document ID of the project' },
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
          project: { type: 'string', description: 'The automerge index document ID of the project' },
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
          project: { type: 'string', description: 'The automerge index document ID of the project' },
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
          project: { type: 'string', description: 'The automerge index document ID of the project' },
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
          project: { type: 'string', description: 'The automerge index document ID of the project' },
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
  args: ToolArgs,
  manager: ConnectionManager
): Promise<CallToolResult> {
  switch (name) {
    case 'connect_project':
      return handleConnectProject(args, manager);
    case 'list_files':
      return handleListFiles(args, manager);
    case 'read_file':
      return handleReadFile(args, manager);
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
