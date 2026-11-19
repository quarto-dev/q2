/**
 * The permanent form of Gate 0 item 12 (GE-9): after git releases `index.lock`
 * at the end of a checkout of a few thousand files, how late does VS Code
 * deliver the last buffer reload? The editor-only gate trusts a 1 s quiet window
 * (G20, G32); this test records the measured gap and fails above the window,
 * which triggers the experimental fallback in G20.
 *
 * Not a candidate for the permanent subset: it fails by design when the machine
 * is slow. Run it with `npm run test:e2e:timing`; GE-17's `c2` run records the
 * Windows number.
 *
 * It measures VS Code alone (a plain git repository, no extension activity).
 */
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { expect, test } from '@playwright/test';
import { git, makeGitRepo } from '../helpers/git';
import { sleep } from '../helpers/hub';
import { launchVscode, type RunningVscode } from '../helpers/launch';
import { activeEditorText, openFile } from '../helpers/palette';

const QUIET_WINDOW_MS = 1000;
const FILE_COUNT = Number(process.env.QUARTO_HUB_TIMING_FILES) || 3000;

let vscode: RunningVscode | undefined;
test.afterEach(async () => {
  await vscode?.close();
  vscode = undefined;
});

test(`the last buffer reload arrives within ${QUIET_WINDOW_MS} ms of index.lock being released (${FILE_COUNT} files)`, async () => {
  const files: Record<string, string> = {};
  for (let i = 0; i < FILE_COUNT; i++) files[`docs/d${String(i).padStart(5, '0')}.qmd`] = `main ${i}\n`;
  files['zzz-last.qmd'] = 'last on main\n';
  const { dir } = await makeGitRepo(files);
  try {
    await git(dir, 'switch', '-q', '-c', 'side');
    for (let i = 0; i < FILE_COUNT; i++) await fs.writeFile(path.join(dir, `docs/d${String(i).padStart(5, '0')}.qmd`), `side ${i}\n`);
    await fs.writeFile(path.join(dir, 'zzz-last.qmd'), 'last on side\n');
    await git(dir, 'commit', '-q', '-a', '-m', 'side');
    await git(dir, 'switch', '-q', 'main');

    vscode = await launchVscode({ workspace: dir });
    const { page } = vscode;
    await openFile(page, 'zzz-last.qmd');
    await expect.poll(() => activeEditorText(page), { timeout: 30_000 }).toContain('last on main');
    await sleep(1500);

    // Run the checkout and watch from outside VS Code with two independent loops, so the slow
    // editor query never delays seeing the lock go away.
    const lock = path.join(dir, '.git', 'index.lock');
    let lockSeen = false;
    let releasedAt = 0;
    let reloadedAt = 0;
    let slowestQueryMs = 0;
    const started = Date.now();
    const lockLoop = (async () => {
      while (!releasedAt && Date.now() - started < 60_000) {
        const exists = await fs.stat(lock).then(() => true, () => false);
        if (exists) lockSeen = true;
        else if (lockSeen) releasedAt = Date.now();
        await sleep(5);
      }
    })();
    const textLoop = (async () => {
      while (!reloadedAt && Date.now() - started < 60_000) {
        const q0 = Date.now();
        const text = await activeEditorText(page);
        slowestQueryMs = Math.max(slowestQueryMs, Date.now() - q0);
        if (text.includes('last on side')) reloadedAt = Date.now();
        else await sleep(10);
      }
    })();
    await git(dir, 'switch', '-q', 'side');
    await Promise.all([lockLoop, textLoop]);
    if (!releasedAt) releasedAt = Date.now(); // the lock was released between two polls

    expect(reloadedAt, 'the buffer reloaded').toBeGreaterThan(0);
    const gap = Math.max(0, reloadedAt - releasedAt);
    // The reload is seen no later than one editor query after it happened, so the gap is an upper bound
    // within `slowestQueryMs`.
    const note = `lock-release to last-reload gap: ${gap} ms (${FILE_COUNT} files, ${process.platform}; slowest editor query ${slowestQueryMs} ms); window ${QUIET_WINDOW_MS} ms`;
    console.log(note);
    test.info().annotations.push({ type: 'measured-gap-ms', description: String(gap) });
    expect(gap, note).toBeLessThanOrEqual(QUIET_WINDOW_MS);
  } finally {
    await fs.rm(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }).catch(() => {});
  }
});
