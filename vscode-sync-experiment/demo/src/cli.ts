/** The launcher (`npm run demo -w quarto-hub-vscode -- --preset hub-browser [options]`). */
import { createInterface } from 'node:readline';
import { Demo, defaultOptions } from './demo';
import { runStory, StoryFailure, type DriverEnv } from './driver';
import { storyFor } from '../stories';
import { endPlan } from './ending';
import { parseLauncherArgs } from './launcher-args';

const USAGE = `usage: npm run demo -w quarto-hub-vscode -- [options]
  --preset hub-browser|hub-vscode-host|hub-vscode-clone|vscode-share   (default hub-browser)
  --play                 run the scripted story (default: free play)
  --pace auto|key        pacing of the story (default auto)
  --dwell <ms>           dwell in auto pace (default 2500)
  --captions inline|strip|both|off   (default inline)
  --narration inline|strip|both|off  (default strip)
  --fast                 no pacing, no captions, every check (implies --play); every window is still shown
  --headless             do not show the browser windows (unattended runs; the tests always use it)
  --exit                 end the run when the story ends instead of staying up for free play (exit code 0, or 1 if a
                         step failed), so stories can run one after another without Ctrl-C
  --linger <ms>          with --exit: how long to leave the windows up first (default 5000)
  --screen <n>           monitor to place windows on (default 0)
  --seed <n>             seed of the typing tempo (default 1); one seed repeats a recorded run
  --root <dir>           demo root directory
`;

export async function playStory(d: Demo): Promise<void> {
  const keyWaiters: (() => void)[] = [];
  d.hostTerminal.onInput(() => keyWaiters.splice(0).forEach((f) => f()));
  const env: DriverEnv = {
    caption: (t) => d.say('caption', t),
    type: (c, who) => d.type(c, who),
    fail: (t) => {
      // The tail of each terminal goes in the log, so a failed run can be read afterward.
      d.record('story', 'terminal-tail', t, { host: d.hostTerminal.output().slice(-1500), guest: d.guestTerminal?.output().slice(-1500) });
      d.say('warning', `FAILED ${t}`);
    },
    sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
    waitKey: () => new Promise<void>((r) => keyWaiters.push(r)),
    log: (e) => d.record('story', e.result, e.id, e),
  };
  await runStory(storyFor(d), env, { pace: d.o.pace, dwellMs: d.o.dwellMs, fast: d.o.fast });
}

async function main() {
  const a = parseLauncherArgs(process.argv.slice(2));
  if (a.help) return void console.log(USAGE);
  const { preset, play } = a;
  const d = await Demo.create(defaultOptions(a.options));
  let code = 0;
  try {
    await d.openHostTerminal();
    if (play && d.kinds.host === 'vscode') await d.openHostVscodeEarly();
    if (!play) {
      if (preset === 'vscode-share') {
        await d.startQhubProcess();
        await d.vscodeShareAll();
      } else {
        await d.startHubProcess();
        await d.openEditors();
      }
    }
    d.startObserver();
    console.log(`demo root: ${d.root}\nevent log: ${d.logs}/events.jsonl\nhub: ${d.hubUrl || '(started by the story)'}`);
    if (play) {
      await playStory(d);
      await d.recordFinalState();
      console.log('story finished');
    }
    const end = endPlan({ play, fast: !!a.options.fast, exit: a.exit, linger: a.linger });
    if (end.lingerMs > 0) {
      console.log(`leaving the windows up for ${Math.round(end.lingerMs / 1000)} s, then ending`);
      await new Promise((r) => setTimeout(r, end.lingerMs));
    }
    if (end.hold) {
      console.log('running; type reset and Enter to restore the starting state, Ctrl-C to stop');
      const rl = createInterface({ input: process.stdin });
      rl.on('line', (line) => {
        if (line.trim() !== 'reset') return;
        void d.reset().then(
          () => console.log('the journal has been restored to its starting state'),
          (e) => console.error('reset failed:', e),
        );
      });
      await new Promise<void>((r) => process.once('SIGINT', () => r()));
      rl.close();
    }
  } catch (e) {
    code = 1;
    console.error(e instanceof StoryFailure ? `story failed: ${e.message}` : e);
  } finally {
    await d.stop();
  }
  process.exit(code);
}

if (process.argv[1] && import.meta.url === new URL(`file://${process.argv[1]}`).href) void main();
