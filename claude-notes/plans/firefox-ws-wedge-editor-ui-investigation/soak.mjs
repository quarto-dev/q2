// bd-ug0doz5q soak: cycle `q2 preview --ui editor` servers under ONE real
// Firefox until a project fails to open, then dump the evidence.
//
// Usage (from repo root, after `npm install`):
//   node claude-notes/plans/firefox-ws-wedge-editor-ui-investigation/soak.mjs \
//     [--q2 target/debug/q2] [--cycles 40] [--tabs 3] [--close-old-tabs] \
//     [--kill-after-ms 1500] [--browser firefox|chromium] [--project <dir>]
//
// Each cycle: start a server on a fresh port, open --tabs tabs on its URL,
// require the editor to load (or the "unavailable on the sync server" error
// to appear), then kill the server. Old tabs stay open (the suspected
// aggravator: they retry the dead port) unless --close-old-tabs.
//
// Evidence per page (injected before any page script runs):
//   * every WebSocket: construct/open/close/error with timestamps, URL, and
//     how long it sat CONNECTING; a live list of sockets still CONNECTING.
// Evidence per cycle: server stdout/stderr, /health docId, time-to-open.
// On the first failure everything is printed and written to --out.
import { chromium, firefox } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtemp, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';

const args = process.argv.slice(2);
const opt = (n, d) => { const i = args.indexOf(`--${n}`); return i < 0 ? d : args[i + 1]; };
const flag = (n) => args.includes(`--${n}`);
const q2 = opt('q2', 'target/debug/q2');
const cycles = Number(opt('cycles', 40));
const tabsPerCycle = Number(opt('tabs', 3));
const killAfterMs = Number(opt('kill-after-ms', 1500));
const closeOld = flag('close-old-tabs');
const browserName = opt('browser', 'firefox');
const out = opt('out', path.join(process.env.CLAUDE_JOB_DIR ?? tmpdir(), 'soak-evidence.json'));
const OPEN_TIMEOUT_MS = Number(opt('open-timeout-ms', 45000));

const t0 = Date.now();
const log = (m) => console.log(`[+${((Date.now() - t0) / 1000).toFixed(1).padStart(6)}s] ${m}`);

const projectDir = opt('project', null) ?? await mkdtemp(path.join(tmpdir(), 'q2-soak-'));
if (!opt('project', null)) await writeFile(path.join(projectDir, 'index.qmd'), '# Soak\n\nbody\n');

function startServer() {
  const proc = spawn(q2, ['preview', '--ui', 'editor', '--no-browser', projectDir], { stdio: ['ignore', 'pipe', 'pipe'] });
  const lines = [];
  return new Promise((resolve, reject) => {
    const onData = (c) => {
      const s = c.toString(); lines.push(s);
      const m = lines.join('').match(/https?:\/\/127\.0\.0\.1:\d+\/\S*/);
      if (m && !proc.__resolved) { proc.__resolved = true; resolve({ proc, url: m[0], lines }); }
    };
    proc.stdout.on('data', onData); proc.stderr.on('data', onData);
    proc.on('exit', (c) => reject(new Error(`server exited early (${c}): ${lines.join('')}`)));
    setTimeout(() => reject(new Error('no URL printed: ' + lines.join(''))), 30000);
  });
}

// Runs in every page before app code: wrap WebSocket to record lifecycle.
const WS_SPY = () => {
  const L = (window.__wsLog = []);
  const live = (window.__wsConnecting = new Set());
  const Orig = window.WebSocket;
  let n = 0;
  window.WebSocket = function (url, protocols) {
    const id = ++n, t = performance.now();
    const ws = protocols === undefined ? new Orig(url) : new Orig(url, protocols);
    L.push({ id, ev: 'new', url: String(url), t });
    live.add(id);
    ws.addEventListener('open', () => { live.delete(id); L.push({ id, ev: 'open', after: performance.now() - t, t: performance.now() }); });
    ws.addEventListener('error', () => L.push({ id, ev: 'error', after: performance.now() - t, rs: ws.readyState, t: performance.now() }));
    ws.addEventListener('close', (e) => { live.delete(id); L.push({ id, ev: 'close', code: e.code, after: performance.now() - t, t: performance.now() }); });
    return ws;
  };
  window.WebSocket.prototype = Orig.prototype;
  Object.assign(window.WebSocket, { CONNECTING: 0, OPEN: 1, CLOSING: 2, CLOSED: 3 });
};

// macOS 27 puts ~/Library/Application Support/Firefox behind TCC and Playwright's
// bundled Firefox hangs on launch; a fresh CFFIXED_USER_HOME avoids it (same
// workaround as hub-client/playwright.firefox-env.ts, commit 1d5920d7d).
const launchEnv = process.platform === 'darwin'
  ? { ...process.env, CFFIXED_USER_HOME: await mkdtemp(path.join(tmpdir(), 'pw-firefox-home-')) }
  : process.env;
const browser = await (browserName === 'chromium' ? chromium : firefox).launch({ env: launchEnv });
const ctx = await browser.newContext();
await ctx.addInitScript(WS_SPY);

const allPages = []; // { page, cycle, tab }
let server = null;
const history = [];

async function snapshotPage({ page, cycle, tab }) {
  try {
    return { cycle, tab, closed: page.isClosed(), console: page.__console,
      bodyText: page.isClosed() ? null : (await page.evaluate(() => document.body?.innerText?.slice(0, 600))),
      connecting: page.isClosed() ? null : await page.evaluate(() => [...window.__wsConnecting]),
      wsLog: page.isClosed() ? null : await page.evaluate(() => window.__wsLog) };
  } catch (e) { return { cycle, tab, err: String(e) }; }
}

async function fail(cycle, reason, extra) {
  log(`FAIL in cycle ${cycle}: ${reason}`);
  const evidence = { cycle, reason, args: { cycles, tabsPerCycle, closeOld, browserName }, history, extra,
    pages: await Promise.all(allPages.map(snapshotPage)), serverOutput: server?.lines.join('') };
  await writeFile(out, JSON.stringify(evidence, null, 2));
  const stuck = evidence.pages.filter((p) => p.connecting?.length);
  log(`pages with sockets still CONNECTING: ${stuck.map((p) => `c${p.cycle}t${p.tab}(${p.connecting.length})`).join(', ') || 'none'}`);
  log(`evidence written to ${out}`);
  await browser.close();
  process.exit(1);
}

for (let c = 1; c <= cycles; c++) {
  if (closeOld) { for (const p of allPages) await p.page.close().catch(() => {}); allPages.length = 0; }
  server = await startServer();
  log(`cycle ${c}: server ${server.url}`);
  const port = new URL(server.url).port;
  const health = await fetch(`http://127.0.0.1:${port}/health`).then((r) => r.json()).catch((e) => ({ err: String(e) }));
  const rec = { cycle: c, url: server.url, docId: health.index_document_id, tabs: [] };
  history.push(rec);

  for (let t = 1; t <= tabsPerCycle; t++) {
    const page = await ctx.newPage();
    page.__console = [];
    page.on('console', (m) => page.__console.length < 200 && page.__console.push(`${m.type()}: ${m.text()}`.slice(0, 300)));
    page.on('pageerror', (e) => page.__console.push(`pageerror: ${e}`.slice(0, 300)));
    allPages.push({ page, cycle: c, tab: t });
    const start = Date.now();
    await page.goto(server.url).catch(() => {});
    const verdict = await page.waitForFunction(() => {
      const txt = document.body?.innerText ?? '';
      if (/unavailable on the sync server/i.test(txt)) return 'error';
      if (document.querySelector('.monaco-editor, [data-testid="editor"], .cm-editor')) return 'ok';
      return false;
    }, null, { timeout: OPEN_TIMEOUT_MS, polling: 250 }).then((h) => h.jsonValue()).catch(() => 'timeout');
    const ms = Date.now() - start;
    rec.tabs.push({ tab: t, verdict, ms });
    log(`  tab ${t}: ${verdict} in ${ms} ms`);
    if (verdict !== 'ok') await fail(c, `tab ${t} ${verdict}`, { ms, health });
  }

  await new Promise((r) => setTimeout(r, killAfterMs));
  server.proc.kill('SIGKILL');
  await new Promise((r) => server.proc.once('exit', r)).catch(() => {});
}
log(`no wedge in ${cycles} cycles`);
await writeFile(out, JSON.stringify({ result: 'no-wedge', history }, null, 2));
await browser.close();
