/**
 * Typing into the demo's VS Code windows at a human pace. The demo hard-wraps what its authors type (a long unbroken line
 * runs off the pane), so every wrap must arrive as a newline between the words, with no space and no newline lost.
 *
 * Found by watching a paced run: Myra's paragraph came out as `the secondanswered with` and `seconds, andit kept`. VS Code
 * offers word completions while it is typed into, and Enter accepts an open suggestion instead of inserting a newline. A
 * `--fast` run types too quickly for the suggestion list to open, which is why only paced runs showed it.
 */
import { test, expect } from '@playwright/test';
import { mkdtempSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { launchVscode, type RunningVscode } from '../../../e2e/helpers/launch';
import { activeEditorText, openFile } from '../../../e2e/helpers/palette';
import { vscodeSettings } from '../../src/demo';

let vscode: RunningVscode | undefined;
test.afterEach(async () => {
  await vscode?.close();
  vscode = undefined;
});

// The document already holds the words the paragraph ends its lines with, so VS Code has completions to offer.
const SEED = 'We ordered a second probe, and the second sensor reads seconds apart. Answered, asked, and answering.\n';
// What the demo types: the author's text hard-wrapped at 64 columns.
const WRAPPED = [
  'When we pulsed the humidity in the first chamber, the second',
  'answered with the same pulse train, delayed by 7.3 seconds, and',
  'it kept answering for some minutes after we stopped.',
];

test('typed slowly into VS Code, hard-wrapped text keeps every newline and every space', async () => {
  const dir = mkdtempSync(path.join(os.tmpdir(), 'ge19-typing-'));
  writeFileSync(path.join(dir, 'a.qmd'), SEED);
  vscode = await launchVscode({ workspace: dir, settings: vscodeSettings('host') });
  const page = vscode.page;
  await openFile(page, 'a.qmd');
  await page.locator('.editor-instance .view-lines').first().click();
  await page.keyboard.press(process.platform === 'darwin' ? 'Meta+ArrowDown' : 'Control+End');
  await page.keyboard.press('Enter');
  await page.keyboard.type(WRAPPED.join('\n'), { delay: 70 });
  await expect.poll(() => activeEditorText(page), { timeout: 30_000 }).toContain(WRAPPED[2]!);
  const text = await activeEditorText(page);
  expect(text).toContain(WRAPPED.join('\n'));
});
