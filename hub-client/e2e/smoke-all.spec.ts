/**
 * Smoke-all E2E Test Runner
 *
 * Runs the smoke-all test fixtures (crates/quarto/tests/smoke-all/) through
 * the full Quarto Hub pipeline: Automerge sync → VFS → WASM render → Preview.
 *
 * Each fixture gets its own Automerge project to avoid VFS contamination.
 *
 * Run with: npx playwright test smoke-all
 */

import { test, expect } from '@playwright/test';
import {
  discoverSmokeAllTests,
  shouldSkip,
  type DiscoveredTest,
} from './helpers/smokeAllDiscovery';
import {
  createProjectOnServer,
  seedProjectInBrowser,
  getServerUrl,
} from './helpers/projectFactory';
import { waitForPreviewRender } from './helpers/previewExtraction';
import { runAssertions } from './helpers/smokeAllAssertions';

// ---------------------------------------------------------------------------
// Discovery (synchronous — runs at file evaluation time)
// ---------------------------------------------------------------------------

const allTests: DiscoveredTest[] = discoverSmokeAllTests();

// ---------------------------------------------------------------------------
// Test generation
// ---------------------------------------------------------------------------

test.describe('smoke-all E2E tests', () => {
  // Increase timeout for SASS compilation tests
  test.setTimeout(60000);

  for (const fixture of allTests) {
    const skipReason = shouldSkip(fixture.runConfig);

    for (const spec of fixture.formatSpecs) {
      const testName = `${fixture.relPath} [${spec.format}]`;

      if (skipReason) {
        test.skip(testName, () => {});
        continue;
      }

      test(testName, async ({ page }) => {
        const serverUrl = getServerUrl();

        // Create Automerge project with all fixture files
        const indexDocId = await createProjectOnServer(
          serverUrl,
          fixture.projectFiles.map((f) => ({
            path: f.path,
            content: f.content,
            contentType: 'text' as const,
          })),
        );

        // Load in browser
        await page.goto('/');
        await expect(page.locator('body')).toBeVisible();
        const localId = await seedProjectInBrowser(
          page,
          indexDocId,
          serverUrl,
        );

        // Navigate to the fixture file
        await page.goto(
          `/#/project/${localId}/file/${encodeURIComponent(fixture.renderPath)}`,
        );

        // Wait for render (or error)
        if (!spec.expectsError) {
          await waitForPreviewRender(page, { timeout: 45000 });
        } else {
          // For expected errors, wait a bit for the render attempt to complete
          await page.waitForTimeout(5000);
        }

        // Run assertions
        await runAssertions(
          page,
          fixture.renderPath,
          spec.assertions,
          spec.expectsError,
        );
      });
    }
  }
});
