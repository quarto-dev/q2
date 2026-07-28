/**
 * Local document branches (fork / switch / merge-to-main) — prototype.
 *
 * Exercises the whole loop through the real app: fork main into a local
 * branch, edit the branch in Monaco, switch back and forth (main must be
 * untouched), then merge to main and verify the branch edit landed in the
 * shared document.
 */

import { test, expect, type Page } from '@playwright/test';
import {
  bootstrapProjectSet,
  createProjectOnServer,
  seedProjectInBrowser,
  getServerUrl,
} from './helpers/projectFactory';

const DOC_BODY = 'The original line.';

async function editorText(page: Page): Promise<string> {
  return page.locator('.monaco-editor .view-lines').innerText();
}

test.describe('BranchBar', () => {
  test('fork, edit branch, switch, merge to main', async ({ page }) => {
    const serverUrl = getServerUrl();
    const indexDocId = await createProjectOnServer(serverUrl, [
      { path: 'index.qmd', content: DOC_BODY, contentType: 'text' },
    ]);

    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);
    await page.goto(`/#/p/${localId}/file/index.qmd`);

    // Editor is up with main content, branch bar shows main active.
    await expect(page.locator('.monaco-editor .view-lines')).toContainText(
      'The original line.',
      { timeout: 30000 },
    );
    const bar = page.locator('.branch-bar');
    await expect(bar).toBeVisible();
    await expect(bar.locator('.branch-chip.active')).toHaveText('main');

    // Fork into a named branch — it becomes active.
    await bar.getByText('Fork').click();
    await bar.locator('.branch-name-input').fill('my-idea');
    await bar.locator('.branch-name-input').press('Enter');
    const branchChip = bar.locator('.branch-chip', { hasText: 'my-idea' });
    await expect(branchChip).toHaveClass(/active/);

    // Edit the branch in Monaco.
    await page.locator('.monaco-editor .view-lines').click();
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+ArrowRight' : 'End');
    await page.keyboard.type(' Branch addition.');
    await expect(page.locator('.monaco-editor .view-lines')).toContainText('Branch addition.');

    // Main is untouched: switch back, original text only.
    await bar.locator('.branch-chip', { hasText: 'main' }).click();
    await expect(page.locator('.monaco-editor .view-lines')).toContainText('The original line.');
    expect(await editorText(page)).not.toContain('Branch addition.');

    // Branch kept its edit.
    await branchChip.click();
    await expect(page.locator('.monaco-editor .view-lines')).toContainText('Branch addition.');

    // Merge to main: branch chip disappears, main is active and merged.
    await bar.getByText('Merge to main').click();
    await expect(bar.locator('.branch-chip', { hasText: 'my-idea' })).toHaveCount(0);
    await expect(bar.locator('.branch-chip.active')).toHaveText('main');
    await expect(page.locator('.monaco-editor .view-lines')).toContainText(
      'The original line. Branch addition.',
    );
  });
});
