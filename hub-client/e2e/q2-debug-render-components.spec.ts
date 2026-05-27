/**
 * E2E test for q2-debug render-components dynamic import + interactivity.
 *
 * Fixture: `crates/quarto/tests/playwright-fixtures/q2-debug/`. Moved
 * out of `smoke-all/` once it became clear the smoke-all assertion
 * (data-testid presence) was strictly subsumed by this spec's
 * `.toBeVisible()` check. The whole test surface for this fixture now
 * lives in this script — see also `q2-preview-render-components-comment`
 * and `q2-preview-render-components-write` for the sibling pattern,
 * and `claude-notes/instructions/testing.md` for the
 * smoke-all vs playwright-fixtures distinction.
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
  '../../crates/quarto/tests/playwright-fixtures/q2-debug',
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
