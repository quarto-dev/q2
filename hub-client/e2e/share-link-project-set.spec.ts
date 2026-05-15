/**
 * Red tests for: share link does not add project to receiver's synced project set.
 *
 * Bug reference: bd-xgb4 (see claude-notes/plans/2026-04-16-share-link-project-not-added.md)
 *
 * Scenario: a user with an existing synced project set visits a #/share/... URL.
 * The document opens and can be edited, but it is never added to the synced
 * project set, so it does not appear in the project list on the landing page.
 * A subsequent attempt to add the same document via the "Connect to Project"
 * form fails with "Failed to add project. The document ID may already exist."
 * because the share flow already wrote to IndexedDB and the Connect handler
 * does not dedupe before calling projectStorage.addProject.
 *
 * Regression coverage for the reconciler fix. Both tests were initially
 * introduced as `test.fail()` to confirm the diagnosis; they were flipped
 * to `test()` once the reconciler landed.
 */

import { test, expect, type Page } from '@playwright/test';
import {
  createProjectOnServer,
  getServerUrl,
} from './helpers/projectFactory';
import type {} from './helpers/testHooks';

/**
 * Bootstrap the receiver browser with an existing synced project set.
 *
 * This is the critical precondition for reproducing the bug: when the user
 * already has a project set connected, ProjectSelector renders its list from
 * the synced entries (not from IDB), so a share-flow entry that only makes it
 * into IDB is invisible.
 *
 * We drive the UI rather than poking IDB/Automerge directly — this lets the
 * app's own `createProjectSet` → `setProjectSetPointer` → `setStatus('connected')`
 * sequence happen atomically inside `useProjectSet`, which sidesteps the race
 * where a hand-rolled createProjectSet + reload can land before the server
 * has acknowledged the new doc.
 */
async function bootstrapReceiverProjectSet(
  page: Page,
  syncServer: string,
): Promise<void> {
  await page.goto('/');
  await expect(page.locator('body')).toBeVisible();

  // Fresh browser context lands on the first-time-setup screen.
  await expect(
    page.getByRole('heading', { name: 'Quarto Hub' }),
  ).toBeVisible();
  await expect(
    page.getByText(/Get started by creating a new project set/i),
  ).toBeVisible();

  // Point at the local hub server (the input defaults to the public server).
  await page.locator('#setup-sync-server').fill(syncServer);
  await page
    .getByRole('button', { name: /Create New Project Set/i })
    .click();

  // When the project set finishes connecting, ProjectSelector renders.
  await expect(
    page.getByRole('heading', { name: 'Your Projects' }),
  ).toBeVisible({ timeout: 20000 });
  await expect(
    page.getByRole('button', { name: /Connect to Project/i }),
  ).toBeVisible();
}

/** Build the #/share/... URL that a sender would have produced. */
function buildShareHash(
  indexDocId: string,
  syncServer: string,
  filePath: string,
  name: string,
): string {
  const params = new URLSearchParams();
  params.set('server', syncServer);
  params.set('file', filePath);
  params.set('name', name);
  return `/#/share/${encodeURIComponent(indexDocId)}?${params.toString()}`;
}

test.describe('Share link → synced project set', () => {
  // Each test does: create-on-server + bootstrap-project-set + share-nav +
  // return-to-selector + optional form interaction. That's too much work for
  // Playwright's default 30s per-test budget; 60s keeps us comfortably clear.
  test.setTimeout(60_000);

  test(
    'share link adds the project to the receiver\'s synced project set',
    async ({ browser }) => {
      const syncServer = getServerUrl();

      // Create the "shared" project on the hub server.
      const sharedIndexDocId = await createProjectOnServer(syncServer, [
        {
          path: '_quarto.yml',
          content: 'project:\n  type: default\n',
          contentType: 'text',
        },
        {
          path: 'index.qmd',
          content: [
            '---',
            'title: Share Link Demo',
            '---',
            '',
            '## Hello from share link',
            '',
            'Shared content.',
          ].join('\n'),
          contentType: 'text',
        },
      ]);

      // Fresh browser context = fresh IndexedDB = "new receiver browser".
      // Two pages share IndexedDB within the context, but each page is a
      // fresh React mount — App.tsx's share handler is gated on a
      // `initialLoadRef` that only runs once per mount, so we must visit the
      // share URL in a page that hasn't mounted the app before.
      const receiver = await browser.newContext();

      try {
        // Step 1: bootstrap the receiver's existing synced project set in a
        // throwaway page, then close it.
        const bootstrapPage = await receiver.newPage();
        await bootstrapReceiverProjectSet(bootstrapPage, syncServer);
        await bootstrapPage.close();

        // Step 2: open the share URL in a fresh page. The synced project set
        // pointer persists in IDB across pages in the same context.
        const page = await receiver.newPage();
        const shareHash = buildShareHash(
          sharedIndexDocId,
          syncServer,
          'index.qmd',
          'Share Link Demo',
        );
        await page.goto(shareHash);

        // The share handler redirects to the file route — wait until we're
        // no longer on the raw /#/share/... URL (proves the handler fired).
        await expect
          .poll(() => page.url().includes('/#/share/'), { timeout: 15000 })
          .toBe(false);

        // Step 3: go back to the selector and assert the shared project is
        // visible in the list rendered from the synced set.
        await page.goto('/');
        await expect(
          page.getByRole('heading', { name: 'Your Projects' }),
        ).toBeVisible();
        await expect(page.getByText('Share Link Demo')).toBeVisible({
          timeout: 10000,
        });
      } finally {
        await receiver.close();
      }
    },
  );

  test(
    'after visiting a share link, re-adding the same doc via Connect form is idempotent',
    async ({ browser }) => {
      const syncServer = getServerUrl();

      const sharedIndexDocId = await createProjectOnServer(syncServer, [
        {
          path: '_quarto.yml',
          content: 'project:\n  type: default\n',
          contentType: 'text',
        },
        {
          path: 'index.qmd',
          content: '---\ntitle: Connect Form Demo\n---\n\n## Hello\n',
          contentType: 'text',
        },
      ]);

      const receiver = await browser.newContext();

      try {
        // Bootstrap in a throwaway page — see the explanation in the other test.
        const bootstrapPage = await receiver.newPage();
        await bootstrapReceiverProjectSet(bootstrapPage, syncServer);
        await bootstrapPage.close();

        // Visit the share link in a fresh page so the App.tsx share handler
        // actually runs (it's gated on a once-per-mount ref).
        const sharePage = await receiver.newPage();
        const shareHash = buildShareHash(
          sharedIndexDocId,
          syncServer,
          'index.qmd',
          'Connect Form Demo',
        );
        await sharePage.goto(shareHash);

        // Poll IDB directly until the share handler has persisted the entry.
        await expect
          .poll(
            () =>
              sharePage.evaluate(async (docId) => {
                await window.__quartoTestReady;
                const hooks = window.__quartoTest;
                if (!hooks) throw new Error('__quartoTest missing — rebuild with VITE_E2E=1');
                const entry = await hooks.projectStorage.getProjectByIndexDocId(`automerge:${docId}`);
                return !!entry;
              }, sharedIndexDocId),
            { timeout: 15000 },
          )
          .toBe(true);
        await sharePage.close();

        // Open the Connect form in yet another fresh page — same IDB (so the
        // entry from the share flow is still there), fresh React mount (so
        // we land cleanly on the ProjectSelector without stale Editor state).
        const page = await receiver.newPage();
        await page.goto('/');
        await expect(
          page.getByRole('heading', { name: 'Your Projects' }),
        ).toBeVisible({ timeout: 20000 });
        await page.getByRole('button', { name: /Connect to Project/i }).click();

        await page.locator('#indexDocId').fill(sharedIndexDocId);
        await page.locator('#syncServer').fill(syncServer);
        await page.getByRole('button', { name: 'Connect', exact: true }).click();

        // The misleading error text must NOT appear.
        await expect(
          page.getByText(/Failed to add project\. The document ID may already exist\./),
        ).toHaveCount(0, { timeout: 5000 });
      } finally {
        await receiver.close();
      }
    },
  );

  test(
    'reconciler adopts an orphan IDB entry into the connected project set',
    async ({ browser }) => {
      // This test exercises the reconciler path deterministically. Bug A is
      // timing-dependent (the share handler races the project-set connect),
      // so a naive share-URL test can sometimes pass even on broken code if
      // the local websocket connects fast. Here we simulate the end state
      // Bug A leaves behind — a project in IDB that never reached the set —
      // and assert the reconciler picks it up on the next `connected` tick.
      const syncServer = getServerUrl();

      const orphanIndexDocId = await createProjectOnServer(syncServer, [
        {
          path: '_quarto.yml',
          content: 'project:\n  type: default\n',
          contentType: 'text',
        },
        {
          path: 'index.qmd',
          content: '---\ntitle: Orphan Demo\n---\n',
          contentType: 'text',
        },
      ]);

      const receiver = await browser.newContext();

      try {
        const bootstrapPage = await receiver.newPage();
        await bootstrapReceiverProjectSet(bootstrapPage, syncServer);

        // Seed IDB directly with an entry that the synced project set knows
        // nothing about — this is exactly the state Bug A produces.
        await bootstrapPage.evaluate(
          async ({ indexDocId, server }) => {
            await window.__quartoTestReady;
            const hooks = window.__quartoTest;
            if (!hooks) throw new Error('__quartoTest missing — rebuild with VITE_E2E=1');
            await hooks.projectStorage.addProject(`automerge:${indexDocId}`, server, 'Orphan Demo');
          },
          { indexDocId: orphanIndexDocId, server: syncServer },
        );
        await bootstrapPage.close();

        // Fresh page — triggers useProjectSet init → status='connected' →
        // the reconciler effect fires and should upsert the orphan IDB row.
        const page = await receiver.newPage();
        await page.goto('/');
        await expect(
          page.getByRole('heading', { name: 'Your Projects' }),
        ).toBeVisible({ timeout: 20000 });

        await expect(page.getByText('Orphan Demo')).toBeVisible({
          timeout: 10000,
        });
      } finally {
        await receiver.close();
      }
    },
  );
});
