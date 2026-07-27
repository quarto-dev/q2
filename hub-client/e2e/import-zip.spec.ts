/**
 * E2E: "Import from ZIP" creates a new project from an uploaded archive.
 *
 * Drives the real landing-page UI through the real browser pipeline
 * (real fflate unzip → createNewProject → Automerge → editor render),
 * which also confirms the feature is unaffected by the fflate/jsdom
 * unit-test quirk noted in ProjectSelector.import.test.tsx.
 *
 * See claude-notes/plans/2026-06-01-import-from-zip.md (bd-apv23).
 */

import { test, expect, type Page } from '@playwright/test';
import { zipSync, strToU8 } from 'fflate';
import { getServerUrl, seedUiVariant } from './helpers/projectFactory';

/**
 * Bring a fresh browser context to the project-selector landing page with
 * a connected project set, so the "Import from ZIP" action is available.
 * Mirrors the bootstrap in share-link-project-set.spec.ts.
 */
async function bootstrapProjectSet(page: Page, syncServer: string): Promise<void> {
  // This spec drives the classic ProjectSelector; the app defaults to the
  // collections home. See seedUiVariant.
  await seedUiVariant(page, 'classic');
  await page.goto('/');
  await expect(page.getByRole('heading', { name: 'Quarto Hub' })).toBeVisible();
  await expect(
    page.getByText(/Get started by creating a new project set/i),
  ).toBeVisible();

  await page.locator('#setup-sync-server').fill(syncServer);
  await page.getByRole('button', { name: /Create New Project Set/i }).click();

  await expect(page.getByRole('heading', { name: 'Your Projects' })).toBeVisible({
    timeout: 20000,
  });
}

/** A minimal Quarto project zipped the way exportProjectAsZip would. */
function buildFixtureZip(): Uint8Array {
  // A tiny valid PNG (1x1) to exercise the binary round-trip.
  const pngBytes = new Uint8Array([
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d,
    0x49, 0x48, 0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01,
    0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00,
    0x0a, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0d, 0x0a, 0x2d, 0xb4, 0x00, 0x00, 0x00, 0x00, 0x49,
    0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
  ]);

  return zipSync(
    {
      '_quarto.yml': strToU8('project:\n  type: default\n'),
      'index.qmd': strToU8(
        [
          '---',
          'title: Imported From Zip',
          '---',
          '',
          '## Hello from an imported zip',
          '',
          'This paragraph came from the uploaded archive.',
        ].join('\n'),
      ),
      'logo.png': pngBytes,
    },
    { level: 6 },
  );
}

test.describe('Import from ZIP', () => {
  // Bootstrap a project set + import + create-project + first render is more
  // than the default 30s budget allows.
  test.setTimeout(60_000);

  test('creates a new project from an uploaded ZIP and renders it', async ({ page }) => {
    const syncServer = getServerUrl();

    await bootstrapProjectSet(page, syncServer);

    // Open the import form.
    await page.getByRole('button', { name: /Import from ZIP/i }).click();

    // Upload the fixture archive from memory.
    await page.getByLabel('ZIP File').setInputFiles({
      name: 'My Imported Project.zip',
      mimeType: 'application/zip',
      buffer: Buffer.from(buildFixtureZip()),
    });

    // The title prefills from the filename.
    await expect(page.locator('#importTitle')).toHaveValue('My Imported Project');

    // Point the new project at the local hub server, then import.
    await page.locator('#importSyncServer').fill(syncServer);
    await page.getByRole('button', { name: /Import Project/i }).click();

    // Creation navigates into the project (/#/p/<localId>).
    await expect.poll(() => page.url(), { timeout: 30000 }).toContain('/p/');

    // The imported files appear in the sidebar (text + binary both landed).
    await expect(page.locator('.file-name', { hasText: 'index.qmd' })).toBeVisible({
      timeout: 15000,
    });
    await expect(page.locator('.file-name', { hasText: 'logo.png' })).toBeVisible();

    // Open index.qmd and confirm the imported content renders in the preview.
    const match = page.url().match(/\/p\/([^/]+)/);
    expect(match).not.toBeNull();
    const localId = match![1];
    await page.goto(`/#/p/${localId}/file/index.qmd`);

    const previewFrame = page.frameLocator('iframe.preview-active');
    await expect(previewFrame.locator('body')).toContainText(
      'Hello from an imported zip',
      { timeout: 30000 },
    );
    await expect(previewFrame.locator('body')).toContainText(
      'came from the uploaded archive',
    );
  });
});
