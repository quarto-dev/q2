/**
 * `npm run demo:style-loop -w quarto-hub-vscode -- --runs N --root <dir> [demo:all options]`: play the stories N times
 * (each run in <dir>/run-<i>/<preset>), then tabulate which previews were unstyled. Never run two at once.
 */
import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { DEFAULT_SEQUENCE } from './sequence';
import { formatLoopTable, previewHits, type LoopRun } from './style-loop';

const runAll = join(dirname(fileURLToPath(import.meta.url)), '..', 'run-all.mjs');

async function main(): Promise<void> {
  const argv = process.argv.slice(2);
  let runs = 20;
  let root = '';
  let presets: string[] = [...DEFAULT_SEQUENCE];
  const pass: string[] = [];
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]!;
    if (a === '--runs') runs = Number(argv[++i]);
    else if (a === '--root') root = argv[++i]!;
    else if (a === '--presets') {
      presets = argv[++i]!.split(',');
      pass.push('--presets', presets.join(','));
    } else pass.push(a);
  }
  if (!root) throw new Error('--root <dir> is required');
  const results: LoopRun[] = [];
  let child: ChildProcess | undefined;
  let aborted = false;
  for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP'] as const) process.on(sig, () => ((aborted = true), child?.kill(sig)));
  for (let i = 1; i <= runs && !aborted; i++) {
    const runRoot = join(root, `run-${i}`);
    console.log(`\n##### loop run ${i} of ${runs} -> ${runRoot}`);
    await new Promise<void>((r) => {
      child = spawn(process.execPath, [runAll, '--keep-going', ...pass, '--root', runRoot], { stdio: 'inherit' });
      child.on('exit', () => r());
    });
    for (const preset of presets) {
      const log = join(runRoot, preset, 'logs', 'events.jsonl');
      const text = existsSync(log) ? readFileSync(log, 'utf8') : '';
      const failed = text === '' || /"kind":"fail"|"result":"fail"/.test(text) || /"source":"invariant"/.test(text);
      results.push({ run: i, preset, passed: !failed, hits: previewHits(text) });
    }
  }
  console.log(`\n${formatLoopTable(results)}`);
  for (const r of results) if (r.hits.length) console.log(`run ${r.run} ${r.preset}: ${JSON.stringify(r.hits)}`);
}
void main();
