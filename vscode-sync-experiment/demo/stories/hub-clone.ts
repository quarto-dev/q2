/**
 * The scripted story for `hub-vscode-clone`: a browser host on the hub, and Maurice's VS Code window on his own clone
 * with its own terminal (the H2 and H4 behavior). The hub's switches do not touch the clone; the clone's own
 * `git switch` attaches the doc for that blob and joins the hub's feature doc.
 */
import { gitStatus } from '../../../q2-preview-spa/e2e/helpers/editor';
import { expect } from '@playwright/test';
import type { Step } from '../src/driver';
import { Demo, PY_PAPER } from '../src/demo';
import { EDITS } from '../src/journal-content';
import { disk, diskFlat, shows, showsNear, until } from './hub';
import { readFileSync } from 'node:fs';
import path from 'node:path';

const cloneDisk = (d: Demo, rel: string) => readFileSync(path.join(d.guestDir, rel), 'utf8');

export function hubCloneStory(d: Demo): Step[] {
  let decisions = 0;
  const mark = async () => void (decisions = (await gitStatus(d.hubHttp)).decisions.length);
  return [
    {
      id: 'start-hub',
      caption: 'Myra starts the hub on her journal, following git. Maurice has his own clone, in VS Code.',
      command: d.o.typedHub ? d.hubCommandLine() : undefined,
      action: async () => {
        if (d.o.typedHub) {
          await d.waitForHub();
          await d.openEditors();
        }
      },
      check: async () => {
        await shows(d, 'host', 'Fusion spectra');
        await shows(d, 'guest', 'Fusion spectra');
        expect((await gitStatus(d.hubHttp)).tracking).toBe(d.o.hubGit === 'track');
      },
    },
    {
      id: 'maurice-types',
      caption: "Maurice types in his clone: the edit reaches Myra's disk.",
      action: async () => {
        await d.typeUnder('guest', 'Introduction', EDITS.maurice);
        await mark();
      },
      check: async () => {
        await until("the hub wrote Maurice's text", () => diskFlat(d, PY_PAPER).includes(EDITS.mauriceNeedle));
        await shows(d, 'host', EDITS.mauriceNeedle);
      },
    },
    {
      id: 'commit',
      caption: 'Myra commits and pushes to the team remote. Nothing swaps.',
      command: "git add -A && git commit -qm 'Spectra: motivate the periodogram' && git push -q",
      check: async () => {
        await until('the remote has her commit', () => d.git('rev-parse', 'HEAD').trim() === d.git('--git-dir', d.remoteDir, 'rev-parse', 'main').trim());
        if (d.o.hubGit === 'track') await until('the hub moved forward', async () => (await gitStatus(d.hubHttp)).decisions.slice(decisions).some((x) => x.kind === 'moved-forward'));
        expect(await d.notices('host')).toBe(0);
        await shows(d, 'guest', EDITS.mauriceNeedle);
        await shows(d, 'host', EDITS.mauriceNeedle);
      },
    },
    {
      id: 'switch-feature',
      caption: "Myra switches to feature. Her browser follows; Maurice's clone stays on main.",
      command: 'git switch feature',
      action: mark,
      check: async () => {
        await shows(d, 'host', 'After reanalysis');
        await until("Maurice's disk is untouched", () => cloneDisk(d, PY_PAPER).includes('0.6 *') && !cloneDisk(d, PY_PAPER).includes('0.9 *'));
        await shows(d, 'guest', EDITS.mauriceNeedle);
      },
    },
    {
      id: 'clone-switch',
      caption: 'Maurice switches to feature in his own terminal: his clone joins the same doc.',
      command: 'git switch feature',
      who: 'guest',
      check: async () => {
        await until("Maurice's checkout holds the feature text", () => cloneDisk(d, PY_PAPER).includes('0.9 *'));
      },
    },
    {
      id: 'clone-main',
      caption: 'Maurice goes back to main first, in his terminal.',
      command: 'git switch main',
      who: 'guest',
      check: async () => {
        await until("Maurice's checkout holds the main text", () => cloneDisk(d, PY_PAPER).includes('0.6 *'));
      },
    },
    {
      id: 'switch-main',
      caption: 'Then Myra goes back to main: her browser and the hub follow.',
      command: 'git switch main',
      action: mark,
      check: async () => {
        await shows(d, 'host', 'After reanalysis', false);
        await until('the guest paper holds the main code on the hub', () => disk(d, PY_PAPER).includes('0.6 *'));
      },
    },
    {
      id: 'myra-edits',
      caption: "Both are on main again. Myra types a reply. Maurice pulls her commit while she types, and her words reach him live.",
      action: async () => {
        // Maurice takes a little while, then pulls in his own terminal; Myra is still typing.
        // After the Introduction's last paragraph: she scrolls down to the end of it and continues from there.
        const typing = d.typeAfterRow('host', EDITS.introEnd, EDITS.myra);
        await new Promise((r) => setTimeout(r, 4000));
        await d.type('git pull -q', 'guest');
        await typing;
      },
      check: async () => {
        await until("Maurice's clone holds Myra's commit", () => cloneDisk(d, PY_PAPER).replace(/\s+/g, ' ').includes(EDITS.mauriceNeedle));
        await until("the hub wrote Myra's text", () => diskFlat(d, PY_PAPER).includes(EDITS.myraNeedle));
        await showsNear(d, 'guest', EDITS.introEnd, EDITS.myraNeedle);
        await showsNear(d, 'host', EDITS.introEnd, EDITS.myraNeedle);
      },
    },
    {
      id: 'maurice-replies',
      caption: 'Maurice answers from his clone. Her words reached him, and now his reach her: they have converged.',
      action: async () => {
        await d.typeAfter('guest', EDITS.myra, EDITS.mauriceReply);
      },
      check: async () => {
        await until("the hub wrote Maurice's reply", () => diskFlat(d, PY_PAPER).includes(EDITS.mauriceReplyNeedle));
        await showsNear(d, 'host', EDITS.introEnd, EDITS.mauriceReplyNeedle);
        await showsNear(d, 'guest', EDITS.introEnd, EDITS.mauriceReplyNeedle);
        // Both directions in the one file: Myra's reply is still there beside it.
        await showsNear(d, 'guest', EDITS.introEnd, EDITS.myraNeedle);
      },
    },
  ];
}
