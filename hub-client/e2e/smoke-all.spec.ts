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
  DOM_ASSERTIONS_PENDING_PARITY,
  type DiscoveredTest,
} from './helpers/smokeAllDiscovery';
import {
  bootstrapProjectSet,
  createProjectOnServer,
  seedProjectInBrowser,
  getServerUrl,
} from './helpers/projectFactory';
import { waitForPreviewRender, waitForVfsFiles } from './helpers/previewExtraction';
import type { PreviewIframeKind } from './helpers/previewExtraction';
import { runAssertions } from './helpers/smokeAllAssertions';

// ---------------------------------------------------------------------------
// Discovery (synchronous — runs at file evaluation time)
// ---------------------------------------------------------------------------

const allTests: DiscoveredTest[] = discoverSmokeAllTests();

// ---------------------------------------------------------------------------
// Test generation
// ---------------------------------------------------------------------------

test.describe('smoke-all E2E tests', () => {
  // Increase timeout for SASS compilation tests. On a contended CI
  // runner (ubuntu-latest with 2 cores, 2 workers, a vite dev server,
  // the hub binary, and chromium per worker) the preview render can
  // take 30-50s for the slower fixtures. 90s leaves headroom for the
  // 75s waitForPreviewRender wait below to actually fire.
  test.setTimeout(90000);

  for (const fixture of allTests) {
    const skipReason = shouldSkip(fixture.runConfig, fixture.relPath);

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
            contentType: f.contentType,
            mimeType: f.mimeType,
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

        // Barrier: wait until every project file has synced into the VFS
        // before we wait on the render or run assertions. The files are
        // pushed to the hub as separate Automerge docs and synced down
        // concurrently; without this barrier a multi-file fixture can
        // render (and assert) before its extension/filter/partial files
        // arrive, so a {{< shortcode >}} silently fails to expand. This was
        // the dominant cause of the concentrated hard-failures on the
        // multi-file extension fixtures. Single-file fixtures clear it as
        // soon as the QMD itself syncs.
        await waitForVfsFiles(
          page,
          fixture.projectFiles.map((f) => f.path),
          { timeout: 10000, consoleErrors },
        );

        // Which iframe hub-client mounts (bd-kltzdhle): q2-debug specs use
        // the Q2DebugIframe; a document that itself declares
        // `format: q2-html-render` (or a non-html format) gets the full-DOM
        // MorphIframe; everything else — including fixtures tested under
        // `html` with no `format:` key — renders in the Q2PreviewIframe,
        // because q2-preview is the default renderer for html documents.
        // Note the decision keys on the document's OWN front matter
        // (`fixture.documentFormat`), not on the test-spec dimension
        // (`spec.format`), which is what the assertions are keyed by.
        const kind: PreviewIframeKind =
          spec.format === 'q2-debug' || fixture.documentFormat === 'q2-debug'
            ? 'q2-debug'
            : fixture.documentFormat === 'q2-html-render'
              ? 'q2-html-render'
              : 'q2-preview';

        // Wait for render (or error)
        if (!spec.expectsError) {
          await waitForPreviewRender(page, {
            timeout: 75000,
            consoleErrors,
            kind,
          });
        } else {
          // For expected errors, wait a bit for the render attempt to complete
          await page.waitForTimeout(5000);
        }

        // Run assertions. A fixture listed in DOM_ASSERTIONS_PENDING_PARITY
        // has its DOM selectors skipped in the q2-preview iframe (plan D8);
        // everything else still runs.
        const pendingParity =
          kind === 'q2-preview'
            ? DOM_ASSERTIONS_PENDING_PARITY.get(fixture.relPath)
            : undefined;
        await runAssertions(
          page,
          fixture.renderPath,
          spec.assertions,
          spec.expectsError,
          { kind, skipDomAssertionsFor: pendingParity },
        );
      });
    }
  }
});
