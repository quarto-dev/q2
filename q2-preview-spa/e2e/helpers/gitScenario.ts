/**
 * Shared scaffolding of the git-epochs hub scenarios (GE-13): start a `--git` hub on the fixture repo, open guests,
 * and read or wait on the hub's disk. Waits are retrying assertions on the state asserted (`expect.poll`), never a
 * fixed sleep.
 */

import { expect, type Browser, type BrowserContext, type Page } from '@playwright/test';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { gitStatus, openFile, openProject, type GitStatusBody } from './editor';
import { startPreviewServer, type PreviewServerHandle } from './previewServer';

export interface HubOptions {
  ui: 'viewer' | 'editor';
  allowEdit?: boolean;
  git?: 'track' | 'ignore';
  /** Colocate jj on the repository before the hub starts. */
  jjColocate?: boolean;
}

/** A hub started with `--git <git>` (default `track`) on the fixture repo, on `main`. */
export function startGitHub(opts: HubOptions): Promise<PreviewServerHandle> {
  return startPreviewServer({ gitRepo: true, git: opts.git ?? 'track', ui: opts.ui, allowEdit: opts.allowEdit, jjColocate: opts.jjColocate });
}

export interface Guest {
  context: BrowserContext;
  page: Page;
}

/** A guest in its own browser context, on the project at `url`; `file` opens in the editor when given. */
export async function openGuest(browser: Browser, url: string, file?: string): Promise<Guest> {
  const context = await browser.newContext();
  const page = await context.newPage();
  await openProject(page, url);
  if (file) await openFile(page, file);
  return { context, page };
}

export const readDisk = (hub: PreviewServerHandle, rel: string): Promise<string> =>
  readFile(path.join(hub.projectDir, rel), 'utf8');

export const readDiskBytes = (hub: PreviewServerHandle, rel: string): Promise<Buffer> =>
  readFile(path.join(hub.projectDir, rel));

/** Wait until the file on the hub's disk satisfies `check` (the hub writes after its 500 ms debounce). */
export async function expectDisk(hub: PreviewServerHandle, rel: string, check: (text: string) => boolean, timeout = 20_000): Promise<void> {
  await expect.poll(async () => check(await readDisk(hub, rel)), { timeout }).toBe(true);
}

/** Wait until the status route reports a decision of one of `kinds`; returns the status. */
export async function waitForDecision(hub: PreviewServerHandle, kinds: string[], timeout = 20_000): Promise<GitStatusBody> {
  let last: GitStatusBody | undefined;
  await expect
    .poll(
      async () => {
        last = await gitStatus(hub.url);
        return last.decisions.some((d) => kinds.includes(d.kind));
      },
      { timeout, message: `no decision of kind ${kinds.join('|')}` },
    )
    .toBe(true);
  return last!;
}
