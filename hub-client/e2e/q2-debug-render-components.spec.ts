/**
 * E2E test for q2-debug render-components dynamic import + interactivity.
 *
 * Sister test to the static smoke-all fixture under
 * `crates/quarto/tests/smoke-all/q2-debug/render-components-reactji.qmd`.
 * That fixture verifies the button rendered with "❤️ 1" through the
 * declarative smoke-all assertion pipeline. This spec loads the same
 * qmd + tsx and exercises the click — going from "❤️ 1" to "❤️ 2" — using
 * imperative Playwright APIs, the established pattern for interactivity
 * elsewhere in this repo (see share-link-project-set.spec.ts).
 */

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { test, expect } from '@playwright/test';
import {
  bootstrapProjectSet,
  createProjectOnServer,
  seedProjectInBrowser,
  getServerUrl,
} from './helpers/projectFactory';

const FIXTURE_DIR = resolve(
  import.meta.dirname,
  '../../crates/quarto/tests/smoke-all/q2-debug',
);

const qmdContent = readFileSync(
  resolve(FIXTURE_DIR, 'render-components-reactji.qmd'),
  'utf-8',
);
const tsxContent = readFileSync(
  resolve(FIXTURE_DIR, 'reactji.tsx'),
  'utf-8',
);

test.describe('q2-debug render-components', () => {
  test('clicking the reactji counter increments local React state', async ({
    page,
  }) => {
    const serverUrl = getServerUrl();

    const indexDocId = await createProjectOnServer(serverUrl, [
      {
        path: '_quarto.yml',
        content: 'project:\n  type: default\n',
        contentType: 'text',
      },
      {
        path: 'reactji.tsx',
        content: tsxContent,
        contentType: 'text',
      },
      {
        path: 'render-components-reactji.qmd',
        content: qmdContent,
        contentType: 'text',
      },
    ]);

    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);

    await page.goto(
      `/#/p/${localId}/file/${encodeURIComponent('render-components-reactji.qmd')}`,
    );

    const iframe = page.frameLocator('iframe[src*="q2-debug.html"]');
    const counter = iframe.locator('[data-testid="reaction-❤️"]');

    await expect(counter).toBeVisible({ timeout: 30000 });
    await expect(counter).toHaveText('❤️ 1');

    await counter.click();
    await expect(counter).toHaveText('❤️ 2');
  });
});
