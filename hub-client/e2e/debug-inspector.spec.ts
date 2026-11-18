/**
 * E2E: the in-context live inspector (`quartoDebug.openInspector()`,
 * bd-lb1cxprv) over a real project session — full pipeline: hub server
 * → Automerge sync → editor → lazy-loaded inspector panel in a second
 * React root.
 *
 * The `quartoDebug` gate is enabled via localStorage BEFORE the app
 * boots (prod bundles only install the API when the flag is set).
 */

import { test, expect } from '@playwright/test';
import {
  bootstrapProjectSet,
  createProjectOnServer,
  seedProjectInBrowser,
  getServerUrl,
} from './helpers/projectFactory';

declare global {
  interface Window {
    quartoDebug?: {
      openInspector(): Promise<void>;
      am: {
        repos(): { name: string }[];
        docs(): { role: string }[];
        doctor(): unknown[];
      };
    };
  }
}

test.describe('quartoDebug live inspector', () => {
  test('opens over the live repo, shows the index doc, doctor is healthy, Esc closes', async ({
    page,
  }) => {
    const serverUrl = getServerUrl();

    const indexDocId = await createProjectOnServer(serverUrl, [
      {
        path: '_quarto.yml',
        content: 'project:\n  type: default\n',
        contentType: 'text',
      },
      {
        path: 'index.qmd',
        content: '---\ntitle: Inspector E2E\n---\n\n## Hello inspector\n',
        contentType: 'text',
      },
    ]);

    // Enable the debug gate before any app code runs.
    await page.addInitScript(() => {
      localStorage.setItem('quartoDebug', '1');
    });

    await bootstrapProjectSet(page);
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);
    await page.goto(`/#/p/${localId}/file/index.qmd`);

    // Wait for the project to actually connect: the sync-client repo is
    // live AND its index doc is loaded. The repo exists as soon as connect
    // starts (connect no longer waits for a peer), while the inspector
    // seeds its viewer from the index doc once, when it opens — so a repo
    // with no index yet would open an empty inspector.
    await page.waitForFunction(
      () => {
        const am = window.quartoDebug?.am;
        if (am === undefined) return false;
        return (
          am.repos().some((r: { name: string }) => r.name === 'sync-client') &&
          am.docs().some((d: { role: string }) => d.role === 'index')
        );
      },
      undefined,
      { timeout: 30000 },
    );

    await page.evaluate(() => window.quartoDebug!.openInspector());

    const inspector = page.locator('.quarto-debug-inspector');
    await expect(inspector).toBeVisible();
    await expect(inspector.locator('h1')).toHaveText(
      'Quarto Hub — Live Inspector',
    );

    // The seeded index doc renders with its files map, proving the
    // panel reads the LIVE repo (same doc the editor is editing).
    await expect(inspector.locator('.json-content').first()).toContainText(
      'index.qmd',
      { timeout: 15000 },
    );

    // Doctor pane reports a healthy session.
    await inspector.locator('[role="tab"]', { hasText: 'Doctor' }).click();
    await expect(inspector.locator('.json-content')).toHaveText('[]');

    // Messages pane shows tap traffic from the editor's own connection.
    // Protocol ordering varies (a doc 'request' usually precedes the
    // first 'sync'), so assert presence, not position.
    await inspector.locator('[role="tab"]', { hasText: 'Messages' }).click();
    await expect(
      inspector.locator('.log-messages .type', { hasText: 'sync' }).first(),
    ).toBeVisible({ timeout: 15000 });

    // Esc closes and fully unmounts the second root.
    await page.keyboard.press('Escape');
    await expect(inspector).toHaveCount(0);
  });
});
