/** Pure parts of the runner (`npm run demo:all`) that plays several stories one after another, each in its own launcher process. */

/** The presets the launcher knows (see `Preset` in `demo.ts`). */
export const PRESETS = ['hub-browser', 'hub-vscode-host', 'hub-vscode-clone', 'vscode-share'] as const;

/** The order the demos play in when no list is given. */
export const DEFAULT_SEQUENCE: readonly string[] = ['hub-vscode-clone', 'hub-browser', 'vscode-share'];

export interface SequencePlan {
  presets: string[];
  /** Keep playing the remaining stories after one fails. */
  keepGoing: boolean;
  /** A parent directory: each story gets `<rootParent>/<preset>`, since two runs cannot share a demo root. */
  rootParent?: string;
  /** Every other flag, untouched and in order, for each launcher run. */
  passthrough: string[];
}

/** Take the runner's own flags (`--presets`, `--keep-going`, `--root`) out of `argv`; the rest goes to the launcher. */
export function parseSequence(argv: string[]): SequencePlan {
  let presets: string[] = [...DEFAULT_SEQUENCE];
  let keepGoing = false;
  let rootParent: string | undefined;
  const passthrough: string[] = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]!;
    if (a === '--keep-going') keepGoing = true;
    else if (a === '--presets' || a.startsWith('--presets=')) {
      const raw = a === '--presets' ? (argv[++i] ?? '') : a.slice('--presets='.length);
      presets = raw.split(',').map((p) => p.trim()).filter(Boolean);
    } else if (a === '--root' || a.startsWith('--root=')) {
      rootParent = a === '--root' ? argv[++i] : a.slice('--root='.length);
    } else if (a === '--preset' || a.startsWith('--preset=')) {
      throw new Error('--preset runs one story with `npm run demo`; to choose the list here, use --presets a,b,c');
    } else passthrough.push(a);
  }
  if (presets.length === 0) throw new Error('--presets needs at least one preset');
  const unknown = presets.filter((p) => !(PRESETS as readonly string[]).includes(p));
  if (unknown.length > 0) throw new Error(`unknown preset ${unknown.join(', ')}; the presets are ${PRESETS.join(', ')}`);
  return { presets, keepGoing, rootParent, passthrough };
}

/** The launcher's arguments for one story: play it, end when it ends, then the pass-through flags. */
export function childArgs(preset: string, plan: SequencePlan): string[] {
  const args = ['--preset', preset, '--play', '--exit', ...plan.passthrough];
  if (plan.rootParent !== undefined) args.push('--root', `${plan.rootParent.replace(/\/+$/, '')}/${preset}`);
  return args;
}

export function formatDuration(seconds: number): string {
  const s = Math.round(seconds);
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m${String(s % 60).padStart(2, '0')}s`;
}

export interface StoryResult {
  preset: string;
  /** The launcher's exit code; undefined when the story never ran. */
  code: number | undefined;
  seconds: number;
}

/** One line per story (`pass`, `FAIL` with its exit code, or `skip`) and the total time. */
export function summarize(results: StoryResult[]): string {
  const width = Math.max(...results.map((r) => r.preset.length));
  const lines = results.map((r) => {
    const name = r.preset.padEnd(width);
    if (r.code === undefined) return `skip  ${name}`;
    const status = r.code === 0 ? 'pass' : 'FAIL';
    return `${status}  ${name}  ${formatDuration(r.seconds)}${r.code === 0 ? '' : `  (exit ${r.code})`}`;
  });
  const total = results.reduce((t, r) => t + r.seconds, 0);
  return [...lines, `total  ${formatDuration(total)}`].join('\n');
}
