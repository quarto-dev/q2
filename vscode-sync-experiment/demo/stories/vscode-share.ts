/**
 * The scripted story for `hubless` (DM6): a standalone `q2 hub` started in the host terminal, Myra's VS Code window
 * sharing the folder on camera, and Maurice's terminal pasting the copied `q2 preview --hub-url` command. No tracking
 * hub is involved: the originator's VS Code window is the owner, and the guest follows through the relay. Nothing runs
 * code until Myra runs `q2 provide-hub`, which executes the paper and sends the result to every collaborator.
 */
import { expect } from '@playwright/test';
import type { Step } from '../src/driver';
import { Demo, R_PAPER } from '../src/demo';
import { EDITS } from '../src/journal-content';
import { diskFlat, disk, shows, until } from './hub';

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
      paste: 'all', // copied from the share dialog
      action: async () => {
        await d.waitForGuestPreview();
        await d.openGuestBrowser(d.guestPreviewUrl);
      },
      check: async () => {
        await shows(d, 'guest', 'Tip extension');
      },
    },
    {
      id: 'provide-compute',
      caption: 'Nothing here runs R yet. Myra runs q2 provide-hub on the shared project, reviews the document it will run, and accepts: the chart appears in Maurice\'s preview.',
      action: async () => {
        // Maurice scrolls to the code chunk first: the chart will appear right there, nothing there to see yet.
        await d.guestEditor!.page.frameLocator('iframe').locator('h2', { hasText: 'Results' }).first().evaluate((e) => e.scrollIntoView({ block: 'start' }));
        await d.pause(1500);
        await d.typeNoWait(`q2 provide-hub ${d.projectId} --server ${d.qhubWs} --file ${R_PAPER}`);
        await d.waitOutput(/Review it, then choose/, 'host', 90_000);
        // Her consent comes almost at once: she knows what she is about to run.
        await d.pause(600);
        await d.typeNoWait('1');
        await d.waitPrompt(180_000);
      },
      check: async () => {
        // The result travels through the hub: no process held the project before this, so no chart showed.
        const img = d.guestEditor!.page.frameLocator('iframe').locator('img').first();
        await img.waitFor({ state: 'visible', timeout: 60_000 });
        d.record('finding', 'live-chunks-hubless', 'the R paper shows its chart through --hub-url once a provider runs it', { seen: true });
      },
    },
    {
      id: 'maurice-types',
      caption: 'Maurice types in the browser: the edit appears in her editor.',
      action: () => d.typeUnder('guest', 'Introduction', EDITS.tip),
      check: async () => {
        await shows(d, 'guest', EDITS.tipNeedle);
        await shows(d, 'host', EDITS.tipNeedle);
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
          return diskFlat(d, R_PAPER).includes(EDITS.tipNeedle);
        });
      },
      check: async () => {
        expect(diskFlat(d, R_PAPER)).toContain(EDITS.tipNeedle);
      },
    },
    {
      id: 'commit',
      caption: 'Myra commits.',
      command: "git add -A && git commit -qm 'Tip extension: turgor lags humidity'",
      check: async () => {
        await until('the commit exists', () => d.git('log', '-1', '--format=%s').trim() === 'Tip extension: turgor lags humidity');
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
    {
      id: 'copy-edit',
      caption: 'Back on main, Myra rereads the Introduction: Maurice\'s paragraph and her own both open with "Tip extension". She rewrites her opening.',
      action: async () => {
        await d.replaceLead('host', EDITS.copyLine, 2, EDITS.copyLead);
        // VS Code writes the file only when it is saved.
        const page = d.vscode.host!.page;
        await until('the copy edit reached the saved file', async () => {
          await page.keyboard.press(process.platform === 'darwin' ? 'Meta+S' : 'Control+S');
          await new Promise((r) => setTimeout(r, 700));
          return diskFlat(d, R_PAPER).includes(EDITS.copyNeedle);
        });
      },
      check: async () => {
        await shows(d, 'host', EDITS.copyNeedle);
        await shows(d, 'guest', EDITS.copyNeedle);
        // Maurice's paragraph is untouched.
        expect(diskFlat(d, R_PAPER)).toContain(EDITS.tipNeedle);
      },
    },
    {
      id: 'copy-commit',
      caption: 'Myra commits the copy edit.',
      command: "git commit -qam 'Copy edit: vary the openings'",
      check: async () => {
        await until('the commit exists', () => d.git('log', '-1', '--format=%s').trim() === 'Copy edit: vary the openings');
      },
    },
  ];
}

