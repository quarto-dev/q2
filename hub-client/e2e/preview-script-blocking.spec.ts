/**
 * Negative security spec for quarto-dev/q2#128 (bd-sxx1az83).
 *
 * The Safari fix adds `allow-scripts` to the preview iframe sandbox —
 * the classic sandbox-escape combination with `allow-same-origin` —
 * and neutralizes in-document scripts with an injected CSP meta
 * (`script-src 'none'`). This spec pins the guarantee that NO script in
 * preview content executes, in real browsers (jsdom enforces neither the
 * sandbox nor CSP, so this can only be e2e). It runs under both chromium
 * and webkit (see the webkit project's testMatch in playwright.config.ts).
 *
 * Each vector below attempts to mutate the parent document's title. This
 * spec is the regression test for the escape combination: it passes both
 * pre-fix (sandbox blocks scripts) and post-fix (CSP blocks scripts), and
 * fails if the CSP is ever lost while `allow-scripts` remains.
 */

import { test, expect } from '@playwright/test';
import {
  bootstrapProjectSet,
  createProjectOnServer,
  seedProjectInBrowser,
  getServerUrl,
} from './helpers/projectFactory';

test.describe('Preview script blocking (q2#128)', () => {
  test('no script in preview content executes — tag, inline handler, javascript: URL, nested srcdoc', async ({
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
        path: 'index.qmd',
        content: [
          '---',
          'title: Script Blocking',
          '---',
          '',
          '## Script blocking test',
          '',
          '```{=html}',
          '<script id="evil-script">top.document.title = "PWNED-script-tag";</script>',
          '<button type="button" id="evil-inline" onclick="top.document.title=\'PWNED-inline\'">inline handler</button>',
          '<a id="evil-jsurl" href="javascript:top.document.title=\'PWNED-jsurl\'">javascript url</a>',
          '<iframe id="evil-nested" srcdoc="&lt;script&gt;top.document.title=\'PWNED-nested\'&lt;/script&gt;"></iframe>',
          '```',
        ].join('\n'),
        contentType: 'text',
      },
    ]);

    await bootstrapProjectSet(page, serverUrl);
    const localId = await seedProjectInBrowser(page, indexDocId, serverUrl);

    await page.goto(`/#/p/${localId}/file/index.qmd`);

    const previewFrame = page.frameLocator('iframe.preview-active');
    await expect(previewFrame.locator('body')).toContainText('Script blocking test', {
      timeout: 30000,
    });

    // Prove the attack payloads actually made it into the preview DOM —
    // otherwise the no-execution assertions below are vacuous.
    await expect(previewFrame.locator('script#evil-script')).toHaveCount(1);
    await expect(previewFrame.locator('#evil-inline')).toHaveCount(1);
    await expect(previewFrame.locator('#evil-jsurl')).toHaveCount(1);
    await expect(previewFrame.locator('iframe#evil-nested')).toHaveCount(1);

    // The app sets the title from file + project; capture it once the
    // preview has settled, before poking the attack vectors.
    const titleBefore = await page.title();

    // Trigger the click-dependent vectors (the <script> tag and the
    // nested srcdoc iframe would already have run on load if unblocked).
    await previewFrame.locator('#evil-inline').click();
    await previewFrame.locator('#evil-jsurl').click();

    // Give any rogue script a real chance to run.
    await page.waitForTimeout(500);

    expect(await page.title()).toBe(titleBefore);
    expect(await page.title()).not.toContain('PWNED');
  });
});
