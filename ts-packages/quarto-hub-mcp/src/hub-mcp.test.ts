/**
 * Tests for the Quarto Hub MCP Server.
 *
 * These tests spawn the actual MCP server as a child process and
 * communicate with it via JSON-RPC over stdio, just like a real
 * MCP client (Claude Code, Cursor, etc.) would.
 *
 * Tests marked with "live" require connectivity to the automerge
 * sync server at wss://sync.automerge.org.
 */

import { describe, it, expect, beforeAll, afterAll, afterEach } from 'vitest';
import { McpTestClient } from './mcp-test-client.js';

// The hello world project on sync.automerge.org
const SYNC_SERVER = 'wss://sync.automerge.org';
const HELLO_WORLD_DOC = 'automerge:2knrbhSpo36X5Kk6ADkAX6qZLnfM';

// ============================================================================
// Protocol tests (no sync server needed)
// ============================================================================

describe('MCP protocol', () => {
  let client: McpTestClient;

  // Use a dummy server URL — protocol tests don't actually connect to it
  beforeAll(async () => {
    client = new McpTestClient();
    await client.start(['--server', 'wss://dummy.example.com']);
  });

  afterAll(async () => {
    await client.stop();
  });

  it('should list all tools in read-write mode', async () => {
    const tools = await client.listTools();
    const names = tools.map(t => t.name).sort();
    expect(names).toEqual([
      'connect_project',
      'create_file',
      'create_folder',
      'create_project',
      'delete_file',
      'delete_folder',
      'disconnect_project',
      'get_file_history',
      'get_outline',
      'get_project_info',
      'list_files',
      'list_presence',
      'list_projects',
      'patch_file',
      'read_file',
      'rename_file',
      'restore_file_version',
      'search_files',
      'wait_for_change',
      'write_file',
    ]);
  });

  it('should include proper annotations on tools', async () => {
    const tools = await client.listTools();
    const readFile = tools.find(t => t.name === 'read_file');
    expect(readFile?.annotations).toEqual({
      readOnlyHint: true,
      destructiveHint: false,
      idempotentHint: true,
    });

    const deleteFile = tools.find(t => t.name === 'delete_file');
    expect(deleteFile?.annotations).toEqual({
      readOnlyHint: false,
      destructiveHint: true,
      idempotentHint: false,
    });
  });

  it('should include proper input schemas on tools', async () => {
    const tools = await client.listTools();
    const readFile = tools.find(t => t.name === 'read_file');
    // Post-SDK-v2 (BP-16): schemas are zod-derived and declare the
    // 2020-12 dialect (BP-11, SEP-1613). `path` is no longer schema-
    // required: a share URL's `file=` may supply it (see tools.ts
    // PATH_DEFAULTABLE); the server enforces it post-normalization.
    expect(readFile?.inputSchema).toEqual({
      $schema: 'https://json-schema.org/draft/2020-12/schema',
      type: 'object',
      properties: {
        project: { type: 'string', description: expect.any(String) },
        path: { type: 'string', description: expect.any(String) },
        // ERG-3 ranges/truncation + CAP-4 metadata_only (Phase 2).
        offset: { type: 'integer', minimum: 1, maximum: 9007199254740991, description: expect.any(String) },
        limit: { type: 'integer', minimum: 1, maximum: 9007199254740991, description: expect.any(String) },
        max_bytes: { type: 'integer', minimum: 16, maximum: 1048576, description: expect.any(String) },
        metadata_only: { type: 'boolean', description: expect.any(String) },
        // CAP-11 section selector (Phase 4).
        section: { type: 'string', description: expect.any(String) },
      },
      required: ['project'],
    });
  });

  // A share URL whose server= names a different hub now ROUTES the call
  // to that hub (bd-qt7h8h5g) instead of being rejected. Here the foreign
  // hub is a guaranteed-unreachable loopback port, so the call fails at
  // the no-auth probe — with a connection error, not the old
  // "restart with --server" rejection. Offline-deterministic.
  it('should route (not reject) a share URL pointing at a different hub', async () => {
    const result = await client.callTool('connect_project', {
      project:
        'https://quarto-hub.com/#/share/abc123?server=ws%3A%2F%2F127.0.0.1%3A1%2Fws',
    });
    expect(result.isError).toBe(true);
    const msg = result.content[0]!.text;
    expect(msg).not.toContain('Restart quarto-hub-mcp');
    expect(msg).not.toContain('targets Quarto Hub server');
  });
});

describe('MCP protocol (read-only mode)', () => {
  let client: McpTestClient;

  beforeAll(async () => {
    client = new McpTestClient();
    await client.start(['--server', 'wss://dummy.example.com', '--read-only']);
  });

  afterAll(async () => {
    await client.stop();
  });

  it('should only list read-only tools', async () => {
    const tools = await client.listTools();
    const names = tools.map(t => t.name).sort();
    expect(names).toEqual([
      'connect_project',
      'disconnect_project',
      'get_file_history',
      'get_outline',
      'get_project_info',
      'list_files',
      'list_presence',
      'list_projects',
      'read_file',
      'search_files',
      'wait_for_change',
    ]);
  });

  it('should reject unknown tools', async () => {
    // Post-SDK-v2 (BP-15): a tool the server doesn't have (here a write
    // tool in read-only mode) is a JSON-RPC -32602 protocol error, not
    // an isError result — McpTestClient.callTool throws on it.
    await expect(
      client.callTool('write_file', {
        project: 'test',
        path: 'test.qmd',
        content: 'hello',
      }),
    ).rejects.toThrow('Tool write_file not found');
  });
});

// ============================================================================
// Live integration tests (require sync.automerge.org)
// ============================================================================

describe('live: connect and read', () => {
  let client: McpTestClient;

  beforeAll(async () => {
    client = new McpTestClient();
    await client.start(['--server', SYNC_SERVER]);
  }, 15000);

  afterAll(async () => {
    await client.stop();
  });

  it('should connect to the hello world project', async () => {
    const result = await client.callTool('connect_project', {
      project: HELLO_WORLD_DOC,
    });
    expect(result.isError).toBeUndefined();
    const data = JSON.parse(result.content[0]!.text);
    expect(data.project).toBe(HELLO_WORLD_DOC);
    expect(data.files).toBeInstanceOf(Array);
    expect(data.files.length).toBeGreaterThan(0);

    // Should contain known files
    const paths = data.files.map((f: { path: string }) => f.path);
    expect(paths).toContain('index.qmd');
    expect(paths).toContain('_quarto.yml');
  }, 15000);

  it('should list files in the hello world project', async () => {
    const result = await client.callTool('list_files', {
      project: HELLO_WORLD_DOC,
    });
    expect(result.isError).toBeUndefined();
    const files = JSON.parse(result.content[0]!.text);
    expect(files).toBeInstanceOf(Array);

    // Check that text and binary files are correctly typed
    const indexQmd = files.find((f: { path: string }) => f.path === 'index.qmd');
    expect(indexQmd?.type).toBe('text');

    const image = files.find((f: { path: string }) => f.path === 'code.png');
    expect(image?.type).toBe('binary');
  }, 15000);

  it('should read a text file', async () => {
    const result = await client.callTool('read_file', {
      project: HELLO_WORLD_DOC,
      path: 'index.qmd',
    });
    expect(result.isError).toBeUndefined();
    const content = result.content[0]!.text;
    // The hello world index.qmd has YAML frontmatter
    expect(content).toContain('---');
    expect(content).toContain('title:');
  }, 15000);

  it('should read _quarto.yml', async () => {
    const result = await client.callTool('read_file', {
      project: HELLO_WORLD_DOC,
      path: '_quarto.yml',
    });
    expect(result.isError).toBeUndefined();
    const content = result.content[0]!.text;
    expect(content.length).toBeGreaterThan(0);
  }, 15000);

  it('should read a binary file (CAP-4)', async () => {
    const result = await client.callTool('read_file', {
      project: HELLO_WORLD_DOC,
      path: 'code.png',
    });
    expect(result.isError).toBeUndefined();
    // An image MIME type comes back as an `image` content block, with
    // structured metadata alongside (Phase 2: binary rides read_file).
    const imageBlock = result.content.find((b) => b.type === 'image') as
      | { type: 'image'; data: string; mimeType: string }
      | undefined;
    expect(imageBlock).toBeDefined();
    expect(imageBlock?.mimeType).toBe('image/png');
    const meta = (result as { structuredContent?: Record<string, unknown> }).structuredContent;
    expect(meta).toMatchObject({ path: 'code.png', type: 'binary', mimeType: 'image/png' });
  }, 15000);

  it('should error on reading a non-existent file', async () => {
    const result = await client.callTool('read_file', {
      project: HELLO_WORLD_DOC,
      path: 'does-not-exist.qmd',
    });
    expect(result.isError).toBe(true);
    expect(result.content[0]!.text).toContain('File not found');
  }, 15000);

  // End-to-end share-URL handling: the `project` argument is a full
  // quarto-hub.com share link rather than a bare id. The server must extract
  // the id from the `#/share/<id>` fragment, and `file=` must supply a default
  // `path` so read_file works with no explicit path. (bd-m4slev7a)
  const SHARE_URL =
    `https://quarto-hub.com/#/share/${HELLO_WORLD_DOC}` +
    `?server=wss%3A%2F%2Fsync.automerge.org&file=index.qmd&name=Hello+World`;

  it('should connect using a share URL in place of an id', async () => {
    const result = await client.callTool('connect_project', { project: SHARE_URL });
    expect(result.isError).toBeUndefined();
    const data = JSON.parse(result.content[0]!.text);
    expect(data.project).toBe(HELLO_WORLD_DOC);
    const paths = data.files.map((f: { path: string }) => f.path);
    expect(paths).toContain('index.qmd');
  }, 15000);

  it('should read the share URL file= with no explicit path', async () => {
    const result = await client.callTool('read_file', { project: SHARE_URL });
    expect(result.isError).toBeUndefined();
    const content = result.content[0]!.text;
    // index.qmd (named by file= in the share URL) has YAML frontmatter.
    expect(content).toContain('---');
    expect(content).toContain('title:');
  }, 15000);

  it('should let an explicit path override the share URL file=', async () => {
    const result = await client.callTool('read_file', {
      project: SHARE_URL,
      path: '_quarto.yml',
    });
    expect(result.isError).toBeUndefined();
    expect(result.content[0]!.text.length).toBeGreaterThan(0);
  }, 15000);
});

describe('live: create project and mutate files', () => {
  let client: McpTestClient;
  let projectId: string;

  beforeAll(async () => {
    client = new McpTestClient();
    await client.start(['--server', SYNC_SERVER]);
  }, 15000);

  afterAll(async () => {
    await client.stop();
  });

  it('should create a new project', async () => {
    const result = await client.callTool('create_project', {
      files: [
        { path: 'hello.qmd', content: '---\ntitle: Test\n---\n\nHello world\n' },
        { path: '_quarto.yml', content: 'project:\n  type: default\n' },
      ],
    });
    expect(result.isError).toBeUndefined();
    const data = JSON.parse(result.content[0]!.text);
    expect(data.indexDocId).toBeTruthy();
    expect(data.files).toHaveLength(2);
    projectId = data.indexDocId;
  }, 15000);

  it('should read files from the new project', async () => {
    const result = await client.callTool('read_file', {
      project: projectId,
      path: 'hello.qmd',
    });
    expect(result.isError).toBeUndefined();
    expect(result.content[0]!.text).toContain('Hello world');
  }, 15000);

  it('should write (update) a file', async () => {
    const result = await client.callTool('write_file', {
      project: projectId,
      path: 'hello.qmd',
      content: '---\ntitle: Updated\n---\n\nUpdated content\n',
    });
    expect(result.isError).toBeUndefined();
    // ERG-1: write results are { path, hash }.
    const written = JSON.parse(result.content[0]!.text);
    expect(written.path).toBe('hello.qmd');
    expect(written.hash).toMatch(/^sha256:/);

    // Verify the update
    const readResult = await client.callTool('read_file', {
      project: projectId,
      path: 'hello.qmd',
    });
    expect(readResult.content[0]!.text).toContain('Updated content');
  }, 15000);

  it('should patch a file', async () => {
    const result = await client.callTool('patch_file', {
      project: projectId,
      path: 'hello.qmd',
      old_string: 'Updated content',
      new_string: 'Patched content',
    });
    expect(result.isError).toBeUndefined();
    // ERG-1: patch results are { path, hash }.
    const patched = JSON.parse(result.content[0]!.text);
    expect(patched.path).toBe('hello.qmd');
    expect(patched.hash).toMatch(/^sha256:/);

    // Verify the patch
    const readResult = await client.callTool('read_file', {
      project: projectId,
      path: 'hello.qmd',
    });
    expect(readResult.content[0]!.text).toContain('Patched content');
  }, 15000);

  it('should error when patch old_string not found', async () => {
    const result = await client.callTool('patch_file', {
      project: projectId,
      path: 'hello.qmd',
      old_string: 'this string does not exist in the file',
      new_string: 'replacement',
    });
    expect(result.isError).toBe(true);
    expect(result.content[0]!.text).toContain('not found');
  }, 15000);

  it('should create a new file', async () => {
    const result = await client.callTool('create_file', {
      project: projectId,
      path: 'new-file.qmd',
      content: 'Brand new file',
    });
    expect(result.isError).toBeUndefined();
    // ERG-1/ERG-2: create results are { path, hash, created, synced }.
    const createdFile = JSON.parse(result.content[0]!.text);
    expect(createdFile.created).toBe(true);
    expect(createdFile.hash).toMatch(/^sha256:/);

    // Verify it exists (ERG-1: read results are { path, hash, content }).
    const readResult = await client.callTool('read_file', {
      project: projectId,
      path: 'new-file.qmd',
    });
    expect(JSON.parse(readResult.content[0]!.text).content).toBe('Brand new file');
  }, 15000);

  it('should error when creating a file that already exists', async () => {
    const result = await client.callTool('create_file', {
      project: projectId,
      path: 'hello.qmd',
      content: 'duplicate',
    });
    expect(result.isError).toBe(true);
    expect(result.content[0]!.text).toContain('already exists');
  }, 15000);

  it('should rename a file', async () => {
    const result = await client.callTool('rename_file', {
      project: projectId,
      old_path: 'new-file.qmd',
      new_path: 'renamed-file.qmd',
    });
    expect(result.isError).toBeUndefined();
    // ERG-2: rename results are { old_path, new_path, renamed, synced }.
    const renamed = JSON.parse(result.content[0]!.text);
    expect(renamed.renamed).toBe(true);
    expect(renamed.new_path).toBe('renamed-file.qmd');

    // Old path should not exist
    const oldResult = await client.callTool('read_file', {
      project: projectId,
      path: 'new-file.qmd',
    });
    expect(oldResult.isError).toBe(true);

    // New path should exist
    const newResult = await client.callTool('read_file', {
      project: projectId,
      path: 'renamed-file.qmd',
    });
    expect(JSON.parse(newResult.content[0]!.text).content).toBe('Brand new file');
  }, 15000);

  it('should delete a file', async () => {
    const result = await client.callTool('delete_file', {
      project: projectId,
      path: 'renamed-file.qmd',
    });
    expect(result.isError).toBeUndefined();
    // ERG-2: delete results are { path, deleted, synced }.
    const deletedFile = JSON.parse(result.content[0]!.text);
    expect(deletedFile.deleted).toBe(true);

    // File should no longer exist
    const readResult = await client.callTool('read_file', {
      project: projectId,
      path: 'renamed-file.qmd',
    });
    expect(readResult.isError).toBe(true);
    expect(readResult.content[0]!.text).toContain('File not found');
  }, 15000);

  it('should error when deleting a non-existent file', async () => {
    const result = await client.callTool('delete_file', {
      project: projectId,
      path: 'nonexistent.qmd',
    });
    expect(result.isError).toBe(true);
    expect(result.content[0]!.text).toContain('File not found');
  }, 15000);

  it('should write_file to create a file that does not exist', async () => {
    const result = await client.callTool('write_file', {
      project: projectId,
      path: 'created-via-write.qmd',
      content: 'Created via write_file',
    });
    expect(result.isError).toBeUndefined();
    // ERG-1: the create arm returns { path, hash, created: true }.
    const created = JSON.parse(result.content[0]!.text);
    expect(created.created).toBe(true);
    expect(created.hash).toMatch(/^sha256:/);

    const readResult = await client.callTool('read_file', {
      project: projectId,
      path: 'created-via-write.qmd',
    });
    expect(JSON.parse(readResult.content[0]!.text).content).toBe('Created via write_file');
  }, 15000);
});
