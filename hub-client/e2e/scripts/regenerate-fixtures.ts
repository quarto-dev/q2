#!/usr/bin/env npx tsx
/**
 * Regenerate E2E Test Fixtures
 *
 * This script creates test projects using quarto-sync-client and records
 * their document IDs in fixture-manifest.json for E2E tests.
 *
 * Usage:
 *   # With an external sync server running:
 *   SYNC_SERVER_URL=ws://localhost:3030 npm run e2e:regenerate-fixtures
 *
 *   # Or with default URL (ws://localhost:3030):
 *   npm run e2e:regenerate-fixtures
 *
 * Prerequisites:
 *   - A sync server must be running at the specified URL
 *   - The sync server should use persistent storage (for fixtures to survive restart)
 *
 * Output:
 *   - e2e/fixtures/fixture-manifest.json - Maps project names to document IDs
 *
 * Note: To create fixtures that can be checked into git, you'll need to:
 *   1. Run a sync server with file-based storage (NodeFSStorageAdapter)
 *   2. Run this script to create projects
 *   3. Copy the sync server's storage directory to e2e/fixtures/automerge-data/
 *   4. This will be automated in a future version with local server support
 */

import { createSyncClient } from '@quarto/quarto-sync-client';
import { writeFileSync, mkdirSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { TEST_PROJECTS, type TestProjectKey } from '../fixtures/testProjects.js';

// Get directory of this script
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

// Configuration
const SYNC_SERVER_URL = process.env.SYNC_SERVER_URL || 'ws://localhost:3030';
const FIXTURE_DIR = join(__dirname, '../fixtures');

interface FixtureManifest {
  schemaVersion: string;
  generatedAt: string;
  syncServerUrl: string;
  projects: {
    [key: string]: {
      indexDocId: string;
      description: string;
      files: string[];
    };
  };
}

async function createTestProject(
  projectKey: TestProjectKey,
  syncServerUrl: string
): Promise<{ indexDocId: string; files: string[] }> {
  const project = TEST_PROJECTS[projectKey];
  console.log(`\nCreating project: ${projectKey} (${project.description})`);

  // Create a sync client with minimal callbacks (we just need to create the project)
  const client = createSyncClient({
    onFileAdded: (path) => console.log(`  + ${path}`),
    onFileChanged: () => {},
    onBinaryChanged: () => {},
    onFileRemoved: () => {},
    onError: (error) => console.error(`  Error: ${error.message}`),
  });

  try {
    // Create the project with all files
    const result = await client.createNewProject({
      syncServer: syncServerUrl,
      files: project.files.map((f) => ({
        path: f.path,
        content: f.content,
        contentType: 'text' as const,
      })),
    });

    console.log(`  Created with indexDocId: ${result.indexDocId}`);

    return {
      indexDocId: result.indexDocId,
      files: result.files.map((f) => f.path),
    };
  } finally {
    // Disconnect to clean up
    await client.disconnect();
  }
}

async function main() {
  console.log('='.repeat(60));
  console.log('E2E Fixture Regeneration');
  console.log('='.repeat(60));
  console.log(`Sync Server: ${SYNC_SERVER_URL}`);
  console.log(`Output: ${FIXTURE_DIR}/fixture-manifest.json`);

  // Ensure fixture directory exists
  mkdirSync(FIXTURE_DIR, { recursive: true });

  const manifest: FixtureManifest = {
    schemaVersion: '0.0.1',
    generatedAt: new Date().toISOString(),
    syncServerUrl: SYNC_SERVER_URL,
    projects: {},
  };

  // Create each test project
  const projectKeys = Object.keys(TEST_PROJECTS) as TestProjectKey[];

  for (const key of projectKeys) {
    try {
      const result = await createTestProject(key, SYNC_SERVER_URL);
      manifest.projects[key] = {
        indexDocId: result.indexDocId,
        description: TEST_PROJECTS[key].description,
        files: result.files,
      };

      // Small delay between projects to avoid overwhelming the server
      await new Promise((r) => setTimeout(r, 500));
    } catch (error) {
      console.error(`\nFailed to create project ${key}:`);
      console.error(error);
      process.exit(1);
    }
  }

  // Write manifest
  const manifestPath = join(FIXTURE_DIR, 'fixture-manifest.json');
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');

  console.log('\n' + '='.repeat(60));
  console.log('Fixtures generated successfully!');
  console.log('='.repeat(60));
  console.log(`\nManifest written to: ${manifestPath}`);
  console.log('\nProjects created:');
  for (const [key, project] of Object.entries(manifest.projects)) {
    console.log(`  - ${key}: ${project.indexDocId}`);
  }

  console.log('\nNext steps:');
  console.log('  1. If using file-based sync server storage:');
  console.log('     - Copy the storage directory to e2e/fixtures/automerge-data/');
  console.log('     - Commit the fixtures to git');
  console.log('  2. Run E2E tests with: npm run test:e2e');
}

// Run
main().catch((error) => {
  console.error('Fatal error:', error);
  process.exit(1);
});
