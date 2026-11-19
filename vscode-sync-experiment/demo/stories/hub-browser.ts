/** The scripted hub story (DM7) for `hub-browser`: four beats on one hub and one repository. */
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { expect } from '@playwright/test';
import { editorText, noticeLocator, pausedIndicator } from '../../../q2-preview-spa/e2e/helpers/editor';
import { gitStatus } from '../../../q2-preview-spa/e2e/helpers/editor';
import type { Step } from '../src/driver';
import { Demo, PY_PAPER, R_PAPER } from '../src/demo';

const disk = (d: Demo, rel: string) => readFileSync(path.join(d.journal, rel), 'utf8');
const T = 30_000;

/** Type at the end of the document. (Monaco on macOS ends the document with Cmd+Down; Ctrl+End does nothing there.) */
async function typeAtEnd(page: import('@playwright/test').Page, text: string): Promise<void> {
  await page.locator('.monaco-editor .view-lines').first().click();
  await page.keyboard.press(process.platform === 'darwin' ? 'Meta+ArrowDown' : 'Control+End');
  await page.keyboard.type(text);
}

async function until(what: string, f: () => boolean | Promise<boolean>, timeout = 30_000): Promise<void> {
  await expect.poll(async () => f(), { timeout, message: what }).toBe(true);
}

export function hubBrowserStory(d: Demo): Step[] {
  const host = () => d.hostEditor!.page;
  const guest = () => d.guestEditor!.page;
  let decisions = 0;
  const mark = async () => void (decisions = (await gitStatus(d.hubHttp)).decisions.length);
  const newDecision = async (kinds: string[]) =>
    (await gitStatus(d.hubHttp)).decisions.slice(decisions).some((x) => kinds.includes(x.kind));

  return [
    {
      id: 'start-hub',
      caption: 'Myra starts the hub on her journal, following git.',
      command: d.o.typedHub ? d.hubCommandLine() : undefined,
      action: async () => {
        if (d.o.typedHub) {
          await d.waitForHub();
          await d.openEditors();
        }
      },
      check: async () => {
        await expect(editorText(host())).toContainText('Tip extension', { timeout: T });
        await expect(editorText(guest())).toContainText('Fusion spectra', { timeout: T });
        expect((await gitStatus(d.hubHttp)).tracking).toBe(d.o.hubGit === 'track');
      },
    },
    {
      id: 'maurice-types',
      caption: 'Maurice, the guest, types into his paper.',
      action: async () => {
        await typeAtEnd(guest(), 'Maurice adds a sentence. ');
        await mark();
      },
      check: async () => {
        await until('the hub wrote Maurice\'s text', () => disk(d, PY_PAPER).includes('Maurice adds a sentence.'));
      },
    },
    {
      id: 'commit',
      caption: 'Myra commits. Nothing swaps and no notice appears.',
      command: "git add -A && git commit -qm 'Spectra: a sentence from Maurice'",
      check: async () => {
        // An untracked hub (the E8 control) records no decisions.
        if (d.o.hubGit === 'track') await until('the hub moved forward', () => newDecision(['moved-forward']));
        for (const p of [host(), guest()]) await expect(noticeLocator(p)).toHaveCount(0);
        await expect(editorText(guest())).toContainText('Maurice adds a sentence.');
      },
    },
    {
      id: 'switch-feature',
      caption: 'Myra switches to the feature branch: the guests follow.',
      command: 'git switch feature',
      action: mark,
      check: async () => {
        // Monaco shows only the visible lines: the abstract is at the top of the host's paper; the code is below the fold.
        await expect(editorText(host())).toContainText('After reanalysis', { timeout: T });
        await until('the guest paper holds the new code', () => disk(d, PY_PAPER).includes('0.9 *'));
        await expect(noticeLocator(guest())).toHaveCount(1, { timeout: T });
        await until('the corrigendum appeared on disk', () => disk(d, 'celia-2026-corrigendum-variance-components.qmd').length > 0);
        await until('the pilot study left the project', () => !d.git('ls-files').includes('rott-mildew'));
      },
    },
    {
      id: 'switch-main',
      caption: 'And back to main: everything reverses.',
      command: 'git switch main',
      action: mark,
      check: async () => {
        await expect(editorText(host())).not.toContainText('After reanalysis', { timeout: T });
        await until('the guest paper holds the main code', () => disk(d, PY_PAPER).includes('0.6 *'));
        await until('the pilot study is back', () => disk(d, 'rott-mildew-2025-preliminary-fusion.qmd').length > 0);
      },
    },
    {
      id: 'guest-edit',
      caption: 'Maurice has an unsaved idea...',
      action: async () => {
        await typeAtEnd(guest(), 'UNSAVED-IDEA ');
        await until('the hub wrote the idea', () => disk(d, PY_PAPER).includes('UNSAVED-IDEA'));
        await mark();
      },
      check: async () => expect(editorText(guest())).toContainText('UNSAVED-IDEA'),
    },
    {
      id: 'reset-hard',
      caption: '...and Myra runs git reset --hard. The guest\'s edit is discarded, and the status says so.',
      command: 'git reset --hard -q',
      check: async () => {
        await expect(editorText(guest())).not.toContainText('UNSAVED-IDEA', { timeout: T });
        await until('the status reports the discarded edit', async () => (await gitStatus(d.hubHttp)).decisions.slice(decisions).some((x) => (x.discarded ?? 0) >= 1));
      },
    },
    {
      id: 'rebase',
      caption: 'A rebase onto revision stops on a conflict: sync pauses.',
      command: 'git rebase revision',
      action: mark,
      check: async () => {
        await until('the hub paused', async () => (await gitStatus(d.hubHttp)).paused);
        await expect(pausedIndicator(host())).toBeVisible({ timeout: T });
        await expect(editorText(host())).not.toContainText(/<<<<<<<|>>>>>>>/);
      },
    },
    {
      id: 'resolve',
      caption: 'Myra keeps her wording and continues the rebase: one swap.',
      command: `git checkout --theirs ${R_PAPER} && git add -A && git rebase --continue`,
      check: async () => {
        await until('the hub resumed', async () => !(await gitStatus(d.hubHttp)).paused);
        await expect(pausedIndicator(host())).toHaveCount(0, { timeout: T });
        await expect(editorText(host())).toContainText('differs reliably', { timeout: T });
        await expect(editorText(host())).not.toContainText(/<<<<<<<|>>>>>>>/);
      },
    },
  ];
}
