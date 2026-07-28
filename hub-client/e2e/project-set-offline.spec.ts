/**
 * E2E regression for #405: a fresh browser can create its empty personal
 * project set while the configured sync WebSocket is unavailable. The empty
 * root must survive a reload from IndexedDB and retain the configured URL so
 * the reconnecting adapter can sync it when that server becomes reachable.
 */

import { expect, test } from '@playwright/test';
import { seedUiVariant } from './helpers/projectFactory';
import type {} from './helpers/testHooks';

const OFFLINE_SYNC_SERVER = 'ws://127.0.0.1:65534/offline-project-set';

test('fresh setup creates and reloads an empty personal root while sync is offline', async ({
  page,
}) => {
  await page.route('/auth/me', (route) =>
    route.fulfill({
      status: 401,
      contentType: 'application/json',
      body: '{"error":"unauthorized"}',
    }),
  );
  await seedUiVariant(page, 'collections');
  await page.goto('/');

  await expect(
    page.getByText(/Get started by creating a new project set/i),
  ).toBeVisible();
  await page.locator('#setup-sync-server').fill(OFFLINE_SYNC_SERVER);
  await page.getByRole('button', { name: /Create New Project Set/i }).click();

  await expect(page.getByPlaceholder('Search projects…')).toBeVisible({
    timeout: 5000,
  });

  const firstRoot = await page.evaluate(async () => {
    await window.__quartoTestReady;
    const root = window.__quartoTest?.projectSet.listCollections()[0];
    return (
      root && {
        docId: root.docId,
        syncServer: root.syncServer,
        entries: root.entries,
      }
    );
  });
  expect(firstRoot).toEqual({
    docId: expect.any(String),
    syncServer: OFFLINE_SYNC_SERVER,
    entries: [],
  });

  await page.reload();
  await expect(page.getByPlaceholder('Search projects…')).toBeVisible({
    timeout: 5000,
  });

  const reloadedRoot = await page.evaluate(async () => {
    await window.__quartoTestReady;
    const root = window.__quartoTest?.projectSet.listCollections()[0];
    return (
      root && {
        docId: root.docId,
        syncServer: root.syncServer,
        entries: root.entries,
      }
    );
  });
  expect(reloadedRoot).toEqual(firstRoot);
});
