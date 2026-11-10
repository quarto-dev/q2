/**
 * Measurement, not a gate (preview VFS contract D2): the cold render of a
 * single-file deck whose theme, partials and bibliography arrive through
 * the miss routes, behind a delay proxy at several round-trip times.
 * Prints wall time to first themed render and the miss-route calls per arm
 * and status. Run with `Q2_LATENCY_MEASURE=1 npx playwright test
 * --project=chromium e2e/demand-vfs-latency.spec.ts`.
 */

import { test, expect, type Page } from '@playwright/test';
import { startPreviewServer } from './helpers/previewServer';
import { startDelayProxy } from './helpers/delayProxy';

test.skip(!process.env.Q2_LATENCY_MEASURE, 'measurement; set Q2_LATENCY_MEASURE=1');

const FILES = [
  {
    path: 'deck.qmd',
    content:
      '---\ntitle: Deck\nbibliography: refs.bib\nformat:\n  html:\n    theme: styles/deck.scss\n---\n\n# Themed\n\nSee @knuth84.\n',
  },
  { path: 'refs.bib', content: '@book{knuth84, author = {Donald Knuth}, title = {T}, year = {1984}}\n' },
  { path: 'styles/deck.scss', content: '/*-- scss:defaults --*/\n@import "partials/vars";\n$body-bg: $deck-bg !default;\n\n/*-- scss:rules --*/\n' },
  { path: 'styles/partials/_vars.scss', content: '$deck-bg: #abcdef;\n@import "colors";\n' },
  { path: 'styles/partials/_colors.scss', content: '$deck-fg: #222;\n' },
];

const background = (page: Page) =>
  page.evaluate(() => {
    const doc = document.querySelector('iframe')?.contentDocument;
    return doc?.body ? getComputedStyle(doc.body).backgroundColor : null;
  });

for (const rtt of [0, 20, 50, 100]) {
  test(`cold render at ${rtt} ms RTT`, async ({ page }) => {
    test.setTimeout(180_000);
    const server = await startPreviewServer({ targetFile: 'deck.qmd', fixtureFiles: FILES });
    const proxy = await startDelayProxy(new URL(server.url), rtt);
    try {
      const t0 = Date.now();
      await page.goto(proxy.url);
      await expect.poll(() => background(page), { timeout: 150_000, intervals: [50] }).toBe('rgb(171, 205, 239)');
      const elapsed = Date.now() - t0;
      const stats = await page.evaluate(() =>
        (window as unknown as { __q2MissStats: () => Promise<Record<string, number>> }).__q2MissStats(),
      );
      console.log(`LATENCY rtt=${rtt}ms cold=${elapsed}ms pulls=${JSON.stringify(stats)}`);
    } finally {
      await proxy.close();
      await server.stop();
    }
  });
}
