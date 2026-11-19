import type { Page } from '@playwright/test';

/** Run a command from the command palette by its title. */
export async function runCommand(page: Page, title: string): Promise<void> {
  await page.keyboard.press('F1');
  const input = page.locator('.quick-input-widget input.input');
  await input.waitFor({ state: 'visible' });
  await input.fill(`>${title}`);
  // Wait for the list to settle on the command before accepting it.
  await page.locator('.quick-input-list .monaco-list-row', { hasText: title }).first().waitFor();
  await page.keyboard.press('Enter');
}

/** Type into the open input box (the one a command asked a question with) and accept it. */
export async function fillInputBox(page: Page, text: string): Promise<void> {
  const input = page.locator('.quick-input-widget input.input');
  await input.waitFor({ state: 'visible' });
  await input.fill(text);
  await page.keyboard.press('Enter');
}

/** Open a file in the workspace by name through Quick Open. */
export async function openFile(page: Page, name: string): Promise<void> {
  await page.keyboard.press(process.platform === 'darwin' ? 'Meta+P' : 'Control+P');
  const input = page.locator('.quick-input-widget input.input');
  await input.waitFor({ state: 'visible' });
  await input.fill(name);
  await page.locator('.quick-input-list .monaco-list-row', { hasText: name }).first().waitFor();
  await page.keyboard.press('Enter');
}

/** The text of the active editor, as rendered (lines joined with \n). */
export async function activeEditorText(page: Page): Promise<string> {
  const lines = await page.locator('.editor-instance .view-lines .view-line').allInnerTexts();
  // Monaco renders spaces as non-breaking spaces.
  return lines.map((l) => l.replace(/ /g, ' ')).join('\n');
}
