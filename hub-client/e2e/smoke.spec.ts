/**
 * Smoke tests for hub-client E2E infrastructure.
 *
 * Verifies the basic setup: app loads, hub server is running.
 */

import { test, expect } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { SERVER_INFO_PATH } from './helpers/globalSetup';
import type { ServerInfo } from './helpers/globalSetup';

function readServerInfo(): ServerInfo {
  return JSON.parse(readFileSync(SERVER_INFO_PATH, 'utf-8'));
}

test.describe('Smoke Tests', () => {
  test('should load the application', async ({ page }) => {
    await page.goto('/');
    await expect(page.locator('body')).toBeVisible();
    const title = await page.title();
    expect(title).toBeTruthy();
  });

  test('should have hub server running', () => {
    const info = readServerInfo();
    expect(info.url).toBeTruthy();
    expect(info.url).toMatch(/^ws:\/\/127\.0\.0\.1:\d+$/);
    expect(info.port).toBe(3030);
  });
});
