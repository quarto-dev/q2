/**
 * The scripted hub story (DM7) for `hub-browser` and `hub-vscode-host`: four beats on one hub and one repository.
 * Each side's editor is a browser or a VS Code window (`Demo.kinds`); the checks read both through `Demo.shown`.
 */
import { readFileSync } from 'node:fs';
import path from 'node:path';
import { expect } from '@playwright/test';
import { gitStatus, pausedIndicator } from '../../../q2-preview-spa/e2e/helpers/editor';
import type { Step } from '../src/driver';
import { Demo, PY_PAPER, R_PAPER } from '../src/demo';

export const disk = (d: Demo, rel: string) => readFileSync(path.join(d.journal, rel), 'utf8');
export const T = 30_000;

export async function until(what: string, f: () => boolean | Promise<boolean>, timeout = 30_000): Promise<void> {
  await expect.poll(async () => f(), { timeout, message: what }).toBe(true);
}

/** Wait for an editor to show (or stop showing) `needle`. Monaco renders only the visible lines: needles are near the top. */
export async function shows(d: Demo, who: 'host' | 'guest', needle: string | RegExp, present = true): Promise<void> {
  await until(
    `${who} editor ${present ? 'shows' : 'does not show'} ${needle}`,
    async () => {
      const t = await d.shown(who);
      return (typeof needle === 'string' ? t.includes(needle) : needle.test(t)) === present;
    },
    T,
  );
}

export function hubStory(d: Demo): Step[] {
  let decisions = 0;
  const mark = async () => void (decisions = (await gitStatus(d.hubHttp)).decisions.length);
  const newDecision = async (kinds: string[]) =>
    (await gitStatus(d.hubHttp)).decisions.slice(decisions).some((x) => kinds.includes(x.kind));
  const guest: 'host' | 'guest' = 'guest';

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
        await shows(d, 'host', 'Tip extension');
        await shows(d, 'guest', 'Fusion spectra');
        expect((await gitStatus(d.hubHttp)).tracking).toBe(d.o.hubGit === 'track');
      },
    },
    {
      id: 'maurice-types',
      caption: 'Maurice, the guest, types into his paper.',
      action: async () => {
        await d.typeAtEnd(guest, 'Maurice adds a sentence. ');
        await mark();
      },
      check: async () => {
        await until("the hub wrote Maurice's text", () => disk(d, PY_PAPER).includes('Maurice adds a sentence.'));
      },
    },
    {
      id: 'commit',
      caption: 'Myra commits. Nothing swaps and no notice appears.',
      command: "git add -A && git commit -qm 'Spectra: a sentence from Maurice'",
      check: async () => {
        // An untracked hub (the E8 control) records no decisions.
        if (d.o.hubGit === 'track') await until('the hub moved forward', () => newDecision(['moved-forward']));
        for (const who of ['host', 'guest'] as const) expect(await d.notices(who)).toBe(0);
        await shows(d, guest, 'Maurice adds a sentence.');
      },
    },
    {
      id: 'switch-feature',
      caption: 'Myra switches to the feature branch: the guests follow.',
      command: 'git switch feature',
      action: mark,
      check: async () => {
        await shows(d, 'host', 'After reanalysis');
        await until('the guest paper holds the new code', () => disk(d, PY_PAPER).includes('0.9 *'));
        await until('the guest saw the swap notice', async () => (await d.notices(guest)) === 1);
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
        await shows(d, 'host', 'After reanalysis', false);
        await until('the guest paper holds the main code', () => disk(d, PY_PAPER).includes('0.6 *'));
        await until('the pilot study is back', () => disk(d, 'rott-mildew-2025-preliminary-fusion.qmd').length > 0);
      },
    },
    {
      id: 'guest-edit',
      caption: 'Maurice has an unsaved idea...',
      action: async () => {
        await d.typeAtEnd(guest, 'UNSAVED-IDEA ');
        await until('the hub wrote the idea', () => disk(d, PY_PAPER).includes('UNSAVED-IDEA'));
        await mark();
      },
      check: async () => shows(d, guest, 'UNSAVED-IDEA'),
    },
    {
      id: 'reset-hard',
      caption: "...and Myra runs git reset --hard. The guest's edit is discarded, and the status says so.",
      command: 'git reset --hard -q',
      check: async () => {
        await shows(d, guest, 'UNSAVED-IDEA', false);
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
        if (d.kinds.host === 'browser') await expect(pausedIndicator(d.hostEditor!.page)).toBeVisible({ timeout: T });
        await shows(d, 'host', /<<<<<<<|>>>>>>>/, false);
      },
    },
    {
      id: 'resolve',
      caption: 'Myra keeps her wording and continues the rebase: one swap.',
      command: `git checkout --theirs ${R_PAPER} && git add -A && git rebase --continue`,
      check: async () => {
        await until('the hub resumed', async () => !(await gitStatus(d.hubHttp)).paused);
        if (d.kinds.host === 'browser') await expect(pausedIndicator(d.hostEditor!.page)).toHaveCount(0, { timeout: T });
        await shows(d, 'host', 'differs reliably');
        await shows(d, 'host', /<<<<<<<|>>>>>>>/, false);
      },
    },
  ];
}
