/**
 * E2E: first-run onboarding creates the project set silently (bd-4h1hv60p).
 *
 * A brand-new browser context used to land on a "Create New Project Set"
 * form after sign-in. Now the app establishes the personal root collection
 * on its own against DEFAULT_SYNC_SERVER and goes straight to the home:
 *
 *   - the collections home's "No projects yet" empty state appears with no
 *     setup form in between, the root is connected, and a project seeded
 *     afterwards lands in it (the auto-created root is a working root);
 *   - the classic selector variant lands the same way;
 *   - a `#/link-project-set/…` boot URL still makes the *linked* set the
 *     root — the silent create must stand aside for it.
 *
 * Plan: claude-notes/plans/2026-09-15-auto-create-project-set-on-first-run.md
 */

import { test, expect } from '@playwright/test';
import {
  bootstrapProjectSet,
  bootstrapProjectsHome,
  createProjectOnServer,
  getServerUrl,
  seedProjectInBrowser,
  waitForProjectSetDocId,
} from './helpers/projectFactory';

/** A bare bs58 Automerge document id (the service reports ids unprefixed). */
const BS58_DOC_ID = /^[1-9A-HJ-NP-Za-km-z]{20,}$/;

test.describe('First run', () => {
  test.setTimeout(60_000);

  test('lands on the empty collections home with a connected, usable root', async ({ page }) => {
    await bootstrapProjectsHome(page);

    // Straight to the empty state — the old setup card is gone.
    await expect(page.getByRole('heading', { name: 'No projects yet' })).toBeVisible();
    await expect(page.getByText(/Get started by creating a new project set/i)).toHaveCount(0);
    await expect(page.locator('#setup-sync-server')).toHaveCount(0);

    const docId = await waitForProjectSetDocId(page);
    expect(docId).toMatch(BS58_DOC_ID);

    // The auto-created root accepts projects like any other.
    const syncServer = getServerUrl();
    const indexDocId = await createProjectOnServer(syncServer, [
      { path: 'index.qmd', content: '---\ntitle: First\n---\n\nHello.\n', contentType: 'text' },
    ]);
    await seedProjectInBrowser(page, indexDocId, syncServer, 'First Project');
    await expect(page.locator('.qh-row', { hasText: 'First Project' })).toBeVisible({ timeout: 15000 });
  });

  test('lands on the classic selector the same way', async ({ page }) => {
    await bootstrapProjectSet(page);
    await expect(page.locator('#setup-sync-server')).toHaveCount(0);
    expect(await waitForProjectSetDocId(page)).toMatch(BS58_DOC_ID);
  });

  test('a link-project-set boot URL makes the linked set the root', async ({ browser, page }) => {
    // Browser A: an established user whose "Link another browser…" URL we
    // reconstruct (same shape as buildProjectSetLinkUrl produces).
    await bootstrapProjectsHome(page);
    const sourceDocId = await waitForProjectSetDocId(page);
    expect(sourceDocId).toMatch(BS58_DOC_ID);
    const params = new URLSearchParams({ server: '/ws' });
    const linkUrl = `/#/link-project-set/${encodeURIComponent(sourceDocId!)}?${params.toString()}`;

    // Browser B: a fresh context opening that link must adopt A's set as its
    // root instead of auto-creating an empty one first.
    const context = await browser.newContext();
    try {
      const receiver = await context.newPage();
      await receiver.goto(linkUrl);
      await expect(receiver.getByPlaceholder('Search projects…')).toBeVisible({ timeout: 20000 });
      expect(await waitForProjectSetDocId(receiver)).toBe(sourceDocId);
    } finally {
      await context.close();
    }
  });
});
