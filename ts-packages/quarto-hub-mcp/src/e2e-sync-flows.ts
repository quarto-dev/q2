/**
 * Sync-flow bodies for e2e-auth.test.ts — the "does sync work" half,
 * factored out of the test file. (Originally factored for a second
 * transport arm that was dropped when the UDS plan went peering-only;
 * the extraction stays because it keeps e2e-auth.test.ts lean.)
 * Auth-flow steps (login, refresh, revoke, ban) live in
 * e2e-auth.test.ts.
 */

import { expect } from 'vitest';

import type { McpTestClient } from './mcp-test-client.js';

/**
 * `create_project` with a single file, then `read_file` it back.
 * Returns the new project's indexDocId. `expectedMarker` defaults to
 * the trimmed file content.
 */
export async function createProjectAndReadBack(
  client: McpTestClient,
  fileName: string,
  fileContent: string,
  expectedMarker?: string,
): Promise<string> {
  const created = await client.callTool('create_project', {
    files: [{ path: fileName, content: fileContent }],
  });
  const createdText = created.content[0]!.text;
  expect(createdText, createdText).toContain('indexDocId');
  const projectId = (JSON.parse(createdText) as { indexDocId: string }).indexDocId;

  const read = await client.callTool('read_file', { project: projectId, path: fileName });
  expect(read.content[0]!.text).toContain(expectedMarker ?? fileContent.trim());
  return projectId;
}

/** `connect_project` to an existing project and read a file back. */
export async function connectProjectAndReadBack(
  client: McpTestClient,
  projectId: string,
  fileName: string,
  expectedMarker: string,
): Promise<void> {
  const connected = await client.callTool('connect_project', { project: projectId });
  expect(connected.content[0]!.text).toContain(fileName);
  const read = await client.callTool('read_file', { project: projectId, path: fileName });
  expect(read.content[0]!.text).toContain(expectedMarker);
}
