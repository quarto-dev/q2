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
  bootstrapProjectSet,
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
        // Collect console errors and HTTP failures for diagnostics
        const consoleErrors: string[] = [];
        page.on('console', (msg) => {
          if (msg.type() === 'error') {
            consoleErrors.push(msg.text());
          }
        });
        page.on('pageerror', (err) => {
          consoleErrors.push(err.message);
        });
        page.on('response', (resp) => {
          if (resp.status() >= 500) {
            consoleErrors.push(`HTTP ${resp.status()} from ${resp.url()}`);
          }
        });

        const serverUrl = getServerUrl();

        // Create Automerge project with all fixture files.
        // Sync the render target (QMD) LAST so that extension/filter files
        // are already in the VFS when the Preview component's first render fires.
        const sortedFiles = [...fixture.projectFiles].sort((a, b) => {
          const aIsTarget = a.path === fixture.renderPath ? 1 : 0;
          const bIsTarget = b.path === fixture.renderPath ? 1 : 0;
          return aIsTarget - bIsTarget;
        });
        const indexDocId = await createProjectOnServer(
          serverUrl,
          sortedFiles.map((f) => ({
            path: f.path,
            content: f.content,
            contentType: 'text' as const,
          })),
        );

        // Load in browser. bootstrapProjectSet drives the first-time-setup UI
        // so the App lands in `connected` status before we add the legacy IDB
        // project entry; otherwise the entry would trigger the
        // "Upgrade: Synced Project List" screen and block navigation.
        await bootstrapProjectSet(page, serverUrl);
        const localId = await seedProjectInBrowser(
          page,
          indexDocId,
          serverUrl,
        );

        // Navigate to the fixture file
        await page.goto(
          `/#/p/${localId}/file/${encodeURIComponent(fixture.renderPath)}`,
        );

        // Format-driven dispatch: q2-debug uses the AstIframe, everything
        // else uses the html preview iframe.
        const kind = spec.format === 'q2-debug' ? 'q2-debug' : 'html';

        // Wait for render (or error)
        if (!spec.expectsError) {
          await waitForPreviewRender(page, {
            timeout: 45000,
            consoleErrors,
            kind,
          });
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
          { kind },
        );
      });
    }
  }
});
