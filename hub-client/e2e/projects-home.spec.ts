/**
 * E2E: collections-based projects home (PR #394's default UI variant).
 *
 * The rest of the suite pins `qh-ui-variant='classic'` and keeps covering the
 * classic ProjectSelector (see seedUiVariant in helpers/projectFactory.ts);
 * this spec is the coverage for the collections home itself:
 *
 *   - boots into the home after first-time setup
 *   - creates a collection via the in-app dialog
 *   - moves a project into it through the ⋯ → "Move to collection" menu
 *   - right-clicking a project card opens the same contextual menu (bd-je3w8q39)
 *   - the per-collection sort button reorders cards (bd-je3w8q39)
 *
 * Strand: bd-cbuc8n0e. Plan:
 * claude-notes/plans/2026-07-22-e2e-classic-pin-and-projects-home-spec.md
 */

import { test, expect, type Page } from '@playwright/test';
import {
  bootstrapProjectsHome,
  createProjectOnServer,
  getServerUrl,
  seedProjectInBrowser,
} from './helpers/projectFactory';
import type {} from './helpers/testHooks';

/** Create a tiny project on the hub and land it in the synced project set. */
async function seedNamedProject(
  page: Page,
  syncServer: string,
  name: string,
): Promise<void> {
  const indexDocId = await createProjectOnServer(syncServer, [
    {
      path: 'index.qmd',
      content: `---\ntitle: ${name}\n---\n\nHello from ${name}.\n`,
      contentType: 'text',
    },
  ]);
  await seedProjectInBrowser(page, indexDocId, syncServer, name);
  // The home renders entries reactively from the synced set — the seeded
  // project must show up in the "Everything else" list without a reload.
  await expect(
    page.locator('.ph-row', { hasText: name }),
  ).toBeVisible({ timeout: 15000 });
}

/** Drive "＋ New collection" → name dialog → Create. */
async function createCollection(page: Page, name: string): Promise<void> {
  await page.getByRole('button', { name: '＋ New collection' }).click();
  await expect(page.getByRole('heading', { name: 'New collection' })).toBeVisible();
  await page.getByRole('textbox', { name: 'Name' }).fill(name);
  await page.getByRole('button', { name: 'Create', exact: true }).click();
  await expect(collectionSection(page, name)).toBeVisible();
}

/** The <section> for one collection, located by its header name. */
function collectionSection(page: Page, name: string) {
  return page
    .locator('section.ph-collection')
    .filter({ has: page.locator('.ph-collection-name', { hasText: name }) });
}

/** Move a project from "Everything else" into a collection via its ⋯ menu. */
async function moveProjectToCollection(
  page: Page,
  projectName: string,
  collectionName: string,
): Promise<void> {
  const row = page.locator('.ph-row', { hasText: projectName });
  await row.getByRole('button', { name: '⋯' }).click();
  await page.getByRole('button', { name: /^Move to collection/ }).click();
  await page
    .locator('.ph-submenu')
    .getByRole('button', { name: collectionName, exact: true })
    .click();
  await expect(
    collectionSection(page, collectionName).locator('.ph-card', { hasText: projectName }),
  ).toBeVisible({ timeout: 10000 });
}

test.describe('Collections projects home', () => {
  // Each test bootstraps a project set, creates server-side projects, and
  // drives several menus — comfortably more than the default 30s budget.
  test.setTimeout(90_000);

  test('boots into the home and organizes a project into a new collection', async ({ page }) => {
    const syncServer = getServerUrl();
    await bootstrapProjectsHome(page, syncServer);

    await seedNamedProject(page, syncServer, 'Alpha Project');
    await createCollection(page, 'E2E Shelf');
    await moveProjectToCollection(page, 'Alpha Project', 'E2E Shelf');

    // The collection header counts its one project, and the project left the
    // "Everything else" list.
    await expect(
      collectionSection(page, 'E2E Shelf').locator('.ph-collection-count'),
    ).toHaveText('1');
    await expect(page.locator('.ph-row', { hasText: 'Alpha Project' })).toHaveCount(0);
  });

  test('right-click opens the project menu; per-collection sort reorders cards', async ({ page }) => {
    const syncServer = getServerUrl();
    await bootstrapProjectsHome(page, syncServer);

    await seedNamedProject(page, syncServer, 'Beta Project');
    await seedNamedProject(page, syncServer, 'Alpha Project');
    await createCollection(page, 'Sortable');
    await moveProjectToCollection(page, 'Beta Project', 'Sortable');
    await moveProjectToCollection(page, 'Alpha Project', 'Sortable');

    const section = collectionSection(page, 'Sortable');

    // Right-click on a card opens the same contextual menu as its ⋯ button.
    await section.locator('.ph-card', { hasText: 'Alpha Project' }).click({ button: 'right' });
    const menu = section.locator('.ph-card', { hasText: 'Alpha Project' }).getByRole('menu');
    await expect(menu).toBeVisible();
    await expect(menu.getByRole('button', { name: 'Open', exact: true })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(menu).not.toBeVisible();

    // Per-collection sort: switch to A-to-Z and assert the card order. (The
    // two projects were seeded milliseconds apart, so recency order between
    // them is not asserted — A-to-Z is the deterministic check.)
    await section.getByRole('button', { name: /^Sort collection/ }).click();
    await section.getByRole('button', { name: 'A to Z', exact: true }).click();
    await expect(
      section.getByRole('button', { name: /^Sort collection \(A to Z\)/ }),
    ).toBeVisible();
    await expect(section.locator('.ph-card .ph-card-name')).toHaveText([
      'Alpha Project',
      'Beta Project',
    ]);
  });
});
