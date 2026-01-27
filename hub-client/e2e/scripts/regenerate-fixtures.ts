#!/usr/bin/env npx tsx
/**
 * Bootstrap E2E Test Fixtures
 *
 * This script creates test projects as Automerge documents and saves them
 * to disk for reproducible E2E tests.
 *
 * Usage:
 *   npm run bootstrap-test-fixtures
 *
 * What it does:
 *   1. Creates Automerge documents using a local Repo with file storage
 *   2. Records document IDs in fixture-manifest.json
 *   3. Data is persisted in automerge-data/
 *
 * Output:
 *   - e2e/fixtures/automerge-data/  - Binary Automerge storage (check into git)
 *   - e2e/fixtures/fixture-manifest.json - Maps project names to document IDs
 *
 * After running, commit the fixtures directory to git for reproducible E2E tests.
 */

import { Repo } from '@automerge/automerge-repo';
import { NodeFSStorageAdapter } from '@automerge/automerge-repo-storage-nodefs';
import type { IndexDocument, TextDocumentContent } from '@quarto/quarto-automerge-schema';
import { writeFileSync, mkdirSync, rmSync, existsSync } from 'fs';
import { join, dirname } from 'path';
import { fileURLToPath } from 'url';
import { TEST_PROJECTS, type TestProjectKey } from '../fixtures/testProjects.js';

// Get directory of this script
const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

// Configuration
const FIXTURE_DIR = join(__dirname, '../fixtures');
const AUTOMERGE_DATA_DIR = join(FIXTURE_DIR, 'automerge-data');

interface FixtureManifest {
  schemaVersion: string;
  generatedAt: string;
  projects: {
    [key: string]: {
      indexDocId: string;
      description: string;
      files: string[];
    };
  };
}

/**
 * Create a test project directly in the Repo.
 */
function createTestProject(
  repo: Repo,
  projectKey: TestProjectKey
): { indexDocId: string; files: string[] } {
  const project = TEST_PROJECTS[projectKey];
  console.log(`\n  Creating project: ${projectKey} (${project.description})`);

  // Create the index document
  const indexHandle = repo.create<IndexDocument>();
  indexHandle.change((doc) => {
    doc.files = {};
  });

  const createdFiles: string[] = [];

  // Create each file document
  for (const file of project.files) {
    const fileHandle = repo.create<TextDocumentContent>();
    fileHandle.change((doc) => {
      doc.text = file.content;
    });

    // Add file to index
    indexHandle.change((doc) => {
      doc.files[file.path] = fileHandle.documentId;
    });

    console.log(`    + ${file.path}`);
    createdFiles.push(file.path);
  }

  const indexDocId = indexHandle.documentId;
  console.log(`    Created with indexDocId: ${indexDocId}`);

  return { indexDocId, files: createdFiles };
}

async function main() {
  console.log('='.repeat(60));
  console.log('E2E Fixture Bootstrap');
  console.log('='.repeat(60));

  // Clean up existing fixtures
  if (existsSync(AUTOMERGE_DATA_DIR)) {
    console.log(`\nCleaning existing fixtures: ${AUTOMERGE_DATA_DIR}`);
    rmSync(AUTOMERGE_DATA_DIR, { recursive: true, force: true });
  }

  // Ensure directories exist
  mkdirSync(AUTOMERGE_DATA_DIR, { recursive: true });

  // Create a Repo with file storage (no network needed for fixture generation)
  console.log(`\nInitializing Repo with storage at: ${AUTOMERGE_DATA_DIR}`);
  const storageAdapter = new NodeFSStorageAdapter(AUTOMERGE_DATA_DIR);

  const repo = new Repo({
    storage: storageAdapter,
    // No network - we're creating fixtures locally
  });

  const manifest: FixtureManifest = {
    schemaVersion: '0.0.1',
    generatedAt: new Date().toISOString(),
    projects: {},
  };

  // Create each test project
  console.log('\nCreating test projects...');
  const projectKeys = Object.keys(TEST_PROJECTS) as TestProjectKey[];

  for (const key of projectKeys) {
    const result = createTestProject(repo, key);
    manifest.projects[key] = {
      indexDocId: result.indexDocId,
      description: TEST_PROJECTS[key].description,
      files: result.files,
    };
  }

  // Wait for storage to flush
  // The storage adapter debounces writes, so we need to wait for them to complete
  console.log('\nWaiting for storage to flush...');
  await new Promise((r) => setTimeout(r, 2000));

  // Write manifest
  const manifestPath = join(FIXTURE_DIR, 'fixture-manifest.json');
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + '\n');

  console.log('\n' + '='.repeat(60));
  console.log('Fixtures generated successfully!');
  console.log('='.repeat(60));
  console.log(`\nManifest: ${manifestPath}`);
  console.log(`Storage:  ${AUTOMERGE_DATA_DIR}`);
  console.log('\nProjects created:');
  for (const [key, project] of Object.entries(manifest.projects)) {
    console.log(`  - ${key}: ${project.indexDocId}`);
  }

  console.log('\nNext steps:');
  console.log('  git add hub-client/e2e/fixtures/');
  console.log('  git commit -m "Add E2E test fixtures"');
}

// Run
main().catch((error) => {
  console.error('Fatal error:', error);
  process.exit(1);
});
