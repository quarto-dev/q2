/**
 * E2E Test Fixture Setup Helpers
 *
 * Manages the lifecycle of Automerge fixture data for E2E tests:
 * - Copies checked-in fixtures to a temp directory before tests
 * - Cleans up temp directory after tests
 * - Provides isolation between test runs
 */

import { copyFileSync, mkdirSync, mkdtempSync, readdirSync, rmSync, statSync } from 'fs';
import { join } from 'path';
import { tmpdir } from 'os';

const FIXTURE_SOURCE_DIR = join(__dirname, '../fixtures/automerge-data');

/**
 * Copy directory recursively (simple implementation without external deps)
 */
function copyDirSync(src: string, dest: string): void {
  mkdirSync(dest, { recursive: true });

  for (const entry of readdirSync(src, { withFileTypes: true })) {
    const srcPath = join(src, entry.name);
    const destPath = join(dest, entry.name);

    if (entry.isDirectory()) {
      copyDirSync(srcPath, destPath);
    } else {
      copyFileSync(srcPath, destPath);
    }
  }
}

/**
 * Check if the fixture source directory exists and has content
 */
export function fixturesExist(): boolean {
  try {
    const stat = statSync(FIXTURE_SOURCE_DIR);
    if (!stat.isDirectory()) return false;

    const entries = readdirSync(FIXTURE_SOURCE_DIR);
    return entries.length > 0;
  } catch {
    return false;
  }
}

/**
 * Set up test fixtures by copying them to a temporary directory.
 *
 * @returns Path to the temporary directory containing the fixtures
 */
export function setupTestFixtures(): string {
  const tempDir = mkdtempSync(join(tmpdir(), 'hub-client-e2e-'));

  // Only copy if fixtures exist (they won't until Phase 3 is complete)
  if (fixturesExist()) {
    copyDirSync(FIXTURE_SOURCE_DIR, join(tempDir, 'automerge-data'));
  } else {
    // Create empty automerge-data directory for fresh starts
    mkdirSync(join(tempDir, 'automerge-data'), { recursive: true });
  }

  return tempDir;
}

/**
 * Clean up test fixtures by removing the temporary directory.
 *
 * @param tempDir Path to the temporary directory to clean up
 */
export function cleanupTestFixtures(tempDir: string): void {
  try {
    rmSync(tempDir, { recursive: true, force: true });
  } catch (error) {
    // Log but don't fail - temp cleanup isn't critical
    console.warn(`Warning: Failed to clean up temp directory ${tempDir}:`, error);
  }
}

/**
 * Get the path to the fixture manifest file
 */
export function getFixtureManifestPath(): string {
  return join(__dirname, '../fixtures/fixture-manifest.json');
}
