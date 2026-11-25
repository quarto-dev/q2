/**
 * `npm run demo:all -w quarto-hub-vscode -- [options]`: play several stories one after another, each in its own launcher
 * process (so each one tears down its own windows, ports, and browsers), and end with a summary. The default order is
 * hub-vscode-clone, hub-browser, vscode-share. The pure parts are in `sequence.ts`.
 */
import { spawn, type ChildProcess } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { childArgs, DEFAULT_SEQUENCE, parseSequence, summarize, type StoryResult } from './sequence';

const USAGE = `usage: npm run demo:all -w quarto-hub-vscode -- [options]
  --presets a,b,c   the stories to play, in order (default ${DEFAULT_SEQUENCE.join(',')})
  --keep-going      play the remaining stories after one fails (default: stop at the first failure)
  --root <dir>      parent directory: each story gets <dir>/<preset>
  every other option goes to each launcher run, for example --fast, --dwell <ms>, --linger <ms>, --seed <n>, --screen <n>
Each story is played with --play --exit: it ends when the story ends, so no Ctrl-C is needed. Ctrl-C stops the story
that is running and the rest of the list. The exit code is 0 when every story passed, 1 otherwise.
`;

const launcher = join(dirname(fileURLToPath(import.meta.url)), '..', 'launch.mjs');

async function main(): Promise<void> {
  const argv = process.argv.slice(2);
  if (argv.includes('--help') || argv.includes('-h')) return void console.log(USAGE);
  let plan;
  try {
    plan = parseSequence(argv);
  } catch (e) {
    console.error(e instanceof Error ? e.message : e);
    console.error(USAGE);
    process.exit(2);
  }
  const results: StoryResult[] = plan.presets.map((preset) => ({ preset, code: undefined, seconds: 0 }));
  let current: ChildProcess | undefined;
  let aborted = false;
  // Ctrl-C signals the whole process group, but `kill <pid>` reaches only this process: forward it to the story that is
  // running (the launcher tears down its windows on it), and do not start the next one.
  for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP'] as const) {
    process.on(sig, () => {
      aborted = true;
      current?.kill(sig);
    });
  }
  for (const [i, preset] of plan.presets.entries()) {
    if (aborted) break;
    console.log(`\n=== ${i + 1} of ${plan.presets.length}: ${preset} ===`);
    const started = Date.now();
    const code = await new Promise<number>((resolve) => {
      current = spawn(process.execPath, [launcher, ...childArgs(preset, plan)], { stdio: 'inherit' });
      current.on('exit', (c, signal) => resolve(c ?? (signal ? 1 : 0)));
    });
    results[i] = { preset, code, seconds: (Date.now() - started) / 1000 };
    if (code !== 0 && !plan.keepGoing) break;
  }
  console.log(`\n${summarize(results)}`);
  process.exit(aborted || results.some((r) => r.code !== undefined && r.code !== 0) ? 1 : 0);
}

void main();
