/**
 * Roundtrip integration tests: create project → disconnect → reconnect → verify.
 *
 * Runs the same scenarios against both the Rust hub server and the
 * TypeScript automerge-repo-sync-server, making comparison automatic.
 */

import { describe, test, beforeAll, afterAll, expect } from 'vitest';
import {
  startHubServer,
  startTsSyncServer,
  tsSyncServerAvailable,
  type ServerHandle,
} from './server-manager.js';
import {
  createTestProject,
  verifyProject,
  type TestFile,
  type ExpectedFile,
} from './sync-test-helpers.js';

// ---------------------------------------------------------------------------
// Test data
// ---------------------------------------------------------------------------

const PROJECT_FILES: TestFile[] = [
  {
    path: '_quarto.yml',
    content: 'project:\n  type: website\n  title: "Test Project"\n',
    contentType: 'text',
  },
  {
    path: 'index.qmd',
    content: '---\ntitle: "Home"\n---\n\n# Welcome\n\nThis is a test project.\n',
    contentType: 'text',
  },
  {
    path: 'about.qmd',
    content: '---\ntitle: "About"\n---\n\n## About this project\n\nIt exists for testing.\n',
    contentType: 'text',
  },
];

const EXPECTED_FILES: ExpectedFile[] = PROJECT_FILES.map((f) => ({
  path: f.path,
  content: f.content,
  contentType: 'text' as const,
}));

// Use different ports to avoid conflicts between concurrent test suites
const HUB_PORT = 18_100;
const TS_PORT = 18_200;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Run the create-disconnect-reconnect-verify cycle.
 */
async function roundtrip(
  serverUrl: string,
  files: TestFile[],
  expected: ExpectedFile[],
  delayMs: number = 0,
) {
  // 1. Create project
  const { indexDocId, client } = await createTestProject(serverUrl, files);
  console.log(`  Created project: indexDocId=${indexDocId}`);

  // 2. Give the server time to persist before disconnecting
  await sleep(2000);

  // 3. Disconnect the creator
  await client.disconnect();
  console.log('  Creator disconnected');

  // 4. Optional delay between disconnect and reconnect
  if (delayMs > 0) {
    console.log(`  Waiting ${delayMs}ms before reconnect...`);
    await sleep(delayMs);
  }

  // 5. Reconnect with a fresh client and verify
  console.log('  Verifying with fresh client...');
  const result = await verifyProject(serverUrl, indexDocId, expected);

  // 6. Report
  if (!result.ok) {
    console.error('  VERIFICATION FAILED');
    if (result.missing.length > 0) {
      console.error('  Missing files:', result.missing);
    }
    for (const m of result.contentMismatch) {
      console.error(`  Content mismatch: ${m.path}`);
      console.error(`    expected: ${JSON.stringify(m.expected).slice(0, 100)}`);
      console.error(`    actual:   ${JSON.stringify(m.actual).slice(0, 100)}`);
    }
    console.log('  Found files:', [...result.foundFiles.keys()]);
  }

  return result;
}

// ---------------------------------------------------------------------------
// TS sync server tests (baseline — these should pass)
//
// The reference server lives in external-sources/, which is not
// version-controlled, so this tier can only run on a checkout that has it.
// It is skipped (not failed) elsewhere, including in CI. See GH #250.
// ---------------------------------------------------------------------------

describe.skipIf(!tsSyncServerAvailable())('ts-sync-server', () => {
  let server: ServerHandle;

  beforeAll(async () => {
    console.log('Starting TS sync server...');
    server = await startTsSyncServer({ port: TS_PORT });
    console.log(`TS sync server ready at ${server.url}`);
  }, 60_000);

  afterAll(async () => {
    if (server) {
      await server.stop();
      console.log('TS sync server stopped');
    }
  });

  test('create project and reconnect (no delay)', async () => {
    const result = await roundtrip(server.url, PROJECT_FILES, EXPECTED_FILES, 0);
    expect(result.ok, `Missing: ${result.missing}, Mismatches: ${result.contentMismatch.length}`).toBe(true);
    expect(result.missing).toEqual([]);
    expect(result.contentMismatch).toEqual([]);
  });

  test('create project and reconnect (1s delay)', async () => {
    const result = await roundtrip(server.url, PROJECT_FILES, EXPECTED_FILES, 1000);
    expect(result.ok).toBe(true);
  });

  test('create project and reconnect (5s delay)', async () => {
    const result = await roundtrip(server.url, PROJECT_FILES, EXPECTED_FILES, 5000);
    expect(result.ok).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// Hub server tests (these are expected to fail / expose the bug)
// ---------------------------------------------------------------------------

describe('hub', () => {
  let server: ServerHandle;

  beforeAll(async () => {
    console.log('Starting hub server...');
    server = await startHubServer({ port: HUB_PORT });
    console.log(`Hub server ready at ${server.url}`);
  }, 120_000);

  afterAll(async () => {
    if (server) {
      await server.stop();
      console.log('Hub server stopped');
    }
  });

  test('create project and reconnect (no delay)', async () => {
    const result = await roundtrip(server.url, PROJECT_FILES, EXPECTED_FILES, 0);
    expect(result.ok, `Missing: ${result.missing}, Mismatches: ${result.contentMismatch.length}`).toBe(true);
    expect(result.missing).toEqual([]);
    expect(result.contentMismatch).toEqual([]);
  });

  test('create project and reconnect (1s delay)', async () => {
    const result = await roundtrip(server.url, PROJECT_FILES, EXPECTED_FILES, 1000);
    expect(result.ok).toBe(true);
  });

  test('create project and reconnect (5s delay)', async () => {
    const result = await roundtrip(server.url, PROJECT_FILES, EXPECTED_FILES, 5000);
    expect(result.ok).toBe(true);
  });
});
