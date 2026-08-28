/**
 * Connection-status dialog harness route: asserts every content variant
 * the canned data is meant to exercise actually renders — status badges,
 * the inline diff's del/ins spans, the patch-list fallback with its
 * overflow line, and the connection log. axe coverage lives in
 * baseline-a11y.harness.spec.ts; this spec guards the canned-data
 * plumbing from render-logic drift.
 */

import { test, expect } from '@playwright/test';
import { bootHarness } from './helpers/harness';

test.setTimeout(60_000);

test('connection status dialog renders all content variants', async ({ page }) => {
  await bootHarness(page, 'dialog-connection-status', '.connection-status-dialog', 'light');
  const dialog = page.locator('.connection-status-dialog');

  // Status rows: browser online, WebSocket open, peer established.
  await expect(dialog.locator('.connection-status-badge.online')).toHaveCount(3);

  // Per-doc stats table: relative timestamps, not the empty state.
  await expect(dialog.locator('.connection-status-table')).toContainText('42s ago');
  await expect(dialog.locator('.connection-status-table')).not.toContainText('Never');

  // Inline diff: one deleted and one inserted span ("dog" → "cat").
  const diff = dialog.locator('.connection-status-inline-diff');
  await expect(diff.locator('.del')).toHaveCount(1);
  await expect(diff.locator('.del')).toContainText('dog');
  await expect(diff.locator('.ins')).toHaveCount(1);
  await expect(diff.locator('.ins')).toContainText('cat');

  // Patch-list fallback (project section): formatted patches plus the
  // overflow line (patchCount 24, 2 shown).
  const patchLists = dialog.locator('.connection-status-patches');
  await expect(patchLists.first()).toContainText('… and 22 more');

  // Connection log entries render.
  await expect(patchLists.last()).toContainText('ws-open');
  await expect(patchLists.last()).toContainText('peer-handshake');
});
