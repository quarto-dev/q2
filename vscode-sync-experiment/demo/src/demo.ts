/**
 * The demo environment (DM1): one object owns the runtime directory, the hub, the terminals, the browser windows, the
 * observer, and the event log. The launcher, the scripted stories, `demo reset`, and the end-to-end tests all drive it.
 */

import { execFileSync, spawn, type ChildProcess } from 'node:child_process';
import { appendFileSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium, type Browser, type BrowserContext, type Page } from '@playwright/test';
import { routeMessage, StripBuffer, type Where } from './captions';
import { buildJournal, gitEnv } from './journal';
import { computeLayout, type Layout, type Rect } from './layout';
import { checkInvariants, type Snapshot } from './invariants';
import { deriveEvents, narrate, type DemoEvent, type StatusBody } from './observer';
import { missingToolAdvice, setupCommands } from './setup';
import { startTerminalServer, type TerminalServer } from './terminal-server';
import { terminalAssets, terminalPageHtml, STRIP_LINES } from './terminal-page';

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, '..', '..', '..');
const assets = path.join(here, '..', 'journal');
export const Q2_BINARY = process.env.Q2_BINARY ?? path.join(repoRoot, 'target', 'debug', 'q2');

export type Preset = 'hub-browser' | 'hub-vscode-host' | 'hub-vscode-clone' | 'hubless';

export interface DemoOptions {
  preset: Preset;
  captions: Where;
  narration: Where;
  pace: 'auto' | 'key';
  dwellMs: number;
  /** Headless, no pacing, no captions: the rot check. */
  fast: boolean;
  /** No visible windows and no placement (default: the value of `fast`). Captions and narration still work. */
  headless?: boolean;
  /** Start the hub with `--git <value>` (E8 uses `ignore`). */
  hubGit: 'track' | 'ignore';
  /** The hub is started by typing in the host terminal (the scripted story) instead of by the launcher. */
  typedHub: boolean;
  /** A directory to use as the demo root (default: a fresh temporary one). */
  root?: string;
  /** The monitor to place windows on (index into NSScreen.screens; default 0, the main one). */
  screen: number;
  /** Observer poll period. */
  pollMs: number;
}

export const defaultOptions = (over: Partial<DemoOptions> = {}): DemoOptions => ({
  preset: 'hub-browser',
  captions: 'inline',
  narration: 'strip',
  pace: 'auto',
  dwellMs: 2500,
  fast: false,
  hubGit: 'track',
  typedHub: false,
  screen: 0,
  pollMs: 700,
  ...over,
});

export const R_PAPER = 'celia-2026-tip-extension-lmm.qmd';
export const PY_PAPER = 'morel-2026-fusion-spectra.qmd';

const stripAnsi = (s: string) => s.replace(/\x1b\[[0-9;?]*[ -/]*[@-~]/g, '').replace(/\x1b\][^\x07]*\x07/g, '');

export function screenFrame(index: number): Rect {
  const js = `ObjC.import('AppKit'); const ss=$.NSScreen.screens; const p=ss.objectAtIndex(0).frame; const s=ss.objectAtIndex(${index}); const f=s.frame; const v=s.visibleFrame; JSON.stringify({left:v.origin.x, top:(p.size.height-(v.origin.y+v.size.height)), width:v.size.width, height:v.size.height})`;
  try {
    return JSON.parse(execFileSync('osascript', ['-l', 'JavaScript', '-e', js], { encoding: 'utf8' }).trim()) as Rect;
  } catch {
    return { left: 0, top: 30, width: 1920, height: 1250 };
  }
}

export class Demo {
  readonly root: string;
  readonly journal: string;
  readonly logs: string;
  readonly strip = new StripBuffer(STRIP_LINES);
  hostTerminal!: TerminalServer;
  hubUrl = '';
  hubHttp = '';
  projectId = '';
  browser?: Browser;
  hostEditor?: { context: BrowserContext; page: Page };
  guestEditor?: { context: BrowserContext; page: Page };
  terminalPages: { context: BrowserContext; page: Page }[] = [];
  readonly typed: string[] = [];
  readonly events: { time: string; source: string; kind: string; summary: string; details: unknown }[] = [];
  private hubProc?: ChildProcess;
  private pollTimer?: NodeJS.Timeout;
  private prevStatus: StatusBody | null = null;
  private prevFiles: string[] | undefined;
  private lastActivity = 0;
  private warned = new Set<string>();
  private rLibrary = '';
  layout!: Layout;

  private constructor(readonly o: DemoOptions) {
    this.root = o.root ?? mkdtempSync('/tmp/ge19-demo-');
    this.journal = path.join(this.root, 'journal');
    this.logs = path.join(this.root, 'logs');
  }

  /** Checks the tools, builds the runtime directory and the engine environments, starts the host terminal. */
  static async create(o: DemoOptions): Promise<Demo> {
    const d = new Demo(o);
    await d.setup();
    return d;
  }

  // ---- environment -------------------------------------------------------------------------------------------

  private which(tool: string): boolean {
    if (tool === 'q2') return existsSync(Q2_BINARY);
    const bin = tool === 'R' ? 'R' : tool;
    try {
      execFileSync('which', [bin], { stdio: 'ignore' });
      return true;
    } catch {
      return false;
    }
  }

  /** The environment of every process and terminal the demo starts (DM2, DM10). */
  env(): Record<string, string> {
    const venv = path.join(this.journal, '.venv');
    return {
      ...(gitEnv() as Record<string, string>),
      GIT_PAGER: 'cat',
      R_LIBS_USER: this.rLibrary,
      VIRTUAL_ENV: venv,
      JUPYTER_PATH: path.join(venv, 'share', 'jupyter'),
      PATH: [path.join(venv, 'bin'), path.dirname(Q2_BINARY), process.env.PATH ?? ''].join(path.delimiter),
      RUST_LOG: process.env.RUST_LOG ?? 'warn',
    };
  }

  private async setup(): Promise<void> {
    const advice = missingToolAdvice((t) => this.which(t));
    if (advice.length) throw new Error(`The demo needs more tools:\n- ${advice.join('\n- ')}`);
    for (const dir of [this.root, this.logs, this.journal, path.join(this.root, 'data')]) mkdirSync(dir, { recursive: true });
    for (const f of ['rproject.toml', 'rv.lock']) copyFileSync(path.join(assets, f), path.join(this.root, f));
    this.writeJournal();
    for (const c of setupCommands({ root: this.root, journal: this.journal })) {
      const out = execFileSync(c.cmd, c.args, { cwd: c.cwd, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
      if (c.cmd === 'rv' && c.args[0] === 'library') this.rLibrary = path.resolve(this.root, out.trim().split('\n').pop()!.trim());
    }
    // The shell inherits a library path that must exist for R_LIBS_USER to be useful.
    if (!existsSync(this.rLibrary)) throw new Error(`rv library ${this.rLibrary} does not exist after rv sync`);
    this.hostTerminal = await startTerminalServer({
      cwd: this.journal,
      env: this.env(),
      pageHtml: (ws) => terminalPageHtml(ws, 'Myra — terminal'),
      assets: terminalAssets(),
    });
    this.hostTerminal.onCommand((line) => {
      this.typed.push(line);
      this.lastActivity = Date.now();
      this.record('terminal', 'command', line, { line });
    });
  }

  /** (Re)build the journal repository, keeping `.venv`. */
  private writeJournal(): void {
    for (const e of readdirSync(this.journal)) if (e !== '.venv') rmSync(path.join(this.journal, e), { recursive: true, force: true });
    buildJournal(this.journal);
  }

  // ---- logging and narration ---------------------------------------------------------------------------------

  record(source: string, kind: string, summary: string, details: unknown = {}): void {
    const e = { time: new Date().toISOString(), source, kind, summary, details };
    this.events.push(e);
    try {
      appendFileSync(path.join(this.logs, 'events.jsonl'), JSON.stringify(e) + '\n');
    } catch {
      /* logging never stops the demo */
    }
  }

  /** Route a caption, narration, or warning to the host terminal (DM3). Display only: never into the PTY. */
  say(kind: 'caption' | 'narration' | 'warning', text: string): void {
    if (this.o.fast && kind === 'caption') return;
    const r = routeMessage({ kind, text }, { captions: this.o.captions, narration: this.o.narration });
    if (r.inline) this.hostTerminal.display(`\r\x1b[2K${r.inline}\r\n$ `);
    if (r.strip) {
      this.strip.push(r.strip);
      this.hostTerminal.strip(this.strip.lines());
    }
  }

  clearStrip(): void {
    this.strip.clear();
    this.hostTerminal.strip([]);
  }

  // ---- the hub -------------------------------------------------------------------------------------------------

  hubArgs(): string[] {
    return ['preview', '--no-browser', '--git', this.o.hubGit, '--ui', 'editor', '--allow-edit', '--data-dir', path.join(this.root, 'data'), '.'];
  }

  /** The command line a person (or the scripted story) types in the host terminal; the hub's output goes to a log file. */
  hubCommandLine(): string {
    return `q2 ${this.hubArgs().join(' ')} > ../logs/hub.log 2>&1 &`;
  }

  /** Start the hub from the launcher (free play). */
  async startHubProcess(): Promise<void> {
    const log = path.join(this.logs, 'hub.log');
    writeFileSync(log, '');
    const proc = spawn(Q2_BINARY, this.hubArgs(), { cwd: this.journal, env: { ...process.env, ...this.env() }, stdio: ['ignore', 'pipe', 'pipe'] });
    proc.stdout?.on('data', (c) => appendFileSync(log, c));
    proc.stderr?.on('data', (c) => appendFileSync(log, c));
    this.hubProc = proc;
    await this.waitForHub();
  }

  /** Wait for the hub's banner in `logs/hub.log` and for `/health`; sets the URLs and the project id. */
  async waitForHub(timeoutMs = 60_000): Promise<void> {
    const log = path.join(this.logs, 'hub.log');
    const deadline = Date.now() + timeoutMs;
    let url = '';
    while (Date.now() < deadline) {
      const text = existsSync(log) ? readFileSync(log, 'utf8') : '';
      const m = text.match(/→\s+(http:\/\/[^\s]+)/);
      if (m) {
        url = m[1]!;
        break;
      }
      await new Promise((r) => setTimeout(r, 100));
    }
    if (!url) throw new Error(`the hub printed no URL within ${timeoutMs} ms; see ${log}`);
    this.hubUrl = url;
    this.hubHttp = new URL('/', url).toString();
    for (;;) {
      try {
        const body = (await (await fetch(new URL('health', this.hubHttp))).json()) as { index_document_id: string };
        this.projectId = body.index_document_id;
        return;
      } catch {
        if (Date.now() > deadline) throw new Error('the hub did not answer /health');
        await new Promise((r) => setTimeout(r, 100));
      }
    }
  }

  async stopHub(): Promise<void> {
    if (this.hubProc) {
      const p = this.hubProc;
      this.hubProc = undefined;
      await new Promise<void>((resolve) => {
        const t = setTimeout(() => {
          p.kill('SIGKILL');
          resolve();
        }, 8000);
        p.once('exit', () => {
          clearTimeout(t);
          resolve();
        });
        p.kill('SIGTERM');
      });
    } else {
      try {
        execFileSync('pkill', ['-f', `${Q2_BINARY} preview .*${path.join(this.root, 'data')}`], { stdio: 'ignore' });
      } catch {
        /* none running */
      }
    }
    await new Promise((r) => setTimeout(r, 800));
  }

  // ---- windows -------------------------------------------------------------------------------------------------

  get headless(): boolean {
    return this.o.headless ?? this.o.fast;
  }

  private async window(): Promise<{ context: BrowserContext; page: Page }> {
    this.browser ??= await chromium.launch({ headless: this.headless, args: this.headless ? [] : ['--window-size=960,600'] });
    const context = await this.browser.newContext({ viewport: this.headless ? { width: 960, height: 938 } : null });
    return { context, page: await context.newPage() };
  }

  private async place(w: { context: BrowserContext; page: Page }, r: Rect | undefined): Promise<void> {
    if (this.headless || !r) return;
    const cdp = await w.context.newCDPSession(w.page);
    const { windowId } = (await cdp.send('Browser.getWindowForTarget')) as { windowId: number };
    await cdp.send('Browser.setWindowBounds', { windowId, bounds: { left: r.left, top: r.top, width: r.width, height: r.height, windowState: 'normal' } });
    await cdp.detach().catch(() => {});
  }

  computeLayout(): Layout {
    this.layout = computeLayout(screenFrame(this.o.screen), { guestTerminal: this.o.preset === 'hub-vscode-clone' });
    return this.layout;
  }

  /** Open the host terminal window (the page the driver and a person type into). */
  async openHostTerminal(): Promise<void> {
    this.computeLayout();
    const w = await this.window();
    if (this.headless) {
      await w.page.goto(this.hostTerminal.pageUrl);
    } else {
      // A popup window has no tab strip or toolbar, so it can be as short as the layout asks (a normal window cannot
      // be shorter than about 375 points).
      const opener = w.page;
      await opener.goto('about:blank');
      const [popup] = await Promise.all([
        opener.waitForEvent('popup'),
        opener.evaluate((u) => void window.open(u, 'terminal', 'popup=yes,width=900,height=200'), this.hostTerminal.pageUrl),
      ]);
      await popup.waitForLoadState();
      await opener.close();
      w.page = popup;
    }
    await this.place(w, this.layout.hostTerminal);
    this.terminalPages.push(w);
  }

  /** Open the two browser editors on the hub. */
  async openEditors(): Promise<void> {
    const host = await this.window();
    const guest = await this.window();
    await Promise.all([this.place(host, this.layout.hostEditor), this.place(guest, this.layout.guestEditor)]);
    this.hostEditor = host;
    this.guestEditor = guest;
    await Promise.all([this.openProject(host.page, R_PAPER), this.openProject(guest.page, PY_PAPER)]);
  }

  private async openProject(page: Page, file: string): Promise<void> {
    const { openProject, openFile } = await import('../../../q2-preview-spa/e2e/helpers/editor');
    try {
      await openProject(page, this.hubUrl);
      await openFile(page, file);
    } catch (e) {
      await page.screenshot({ path: path.join(this.logs, `open-failed-${Date.now()}.png`) }).catch(() => {});
      throw e;
    }
  }

  // ---- the observer ----------------------------------------------------------------------------------------------

  private listFiles(): string[] {
    const out: string[] = [];
    const walk = (dir: string) => {
      for (const e of readdirSync(dir, { withFileTypes: true })) {
        // Engine output (knitr's .rmarkdown and *_files, jupyter's *_files) is not the owner's project content.
        if (e.name.startsWith('.') || e.name === '_site' || e.name === '__pycache__' || e.name.endsWith('_files') || e.name.endsWith('.rmarkdown')) continue;
        const p = path.join(dir, e.name);
        if (e.isDirectory()) walk(p);
        else out.push(path.relative(this.journal, p));
      }
    };
    walk(this.journal);
    return out.sort();
  }

  startObserver(): void {
    this.pollTimer = setInterval(() => void this.poll().catch(() => {}), this.o.pollMs);
  }

  stopObserver(): void {
    if (this.pollTimer) clearInterval(this.pollTimer);
    this.pollTimer = undefined;
  }

  clearObserver(): void {
    this.prevStatus = null;
    this.prevFiles = undefined;
    this.warned.clear();
  }

  async status(): Promise<StatusBody> {
    return (await (await fetch(new URL('api/preview/git-status', this.hubHttp))).json()) as StatusBody;
  }

  private async poll(): Promise<void> {
    if (!this.hubHttp) return;
    const next = await this.status();
    const files = this.listFiles();
    const events: DemoEvent[] = deriveEvents(this.prevStatus, next, { prevFiles: this.prevFiles, nextFiles: files });
    for (const e of events) {
      this.lastActivity = Date.now();
      const text = narrate(e);
      this.record('observer', e.kind, text, e.details);
      this.say('narration', text);
    }
    this.prevStatus = next;
    this.prevFiles = files;
    // Invariants, on the files of the hub's disk (the conflict-marker check reads every file the owner can see).
    const docs = files
      .filter((f) => f.endsWith('.qmd'))
      .map((f) => ({ path: f, text: readFileSync(path.join(this.journal, f), 'utf8') }));
    const snap: Snapshot = {
      quiet: !next.pending && !next.paused && Date.now() - this.lastActivity > 2000,
      docs,
      hubDisk: {},
      pointerDocs: {},
      clones: [],
      followers: [],
    };
    for (const v of checkInvariants(snap)) {
      const key = `${v.id}:${v.summary}`;
      if (this.warned.has(key)) continue;
      this.warned.add(key);
      this.record('invariant', v.id, v.summary);
      this.say('warning', `invariant: ${v.summary}`);
    }
  }

  // ---- driving the terminal ----------------------------------------------------------------------------------------

  /** Wait until the host shell shows its prompt again, after any typed command. */
  async waitPrompt(timeoutMs = 60_000): Promise<void> {
    const deadline = Date.now() + timeoutMs;
    let last = '';
    for (;;) {
      const o = stripAnsi(this.hostTerminal.output());
      if (/\$ $/.test(o) && o === last) return;
      last = o;
      if (Date.now() > deadline) throw new Error('the shell did not return to its prompt');
      await new Promise((r) => setTimeout(r, 150));
    }
  }

  async type(command: string): Promise<void> {
    await this.hostTerminal.type(command, { charDelayMs: this.o.fast ? 0 : 40 });
    await this.waitPrompt();
  }

  /** Run `git` as Myra would, in the journal, without the terminal (used by checks and fixtures). */
  git(...args: string[]): string {
    return execFileSync('git', args, { cwd: this.journal, env: gitEnv(), encoding: 'utf8' });
  }

  // ---- reset (DM12) -------------------------------------------------------------------------------------------------

  async reset(): Promise<void> {
    const { resetDemo } = await import('./reset');
    await resetDemo(
      {
        stopProcesses: async () => {
          this.stopObserver();
          await this.stopHub();
        },
        rebuildJournal: async () => {
          // The shell must not sit inside the directory while it is rebuilt.
          await this.hostTerminal.type('cd ~ && cd ' + JSON.stringify(this.journal), { charDelayMs: 0 });
          await this.waitPrompt();
          this.writeJournal();
          // A fresh hub needs fresh state: the data directory keys the project id (G19).
          rmSync(path.join(this.root, 'data'), { recursive: true, force: true });
          mkdirSync(path.join(this.root, 'data'), { recursive: true });
        },
        rebuildGuestClone: async () => {},
        startHub: async () => {
          await this.startHubProcess();
          return this.projectId;
        },
        navigateBrowsers: async () => {
          for (const w of [this.hostEditor, this.guestEditor]) {
            if (!w) continue;
            await w.page.goto(this.hubUrl);
          }
          if (this.hostEditor) await this.openProject(this.hostEditor.page, R_PAPER);
          if (this.guestEditor) await this.openProject(this.guestEditor.page, PY_PAPER);
        },
        rejoinVscode: async () => {},
        clearObserver: async () => this.clearObserver(),
        clearStrip: async () => this.clearStrip(),
        announce: (t) => {
          this.record('demo', 'reset', t);
          this.hostTerminal.display(`\r\x1b[2K\x1b[2;3m${t}\x1b[0m\r\n$ `);
        },
      },
      { hasClone: this.o.preset === 'hub-vscode-clone', hasVscode: this.o.preset !== 'hub-browser' },
    );
    this.startObserver();
  }

  async stop(): Promise<void> {
    this.stopObserver();
    await this.browser?.close().catch(() => {});
    await this.stopHub();
    await this.hostTerminal.close().catch(() => {});
  }

  /** Remove the runtime directory (tests; a person keeps theirs). */
  removeRoot(): void {
    if (process.env.DEMO_KEEP) return;
    if (existsSync(this.root) && statSync(this.root).isDirectory()) rmSync(this.root, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  }
}
