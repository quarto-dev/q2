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
import { checkInvariants, stablePointerTexts, type Snapshot } from './invariants';
import { deriveEvents, narrate, type DemoEvent, type StatusBody } from './observer';
import { missingToolAdvice, setupCommands } from './setup';
import { startTerminalServer, type TerminalServer } from './terminal-server';
import { terminalAssets, terminalPageHtml, STRIP_LINES } from './terminal-page';
import { launchVscode, type RunningVscode } from '../../e2e/helpers/launch';
import { HeadlessPeer, ensureWasm } from '../../e2e/helpers/hub';
import { bindFolder, deriveWsUrl } from '../../src/core/join';
import { activeEditorText, fillInputBox, openFile as openVscodeFile, runCommand } from '../../e2e/helpers/palette';
import net from 'node:net';
import { createTypist, type Typist } from './typing';
import { connectDocReader, type DocReader } from './doc-reader';

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
  /** Seed of the human typing tempo (default 1), so a recorded run repeats. */
  seed: number;
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
  seed: 1,
  ...over,
});

/**
 * VS Code's own default for auto save is off, and the demo keeps it. With `files.autoSave: afterDelay` (the e2e helper's
 * default) an editor-only window on the hub's own folder wrote the old text of the open paper over the hub's disk after a
 * flip (found in GE-19; see the plan's notes), so `git switch` back was refused as a dirty tree.
 */
const VSCODE_SETTINGS = { 'files.autoSave': 'off' };

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
  /** The guest's terminal, in presets where the guest has its own repository (`hub-vscode-clone`). */
  guestTerminal?: TerminalServer;
  /** Maurice's clone of the journal (presets with a clone guest). */
  readonly guestDir: string;
  vscode: { host?: RunningVscode; guest?: RunningVscode } = {};
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
  /** A violation is reported once it holds in two polls in a row (a transient state mid-swap is not a violation). */
  private seen = new Map<string, number>();
  /** Test seam: replaces the raw-Repo reader of the pointed docs. */
  docReader?: DocReader;
  private typist: Typist;
  private pointerSince = new Map<string, { text: string; since: number }>();
  private rLibrary = '';
  layout!: Layout;

  private constructor(readonly o: DemoOptions) {
    this.root = o.root ?? mkdtempSync('/tmp/ge19-demo-');
    this.journal = path.join(this.root, 'journal');
    this.logs = path.join(this.root, 'logs');
    this.guestDir = path.join(this.root, 'maurice');
    this.typist = createTypist({ baseMs: o.fast ? 0 : 40, seed: o.seed });
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
    if (this.o.preset === 'hub-vscode-clone' || this.o.preset === 'hubless') {
      // Maurice's clone, or (hubless) an empty directory: his only tool there is the pasted `q2 preview` command.
      if (this.o.preset === 'hubless') {
        mkdirSync(this.guestDir, { recursive: true });
        this.qhubPort = await Demo.freePort();
      }
      else this.cloneJournal();
      this.guestTerminal = await startTerminalServer({
        cwd: this.guestDir,
        env: this.env(),
        pageHtml: (ws) => terminalPageHtml(ws, 'Maurice — terminal'),
        assets: terminalAssets(),
      });
      this.guestTerminal.onCommand((line) => {
        this.typed.push(line);
        this.lastActivity = Date.now();
        this.record('terminal', 'command', `(Maurice) ${line}`, { line, who: 'guest' });
      });
    }
  }

  /** Maurice's clone: a plain `git clone` of Myra's journal (before it is attached to the project). */
  private cloneJournal(): void {
    rmSync(this.guestDir, { recursive: true, force: true });
    execFileSync('git', ['clone', '--quiet', this.journal, this.guestDir], { env: gitEnv(), stdio: 'ignore' });
  }

  terminal(who: 'host' | 'guest'): TerminalServer {
    return who === 'host' ? this.hostTerminal : this.guestTerminal!;
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

  // ---- hubless (DM6): a standalone q2 hub, a VS Code originator, and a pasted q2 preview command ------------------------

  qhubPort = 0;
  guestPreviewUrl = '';
  shareCommand = '';
  private qhubProc?: ChildProcess;
  private guestPreviewProc?: ChildProcess;

  get qhubWs(): string {
    return `ws://127.0.0.1:${this.qhubPort}`;
  }

  private static freePort(): Promise<number> {
    return new Promise((resolve, reject) => {
      const srv = net.createServer();
      srv.once('error', reject);
      srv.listen(0, '127.0.0.1', () => {
        const { port } = srv.address() as net.AddressInfo;
        srv.close(() => resolve(port));
      });
    });
  }

  /** The command line typed in the host terminal: a standalone sync server on loopback, its log in a file. */
  qhubCommandLine(): string {
    return `q2 hub --no-project --port ${this.qhubPort} --data-dir ../data-hub > ../logs/qhub.log 2>&1 &`;
  }

  async waitForQhub(timeoutMs = 60_000): Promise<void> {
    const deadline = Date.now() + timeoutMs;
    for (;;) {
      try {
        await fetch(`http://127.0.0.1:${this.qhubPort}/`);
        return;
      } catch {
        if (Date.now() > deadline) throw new Error(`q2 hub did not answer on ${this.qhubPort}; see ${this.logs}/qhub.log`);
        await new Promise((r) => setTimeout(r, 150));
      }
    }
  }

  /** Free play and reset: the launcher starts the standalone hub itself. */
  async startQhubProcess(): Promise<void> {
    this.qhubPort ||= await Demo.freePort();
    const log = path.join(this.logs, 'qhub.log');
    writeFileSync(log, '');
    const proc = spawn(Q2_BINARY, ['hub', '--no-project', '--port', String(this.qhubPort), '--data-dir', path.join(this.root, 'data-hub')], {
      cwd: this.root,
      env: { ...process.env, ...this.env() },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    proc.stdout?.on('data', (c) => appendFileSync(log, c));
    proc.stderr?.on('data', (c) => appendFileSync(log, c));
    this.qhubProc = proc;
    await this.waitForQhub();
  }

  /** The originator's VS Code window on the journal, with Share this folder run up to the confirmation modal. */
  async originateStart(): Promise<void> {
    await ensureWasm();
    const v = await launchVscode({ workspace: this.journal, env: { QUARTO_HUB_TEST_SYNC_SERVER: this.qhubWs }, settings: VSCODE_SETTINGS });
    this.vscode.host = v;
    this.computeLayout();
    await this.placeVscode(v, this.layout.hostEditor);
    await runCommand(v.page, 'Quarto Hub: Share this folder');
    await fillInputBox(v.page, this.qhubWs);
    await v.page.locator('.monaco-dialog-box').waitFor({ state: 'visible', timeout: 30_000 });
  }

  /** Confirm the modal, wait for the share panel, and take the `q2 preview` command it copies. */
  async originateConfirm(): Promise<{ id: string; command: string }> {
    const v = this.vscode.host!;
    await v.page.locator('.monaco-dialog-box').getByRole('button', { name: 'Share' }).click();
    await v.app.evaluate(({ clipboard: c }) => c.writeText(''));
    const row = v.page.locator('.quick-input-list .monaco-list-row', { hasText: 'q2 preview command' });
    await row.waitFor({ timeout: 60_000 });
    await row.click();
    const deadline = Date.now() + 30_000;
    let command = '';
    while (!command && Date.now() < deadline) {
      command = await v.app.evaluate(({ clipboard: c }) => c.readText());
      if (!command) await new Promise((r) => setTimeout(r, 150));
    }
    const m = /^q2 preview --hub-url (\S+) --project (\S+) --ui editor --allow-edit$/.exec(command);
    if (!m) throw new Error(`the copied command is not a q2 preview command: ${command}`);
    await v.page.keyboard.press('Escape');
    this.shareCommand = command;
    this.projectId = m[2]!;
    return { id: m[2]!, command };
  }

  /** What the guest terminal types: the copied command, without a browser window, its log in a file. */
  guestPreviewCommandLine(): string {
    return `${this.shareCommand} --no-browser --data-dir ../data-guest > ../logs/guest-preview.log 2>&1 &`;
  }

  async waitForGuestPreview(timeoutMs = 60_000): Promise<string> {
    const log = path.join(this.logs, 'guest-preview.log');
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      const text = existsSync(log) ? readFileSync(log, 'utf8') : '';
      const m = text.match(/→\s+(http:\/\/[^\s]+)/);
      if (m) {
        this.guestPreviewUrl = m[1]!;
        return this.guestPreviewUrl;
      }
      await new Promise((r) => setTimeout(r, 150));
    }
    throw new Error(`the guest preview printed no URL; see ${log}`);
  }

  /** Free play and reset: share silently, start the guest preview from the launcher, and open the guest's browser on it. */
  async hublessAll(): Promise<void> {
    await this.originateStart();
    await this.originateConfirm();
    await openVscodeFile(this.vscode.host!.page, R_PAPER);
    const log = path.join(this.logs, 'guest-preview.log');
    writeFileSync(log, '');
    const parts = this.shareCommand.split(' ').slice(1);
    const proc = spawn(Q2_BINARY, [...parts, '--no-browser', '--data-dir', path.join(this.root, 'data-guest')], {
      cwd: this.guestDir,
      env: { ...process.env, ...this.env() },
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    proc.stdout?.on('data', (c) => appendFileSync(log, c));
    proc.stderr?.on('data', (c) => appendFileSync(log, c));
    this.guestPreviewProc = proc;
    await this.waitForGuestPreview();
    await this.openGuestBrowser(this.guestPreviewUrl);
  }

  async openGuestBrowser(url: string): Promise<void> {
    this.computeLayout();
    if (!this.guestEditor) {
      const guest = await this.window();
      await this.place(guest, this.layout.guestEditor);
      this.guestEditor = guest;
    }
    const { openProject, openFile } = await import('../../../q2-preview-spa/e2e/helpers/editor');
    await openProject(this.guestEditor.page, url);
    await openFile(this.guestEditor.page, R_PAPER);
  }

  private async killProc(p: ChildProcess | undefined): Promise<void> {
    if (!p || p.exitCode !== null) return;
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
  }

  /** Stop the standalone hub and the guest preview, whether the launcher or the terminal started them. */
  async stopHubless(): Promise<void> {
    await this.killProc(this.guestPreviewProc);
    await this.killProc(this.qhubProc);
    this.guestPreviewProc = this.qhubProc = undefined;
    // Processes the terminal started are found by their listening ports (never by name: other demos may be running).
    const ports = [this.qhubPort, this.guestPreviewUrl ? Number(new URL(this.guestPreviewUrl).port) : 0].filter(Boolean);
    for (const port of ports) {
      try {
        const pids = execFileSync('lsof', ['-ti', `tcp:${port}`, '-sTCP:LISTEN'], { encoding: 'utf8' }).split('\n').filter(Boolean);
        for (const pid of pids) process.kill(Number(pid), 'SIGTERM');
      } catch {
        /* none listening */
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
    this.layout = computeLayout(screenFrame(this.o.screen), { guestTerminal: this.o.preset === 'hub-vscode-clone' || this.o.preset === 'hubless' });
    return this.layout;
  }

  /** Open the host terminal window (the page the driver and a person type into). */
  async openHostTerminal(): Promise<void> {
    this.computeLayout();
    await this.openTerminalWindow(this.hostTerminal, this.layout.hostTerminal);
    if (this.guestTerminal) await this.openTerminalWindow(this.guestTerminal, this.layout.guestTerminal);
  }

  private async openTerminalWindow(term: TerminalServer, rect: Rect | undefined): Promise<void> {
    const w = await this.window();
    if (this.headless) {
      await w.page.goto(term.pageUrl);
    } else {
      // A popup window has no tab strip or toolbar, so it can be as short as the layout asks (a normal window cannot
      // be shorter than about 375 points).
      const opener = w.page;
      await opener.goto('about:blank');
      const [popup] = await Promise.all([
        opener.waitForEvent('popup'),
        opener.evaluate((u) => void window.open(u, 'terminal', 'popup=yes,width=900,height=200'), term.pageUrl),
      ]);
      await popup.waitForLoadState();
      await opener.close();
      w.page = popup;
    }
    await this.place(w, rect);
    this.terminalPages.push(w);
  }

  /** The kind of editor each side has in this preset. */
  get kinds(): { host: 'browser' | 'vscode'; guest: 'browser' | 'vscode' } {
    const host = this.o.preset === 'hub-vscode-host' || this.o.preset === 'hubless' ? 'vscode' : 'browser';
    return { host, guest: this.o.preset === 'hub-vscode-clone' ? 'vscode' : 'browser' };
  }

  get wsUrl(): string {
    return deriveWsUrl(this.hubHttp);
  }

  /** Open the two editors on the hub: browsers, or VS Code windows (an editor-only host, a clone guest). */
  async openEditors(): Promise<void> {
    if (this.kinds.host === 'browser') {
      const host = await this.window();
      await this.place(host, this.layout.hostEditor);
      this.hostEditor = host;
      await this.openProject(host.page, R_PAPER);
    } else {
      await this.openVscodeHost();
    }
    if (this.kinds.guest === 'browser') {
      const guest = await this.window();
      await this.place(guest, this.layout.guestEditor);
      this.guestEditor = guest;
      await this.openProject(guest.page, PY_PAPER);
    } else {
      await this.openVscodeGuest();
    }
  }

  private async placeVscode(v: RunningVscode, r: Rect): Promise<void> {
    await v.app.evaluate(({ BrowserWindow }, b) => {
      const w = BrowserWindow.getAllWindows()[0]!;
      w.setBounds({ x: Math.round(b.left), y: Math.round(b.top), width: Math.round(b.width), height: Math.round(b.height) });
    }, r);
  }

  /** The host's VS Code: an editor-only window on the journal folder the hub serves. */
  private async openVscodeHost(): Promise<void> {
    await ensureWasm();
    await bindFolder({ root: this.journal, indexDocId: this.projectId, syncServer: this.wsUrl, mode: 'editor-only' });
    const v = await launchVscode({ workspace: this.journal, settings: VSCODE_SETTINGS });
    this.vscode.host = v;
    await this.placeVscode(v, this.layout.hostEditor);
    await v.page.locator('.statusbar').getByText('Quarto Hub: editor only').waitFor({ timeout: 60_000 });
    await openVscodeFile(v.page, R_PAPER);
  }

  /** The guest's VS Code: a clone of the journal attached to the project, with Maurice's terminal beside it. */
  private async openVscodeGuest(): Promise<void> {
    await ensureWasm();
    const peer = await HeadlessPeer.joinAsClone(this.guestDir, this.wsUrl, this.projectId);
    await peer.stop();
    const v = await launchVscode({ workspace: this.guestDir, settings: VSCODE_SETTINGS });
    this.vscode.guest = v;
    await this.placeVscode(v, this.layout.guestEditor);
    await v.page.locator('.statusbar').getByText('Quarto Hub: clone').waitFor({ timeout: 60_000 });
    await openVscodeFile(v.page, PY_PAPER);
  }

  /** What an editor shows right now (browser: the rendered Monaco lines; VS Code: the active editor's lines). */
  async shown(who: 'host' | 'guest'): Promise<string> {
    if (this.kinds[who] === 'vscode') return activeEditorText(this.vscode[who]!.page);
    const { shownText } = await import('../../../q2-preview-spa/e2e/helpers/editor');
    return shownText((who === 'host' ? this.hostEditor : this.guestEditor)!.page).catch(() => '');
  }

  /** Click into the editor, go to the end of the document, and type (Cmd+Down on macOS; Ctrl+End does nothing there). */
  async typeAtEnd(who: 'host' | 'guest', text: string): Promise<void> {
    const page = this.kinds[who] === 'vscode' ? this.vscode[who]!.page : (who === 'host' ? this.hostEditor : this.guestEditor)!.page;
    await page.locator(this.kinds[who] === 'vscode' ? '.editor-instance .view-lines' : '.monaco-editor .view-lines').first().click();
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+ArrowDown' : 'Control+End');
    const plan = this.typist.plan(text);
    const wait = (ms: number) => (ms > 0 ? new Promise((r) => setTimeout(r, ms)) : Promise.resolve());
    await wait(plan.lead);
    let i = 0;
    for (const ch of text) {
      await wait(plan.gaps[i++] ?? 0);
      await page.keyboard.type(ch);
    }
  }

  /** Scroll an editor to the top of its document (Monaco renders only the visible lines). */
  async scrollTop(who: 'host' | 'guest'): Promise<void> {
    const page = this.kinds[who] === 'vscode' ? this.vscode[who]!.page : (who === 'host' ? this.hostEditor : this.guestEditor)!.page;
    await page.locator(this.kinds[who] === 'vscode' ? '.editor-instance .view-lines' : '.monaco-editor .view-lines').first().click();
    await page.keyboard.press(process.platform === 'darwin' ? 'Meta+ArrowUp' : 'Control+Home');
  }

  /** The number of swap notices an editor shows (always 0 for VS Code, which shows notifications, not this banner). */
  async notices(who: 'host' | 'guest'): Promise<number> {
    if (this.kinds[who] === 'vscode') return 0;
    const { noticeLocator } = await import('../../../q2-preview-spa/e2e/helpers/editor');
    return noticeLocator((who === 'host' ? this.hostEditor : this.guestEditor)!.page).count();
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
    this.seen.clear();
    // A new hub is a new project: the reader reconnects on the next poll.
    void this.docReader?.close();
    this.docReader = undefined;
    this.pointerSince.clear();
  }

  async status(): Promise<StatusBody> {
    return (await (await fetch(new URL('api/preview/git-status', this.hubHttp))).json()) as StatusBody;
  }

  private async poll(): Promise<void> {
    // `hubless` has no tracking hub, so no status route: the observer then narrates files and checks invariants only.
    if (!this.hubHttp && this.o.preset !== 'hubless') return;
    const next: StatusBody | null = this.hubHttp ? await this.status() : null;
    const files = this.listFiles();
    const events: DemoEvent[] = next ? deriveEvents(this.prevStatus, next, { prevFiles: this.prevFiles, nextFiles: files }) : [];
    for (const e of events) {
      this.lastActivity = Date.now();
      const text = narrate(e);
      this.record('observer', e.kind, text, e.details);
      this.say('narration', text);
    }
    this.prevStatus = next ?? this.prevStatus;
    this.prevFiles = files;
    // Invariants, on the files of the hub's disk (the conflict-marker check reads every file the owner can see).
    // While git has the rebase paused, the markers on the disk are git's own, not a defect of the sync.
    const docs = (next?.paused ? [] : files)
      .filter((f) => f.endsWith('.qmd'))
      .map((f) => ({ path: f, text: readFileSync(path.join(this.journal, f), 'utf8') }));
    const quiet = !next?.pending && !next?.paused && Date.now() - this.lastActivity > 2000;
    // The hub's disk against the docs its pointers name (only when quiet, and only where a tracking hub exists).
    const hubDisk: Record<string, string> = {};
    let pointerDocs: Record<string, string> = {};
    if (quiet && this.hubHttp && this.o.preset !== 'hubless') {
      try {
        this.docReader ??= await connectDocReader(this.wsUrl, this.projectId);
        const current = await this.docReader.pointerTexts(docs.map((d) => d.path));
        // The hub writes a doc to disk after a debounce, so only a pointed text that has held still is compared.
        pointerDocs = stablePointerTexts(this.pointerSince, current, Date.now(), 3000);
        for (const d of docs) if (d.path in pointerDocs) hubDisk[d.path] = d.text;
      } catch {
        pointerDocs = {};
      }
    }
    const snap: Snapshot = {
      quiet,
      docs,
      hubDisk,
      pointerDocs,
      clones: this.cloneState(),
      followers: [],
    };
    const present = new Set<string>();
    for (const v of checkInvariants(snap)) {
      const key = `${v.id}:${v.summary}`;
      present.add(key);
      const n = (this.seen.get(key) ?? 0) + 1;
      this.seen.set(key, n);
      if (n < 2 || this.warned.has(key)) continue;
      this.warned.add(key);
      const details: Record<string, unknown> = {};
      if (v.id === 'disk-differs-from-pointer') {
        const m = /for (.+)$/.exec(v.summary);
        const p = m?.[1] ?? '';
        const a = snap.hubDisk[p] ?? '';
        const b = snap.pointerDocs[p] ?? '';
        let i = 0;
        while (i < a.length && i < b.length && a[i] === b[i]) i++;
        Object.assign(details, { diskLength: a.length, pointerLength: b.length, firstDifference: i, disk: a.slice(i, i + 40), pointer: b.slice(i, i + 40) });
      }
      this.record('invariant', v.id, v.summary, details);
      this.say('warning', `invariant: ${v.summary}`);
    }
    for (const k of [...this.seen.keys()]) if (!present.has(k)) this.seen.delete(k);
  }

  /** Maurice's clone, for the invariant that a clone on another branch never has its disk rewritten. */
  private cloneState(): Snapshot['clones'] {
    if (this.o.preset !== 'hub-vscode-clone' || !existsSync(path.join(this.guestDir, '.git'))) return [];
    try {
      const branchOf = (cwd: string) => execFileSync('git', ['branch', '--show-current'], { cwd, env: gitEnv(), encoding: 'utf8' }).trim();
      const branch = branchOf(this.guestDir);
      const hubBranch = branchOf(this.journal);
      let diskRewritten = false;
      if (branch !== hubBranch) {
        // Foreign content: a file whose bytes equal the hub's branch version but not this clone's own branch version.
        const changed = execFileSync('git', ['diff', '--name-only'], { cwd: this.guestDir, env: gitEnv(), encoding: 'utf8' }).split('\n').filter(Boolean);
        for (const f of changed) {
          const here = execFileSync('git', ['hash-object', f], { cwd: this.guestDir, env: gitEnv(), encoding: 'utf8' }).trim();
          const theirs = execFileSync('git', ['rev-parse', `${hubBranch}:${f}`], { cwd: this.journal, env: gitEnv(), encoding: 'utf8' }).trim();
          if (here === theirs) diskRewritten = true;
        }
      }
      return [{ name: 'Maurice\'s clone', branch, hubBranch, diskRewritten }];
    } catch {
      return [];
    }
  }

  // ---- driving the terminal ----------------------------------------------------------------------------------------

  /** Wait until the host shell shows its prompt again, after any typed command. */
  async waitPrompt(timeoutMs = 60_000, who: 'host' | 'guest' = 'host'): Promise<void> {
    const deadline = Date.now() + timeoutMs;
    let last = '';
    for (;;) {
      const o = stripAnsi(this.terminal(who).output());
      if (/\$ $/.test(o) && o === last) return;
      last = o;
      if (Date.now() > deadline) throw new Error('the shell did not return to its prompt');
      await new Promise((r) => setTimeout(r, 150));
    }
  }

  async type(command: string, who: 'host' | 'guest' = 'host'): Promise<void> {
    await this.terminal(who).type(command, { timing: this.typist.plan(command) });
    await this.waitPrompt(60_000, who);
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
          await this.closeVscode();
          await this.stopHub();
          await this.stopHubless();
        },
        rebuildJournal: async () => {
          // The shell must not sit inside the directory while it is rebuilt.
          await this.hostTerminal.type('cd ~ && cd ' + JSON.stringify(this.journal), { charDelayMs: 0 });
          await this.waitPrompt();
          this.writeJournal();
          // A fresh hub needs fresh state: the data directory keys the project id (G19).
          for (const dir of ['data', 'data-hub', 'data-guest']) rmSync(path.join(this.root, dir), { recursive: true, force: true });
          mkdirSync(path.join(this.root, 'data'), { recursive: true });
        },
        rebuildGuestClone: async () => {
          await this.guestTerminal!.type('cd ~ && cd ' + JSON.stringify(this.guestDir), { charDelayMs: 0 });
          await this.waitPrompt(60_000, 'guest');
          this.cloneJournal();
        },
        startHub: async () => {
          if (this.o.preset === 'hubless') {
            // The project id exists only once the folder is shared again; everything follows from that.
            await this.startQhubProcess();
            await this.hublessAll();
          } else {
            await this.startHubProcess();
          }
          return this.projectId;
        },
        navigateBrowsers: async () => {
          if (this.o.preset === 'hubless') return;
          for (const w of [this.hostEditor, this.guestEditor]) {
            if (!w) continue;
            await w.page.goto(this.hubUrl);
          }
          if (this.hostEditor) await this.openProject(this.hostEditor.page, R_PAPER);
          if (this.guestEditor) await this.openProject(this.guestEditor.page, PY_PAPER);
        },
        // A fresh hub has a new project id (G19), so the VS Code windows are relaunched and rebound at the same place
        // (the plan's Join session route would need the share link; the relaunch is recorded as a deviation).
        rejoinVscode: async () => {
          if (this.o.preset === 'hubless') return;
          if (this.kinds.host === 'vscode') await this.openVscodeHost();
          if (this.kinds.guest === 'vscode') await this.openVscodeGuest();
        },
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

  private async closeVscode(): Promise<void> {
    for (const k of ['host', 'guest'] as const) {
      await this.vscode[k]?.close().catch(() => {});
      this.vscode[k] = undefined;
    }
  }

  async stop(): Promise<void> {
    this.stopObserver();
    await this.docReader?.close();
    this.docReader = undefined;
    this.pointerSince.clear();
    await this.closeVscode();
    await this.browser?.close().catch(() => {});
    await this.stopHub();
    await this.stopHubless();
    await this.hostTerminal.close().catch(() => {});
    await this.guestTerminal?.close().catch(() => {});
  }

  /** Remove the runtime directory (tests; a person keeps theirs). */
  removeRoot(): void {
    if (process.env.DEMO_KEEP) return;
    if (existsSync(this.root) && statSync(this.root).isDirectory()) rmSync(this.root, { recursive: true, force: true, maxRetries: 5, retryDelay: 200 });
  }
}
