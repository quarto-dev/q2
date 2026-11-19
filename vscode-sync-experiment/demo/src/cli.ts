/** The launcher (`npm run demo -w quarto-hub-vscode -- --preset hub-browser [options]`). */
import { parseArgs } from 'node:util';
import { Demo, defaultOptions, type Preset } from './demo';
import { runStory, StoryFailure, type DriverEnv } from './driver';
import { storyFor } from '../stories';
import type { Where } from './captions';

const USAGE = `usage: npm run demo -w quarto-hub-vscode -- [options]
  --preset hub-browser|hub-vscode-host|hub-vscode-clone|hubless   (default hub-browser)
  --play                 run the scripted story (default: free play)
  --pace auto|key        pacing of the story (default auto)
  --dwell <ms>           dwell in auto pace (default 2500)
  --captions inline|strip|both|off   (default inline)
  --narration inline|strip|both|off  (default strip)
  --fast                 headless, no pacing, every check (implies --play)
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
  const { values: v } = parseArgs({
    options: {
      preset: { type: 'string' },
      play: { type: 'boolean' },
      pace: { type: 'string' },
      dwell: { type: 'string' },
      captions: { type: 'string' },
      narration: { type: 'string' },
      fast: { type: 'boolean' },
      screen: { type: 'string' },
      seed: { type: 'string' },
      root: { type: 'string' },
      help: { type: 'boolean' },
    },
  });
  if (v.help) return void console.log(USAGE);
  const preset = (v.preset ?? 'hub-browser') as Preset;
  if (!['hub-browser', 'hub-vscode-host', 'hub-vscode-clone', 'hubless'].includes(preset)) throw new Error(`preset ${preset} is not built yet`);
  const play = !!v.play || !!v.fast;
  const d = await Demo.create(
    defaultOptions({
      preset,
      fast: !!v.fast,
      pace: (v.pace as 'auto' | 'key') ?? 'auto',
      dwellMs: v.dwell ? Number(v.dwell) : 2500,
      captions: (v.captions as Where) ?? 'inline',
      narration: (v.narration as Where) ?? 'strip',
      screen: v.screen ? Number(v.screen) : 0,
      seed: v.seed ? Number(v.seed) : 1,
      root: v.root,
      typedHub: play,
    }),
  );
  let code = 0;
  try {
    await d.openHostTerminal();
    if (!play) {
      if (preset === 'hubless') {
        await d.startQhubProcess();
        await d.hublessAll();
      } else {
        await d.startHubProcess();
        await d.openEditors();
      }
    }
    d.startObserver();
    console.log(`demo root: ${d.root}\nevent log: ${d.logs}/events.jsonl\nhub: ${d.hubUrl || '(started by the story)'}`);
    if (play) {
      await playStory(d);
      console.log('story finished');
    }
    if (!v.fast) {
      console.log('running; Ctrl-C to stop');
      await new Promise<void>((r) => process.once('SIGINT', () => r()));
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
