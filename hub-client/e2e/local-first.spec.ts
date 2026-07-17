/**
 * Local-first / connection-gated auth E2E (A7v1, bd-a1gpy16v).
 *
 * Verifies the headline of the connection-gated local-first work through a
 * real browser: with no IdP configured, the SPA opens straight into a usable
 * project selector (no login gate), a project can be created fully locally,
 * and it persists across a reload. The account-level control offers
 * "Connect to a hub" rather than gating the whole app.
 *
 * The Create form's Sync Server URL field is editable (restored per
 * bd-u4p8xhdc follow-up bd-ivkf752c) but MUST default to empty when not
 * connected to a hub — this test asserts that default rather than clearing
 * the field, because a non-empty default is exactly the regression that
 * shipped once already (bd-ivkf752c follow-up): it silently turns local
 * creation into a hub-creation attempt with no session, which
 * createNewProject's resolveActorId callback doesn't abort on, so the
 * project gets created and wired to a real WS adapter, then immediately
 * torn down by App.tsx's auth-loss-teardown effect — a "flash" of the
 * editor before bouncing back to the selector.
 *
 * The hub-connect leg (sign in, open/create a hub project) requires a live
 * OIDC provider and is verified manually — see the plan's A7v1 notes.
 *
 * Plan: claude-notes/plans/2026-07-06-hub-client-connection-gated-local-first.md
 */

import { test, expect } from '@playwright/test';

test.describe('Local-first (connection-gated auth)', () => {
  test('opens with no login gate and offers "Connect to a hub"', async ({ page }) => {
    await page.goto('/');

    // The project selector renders immediately — no LoginScreen gate.
    await expect(page.getByRole('heading', { name: 'Your Projects' })).toBeVisible();

    // The account-level control offers connecting to a hub (disconnected
    // state), not a full-screen sign-in gate.
    await expect(page.getByRole('button', { name: /^connect to a hub$/i })).toBeVisible();

    // The local-first create/import actions are available with no auth.
    await expect(page.getByRole('button', { name: /create new project/i })).toBeVisible();
  });

  test('creates a local project offline and persists it across a reload', async ({ page }) => {
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'Your Projects' })).toBeVisible();

    // Open the create form and pick the first available project type.
    await page.getByRole('button', { name: /create new project/i }).click();
    const typeSelect = page.locator('#projectType');
    await expect(typeSelect).toBeVisible();
    await typeSelect.selectOption({ index: 0 });

    // The create form's Sync Server URL field is editable, but must default
    // to empty (local) since we're not connected to a hub — left untouched
    // here so this test guards the default, not just the override.
    const syncServerInput = page.getByLabel(/sync server url/i);
    await expect(syncServerInput).toBeVisible();
    await expect(syncServerInput).toHaveValue('');

    const title = `Local Project ${Date.now()}`;
    await page.locator('#projectTitle').fill(title);
    await page.getByRole('button', { name: /^create project$/i }).click();

    // We navigate into the newly-created project (the selector is replaced
    // by the editor). The project id lands in the URL hash (route: /#/p/<id>).
    await expect(page).toHaveURL(/#\/p\//, { timeout: 30000 });

    // Reload: the local project set + entry live in IndexedDB, so the
    // project must still be listed with no server round-trip.
    await page.goto('/');
    await expect(page.getByRole('heading', { name: 'Your Projects' })).toBeVisible();
    await expect(page.getByText(title)).toBeVisible({ timeout: 30000 });
  });
});
