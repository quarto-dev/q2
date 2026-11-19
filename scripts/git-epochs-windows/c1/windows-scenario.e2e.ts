/**
 * GE-17 item c1 (W9): the scripted end-to-end scenario of the git epochs epic, against the real `q2` binary, real
 * `git`, and the extension core's headless peers. Temporary tooling for the Windows run; it never lands on
 * `feature/git-epochs`. Runs on macOS and Linux too (that is how the logic was checked before the Windows run).
 *
 * Every step asserts and records what it saw in a transcript (`C1_OUT`, default `c1/results/c1-transcript.json`). The
 * test fails if any step fails. Step 9 is its own test (a second run, with a hub stopped during a `git switch`).
 *
 * Env: `Q2_BINARY` (the q2 to drive), `C1_FILES` (tracked bulk files, default 4000), `C1_DIFFERING` (bulk files the
 * `feature` branch rewrites, default 800), `C1_OUT` (transcript path).
 */
import { execFileSync, spawn, spawnSync, type ChildProcess } from 'node:child_process';
import * as fs from 'node:fs/promises';
import { existsSync, mkdirSync, writeFileSync } from 'node:fs';
import * as net from 'node:net';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';
import { GitRepo, pngBytes, FEATURE_A, FEATURE_B, MAIN_FILES } from '../../../q2-preview-spa/e2e/helpers/gitRepo';
import { gitStatus, type GitStatusBody } from '../../../q2-preview-spa/e2e/helpers/editor';
import { Q2_BINARY } from '../../../q2-preview-spa/e2e/helpers/previewServer';
import { dumpSidecar } from '../../../vscode-sync-experiment/e2e/helpers/registry-dump';
import { ensureWasm, HeadlessPeer, readText, sleep, tempDir, waitFor } from '../../../vscode-sync-experiment/e2e/helpers/hub';

const here = path.dirname(fileURLToPath(import.meta.url));
const FILES = Number(process.env.C1_FILES ?? 4000);
const DIFFERING = Number(process.env.C1_DIFFERING ?? 800);
const OUT = process.env.C1_OUT ?? path.join(here, 'results', 'c1-transcript.json');
const MARKERS = /<<<<<<<|>>>>>>>/;

interface Step {
  step: string;
  ok: boolean;
  observed: unknown;
}
const transcript: Step[] = [];
const cleanups: (() => Promise<void> | void)[] = [];
/** Set once the hub and mirror exist: what to record when a step fails (the hub log, the gate, the decisions, a.qmd on each side). */
let diag: (() => Promise<unknown>) | undefined;

function record(step: string, observed: unknown, ok = true): void {
  transcript.push({ step, ok, observed });
  console.log(`[c1] ${ok ? 'ok  ' : 'FAIL'} ${step}: ${JSON.stringify(observed)}`);
  if (!ok) flush();
}

/** Assert and record in one place: a failed assertion records FAIL and throws. */
async function step<T>(name: string, fn: () => Promise<T>, show: (v: T) => unknown = (v) => v ?? 'ok'): Promise<T> {
  try {
    const observed = await fn();
    record(name, show(observed));
    return observed;
  } catch (e) {
    record(name, String(e instanceof Error ? e.message : e), false);
    if (diag) record(`${name} [diagnostics]`, await diag().catch((d: unknown) => `diagnostics failed: ${String(d)}`), false);
    throw e;
  }
}

function flush(extra: Record<string, unknown> = {}): void {
  mkdirSync(path.dirname(OUT), { recursive: true });
  writeFileSync(
    OUT,
    JSON.stringify({ platform: process.platform, node: process.version, q2: Q2_BINARY, files: FILES, differing: DIFFERING, ...extra, steps: transcript }, null, 2),
  );
}

test.beforeAll(async () => {
  await ensureWasm();
});
test.afterEach(async () => {
  diag = undefined;
  for (const fn of cleanups.splice(0).reverse()) await Promise.resolve(fn()).catch(() => {});
});

// --------------------------------------------------------------------------------------------------------------------
// fixtures and processes
// --------------------------------------------------------------------------------------------------------------------

const bulkPath = (i: number) => `d${String(i % 40).padStart(2, '0')}/f${String(i).padStart(4, '0')}.qmd`;
const bulkBody = (i: number, variant: string) => `---\ntitle: F${i}\n---\n\nbulk ${i} ${variant}\n${'lorem ipsum '.repeat(40)}\n`;

interface Fixture {
  repo: GitRepo;
  logoMain: Buffer;
  logoFeature: Buffer;
}

/** `main`: the GE-13 fixture plus `FILES` bulk files; `feature`: changes a.qmd and the logo, removes c.qmd, adds b.qmd, and rewrites `DIFFERING` bulk files. */
function buildLargeFixture(dir: string): Fixture {
  const repo = GitRepo.init(dir);
  const logoMain = pngBytes(10);
  const logoFeature = pngBytes(200);
  for (const [rel, content] of Object.entries(MAIN_FILES)) repo.writeFile(rel, content);
  repo.writeFile('img/logo.png', logoMain);
  for (let i = 0; i < FILES; i++) repo.writeFile(bulkPath(i), bulkBody(i, 'main'));
  repo.commitAll('main: initial');
  repo.switchCreate('feature');
  repo.writeFile('a.qmd', FEATURE_A);
  repo.writeFile('b.qmd', FEATURE_B);
  repo.writeFile('img/logo.png', logoFeature);
  for (let i = 0; i < DIFFERING; i++) repo.writeFile(bulkPath(i), bulkBody(i, 'feature'));
  execFileSync('git', ['rm', '--quiet', 'c.qmd'], { cwd: dir });
  repo.commitAll('feature: change a and logo, add b, remove c, rewrite bulk files');
  repo.switch('main');
  return { repo, logoMain, logoFeature };
}

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.once('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      const { port } = srv.address() as net.AddressInfo;
      srv.close(() => resolve(port));
    });
  });
}

interface Hub {
  proc: ChildProcess;
  port: number;
  url: string;
  wsUrl: string;
  indexDocId: string;
  log(): string;
  exited(): Promise<number | null>;
}

/** `q2 preview --git track --allow-edit --ui editor --no-browser --port N <dir>`. */
async function startHub(dir: string, dataDir: string): Promise<Hub> {
  const port = await freePort();
  const args = ['preview', '--no-browser', '--data-dir', dataDir, '--allow-edit', '--ui', 'editor', '--git', 'track', '--host', '127.0.0.1', '--port', String(port), dir];
  record('invoke', `${Q2_BINARY} ${args.join(' ')}`);
  // Not `detached`: on win32 that is DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP, a child with no console to attach to
  // (AttachConsole failed: 6) and with Ctrl+C disabled. `windowsHide` gives the child its own, hidden console, which
  // step 9's CTRL_C_EVENT helper attaches to (and refuses to use if it turns out to be the runner's console).
  const proc = spawn(Q2_BINARY, args, {
    stdio: ['ignore', 'pipe', 'pipe'],
    env: { ...process.env, RUST_LOG: process.env.RUST_LOG ?? 'warn' },
    windowsHide: true,
  });
  let log = '';
  proc.stdout?.on('data', (c: Buffer) => (log += c.toString()));
  proc.stderr?.on('data', (c: Buffer) => (log += c.toString()));
  const exited = new Promise<number | null>((r) => proc.once('exit', (code) => r(code)));
  const hub: Hub = {
    proc,
    port,
    url: `http://127.0.0.1:${port}/`,
    wsUrl: `ws://127.0.0.1:${port}/ws`,
    indexDocId: '',
    log: () => log,
    exited: () => exited,
  };
  cleanups.push(async () => {
    if (proc.exitCode === null) {
      proc.kill();
      await Promise.race([exited, sleep(5000)]);
      if (proc.exitCode === null) proc.kill('SIGKILL');
    }
  });
  hub.indexDocId = await waitFor(
    async () => {
      if (proc.exitCode !== null) throw new Error(`q2 exited early (${proc.exitCode}):\n${log}`);
      try {
        const body = (await (await fetch(new URL('health', hub.url))).json()) as { index_document_id?: string };
        return body.index_document_id;
      } catch {
        return undefined;
      }
    },
    'the hub /health',
    90_000,
  );
  return hub;
}

/** Joins the hub's index doc, retrying a few times: a failure records the attempt and the hub log tail, so "unavailable" is diagnosable. */
async function joinWithRetry<T>(hub: Hub, what: string, join: () => Promise<T>): Promise<T> {
  let last: unknown;
  for (let attempt = 1; attempt <= 4; attempt++) {
    try {
      const r = await join();
      if (attempt > 1) record(`${what}: joined on attempt ${attempt}`, { attempt });
      return r;
    } catch (e) {
      last = e;
      record(`${what}: join attempt ${attempt} failed`, { error: String(e instanceof Error ? e.message : e), hubExit: hub.proc.exitCode, hubLogTail: hub.log().slice(-2500) }, false);
      await sleep(15_000);
    }
  }
  throw last;
}

async function cloneOf(hub: Hub, hubDir: string, label: string): Promise<{ dir: string; peer: HeadlessPeer }> {
  const parent = await tempDir(`c1-${label}-`);
  cleanups.push(() => fs.rm(parent, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }));
  const dir = path.join(parent, 'repo');
  execFileSync('git', ['clone', '--quiet', '-c', 'core.autocrlf=true', hubDir, dir], { stdio: 'ignore' });
  for (const [k, v] of [['user.name', 'C1 Clone'], ['user.email', 'c1-clone@example.invalid'], ['commit.gpgsign', 'false']] as const) {
    execFileSync('git', ['config', k, v], { cwd: dir });
  }
  const peer = await joinWithRetry(hub, 'clone', () => HeadlessPeer.joinAsClone(dir, hub.wsUrl, hub.indexDocId));
  cleanups.push(() => peer.stop());
  return { dir, peer };
}

async function mirrorOf(hub: Hub, label: string): Promise<{ dir: string; peer: HeadlessPeer }> {
  const dir = await tempDir(`c1-${label}-`);
  cleanups.push(() => fs.rm(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }));
  const peer = await joinWithRetry(hub, 'mirror', () => HeadlessPeer.joinAsMirror(dir, hub.wsUrl, hub.indexDocId));
  cleanups.push(() => peer.stop());
  return { dir, peer };
}

const read = async (dir: string, rel: string) => (await readText(path.join(dir, rel))) ?? undefined;
const readBin = (dir: string, rel: string) => fs.readFile(path.join(dir, rel)).catch(() => undefined);

async function fileIs(dir: string, rel: string, want: string | ((t: string) => boolean), what: string, timeout = 120_000): Promise<void> {
  await waitFor(async () => {
    const t = await read(dir, rel);
    return t !== undefined && (typeof want === 'string' ? t === want : want(t));
  }, what, timeout);
}
const absent = (dir: string, rel: string, what: string) => waitFor(() => !existsSync(path.join(dir, rel)), what, 120_000);


/** Counts the files under `dir`, skipping `.quarto-hub` (the store). */
async function countFiles(dir: string): Promise<number> {
  let n = 0;
  const walk = async (d: string): Promise<void> => {
    for (const e of await fs.readdir(d, { withFileTypes: true }).catch(() => [])) {
      if (e.name === '.quarto-hub' || e.name === '.git') continue;
      if (e.isDirectory()) await walk(path.join(d, e.name));
      else n++;
    }
  };
  await walk(dir);
  return n;
}

/** Waits until the mirror holds `rel` with `want`, logging the file count every 15 s; on timeout the error carries the count and the hub log tail. */
async function mirrorHolds(hub: Hub, dir: string, rel: string, want: string, what: string, timeout = 480_000): Promise<void> {
  const t0 = Date.now();
  let lastLog = 0;
  try {
    await waitFor(async () => {
      if (Date.now() - lastLog > 15_000) {
        lastLog = Date.now();
        record(`progress: ${what}`, { files: await countFiles(dir), seconds: Math.round((Date.now() - t0) / 1000), hubAlive: hub.proc.exitCode === null });
      }
      return (await read(dir, rel)) === want;
    }, what, timeout);
  } catch (e) {
    const files = await countFiles(dir);
    record(`timeout: ${what}`, { files, expected: FILES, seconds: Math.round((Date.now() - t0) / 1000), hubExit: hub.proc.exitCode, hubLogTail: hub.log().slice(-1500) }, false);
    throw e;
  }
}

async function decisionsOf(hub: Hub): Promise<GitStatusBody['decisions']> {
  return (await gitStatus(hub.url)).decisions;
}
const kinds = (ds: GitStatusBody['decisions']) => ds.map((d) => d.kind);

// --------------------------------------------------------------------------------------------------------------------
// the scenario (steps 1 to 8)
// --------------------------------------------------------------------------------------------------------------------

test('c1 steps 1-8: gate, classifier, registry, flips, unlists, a binary file', async () => {
  const projectDir = await tempDir('c1-hub-');
  const dataDir = await tempDir('c1-data-');
  cleanups.push(() => fs.rm(projectDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }));
  cleanups.push(() => fs.rm(dataDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }));
  cleanups.push(() => flush());

  const fx = await step('1 generate the repo', async () => {
    const f = buildLargeFixture(projectDir);
    const n = f.repo.git('ls-files').split('\n').filter(Boolean).length;
    expect(n).toBeGreaterThanOrEqual(FILES);
    return { f, tracked: n };
  }, (r) => ({ tracked: r.tracked })).then((r) => {
    record('1 tracked files', r.tracked);
    return r.f;
  });
  const repo = fx.repo;

  const hub = await step('2 start the hub', async () => {
    const h = await startHub(projectDir, dataDir);
    const st = await gitStatus(h.url);
    expect(st.tracking).toBe(true);
    return h;
  }, (h) => ({ port: h.port, indexDocId: h.indexDocId }));

  const mirror = await mirrorOf(hub, 'mirror');
  const clone = await cloneOf(hub, projectDir, 'clone');
  diag = async () => {
    const tail = async (dir: string) => ((await read(dir, 'a.qmd')) ?? '<absent>').slice(-120);
    return {
      hubExit: hub.proc.exitCode,
      hubLogTail: hub.log().slice(-3000),
      gitStatus: await gitStatus(hub.url).catch((e: unknown) => String(e)),
      decisions: (await decisionsOf(hub).catch(() => [])).map((d) => `${d.kind}(discarded=${d.discarded})`),
      hubDiskA: await tail(projectDir),
      mirrorA: await tail(mirror.dir),
      cloneA: await tail(clone.dir),
    };
  };
  await step('3 peers attached and the clone edits a.qmd', async () => {
    await fileIs(mirror.dir, 'a.qmd', MAIN_FILES['a.qmd']!, 'the mirror holds a.qmd', 480_000);
    await mirrorHolds(hub, mirror.dir, bulkPath(FILES - 1), bulkBody(FILES - 1, 'main'), 'the mirror holds the last bulk file');
    await fs.appendFile(path.join(clone.dir, 'a.qmd'), 'CLONE-EDIT\n');
    await fileIs(projectDir, 'a.qmd', (t) => t.includes('CLONE-EDIT'), 'the edit reaches the hub disk');
    await fileIs(mirror.dir, 'a.qmd', (t) => t.includes('CLONE-EDIT'), 'the edit reaches the mirror');
    return { hubId: hub.indexDocId };
  });
  const before = await dumpSidecar(hub.wsUrl, hub.indexDocId);
  const docOf = (d: typeof before, p: string) => d.entries.filter((e) => e.path === p).map((e) => e.doc);
  /** The doc registered for `p` at the blob HEAD holds (entries are keyed by path and blob; branches keep their own). */
  const docAtHead = (d: typeof before, p: string) => d.entries.filter((e) => e.path === p && e.blob === repo.git('rev-parse', `HEAD:${p}`).trim()).map((e) => e.doc);
  const aDocMain = docOf(before, 'a.qmd');
  expect(aDocMain.length).toBe(1);
  const cDocMain = docOf(before, 'c.qmd');
  expect(cDocMain.length).toBe(1);

  await step('4 commit on main while the peer edits: no flip, the edit intact', async () => {
    await fs.appendFile(path.join(clone.dir, 'a.qmd'), 'CLONE-EDIT-2\n');
    await fileIs(projectDir, 'a.qmd', (t) => t.includes('CLONE-EDIT-2'), 'the second edit reaches the hub disk');
    repo.commitAll('commit on main while the peer edits');
    await waitFor(async () => kinds(await decisionsOf(hub)).includes('moved-forward'), 'a moved-forward decision', 60_000);
    const ds = await decisionsOf(hub);
    expect(ds.some((d) => d.kind === 'flipped' || d.kind === 'seeded')).toBe(false);
    const text = await read(clone.dir, 'a.qmd');
    expect(text).toContain('CLONE-EDIT-2');
    expect(await read(projectDir, 'a.qmd')).toContain('CLONE-EDIT-2');
    const now = await dumpSidecar(hub.wsUrl, hub.indexDocId);
    expect(docOf(now, 'a.qmd')).toEqual(aDocMain);
    return { decisions: kinds(ds), aDoc: aDocMain };
  });

  const mainA = (await read(projectDir, 'a.qmd'))!;

  await step('5 git switch feature: the gate pauses, then the flip', async () => {
    let switched = false;
    let sawLock = false;
    let sawPaused = false;
    let sawMarker: string[] = [];
    const polling = (async () => {
      const lock = repo.markerPath('index.lock');
      for (;;) {
        const lockNow = existsSync(lock);
        sawLock ||= lockNow;
        if (lockNow || sawLock) {
          const st = await gitStatus(hub.url).catch(() => undefined);
          if (st?.paused) {
            sawPaused = true;
            if (st.markers.length) sawMarker = st.markers;
          }
        }
        if (switched) return;
        await sleep(20);
      }
    })();
    const t0 = Date.now();
    const child = spawn('git', ['switch', '--quiet', 'feature'], { cwd: projectDir, stdio: 'ignore' });
    const code = await new Promise<number | null>((r) => child.once('exit', r));
    const switchMs = Date.now() - t0;
    switched = true;
    await polling;
    expect(code).toBe(0);
    await waitFor(async () => kinds(await decisionsOf(hub)).includes('flipped'), 'a flipped decision', 120_000);
    await waitFor(async () => !(await gitStatus(hub.url)).paused, 'the gate to reopen', 60_000);
    expect(sawLock, 'the poller saw index.lock').toBe(true);
    expect(sawPaused, 'the route reported paused at least once').toBe(true);

    await fileIs(mirror.dir, 'a.qmd', FEATURE_A, 'a.qmd flips');
    await fileIs(mirror.dir, 'b.qmd', FEATURE_B, 'b.qmd is listed');
    await absent(mirror.dir, 'c.qmd', 'c.qmd is unlisted');
    await waitFor(async () => (await readBin(mirror.dir, 'img/logo.png'))?.equals(fx.logoFeature), 'the binary changes');
    // the peer, still on main, keeps its docs and its disk is untouched
    expect(await read(clone.dir, 'a.qmd')).toContain('CLONE-EDIT-2');
    expect(existsSync(path.join(clone.dir, 'c.qmd'))).toBe(true);
    expect(existsSync(path.join(clone.dir, 'b.qmd'))).toBe(false);
    const dump = await dumpSidecar(hub.wsUrl, hub.indexDocId);
    expect(docAtHead(dump, 'a.qmd')).not.toEqual(aDocMain); // the feature blob has its own doc
    expect(docOf(dump, 'a.qmd')).toContain(aDocMain[0]); // and the main doc stays registered
    expect(docOf(dump, 'c.qmd')).toEqual(cDocMain); // unlisted from the index, still registered for arrival
    return { switchMs, sawLock, sawPaused, markers: sawMarker, decisions: kinds(await decisionsOf(hub)) };
  });

  await step('6 git switch main: arrival re-attaches the registered docs, disk equals the doc', async () => {
    repo.switch('main');
    await waitFor(async () => kinds(await decisionsOf(hub)).filter((k) => k === 'flipped').length >= 2, 'the second flip (arrival)', 120_000);
    await fileIs(mirror.dir, 'a.qmd', mainA, 'a.qmd is back to the main text');
    await fileIs(mirror.dir, 'c.qmd', (t) => t.length > 0, 'c.qmd is listed again');
    await absent(mirror.dir, 'b.qmd', 'b.qmd is unlisted');
    await waitFor(async () => (await readBin(mirror.dir, 'img/logo.png'))?.equals(fx.logoMain), 'the binary is back');
    await waitFor(async () => !(await gitStatus(hub.url)).paused, 'the gate to reopen', 60_000);
    const dump = await dumpSidecar(hub.wsUrl, hub.indexDocId);
    expect(docAtHead(dump, 'a.qmd')).toEqual(aDocMain); // the pointer returned to the earlier doc, no fork
    expect(docOf(dump, 'a.qmd').length).toBe(2); // main's doc and feature's doc, nothing new
    expect(docOf(dump, 'c.qmd')).toEqual(cDocMain);
    expect(await read(projectDir, 'a.qmd')).toBe(mainA); // disk equals the doc
    return { aDoc: docAtHead(dump, 'a.qmd'), decisions: kinds(await decisionsOf(hub)) };
  });

  await step('7 save and commit inside the debounce window: a commit, not a flip', async () => {
    const flipsBefore = kinds(await decisionsOf(hub)).filter((k) => k === 'flipped').length;
    await fs.writeFile(path.join(projectDir, 'a.qmd'), `${mainA}SAVED-AND-COMMITTED\n`);
    repo.commitAll('save and commit at once');
    await fileIs(mirror.dir, 'a.qmd', (t) => t.includes('SAVED-AND-COMMITTED'), 'the saved text reaches the mirror');
    await sleep(3000);
    const ds = await decisionsOf(hub);
    expect(kinds(ds).filter((k) => k === 'flipped').length).toBe(flipsBefore);
    const dump = await dumpSidecar(hub.wsUrl, hub.indexDocId);
    expect(docAtHead(dump, 'a.qmd')).toEqual(aDocMain);
    return { flipsBefore, decisions: kinds(ds) };
  });

  await step('8 git reset --hard after a dirty edit: a new doc equal to HEAD, the dirty text stays in the retired doc', async () => {
    const head = (await read(projectDir, 'a.qmd'))!;
    await fs.appendFile(path.join(projectDir, 'a.qmd'), 'DIRTY-EDIT\n');
    await fileIs(mirror.dir, 'a.qmd', (t) => t.includes('DIRTY-EDIT'), 'the dirty edit reaches the mirror');
    const countBefore = (await decisionsOf(hub)).length;
    repo.resetHard();
    await waitFor(async () => (await decisionsOf(hub)).length > countBefore, 'a decision for the reset', 120_000);
    await fileIs(mirror.dir, 'a.qmd', head, 'the mirror equals HEAD');
    expect(await read(projectDir, 'a.qmd')).toBe(head);
    const dump = await dumpSidecar(hub.wsUrl, hub.indexDocId);
    const now = docAtHead(dump, 'a.qmd');
    const fresh = now.filter((d) => d !== aDocMain[0]);
    expect(fresh.length, `one new doc at the HEAD blob (saw ${JSON.stringify(now)})`).toBe(1);
    const retired = dump.retired.filter((e) => e.path === 'a.qmd').map((e) => e.doc);
    // the dirty text stays in the old doc, which the registry still lists (retired or at its own blob entry)
    expect(retired.includes(aDocMain[0]) || docOf(dump, 'a.qmd').includes(aDocMain[0])).toBe(true);
    const last = (await decisionsOf(hub)).slice(countBefore);
    expect(last.some((d) => (d.discarded ?? 0) >= 1)).toBe(true);
    for (const f of ['a.qmd']) expect(MARKERS.test((await read(mirror.dir, f)) ?? '')).toBe(false);
    return { newDoc: fresh, atHead: now, retired, decisions: last.map((d) => `${d.kind}(discarded=${d.discarded})`) };
  });
  flush();
});

// --------------------------------------------------------------------------------------------------------------------
// step 9: stop the hub during a git switch of the large tree
// --------------------------------------------------------------------------------------------------------------------

/** Sends Ctrl+C to the hub: SIGINT on unix, CTRL_C_EVENT through ctrl-c.ps1 on Windows. Returns false when it could not. */
function sendCtrlC(proc: ChildProcess): boolean {
  if (process.platform !== 'win32') return proc.kill('SIGINT');
  const r = spawnSync('powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', path.join(here, 'ctrl-c.ps1'), '-TargetPid', String(proc.pid), '-GuardPid', String(process.pid)], { stdio: 'pipe', encoding: 'utf8' });
  record('9 ctrl-c helper', { status: r.status, stderr: (r.stderr ?? '').slice(0, 300) }, r.status === 0);
  return r.status === 0;
}

test('c1 step 9: stopping the hub during a git switch skips the flush and leaves no conflict markers', async () => {
  const projectDir = await tempDir('c1-hub9-');
  const dataDir = await tempDir('c1-data9-');
  cleanups.push(() => fs.rm(projectDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }));
  cleanups.push(() => fs.rm(dataDir, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }));
  cleanups.push(() => flush());
  const fx = buildLargeFixture(projectDir);
  const repo = fx.repo;
  const hub = await startHub(projectDir, dataDir);
  const mirror = await mirrorOf(hub, 'mirror9');
  await mirrorHolds(hub, mirror.dir, bulkPath(FILES - 1), bulkBody(FILES - 1, 'main'), 'the mirror holds every file');
  // an unflushed edit that must not be lost silently: a dirty doc on the hub's side
  await fs.appendFile(path.join(mirror.dir, 'a.qmd'), 'UNFLUSHED\n');

  let markerSeen = false;
  const child = spawn('git', ['switch', '--quiet', 'feature'], { cwd: projectDir, stdio: 'ignore' });
  const switchDone = new Promise<number | null>((r) => child.once('exit', r));
  const lock = repo.markerPath('index.lock');
  let delivered = false;
  while (child.exitCode === null) {
    if (existsSync(lock)) {
      markerSeen = true;
      delivered = sendCtrlC(hub.proc);
      break;
    }
    await sleep(5);
  }
  const switchCode = await switchDone;
  if (!delivered) {
    record('9 stop the hub during the switch', { markerSeen, delivered, note: 'not run on this machine: Ctrl+C could not be delivered (GE-3 stands in)' }, markerSeen ? true : false);
    test.skip(true, 'step 9 not run: Ctrl+C could not be delivered to the hub');
    return;
  }
  const code = await Promise.race([hub.exited(), sleep(60_000).then(() => 'timeout' as const)]);
  const log = hub.log();
  record('9 stop the hub during the switch', { markerSeen, switchCode, hubExit: code, skippedFlushLogged: /shutdown flush was skipped/.test(log) });
  expect(code, 'the hub exited on Ctrl+C').not.toBe('timeout');
  expect(switchCode).toBe(0);

  // restart on the same data dir: no doc holds conflict markers or half-written text, and the disk is the switched tree
  const hub2 = await startHub(projectDir, dataDir);
  const mirror2 = await mirrorOf(hub2, 'mirror9b');
  await fileIs(mirror2.dir, 'a.qmd', (t) => t.includes('FEATURE'), 'the restarted hub serves the switched tree', 180_000);
  const disk = (await read(projectDir, 'a.qmd'))!;
  expect(MARKERS.test(disk)).toBe(false);
  for (const f of ['a.qmd', 'b.qmd', bulkPath(0), bulkPath(FILES - 1)]) {
    const t = await read(mirror2.dir, f);
    expect(t, `${f} present on the second mirror`).toBeDefined();
    expect(MARKERS.test(t!)).toBe(false);
    expect(t, `${f} equals the working tree`).toBe(await read(projectDir, f));
  }
  record('9 restart', { skippedFlushLogged: /shutdown flush was skipped/.test(log), tail: log.slice(-400) });
  flush();
});
