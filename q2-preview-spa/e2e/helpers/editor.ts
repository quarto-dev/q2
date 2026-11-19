/**
 * Page helpers for the `--ui editor` (hub-client embed) and viewer specs of the git-epochs scenarios (GE-13).
 * `openProject`, `treeItem`, and `editorText` are the ones `embed-import.spec.ts` defines inline (that spec is not
 * edited); the rest are new.
 */

import { expect, type Locator, type Page } from '@playwright/test';

/** A file or folder row; a folder's accessible name starts with its disclosure arrow. */
export const treeItem = (page: Page, name: string | RegExp): Locator =>
  page
    .getByRole('treeitem', {
      name: typeof name === 'string' ? new RegExp(`^(?:[▶▼]\\s*)?${name.replace(/\./g, '\\.')}`) : name,
    })
    .first();

export const editorText = (page: Page): Locator => page.locator('.monaco-editor .view-lines').first();

/** Open the editor at `url` and wait for the file tree to show `file` (the first load boots the WASM). */
export async function openProject(page: Page, url: string, file = 'index.qmd'): Promise<void> {
  await page.goto(url);
  await expect(treeItem(page, file)).toBeVisible({ timeout: 90_000 });
}

/** Open a file in the editor by its tree row and wait for the editor to mount. */
export async function openFile(page: Page, name: string): Promise<void> {
  await treeItem(page, name).click();
  await expect(page.locator('.monaco-editor').first()).toBeVisible({ timeout: 30_000 });
}

/** Put the caret at the end of the document and type `text` (Monaco takes real key events). */
export async function typeInEditor(page: Page, text: string): Promise<void> {
  await page.locator('.monaco-editor .view-lines').first().click();
  await page.keyboard.press('ControlOrMeta+End');
  await page.keyboard.type(text);
}

/** The aggregate replaced-file notice (`role="status"`, GE-7). Present only while a notice is showing. */
export const noticeLocator = (page: Page): Locator => page.getByTestId('swap-notice');

/** The paused-for-git indicator: the preview SPA's banner, or hub-client's `PausedForGitBanner`. */
export const pausedIndicator = (page: Page): Locator =>
  page.locator('[data-testid="paused-banner"], .paused-for-git-banner');

/** The text Monaco shows now, with its non-breaking spaces normalized. */
export async function shownText(page: Page): Promise<string> {
  return (await editorText(page).innerText()).replace(/ /g, ' ');
}

/** The rendered preview inside the viewer's iframe(s): its `body` text, or '' while nothing is mounted. */
export async function previewBodyText(page: Page): Promise<string> {
  for (const frame of page.frames()) {
    if (frame === page.mainFrame()) continue;
    const text = await frame.locator('body').innerText().catch(() => '');
    if (text.trim() !== '') return text;
  }
  return '';
}

export interface GitStatusBody {
  tracking: boolean;
  paused: boolean;
  pending: boolean;
  markers: string[];
  decisions: { kind: string; time_ms: number; files: string[]; step: string | null; discarded: number | null }[];
}

/** `GET /api/preview/git-status` on the hub at `baseUrl`. */
export async function gitStatus(baseUrl: string): Promise<GitStatusBody> {
  const res = await fetch(new URL('api/preview/git-status', baseUrl));
  return (await res.json()) as GitStatusBody;
}
