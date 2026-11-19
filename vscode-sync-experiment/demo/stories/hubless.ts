/**
 * The scripted story for `hubless` (DM6): a standalone `q2 hub` started in the host terminal, Myra's VS Code window
 * sharing the folder on camera, and Maurice's terminal pasting the copied `q2 preview --hub-url` command. No tracking
 * hub is involved: the originator's VS Code window is the owner, and the guest follows through the relay.
 */
import { expect } from '@playwright/test';
import type { Step } from '../src/driver';
import { Demo, R_PAPER } from '../src/demo';
import { disk, shows, until } from './hub';

export function hublessStory(d: Demo): Step[] {
  return [
    {
      id: 'start-hub',
      caption: 'Myra starts a hub on this machine: a plain sync server, no project.',
      command: () => d.qhubCommandLine(),
      action: () => d.waitForQhub(),
      check: async () => {
        expect((await fetch(`http://127.0.0.1:${d.qhubPort}/`)).status).toBeLessThan(500);
      },
    },
    {
      id: 'share-folder',
      caption: 'In VS Code she runs "Share this folder" and reads what it is about to publish.',
      action: () => d.originateStart(),
      check: async () => {
        const dialog = d.vscode.host!.page.locator('.monaco-dialog-box');
        await expect(dialog.locator('.dialog-message-text')).toHaveText(`Share this folder on ${d.qhubWs}?`);
        await expect(dialog.locator('.dialog-message-detail')).toContainText('Anyone with access to that server and the project link can read and edit it.');
      },
    },
    {
      id: 'confirm-share',
      caption: 'She confirms. The project is published and the preview command is copied.',
      action: async () => {
        await d.originateConfirm();
        const { openFile } = await import('../../e2e/helpers/palette');
        await openFile(d.vscode.host!.page, R_PAPER);
      },
      check: async () => {
        expect(d.shareCommand).toMatch(/^q2 preview --hub-url \S+ --project \S+ --ui editor --allow-edit$/);
        await shows(d, 'host', 'Tip extension');
      },
    },
    {
      id: 'guest-preview',
      caption: 'Maurice pastes the command into his terminal and opens the preview.',
      command: () => d.guestPreviewCommandLine(),
      who: 'guest',
      action: async () => {
        await d.waitForGuestPreview();
        await d.openGuestBrowser(d.guestPreviewUrl);
      },
      check: async () => {
        await shows(d, 'guest', 'Tip extension');
        // Research item 7: with no project directory behind either q2 process, do the live chunks show output?
        const img = d.guestEditor!.page.frameLocator('iframe').locator('img').first();
        const seen = await img.waitFor({ state: 'visible', timeout: 25_000 }).then(
          () => true,
          () => false,
        );
        d.record('finding', 'live-chunks-hubless', seen ? 'the R paper shows its chart through --hub-url' : 'the R paper shows no chart through --hub-url', { seen });
      },
    },
    {
      id: 'maurice-types',
      caption: 'Maurice types in the browser: the edit appears in her editor.',
      action: () => d.typeAtEnd('guest', 'Maurice adds a sentence. '),
      check: async () => {
        await shows(d, 'guest', 'Maurice adds a sentence.');
      },
    },
    {
      id: 'save',
      caption: 'Myra saves, so the edit reaches her disk (VS Code writes a file only when it is saved).',
      action: async () => {
        // The edit lands in the open buffer, not on disk, until VS Code saves it; save until it arrives.
        const page = d.vscode.host!.page;
        await until('Maurice\'s text reached the saved file', async () => {
          await page.keyboard.press(process.platform === 'darwin' ? 'Meta+S' : 'Control+S');
          await new Promise((r) => setTimeout(r, 700));
          return disk(d, R_PAPER).includes('Maurice adds a sentence.');
        });
      },
      check: async () => {
        expect(disk(d, R_PAPER)).toContain('Maurice adds a sentence.');
      },
    },
    {
      id: 'commit',
      caption: 'Myra commits.',
      command: "git add -A && git commit -qm 'A sentence from Maurice'",
      check: async () => {
        await until('the commit exists', () => d.git('log', '-1', '--format=%s').trim() === 'A sentence from Maurice');
        expect(await d.notices('guest')).toBe(0);
      },
    },
    {
      id: 'switch-feature',
      caption: 'Myra switches to the feature branch: her editor and the guest follow.',
      command: 'git switch feature',
      check: async () => {
        await d.scrollTop('guest');
        await shows(d, 'host', 'After reanalysis');
        await shows(d, 'guest', 'After reanalysis');
        await until('the guest saw the swap notice', async () => (await d.notices('guest')) === 1);
        await until('the corrigendum appeared on disk', () => disk(d, 'celia-2026-corrigendum-variance-components.qmd').length > 0);
      },
    },
    {
      id: 'switch-main',
      caption: 'And back to main.',
      command: 'git switch main',
      check: async () => {
        await d.scrollTop('guest');
        await shows(d, 'host', 'After reanalysis', false);
        await shows(d, 'guest', 'After reanalysis', false);
        await until('the pilot study is back', () => disk(d, 'rott-mildew-2025-preliminary-fusion.qmd').length > 0);
      },
    },
  ];
}

