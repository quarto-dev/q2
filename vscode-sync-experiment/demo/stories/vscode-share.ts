/**
 * The scripted story for `vscode-share` (DM6): a standalone `q2 hub` started in the host terminal, Myra's VS Code window
 * sharing the folder on camera, and Maurice's terminal pasting the copied `q2 preview --hub-url` command. No tracking
 * hub is involved: the originator's VS Code window is the owner, and the guest follows through the relay. Nothing runs
 * code until Myra runs `q2 provide-hub`, which executes the paper and sends the result to every collaborator.
 */
import { expect } from '@playwright/test';
import type { Step } from '../src/driver';
import { Demo, R_PAPER, lastRowNeedle } from '../src/demo';
import { EDITS } from '../src/journal-content';
import { diskFlat, disk, shows, showsNear, until } from './hub';

/** Myra saves in VS Code, over and over, until her file on disk holds every needle (VS Code writes only on save). */
async function saveHostUntil(d: Demo, ...needles: string[]): Promise<void> {
  const page = d.vscode.host!.page;
  await until('the edits reached the saved file', async () => {
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+S' : 'Control+S');
    await new Promise((r) => setTimeout(r, 700));
    const text = diskFlat(d, R_PAPER);
    return needles.every((n) => text.includes(n));
  });
}

export function vscodeShareStory(d: Demo): Step[] {
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
        // Her consent comes at once: she knows what she is about to run, so one key with no thinking pause.
        await d.pause(250);
        await d.typeNoWait('1', 'host', 'all');
        await d.waitPrompt(180_000);
      },
      check: async () => {
        // The result travels through the hub: no process held the project before this, so no chart showed.
        const img = d.guestEditor!.page.frameLocator('iframe').locator('img').first();
        await img.waitFor({ state: 'visible', timeout: 60_000 });
        d.record('finding', 'live-chunks-vscode-share', 'the R paper shows its chart through --hub-url once a provider runs it', { seen: true });
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
      id: 'switch-telepathy',
      caption: 'Myra remembers something.',
      command: 'git switch hyphal-telepathy',
      check: async () => {
        await d.scrollTop('guest');
        await shows(d, 'host', 'After reanalysis');
        await shows(d, 'guest', 'After reanalysis');
        // The abstract is always on screen; the speculative paragraphs lower down may be below the fold of a short window.
        await shows(d, 'host', 'keeping each other informed');
        await shows(d, 'guest', 'keeping each other informed');
        await until('the guest saw the swap notice', async () => (await d.notices('guest')) === 1);
        await until('the corrigendum appeared on disk', () => disk(d, 'celia-2026-corrigendum-variance-components.qmd').length > 0);
      },
    },
    {
      id: 'telepathy-myra',
      caption: 'Myra adds what the sealed chambers did last night to the paragraph about anticipation.',
      action: async () => {
        const typing = d.appendToParagraph('host', EDITS.echoEnd, EDITS.echoMyra);
        // Maurice sees her words begin to arrive and scrolls to them, once, while she is still typing.
        await d.pause(3000);
        await d.scrollToRow('guest', EDITS.echoEnd);
        await typing;
      },
      check: async () => {
        await showsNear(d, 'host', EDITS.echoEnd, EDITS.echoMyraNeedle);
        // He is already there, and reads all of it before he types his own.
        await showsNear(d, 'guest', EDITS.echoEnd, EDITS.echoMyraNeedle);
        await showsNear(d, 'guest', EDITS.echoEnd, lastRowNeedle(EDITS.echoMyra));
      },
    },
    {
      id: 'telepathy-maurice',
      caption: 'Maurice adds a sentence of his own: he recorded the echo too.',
      action: async () => {
        await d.appendToParagraph('guest', lastRowNeedle(EDITS.echoMyra), EDITS.echoMaurice);
      },
      check: async () => {
        await showsNear(d, 'guest', EDITS.echoEnd, EDITS.echoMauriceNeedle);
        await showsNear(d, 'host', EDITS.echoEnd, EDITS.echoMauriceNeedle);
        await showsNear(d, 'guest', EDITS.echoEnd, EDITS.echoMyraNeedle);
      },
    },
    {
      id: 'telepathy-save',
      caption: 'Myra saves, so both sentences reach her disk.',
      action: async () => {
        await saveHostUntil(d, EDITS.echoMyraNeedle, EDITS.echoMauriceNeedle);
      },
      check: async () => {
        expect(diskFlat(d, R_PAPER)).toContain(EDITS.echoMauriceNeedle);
      },
    },
    {
      id: 'switch-refused',
      caption: 'She tries to go back to main, but git will not: her saved edits would be overwritten.',
      command: 'git switch main',
      check: async () => {
        await d.waitOutput(/would be overwritten by checkout/, 'host', 15_000);
        expect(d.git('branch', '--show-current').trim()).toBe('hyphal-telepathy');
        // Nothing moved: both still show the branch and both sentences.
        await showsNear(d, 'host', EDITS.echoEnd, EDITS.echoMauriceNeedle);
        await showsNear(d, 'guest', EDITS.echoEnd, EDITS.echoMauriceNeedle);
        expect(await d.notices('guest')).toBe(1);
      },
    },
    {
      id: 'telepathy-commit',
      caption: 'She commits it, hurriedly.',
      command: `git commit -qam '${EDITS.telepathyCommit}'`,
      check: async () => {
        await until('the commit exists', () => d.git('log', '-1', '--format=%s').trim() === EDITS.telepathyCommit);
        // A commit changes nothing the readers see.
        await showsNear(d, 'host', EDITS.echoEnd, EDITS.echoMauriceNeedle);
        await showsNear(d, 'guest', EDITS.echoEnd, EDITS.echoMauriceNeedle);
      },
    },
    {
      id: 'switch-main',
      caption: 'Now git lets her go back to main. The speculation stays on its branch, and the two are on main together.',
      command: 'git switch main',
      check: async () => {
        // The swap replaced both documents: each reader starts again from the top.
        await d.scrollTop('host');
        await d.scrollTop('guest');
        for (const who of ['host', 'guest'] as const) {
          await shows(d, who, 'After reanalysis', false);
          // The paragraph Maurice typed on main before the branch is the same one for both.
          await shows(d, who, EDITS.tipNeedle);
        }
        await until('the pilot study is back', () => disk(d, 'rott-mildew-2025-preliminary-fusion.qmd').length > 0);
        expect(d.git('branch', '--show-current').trim()).toBe('main');
        // Rows below the fold are not rendered, so also read the file: the speculation is on its branch, not here.
        expect(diskFlat(d, R_PAPER)).not.toContain(EDITS.echoMyraNeedle);
        expect(diskFlat(d, R_PAPER)).not.toContain(EDITS.echoMauriceNeedle);
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
    {
      id: 'maurice-replies',
      caption: 'Back on main, Maurice answers her copy edit in the browser, in a new paragraph at the end of the Introduction, and it appears in her editor.',
      action: async () => {
        // After the Introduction's last paragraph: he scrolls down to the end of it and continues from there.
        await d.typeAfterRow('guest', EDITS.introEnd, EDITS.tipReply);
      },
      check: async () => {
        await showsNear(d, 'guest', EDITS.introEnd, EDITS.tipReplyNeedle);
        await showsNear(d, 'host', EDITS.introEnd, EDITS.tipReplyNeedle);
      },
    },
    {
      id: 'myra-replies',
      caption: 'Myra answers in VS Code, in the paragraph after his, and it reaches Maurice. Each has now seen the other type after the branch round trip: they have converged.',
      action: async () => {
        await d.typeAfter('host', EDITS.tipReply, EDITS.tipAnswer);
      },
      check: async () => {
        await showsNear(d, 'host', EDITS.introEnd, EDITS.tipAnswerNeedle);
        await showsNear(d, 'guest', EDITS.introEnd, EDITS.tipAnswerNeedle);
        // Both directions in the one file: Maurice's reply is still there beside it.
        await showsNear(d, 'guest', EDITS.introEnd, EDITS.tipReplyNeedle);
      },
    },
  ];
}
