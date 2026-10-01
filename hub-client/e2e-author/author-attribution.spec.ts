/**
 * Author-ID transition E2E (plan 2026-09-30-automerge-author-id-transition,
 * Phase 5; strand bd-r62zad5b).
 *
 * Runs against the auth-ON stack from e2e-author/setup.ts (real hub binary +
 * mock OIDC IdP + same-origin static proxy) and, for the parity scenario, an
 * auth-disabled stack. Verifies end to end:
 *
 *  1. Two authenticated users editing concurrently: every change is
 *     attributed to the right server-minted author, actors are random and
 *     unique per document session, and own edits highlight as "me" in the
 *     replay drawer, the preview's Authors overlay, and the preview's
 *     CurrentActorContext (`__COMMENT_DIAG__.me`).
 *  2. The literal bd-6f21d4c6 H4 scenario as a negative test: one user in
 *     two tabs making simultaneous index-doc edits — no duplicate-seq
 *     error, both tabs converge, the self-heal never fires, and the two
 *     tab sessions still carry distinct actors under one author.
 *  3. Auth-disabled parity: two browser profiles derive stable local
 *     authors (authorIdFromUserId), with the same attribution guarantees.
 *  4. Legacy continuity: a pre-transition document (stable actor, no
 *     author anywhere, crafted with raw automerge) renders attribution via
 *     the actor fallback, and because the legacy actor equals the user's
 *     new author ID (D5), old and new edits resolve to ONE identity key.
 */

import 'fake-indexeddb/auto';
import { test, expect, type Browser, type BrowserContext, type Page } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import * as A from '@automerge/automerge';
import { Repo, generateAutomergeUrl, parseAutomergeUrl } from '@automerge/automerge-repo';
import {
  createSyncClient,
  NodeWebSocketClientAdapter,
  type SyncClientCallbacks,
  type FilePayload,
  type Patch,
} from '@quarto/quarto-sync-client';
import {
  bootstrapProjectSet,
  seedProjectInBrowser,
  type ProjectFile,
} from '../e2e/helpers/projectFactory';
import { waitForPreviewRender } from '../e2e/helpers/previewExtraction';
import { AUTHOR_E2E_INFO_PATH, type AuthorE2EInfo, type AuthorE2EUser } from './setup';

const info: AuthorE2EInfo = JSON.parse(readFileSync(AUTHOR_E2E_INFO_PATH, 'utf-8'));

test.setTimeout(180_000);

const FIXTURE_DIR = resolve(
  import.meta.dirname,
  '../../crates/quarto/tests/playwright-fixtures/q2-preview/render-components-comment',
);
const commentQmd = readFileSync(resolve(FIXTURE_DIR, 'render-components-comment.qmd'), 'utf-8');
const commentTsx = readFileSync(resolve(FIXTURE_DIR, 'comment.tsx'), 'utf-8');

const MAIN_QMD = `---
format: q2-preview
---

# Author E2E

Start line.
`;

// ---------------------------------------------------------------------------
// Node-side helpers (sync clients against the real hubs)
// ---------------------------------------------------------------------------

const silentCallbacks: SyncClientCallbacks = {
  onFileAdded(_path: string, _file: FilePayload) {},
  onFileChanged(_path: string, _text: string, _patches: Patch[]) {},
  onBinaryChanged(_path: string, _data: Uint8Array, _mimeType: string) {},
  onFileRemoved(_path: string) {},
  onFilesChange() {},
  onConnectionChange(_connected: boolean) {},
  onError(error: Error) {
    console.error('[author-e2e client]', error.message);
  },
};

/** GET /auth/author with the user's session cookie (the hub-client path). */
async function fetchAuthorId(
  proxyUrl: string,
  session: string,
  projectId: string,
): Promise<string> {
  const res = await fetch(
    `${proxyUrl}/auth/author?project=${encodeURIComponent(projectId)}`,
    { headers: { cookie: `quarto_hub_token=${session}` } },
  );
  if (!res.ok) throw new Error(`/auth/author -> ${res.status}`);
  return ((await res.json()) as { author_id: string }).author_id;
}

/**
 * Create a project on the AUTH hub as the given user: Bearer on the WS (the
 * hub-mcp path), author resolved via the /auth/author cookie path — exactly
 * what the browser does — so the creation changes carry the author too.
 */
async function createProjectOnAuthHub(
  user: AuthorE2EUser,
  files: ProjectFile[],
): Promise<{ indexDocId: string; authorId: string }> {
  let resolved = '';
  const client = createSyncClient(silentCallbacks);
  const result = await client.createNewProject(
    {
      syncServer: info.hubAuthWsUrl,
      files,
      auth: { getBearer: async () => user.google },
      requireOnline: true,
      peerTimeoutMs: 15_000,
    },
    undefined,
    undefined,
    undefined,
    async (indexDocId: string) => {
      resolved = await fetchAuthorId(info.proxyAuthUrl, user.session, indexDocId);
      return resolved;
    },
  );
  await waitForServerDocuments(
    info.hubAuthHttpUrl,
    [result.indexDocId, ...result.files.map((f) => f.docId)],
    { cookie: `quarto_hub_token=${user.session}` },
  );
  await client.disconnect();
  return { indexDocId: result.indexDocId, authorId: resolved };
}

/** Create an authorless project on the auth-DISABLED hub (D8). */
async function createProjectOnOpenHub(files: ProjectFile[]): Promise<string> {
  const client = createSyncClient(silentCallbacks);
  const result = await client.createNewProject({
    syncServer: info.hubOpenWsUrl,
    files,
    requireOnline: true,
    peerTimeoutMs: 15_000,
  });
  await waitForServerDocuments(
    info.hubOpenHttpUrl,
    [result.indexDocId, ...result.files.map((f) => f.docId)],
  );
  await client.disconnect();
  return result.indexDocId;
}

/** Poll the hub's HTTP API until it acknowledges the given documents. */
async function waitForServerDocuments(
  httpUrl: string,
  docIds: string[],
  authHeaders?: Record<string, string>,
  timeoutMs = 20_000,
): Promise<void> {
  const pending = new Set(docIds);
  const deadline = Date.now() + timeoutMs;
  while (pending.size > 0 && Date.now() < deadline) {
    await Promise.all(
      [...pending].map(async (docId) => {
        try {
          const res = await fetch(`${httpUrl}/api/documents/${docId}`, {
            headers: authHeaders ?? {},
          });
          if (res.ok) pending.delete(docId);
        } catch {
          // hub not ready yet
        }
      }),
    );
    if (pending.size > 0) await new Promise((r) => setTimeout(r, 100));
  }
  if (pending.size > 0) {
    throw new Error(`hub never acknowledged documents: ${[...pending].join(', ')}`);
  }
}

type DecodedChange = ReturnType<typeof A.decodeChange>;

/**
 * Observer sync client: connects, waits until the file doc contains
 * `expectText` (a fresh client's `whenReady` can lag the hub's latest
 * heads), then returns the change history.
 */
async function readFileDocChanges(
  wsUrl: string,
  indexDocId: string,
  path: string,
  google?: string,
  expectText?: string,
): Promise<{
  doc: A.Doc<unknown>;
  changes: DecodedChange[];
  indexIdentities?: Record<string, { name: string }>;
  cleanup: () => Promise<void>;
}> {
  const client = createSyncClient(silentCallbacks);
  await client.connect(wsUrl, indexDocId, undefined, undefined, undefined, {
    auth: google ? { getBearer: async () => google } : undefined,
    requireOnline: true,
    peerTimeoutMs: 15_000,
  });
  const handle = client.getFileHandle(path);
  if (!handle) throw new Error(`observer never got a handle for ${path}`);
  await handle.whenReady();
  const indexIdentities = (
    client.getIndexHandle()?.doc() as { identities?: Record<string, { name: string }> } | undefined
  )?.identities;
  const deadline = Date.now() + 20_000;
  for (;;) {
    const doc = handle.doc() as A.Doc<unknown> | undefined;
    if (doc && (!expectText || String((doc as { text?: unknown }).text ?? '').includes(expectText))) {
      const changes = A.getAllChanges(doc).map((bytes) => A.decodeChange(bytes));
      return {
        doc,
        changes,
        indexIdentities,
        cleanup: async () => {
          await client.disconnect();
        },
      };
    }
    if (Date.now() > deadline) {
      throw new Error(`observer's ${path} never contained ${JSON.stringify(expectText)}`);
    }
    await new Promise((r) => setTimeout(r, 100));
  }
}

interface AttributionEntry {
  actor: string;
  seq: number;
  author: string | null | undefined;
  /** The reader-side resolution: author ?? actor→author index ?? bare actor. */
  key: string;
}

/** Resolve every change's attribution key the way Phase 4 readers do. */
function resolveAttribution(doc: A.Doc<unknown>, changes: DecodedChange[]): AttributionEntry[] {
  return changes.map((ch) => ({
    actor: ch.actor,
    seq: ch.seq,
    author: ch.author,
    key: ch.author ?? A.getAuthorForActor(doc, ch.actor) ?? ch.actor,
  }));
}

const actorsFor = (entries: AttributionEntry[], key: string) =>
  new Set(entries.filter((e) => e.key === key).map((e) => e.actor));

// ---------------------------------------------------------------------------
// Browser helpers
// ---------------------------------------------------------------------------

async function newAuthedContext(browser: Browser, session: string): Promise<BrowserContext> {
  const context = await browser.newContext();
  await context.addCookies([
    { name: 'quarto_hub_token', value: session, domain: '127.0.0.1', path: '/' },
  ]);
  // Force the build-time AUTH_ENABLED flag on (App.tsx): this build has no
  // VITE_GOOGLE_CLIENT_ID, but the hub IS auth-on. Must run before page JS.
  await context.addInitScript(() => {
    (window as any).__QUARTO_TEST_AUTH_ENABLED__ = true;
  });
  return context;
}

/** Bootstrap the app and navigate to a file in the project. Returns the local project id. */
async function openProjectFile(
  page: Page,
  proxyUrl: string,
  indexDocId: string,
  filePath: string,
  authenticated: boolean,
): Promise<string> {
  await bootstrapProjectSet(page, {
    stubAuthMe: !authenticated,
    baseUrl: proxyUrl,
  });
  const localId = await seedProjectInBrowser(page, indexDocId, '/ws', 'Author E2E');
  await gotoProjectFile(page, proxyUrl, localId, filePath);
  return localId;
}

/**
 * Navigate to a project file without seeding. Two tabs of the same browser
 * context share IndexedDB, so the second tab must reuse the first's seed —
 * re-adding the project violates the storage index's uniqueness rule.
 */
async function gotoProjectFile(
  page: Page,
  proxyUrl: string,
  localId: string,
  filePath: string,
): Promise<void> {
  await page.goto(`${proxyUrl}/#/p/${localId}/file/${encodeURIComponent(filePath)}`);
  await expect(page.locator('.monaco-editor .view-lines')).toBeVisible({ timeout: 30_000 });
}

async function cursorToDocEnd(page: Page): Promise<void> {
  await page.locator('.monaco-editor .view-lines').click();
  await page.keyboard.press(process.platform === 'darwin' ? 'Meta+ArrowDown' : 'Control+End');
}

async function typeAtDocEnd(page: Page, text: string): Promise<void> {
  await cursorToDocEnd(page);
  await page.keyboard.type(text);
}

const editorText = (page: Page) => page.locator('.monaco-editor .view-lines');

/** Enable the Authors overlay (idempotent per page load). */
async function enableAuthorsOverlay(page: Page): Promise<void> {
  await waitForPreviewRender(page, { kind: 'q2-preview', timeout: 30_000 });
  await page.getByRole('button', { name: 'Authors overlay off' }).click();
  await expect(page.getByRole('button', { name: 'Authors overlay on' })).toBeVisible({
    timeout: 10_000,
  });
  await waitForPreviewRender(page, { kind: 'q2-preview', timeout: 15_000 });
}

/**
 * Read the Authors overlay as a { attributionKey → concatenated text } map.
 * The overlay paints one span per WORD per run, so per-span text assertions
 * can't see whole phrases — aggregate by key instead.
 */
async function overlayTextByKey(page: Page): Promise<Record<string, string>> {
  const frame = page.frames().find((f) => f.url().includes('q2-preview.html'));
  if (!frame) throw new Error('q2-preview iframe not found');
  return frame.evaluate(() => {
    const out: Record<string, string> = {};
    for (const el of Array.from(document.querySelectorAll('span[data-attr-actor]'))) {
      const key = el.getAttribute('data-attr-actor')!;
      out[key] = (out[key] ?? '') + (el.textContent ?? '');
    }
    return out;
  });
}

/** Poll {@link overlayTextByKey} until `key`'s aggregated text contains `text`. */
async function expectOverlayText(
  page: Page,
  key: string,
  text: string,
  timeoutMs = 20_000,
): Promise<void> {
  await expect
    .poll(async () => (await overlayTextByKey(page))[key] ?? '', { timeout: timeoutMs })
    .toContain(text);
}

// ---------------------------------------------------------------------------
// Scenario 1 — two authenticated users, concurrent editing
// ---------------------------------------------------------------------------

test('auth-on: two users — attribution, unique actors, me-highlighting', async ({ browser }) => {
  const { indexDocId, authorId: authorA } = await createProjectOnAuthHub(info.alice, [
    { path: 'main.qmd', content: MAIN_QMD, contentType: 'text' },
    { path: 'render-components-comment.qmd', content: commentQmd, contentType: 'text' },
    { path: 'comment.tsx', content: commentTsx, contentType: 'text' },
  ]);
  const authorB = await fetchAuthorId(info.proxyAuthUrl, info.bob.session, indexDocId);
  expect(authorA).toMatch(/^[0-9a-f]{64}$/);
  expect(authorB).toMatch(/^[0-9a-f]{64}$/);
  expect(authorA).not.toBe(authorB);

  const ctxA = await newAuthedContext(browser, info.alice.session);
  const ctxB = await newAuthedContext(browser, info.bob.session);
  const pageA = await ctxA.newPage();
  const pageB = await ctxB.newPage();

  await openProjectFile(pageA, info.proxyAuthUrl, indexDocId, 'main.qmd', true);
  await openProjectFile(pageB, info.proxyAuthUrl, indexDocId, 'main.qmd', true);
  await expect(editorText(pageA)).toContainText('Start line.');
  await expect(editorText(pageB)).toContainText('Start line.');

  // Sequential concurrent-style editing: alice types, bob sees it, bob
  // types, alice sees it — each in their own browser.
  await typeAtDocEnd(pageA, '\nalpha from alice\n');
  await expect(editorText(pageB)).toContainText('alpha from alice', { timeout: 20_000 });
  await typeAtDocEnd(pageB, '\nbravo from bob\n');
  await expect(editorText(pageA)).toContainText('bravo from bob', { timeout: 20_000 });

  // --- Protocol level: authors + actors on the file doc's changes ---------
  const { doc, changes, indexIdentities, cleanup } = await readFileDocChanges(
    info.hubAuthWsUrl,
    indexDocId,
    'main.qmd',
    info.alice.google,
    'bravo from bob',
  );
  const entries = resolveAttribution(doc, changes);
  const keys = [...new Set(entries.map((e) => e.key))].sort();
  expect(keys).toEqual([authorA, authorB].sort());
  // Random per-session actors: creation client + alice's browser + bob's
  // browser each get their own; none equals an author ID.
  expect(actorsFor(entries, authorA).size).toBeGreaterThanOrEqual(2);
  expect(actorsFor(entries, authorB).size).toBe(1);
  for (const actor of new Set(entries.map((e) => e.actor))) {
    expect([authorA, authorB]).not.toContain(actor);
  }
  // Every post-creation change resolves to an author, never a bare actor:
  // alice's seq-1 carries the footer, her later seqs resolve via the index.
  expect(entries.every((e) => e.key === authorA || e.key === authorB)).toBe(true);
  // The index identities map is keyed by the two authors (D4).
  expect(Object.keys(indexIdentities ?? {}).sort()).toEqual([authorA, authorB].sort());
  await cleanup();

  // --- Preview: Authors overlay keys spans by the right author ------------
  await enableAuthorsOverlay(pageA);
  await expectOverlayText(pageA, authorA, 'alpha from alice');
  await expectOverlayText(pageA, authorB, 'bravo from bob');

  await enableAuthorsOverlay(pageB);
  await expectOverlayText(pageB, authorA, 'alpha from alice');
  await expectOverlayText(pageB, authorB, 'bravo from bob');

  // --- Preview: CurrentActorContext ("me") inside the iframe --------------
  // The comment fixture's diagnostic component publishes useCurrentActor().
  await pageA.goto(`${info.proxyAuthUrl}/#/p/${await localId(pageA)}/file/render-components-comment.qmd`);
  await waitForPreviewRender(pageA, { kind: 'q2-preview', timeout: 30_000 });
  const commentFrameA = pageA
    .frames()
    .find((f) => f.url().includes('q2-preview.html'));
  expect(commentFrameA).toBeDefined();
  await expect
    .poll(async () => commentFrameA!.evaluate(() => (window as never as {
      __COMMENT_DIAG__?: { me: string | null };
    }).__COMMENT_DIAG__?.me))
    .toBe(authorA);

  await pageB.goto(`${info.proxyAuthUrl}/#/p/${await localId(pageB)}/file/render-components-comment.qmd`);
  await waitForPreviewRender(pageB, { kind: 'q2-preview', timeout: 30_000 });
  const commentFrameB = pageB
    .frames()
    .find((f) => f.url().includes('q2-preview.html'));
  await expect
    .poll(async () => commentFrameB!.evaluate(() => (window as never as {
      __COMMENT_DIAG__?: { me: string | null };
    }).__COMMENT_DIAG__?.me))
    .toBe(authorB);

  // --- Replay drawer: --me highlight on own steps -------------------------
  await pageA.goto(`${info.proxyAuthUrl}/#/p/${await localId(pageA)}/file/main.qmd`);
  await expect(editorText(pageA)).toContainText('bravo from bob', { timeout: 20_000 });
  await pageA.locator('.replay-drawer__toggle').click();
  const chipA = pageA.locator('.replay-drawer__actor');
  await expect(chipA).toBeVisible({ timeout: 10_000 });
  // Replay opens on the latest step — bob's change: bob's key, not "me".
  await expect(chipA).toHaveAttribute('data-actor-key', authorB);
  await expect(chipA).toHaveAttribute('data-current-actor', authorA);
  await expect(chipA).not.toHaveClass(/replay-drawer__actor--me/);
  // Step back to a step by alice: her key, and the --me class must appear.
  await stepBackUntilKey(pageA, authorA);
  await expect(chipA).toHaveAttribute('data-actor-key', authorA);
  await expect(chipA).toHaveClass(/replay-drawer__actor--me/);

  // Mirror for bob: latest step IS his → his key and --me immediately;
  // stepping back to alice's step drops the highlight.
  await pageB.goto(`${info.proxyAuthUrl}/#/p/${await localId(pageB)}/file/main.qmd`);
  await expect(editorText(pageB)).toContainText('bravo from bob', { timeout: 20_000 });
  await pageB.locator('.replay-drawer__toggle').click();
  const chipB = pageB.locator('.replay-drawer__actor');
  await expect(chipB).toBeVisible({ timeout: 10_000 });
  await expect(chipB).toHaveAttribute('data-actor-key', authorB);
  await expect(chipB).toHaveAttribute('data-current-actor', authorB);
  await expect(chipB).toHaveClass(/replay-drawer__actor--me/);
  await stepBackUntilKey(pageB, authorA);
  await expect(chipB).toHaveAttribute('data-actor-key', authorA);
  await expect(chipB).not.toHaveClass(/replay-drawer__actor--me/);

  await ctxA.close();
  await ctxB.close();
});

/**
 * ArrowLeft through the replay drawer until the displayed step's
 * attribution key equals `key` (React renders asynchronously, so poll the
 * DOM attribute rather than trusting a fixed press count).
 */
async function stepBackUntilKey(page: Page, key: string): Promise<void> {
  const chip = page.locator('.replay-drawer__actor');
  for (let i = 0; i < 120; i++) {
    if ((await chip.getAttribute('data-actor-key')) === key) return;
    await page.keyboard.press('ArrowLeft');
    await page.waitForTimeout(120);
  }
  throw new Error(`replay never reached a step keyed ${key.slice(0, 12)}…`);
}

/** The local project id is embedded in the current URL hash. */
async function localId(page: Page): Promise<string> {
  const hash = new URL(page.url()).hash;
  const match = hash.match(/#\/p\/([^/]+)\//);
  if (!match) throw new Error(`no project id in URL ${page.url()}`);
  return match[1]!;
}

// ---------------------------------------------------------------------------
// Scenario 2 — H4 negative test: one user, two tabs, simultaneous edits
// ---------------------------------------------------------------------------

test('auth-on: H4 negative — two tabs, one user, no duplicate-seq', async ({ browser }) => {
  const { indexDocId, authorId: authorA } = await createProjectOnAuthHub(info.alice, [
    { path: 'main.qmd', content: MAIN_QMD, contentType: 'text' },
  ]);

  const ctx = await newAuthedContext(browser, info.alice.session);
  const tab1 = await ctx.newPage();
  const tab2 = await ctx.newPage();

  // Capture the failure signatures of the pre-transition bug and its
  // self-heal: both must stay absent for the whole scenario.
  const failureSignatures: string[] = [];
  for (const [name, page] of [['tab1', tab1], ['tab2', tab2]] as const) {
    page.on('pageerror', (err) => failureSignatures.push(`${name} pageerror: ${err}`));
    page.on('console', (msg) => {
      const text = msg.text();
      if (/duplicate seq|recovered index document/i.test(text)) {
        failureSignatures.push(`${name} console: ${text}`);
      }
    });
  }

  const localId = await openProjectFile(tab1, info.proxyAuthUrl, indexDocId, 'main.qmd', true);
  // Same context: shared IndexedDB, so tab2 reuses tab1's seed (a second
  // addProject would violate the storage index's uniqueness rule).
  await gotoProjectFile(tab2, info.proxyAuthUrl, localId, 'main.qmd');
  await expect(editorText(tab1)).toContainText('Start line.');
  await expect(editorText(tab2)).toContainText('Start line.');

  // Simultaneous edits in both tabs (the bd-6f21d4c6 collision shape).
  await Promise.all([typeAtDocEnd(tab1, '\ntab one edit\n'), typeAtDocEnd(tab2, '\ntab two edit\n')]);

  // Both tabs converge to a document containing both edits.
  await expect(editorText(tab1)).toContainText('tab one edit', { timeout: 20_000 });
  await expect(editorText(tab1)).toContainText('tab two edit', { timeout: 20_000 });
  await expect(editorText(tab2)).toContainText('tab one edit', { timeout: 20_000 });
  await expect(editorText(tab2)).toContainText('tab two edit', { timeout: 20_000 });
  expect(failureSignatures).toEqual([]);

  // Protocol: one author, but three distinct actors (creation + tab1 +
  // tab2) — the post-transition invariant that removes H4 at the source.
  const { doc, changes, cleanup } = await readFileDocChanges(
    info.hubAuthWsUrl,
    indexDocId,
    'main.qmd',
    info.alice.google,
    'tab two edit',
  );
  const entries = resolveAttribution(doc, changes);
  expect([...new Set(entries.map((e) => e.key))]).toEqual([authorA]);
  const actors = new Set(entries.map((e) => e.actor));
  expect(actors.size).toBeGreaterThanOrEqual(3);
  expect(actors.has(authorA)).toBe(false);
  await cleanup();

  // No self-heal fired during the whole scenario (also covers the sync).
  expect(failureSignatures).toEqual([]);
  await ctx.close();
});

// ---------------------------------------------------------------------------
// Scenario 3 — auth-disabled parity: two browser profiles
// ---------------------------------------------------------------------------

/** Read the context's local identity and derive its stable local author. */
async function readLocalAuthor(page: Page): Promise<string> {
  return page.evaluate(async () => {
    const db = await new Promise<IDBDatabase>((res, rej) => {
      const req = indexedDB.open('quarto-hub');
      req.onsuccess = () => res(req.result);
      req.onerror = () => rej(req.error);
    });
    const value = await new Promise<unknown>((res, rej) => {
      const tx = db.transaction('userSettings', 'readonly');
      const get = tx.objectStore('userSettings').get('identity');
      get.onsuccess = () => res(get.result);
      get.onerror = () => rej(get.error);
    });
    db.close();
    const userId = (value as { userId?: string } | undefined)?.userId;
    if (!userId) throw new Error('no userId in userSettings/identity');
    // authorIdFromUserId (hub-client/src/services/userSettings.ts)
    const stripped = userId.replace(/-/g, '').toLowerCase();
    if (/^[0-9a-f]+$/.test(stripped) && stripped.length % 2 === 0) return stripped;
    return Array.from(new TextEncoder().encode(userId))
      .map((b) => b.toString(16).padStart(2, '0'))
      .join('');
  });
}

test('auth-disabled: two profiles — local authors, unique actors', async ({ browser }) => {
  const indexDocId = await createProjectOnOpenHub([
    { path: 'main.qmd', content: MAIN_QMD, contentType: 'text' },
  ]);

  // Two independent browser profiles (separate storage ⇒ separate userIds).
  const ctxA = await browser.newContext();
  const ctxB = await browser.newContext();
  const pageA = await ctxA.newPage();
  const pageB = await ctxB.newPage();

  await openProjectFile(pageA, info.proxyOpenUrl, indexDocId, 'main.qmd', false);
  await openProjectFile(pageB, info.proxyOpenUrl, indexDocId, 'main.qmd', false);
  await expect(editorText(pageA)).toContainText('Start line.');
  await expect(editorText(pageB)).toContainText('Start line.');

  const localA = await readLocalAuthor(pageA);
  const localB = await readLocalAuthor(pageB);
  expect(localA).not.toBe(localB);

  await typeAtDocEnd(pageA, '\nprofile one edit\n');
  await expect(editorText(pageB)).toContainText('profile one edit', { timeout: 20_000 });
  await typeAtDocEnd(pageB, '\nprofile two edit\n');
  await expect(editorText(pageA)).toContainText('profile two edit', { timeout: 20_000 });

  const { doc, changes, cleanup } = await readFileDocChanges(
    info.hubOpenWsUrl,
    indexDocId,
    'main.qmd',
    undefined,
    'profile two edit',
  );
  const entries = resolveAttribution(doc, changes);
  // Both local authors present, each with its own random session actor.
  expect(actorsFor(entries, localA).size).toBe(1);
  expect(actorsFor(entries, localB).size).toBe(1);
  for (const actor of new Set(entries.map((e) => e.actor))) {
    expect([localA, localB]).not.toContain(actor);
  }
  // The authorless creation change (D8) decodes with author null and is
  // still attributable via the bare-actor fallback.
  expect(changes[0]!.author ?? null).toBeNull();
  expect(entries[0]!.key).toBe(entries[0]!.actor);
  await cleanup();

  // Overlay: profile A's own edit is keyed by its local author.
  await enableAuthorsOverlay(pageA);
  await expectOverlayText(pageA, localA, 'profile one edit');
  await expectOverlayText(pageA, localB, 'profile two edit');

  await ctxA.close();
  await ctxB.close();
});

// ---------------------------------------------------------------------------
// Scenario 4 — legacy document: actor fallback + cross-boundary continuity
// ---------------------------------------------------------------------------

test('auth-on: legacy doc renders via actor fallback, continuous identity', async ({
  browser,
}) => {
  // Craft a pre-transition project offline: stable actor, no author
  // anywhere. The actor is alice's author ID for the project (D5 — the
  // author ID is byte-identical to the legacy stable actor), which is what
  // makes attribution continuous across the transition.
  const legacyIndexId = parseAutomergeUrl(generateAutomergeUrl()).documentId;
  const legacyFileId = parseAutomergeUrl(generateAutomergeUrl()).documentId;
  const legacyKey = await fetchAuthorId(info.proxyAuthUrl, info.alice.session, legacyIndexId);

  interface LegacyIndex {
    files: Record<string, string>;
    version: number;
    identities: Record<string, { name: string; color: string }>;
    folders: Record<string, true>;
  }
  let fileDoc = A.from<{ text: string }>({ text: 'legacy first line\n' }, { actor: legacyKey });
  fileDoc = A.change(fileDoc, (d) => {
    d.text += 'legacy second line\n';
  });
  let indexDoc = A.from<LegacyIndex>(
    { files: {}, version: 3, identities: {}, folders: {} },
    { actor: legacyKey },
  );
  indexDoc = A.change(indexDoc, (d) => {
    d.files['legacy.qmd'] = legacyFileId;
    d.identities[legacyKey] = { name: 'Alice Legacy', color: '#3366aa' };
  });
  // Pin the crafting itself: no change carries an author footer.
  for (const bytes of A.getAllChanges(fileDoc)) {
    expect(A.decodeChange(bytes).author ?? null).toBeNull();
    expect(A.decodeChange(bytes).actor).toBe(legacyKey);
  }

  // Upload both documents to the auth hub as alice (Bearer path).
  const repo = new Repo({
    network: [
      new NodeWebSocketClientAdapter(info.hubAuthWsUrl, {
        getBearer: async () => info.alice.google,
      }),
    ],
  });
  repo.import(A.save(indexDoc), { docId: legacyIndexId });
  repo.import(A.save(fileDoc), { docId: legacyFileId });
  await waitForServerDocuments(
    info.hubAuthHttpUrl,
    [legacyIndexId, legacyFileId],
    { cookie: `quarto_hub_token=${info.alice.session}` },
  );
  repo.shutdown();

  const ctxA = await newAuthedContext(browser, info.alice.session);
  const pageA = await ctxA.newPage();
  await openProjectFile(pageA, info.proxyAuthUrl, legacyIndexId, 'legacy.qmd', true);
  await expect(editorText(pageA)).toContainText('legacy first line');

  // Legacy text renders attribution via the actor fallback: the spans carry
  // the bare actor — which equals alice's author ID, so her new edit lands
  // on the SAME attribution key.
  await enableAuthorsOverlay(pageA);
  await expectOverlayText(pageA, legacyKey, 'legacy first line');
  await expectOverlayText(pageA, legacyKey, 'legacy second line');

  await typeAtDocEnd(pageA, 'alice new line\n');
  await waitForPreviewRender(pageA, { kind: 'q2-preview', timeout: 15_000 });
  await expectOverlayText(pageA, legacyKey, 'alice new line');

  // One identity across the boundary: every attribution span on the page
  // carries the same key.
  const spanKeys = Object.keys(await overlayTextByKey(pageA));
  expect(spanKeys).toEqual([legacyKey]);

  // Replay drawer: every step — legacy and new alike — highlights as "me",
  // because both resolve to the same key the app knows as the current user.
  await pageA.locator('.replay-drawer__toggle').click();
  const chip = pageA.locator('.replay-drawer__actor');
  await expect(chip).toBeVisible({ timeout: 10_000 });
  await expect(chip).toHaveAttribute('data-actor-key', legacyKey);
  await expect(chip).toHaveAttribute('data-current-actor', legacyKey);
  await expect(chip).toHaveClass(/replay-drawer__actor--me/);
  // Walk to the very first step (a legacy change, author: null on the
  // change itself); the --me highlight must persist via the actor fallback.
  for (let i = 0; i < 120; i++) {
    const position = await pageA.locator('.replay-drawer__position').innerText();
    if (position.trim().startsWith('1/')) break;
    await pageA.keyboard.press('ArrowLeft');
    await pageA.waitForTimeout(120);
  }
  await expect(pageA.locator('.replay-drawer__position')).toHaveText(/^1\//);
  await expect(chip).toHaveAttribute('data-actor-key', legacyKey);
  await expect(chip).toHaveClass(/replay-drawer__actor--me/);

  // Protocol pin: the browser's new change carries the author footer equal
  // to the legacy actor (continuity at the change level, not just the UI).
  const { doc, changes, cleanup } = await readFileDocChanges(
    info.hubAuthWsUrl,
    legacyIndexId,
    'legacy.qmd',
    info.alice.google,
    'alice new line',
  );
  const entries = resolveAttribution(doc, changes);
  expect([...new Set(entries.map((e) => e.key))]).toEqual([legacyKey]);
  const legacyEntries = entries.filter((e) => e.actor === legacyKey);
  const browserEntries = entries.filter((e) => e.actor !== legacyKey);
  expect(legacyEntries.length).toBeGreaterThanOrEqual(2);
  expect(legacyEntries.every((e) => (e.author ?? null) === null)).toBe(true);
  expect(browserEntries.length).toBeGreaterThanOrEqual(1);
  expect(browserEntries.some((e) => e.author === legacyKey)).toBe(true);
  await cleanup();

  await ctxA.close();
});
