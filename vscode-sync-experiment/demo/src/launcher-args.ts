/** The launcher's command line (`npm run demo -- ...`) as a pure function, so the rules can be tested. */
import { parseArgs } from 'node:util';
import type { DemoOptions, Preset } from './demo';
import type { Where } from './captions';

export const PRESET_NAMES: readonly Preset[] = ['hub-browser', 'hub-vscode-host', 'hub-vscode-clone', 'vscode-share'];

export interface LauncherArgs {
  help: boolean;
  preset: Preset;
  /** Run the scripted story (`--play`; `--fast` implies it). */
  play: boolean;
  /** End the run when the story ends instead of staying up for free play. */
  exit: boolean;
  /** `--linger <ms>`, as typed; read by `endPlan`. */
  linger?: string;
  /** `--timeout <seconds>`: end the run this long after the first editor window is up. Off by default. */
  timeoutSeconds?: number;
  /** What `Demo.create` takes (through `defaultOptions`). */
  options: Partial<DemoOptions>;
}

/**
 * `--fast` and `--headless` are separate. Fast is about pace: no dwell, no typing delay, no captions, every check. Headless
 * is about windows: the browser windows are not shown. A fast run shows every window unless `--headless` is also given; the
 * tests and unattended runs ask for both.
 */
export function parseLauncherArgs(argv: string[]): LauncherArgs {
  const { values: v } = parseArgs({
    args: argv,
    options: {
      preset: { type: 'string' },
      play: { type: 'boolean' },
      pace: { type: 'string' },
      dwell: { type: 'string' },
      captions: { type: 'string' },
      narration: { type: 'string' },
      fast: { type: 'boolean' },
      headless: { type: 'boolean' },
      exit: { type: 'boolean' },
      linger: { type: 'string' },
      timeout: { type: 'string' },
      screen: { type: 'string' },
      seed: { type: 'string' },
      root: { type: 'string' },
      help: { type: 'boolean' },
    },
  });
  const preset = (v.preset ?? 'hub-browser') as Preset;
  if (!PRESET_NAMES.includes(preset)) throw new Error(`preset ${preset} is not built yet`);
  const play = !!v.play || !!v.fast;
  let timeoutSeconds: number | undefined;
  if (v.timeout !== undefined) {
    timeoutSeconds = Number(v.timeout);
    if (v.timeout.trim() === '' || !Number.isFinite(timeoutSeconds) || timeoutSeconds <= 0) throw new Error(`--timeout needs a positive number of seconds, not '${v.timeout}'`);
  }
  return {
    help: !!v.help,
    preset,
    play,
    exit: !!v.exit,
    linger: v.linger,
    timeoutSeconds,
    options: {
      preset,
      fast: !!v.fast,
      headless: !!v.headless,
      pace: (v.pace as 'auto' | 'key') ?? 'auto',
      dwellMs: v.dwell ? Number(v.dwell) : 2500,
      captions: (v.captions as Where) ?? 'inline',
      narration: (v.narration as Where) ?? 'strip',
      screen: v.screen ? Number(v.screen) : 0,
      seed: v.seed ? Number(v.seed) : 1,
      root: v.root,
      typedHub: play,
    },
  };
}
