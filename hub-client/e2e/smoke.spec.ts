/**
 * Smoke tests for hub-client
 *
 * These tests verify the basic E2E infrastructure is working.
 * They should be the first tests to run and fail quickly if
 * the setup is broken.
 */

import { test, expect } from '@playwright/test';

test.describe('Smoke Tests', () => {
  test('should load the application', async ({ page }) => {
    await page.goto('/');

    // The app should load without errors
    // Check for the main app container or a known element
    await expect(page.locator('body')).toBeVisible();

    // The page title should be set
    const title = await page.title();
    expect(title).toBeTruthy();
  });

  test('should have sync server URL in environment', async () => {
    // This test verifies the global setup ran correctly
    const syncServerUrl = process.env.E2E_SYNC_SERVER_URL;
    expect(syncServerUrl).toBeTruthy();
    expect(syncServerUrl).toMatch(/^ws:\/\/localhost:\d+$/);
  });

  test('should have fixture directory in environment', async () => {
    // This test verifies the fixture setup ran correctly
    const fixtureDir = process.env.E2E_FIXTURE_DIR;
    expect(fixtureDir).toBeTruthy();
    expect(fixtureDir).toContain('hub-client-e2e-');
  });
});
