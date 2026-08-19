/**
 * E2E: WCAG 2.2 accessibility behaviors.
 *
 *   - Skip link is the first focusable element and moves focus to
 *     #main-content (2.4.1 Bypass Blocks)
 *   - The New File dialog exposes dialog semantics (4.1.2), contains
 *     Tab within itself and returns focus to its trigger (2.4.3), and
 *     closes on Escape (2.1.1)
 *   - ProjectsHome dialogs expose dialog semantics (4.1.2)
 *
 * Strand: bd-trkzm9rq. Plan:
 * claude-notes/plans/2026-08-19-hub-client-wcag-22.md
 */

import { test, expect } from '@playwright/test';
import {
  bootstrapProjectsHome,
  createProjectOnServer,
  getServerUrl,
  seedProjectInBrowser,
} from './helpers/projectFactory';

test.describe('accessibility (WCAG 2.2)', () => {
  // Each test bootstraps a project set and (for the dialog test) syncs a
  // project and opens the editor — more than the default 30s budget.
  test.setTimeout(90_000);

  test('skip link is first in tab order and focuses the main landmark', async ({
    page,
  }) => {
    const syncServer = getServerUrl();
    await bootstrapProjectsHome(page, syncServer);

    // Wait for the home to finish rendering so no loading screen consumes
    // the first Tab press.
    await expect(page.locator('main#main-content')).toBeVisible({ timeout: 15000 });

    await page.keyboard.press('Tab');
    const skipLink = page.locator('.skip-link');
    await expect(skipLink).toBeFocused();
    // The link becomes visible once focused (visually hidden otherwise).
    await expect(skipLink).toBeVisible();

    await page.keyboard.press('Enter');
    await expect(page.locator('main#main-content')).toBeFocused();
  });

  test('New File dialog: semantics, Tab containment, Escape, focus restore', async ({
    page,
  }) => {
    const syncServer = getServerUrl();
    await bootstrapProjectsHome(page, syncServer);

    // Seed a project and open it in the editor.
    const indexDocId = await createProjectOnServer(syncServer, [
      {
        path: 'index.qmd',
        content: '---\ntitle: A11y Project\n---\n\nHello.\n',
        contentType: 'text',
      },
    ]);
    await seedProjectInBrowser(page, indexDocId, syncServer, 'A11y Project');
    const row = page.locator('.ph-row', { hasText: 'A11y Project' });
    await expect(row).toBeVisible({ timeout: 15000 });
    await row.locator('.ph-row-name').click();

    // The editor's sidebar New-file button is the dialog trigger.
    const newFileBtn = page.locator('.new-file-btn');
    await expect(newFileBtn).toBeVisible({ timeout: 30000 });
    await newFileBtn.click();

    // 4.1.2: role="dialog", aria-modal, accessible name from the title.
    const dialog = page.getByRole('dialog', { name: 'New file' });
    await expect(dialog).toBeVisible();
    await expect(dialog).toHaveAttribute('aria-modal', 'true');
    await expect(dialog.getByRole('button', { name: 'Close' })).toBeVisible();

    // Initial focus lands in the filename input (existing behavior)...
    await expect(dialog.getByLabel('Filename:')).toBeFocused({ timeout: 5000 });

    // ...and 2.4.3: Tab never leaves the dialog — after cycling past the
    // last action it wraps back to the close button.
    for (let i = 0; i < 3; i++) {
      await page.keyboard.press('Tab');
    }
    const focusInside = await page.evaluate(
      () => document.activeElement?.closest('[role="dialog"]') !== null,
    );
    expect(focusInside).toBe(true);

    // 2.1.1: Escape closes; 2.4.3: focus returns to the trigger button.
    await page.keyboard.press('Escape');
    await expect(dialog).not.toBeVisible();
    await expect(newFileBtn).toBeFocused();
  });

  test('ProjectsHome dialogs expose dialog semantics', async ({ page }) => {
    const syncServer = getServerUrl();
    await bootstrapProjectsHome(page, syncServer);

    // The "＋ New collection" button renders in the populated home layout,
    // so seed a project to leave the empty state.
    const indexDocId = await createProjectOnServer(syncServer, [
      {
        path: 'index.qmd',
        content: '---\ntitle: Dialog Project\n---\n\nHello.\n',
        contentType: 'text',
      },
    ]);
    await seedProjectInBrowser(page, indexDocId, syncServer, 'Dialog Project');
    await expect(
      page.locator('.ph-row', { hasText: 'Dialog Project' }),
    ).toBeVisible({ timeout: 15000 });

    await page.getByRole('button', { name: '＋ New collection' }).click();
    const dialog = page.getByRole('dialog', { name: 'New collection' });
    await expect(dialog).toBeVisible();
    await expect(dialog).toHaveAttribute('aria-modal', 'true');

    // Close via its Cancel button (these dialogs close on backdrop
    // mouse-down and Cancel, not Escape).
    await dialog.getByRole('button', { name: 'Cancel' }).click();
    await expect(dialog).not.toBeVisible();
  });
});
