/**
 * GE-14: the orchestrator of the cross-client scenarios. One `Scene` owns every process and temporary directory a
 * scenario starts (a real `q2 preview --git track` hub, browser guests, headless peers, a standalone hub), logs the
 * exact invocation of each, and tears all of it down, so a spec reads as the scenario and nothing outlives it.
 *
 * The hub, the fixture repository, and the browser helpers are GE-13's (`q2-preview-spa/e2e/helpers`); the headless
 * peers are the extension core's (`src/headless`), the same code a VS Code window runs.
 */
import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';
import { chromium, expect, type Browser, type BrowserContext, type Page } from '@playwright/test';
import type { TestHub } from '@quarto/quarto-sync-client/testing';
import { HeadlessPeer, readText, startHub, tempDir, waitFor } from './hub';
import {
  startPreviewServer,
  Q2_BINARY,
  type PreviewServerHandle,
  type StartOptions,
} from '../../../q2-preview-spa/e2e/helpers/previewServer';
import { gitStatus, openFile as openEditorFile, openProject, type GitStatusBody } from '../../../q2-preview-spa/e2e/helpers/editor';
import { buildFixtureRepo, type FixtureBranches, type GitRepo } from '../../../q2-preview-spa/e2e/helpers/gitRepo';

export { Q2_BINARY };
export type { GitStatusBody, PreviewServerHandle };

/** True when the built `q2` supports `--hub-url` and embeds the real editor (the specs skip, with this reason, when not). */
export async function q2SupportsHubUrl(): Promise<boolean> {
  try {
    const out = execFileSync(Q2_BINARY, ['preview', '--help'], { encoding: 'utf8' });
    return out.includes('--hub-url');
  } catch {
    return false;
  }
}

/** The hub's `/health` body. */
export async function health(httpUrl: string): Promise<{ indexDocId: string; projectRoot: string }> {
  const res = await fetch(new URL('health', httpUrl));
  const body = (await res.json()) as { index_document_id: string; project_root: string };
  return { indexDocId: body.index_document_id, projectRoot: body.project_root };
}

export interface HubInfo {
  handle: PreviewServerHandle;
  /** `ws://127.0.0.1:<port>/ws` */
  wsUrl: string;
  indexDocId: string;
  repo: GitRepo & FixtureBranches;
  dir: string;
}

export interface Guest {
  context: BrowserContext;
  page: Page;
}

export class Scene {
  private readonly cleanups: (() => Promise<void> | void)[] = [];
  private browserPromise: Promise<Browser> | undefined;
  /** The exact invocations the scene made, in order; printed at the end of a run so a transcript can be read from the log. */
  readonly invocations: string[] = [];

  log(line: string): void {
    this.invocations.push(line);
    console.log(`[scene] ${line}`);
  }

  onCleanup(fn: () => Promise<void> | void): void {
    this.cleanups.push(fn);
  }

  async dir(prefix: string): Promise<string> {
    const d = await tempDir(prefix);
    this.cleanups.push(() => fs.rm(d, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 }));
    return d;
  }

  browser(): Promise<Browser> {
    this.browserPromise ??= chromium.launch().then((b) => {
      this.cleanups.push(() => b.close());
      return b;
    });
    return this.browserPromise;
  }

  /**
   * `q2 preview --git track --ui editor --allow-edit` on the GE-13 fixture repository (on `main`). The permanent
   * scenarios omit `--share`: a ticket needs the network.
   */
  async trackingHub(opts: Partial<StartOptions> = {}): Promise<HubInfo> {
    // GE14_GIT=ignore is the control run: the same scenarios against a hub that ignores git must fail (the plan's
    // test-first record), so a scenario cannot pass by ordinary sync alone.
    const git = process.env.GE14_GIT === 'ignore' ? 'ignore' : 'track';
    const handle = await startPreviewServer({ gitRepo: true, git, ui: 'editor', allowEdit: true, ...opts });
    this.cleanups.push(() => handle.stop());
    this.log(`${Q2_BINARY} preview --no-browser --data-dir <data> --allow-edit --ui editor --git ${git} ${handle.projectDir}   # ${handle.url}`);
    const { indexDocId } = await health(handle.url);
    return { handle, wsUrl: `ws://127.0.0.1:${handle.port}/ws`, indexDocId, repo: handle.repo!, dir: handle.projectDir };
  }

  /** `q2 preview --hub-url <ws> --project <id> --ui editor --allow-edit`: a browser-facing server over a remote project. */
  async previewOver(hubUrl: string, project: string): Promise<PreviewServerHandle> {
    const handle = await startPreviewServer({ hubUrl, project, ui: 'editor', allowEdit: true });
    this.cleanups.push(() => handle.stop());
    this.log(`${Q2_BINARY} preview --no-browser --data-dir <data> --allow-edit --ui editor --hub-url ${hubUrl} --project ${project}   # ${handle.url}`);
    return handle;
  }

  /** The in-process standalone sync server (no sign-in), for the hub-less scenarios. */
  async standaloneHub(): Promise<TestHub> {
    const hub = await startHub();
    this.cleanups.push(() => hub.stop());
    this.log(`standalone sync server (in-process test hub) at ${hub.url}`);
    return hub;
  }

  /** A browser guest in its own context on the editor at `url`; `file` opens in the editor when given. */
  async guest(url: string, file?: string): Promise<Guest> {
    const browser = await this.browser();
    const context = await browser.newContext();
    const page = await context.newPage();
    this.cleanups.push(() => context.close());
    await openProject(page, url);
    if (file) await openEditorFile(page, file);
    this.log(`browser guest on ${url}${file ? ` (file ${file})` : ''}`);
    return { context, page };
  }

  /** A headless mirror of the project, in a fresh folder. */
  async mirror(wsUrl: string, indexDocId: string): Promise<{ dir: string; peer: HeadlessPeer }> {
    const dir = await this.dir('ge14-mirror-');
    const peer = await HeadlessPeer.joinAsMirror(dir, wsUrl, indexDocId);
    this.cleanups.push(() => peer.stop());
    this.log(`headless mirror joined ${wsUrl} (${indexDocId}) into ${dir}`);
    return { dir, peer };
  }

  /** A headless clone: a `git clone` of the hub's repository (checked out on `branch`), attached to the project. */
  async clone(hub: { dir: string; wsUrl: string; indexDocId: string }, branch = 'main'): Promise<{ dir: string; peer: HeadlessPeer }> {
    const parent = await this.dir('ge14-clone-');
    const dir = path.join(parent, 'repo');
    execFileSync('git', ['clone', '--quiet', hub.dir, dir], { stdio: 'ignore', env: { ...process.env, GIT_CONFIG_GLOBAL: '/dev/null' } });
    for (const [k, v] of [
      ['user.name', 'GE Clone'],
      ['user.email', 'ge-clone@example.invalid'],
      ['commit.gpgsign', 'false'],
      ['core.autocrlf', 'false'],
    ] as const) {
      execFileSync('git', ['config', k, v], { cwd: dir });
    }
    if (branch !== 'main') execFileSync('git', ['switch', '--quiet', '-c', branch, `origin/${branch}`], { cwd: dir });
    const peer = await HeadlessPeer.joinAsClone(dir, hub.wsUrl, hub.indexDocId);
    this.cleanups.push(() => peer.stop());
    this.log(`headless clone of ${hub.dir} on ${branch} attached to ${hub.wsUrl} (${hub.indexDocId}) in ${dir}`);
    return { dir, peer };
  }

  async close(): Promise<void> {
    for (const fn of this.cleanups.splice(0).reverse()) await Promise.resolve(fn()).catch(() => {});
  }
}

export { expect, readText, waitFor };

export const read = async (dir: string, rel: string): Promise<string | undefined> => (await readText(path.join(dir, rel))) ?? undefined;

export async function readBytes(dir: string, rel: string): Promise<Buffer | undefined> {
  return fs.readFile(path.join(dir, rel)).catch(() => undefined);
}

/** Wait until `rel` under `dir` satisfies `check` (undefined means the file is absent). */
export async function expectFile(
  dir: string,
  rel: string,
  check: (text: string | undefined) => boolean,
  what: string,
  timeout = 30_000,
): Promise<void> {
  await expect.poll(async () => check(await read(dir, rel)), { timeout, message: what }).toBe(true);
}

/** Wait until the hub's status route reports a decision of one of `kinds`; returns the status. */
export async function decision(hubUrl: string, kinds: string[], timeout = 30_000): Promise<GitStatusBody> {
  let last: GitStatusBody | undefined;
  await expect
    .poll(
      async () => {
        last = await gitStatus(hubUrl);
        return last.decisions.some((d) => kinds.includes(d.kind));
      },
      { timeout, message: `no decision of kind ${kinds.join('|')}` },
    )
    .toBe(true);
  return last!;
}
