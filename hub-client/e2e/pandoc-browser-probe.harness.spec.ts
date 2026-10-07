/**
 * Feature probe for the browsers the pandoc specs run in (host phase H6). It is the first
 * thing to read when a webkit CI job goes red: the loader needs wasm exnref exception
 * handling, `DecompressionStream`, `crypto.subtle` and the Cache API, and Playwright's Linux
 * WebKit build is not the macOS WebKit the spike verified. A failure here names the missing
 * feature and the browser version instead of surfacing as a load error in every other spec.
 *
 * The browser version is attached to the report, so a green run documents what was verified.
 */
import { expect, test } from '@playwright/test';
import type {} from './helpers/testHooks';
import { EXNREF_PROBE } from '../src/pandoc/pandocLoader';

test('the browser has what pandoc.wasm needs', async ({ page, browserName }, testInfo) => {
  await page.goto('/');
  const features = await page.evaluate(async (probe) => {
    const caches_ = typeof caches === 'undefined' ? false : await caches.open('q2-probe').then(() => caches.delete('q2-probe')).catch(() => false);
    return {
      exnref: WebAssembly.validate(new Uint8Array(probe)),
      decompressionStream: typeof DecompressionStream !== 'undefined',
      subtleCrypto: !!globalThis.crypto?.subtle,
      cacheApi: caches_,
      secureContext: globalThis.isSecureContext,
      worker: typeof Worker !== 'undefined',
    };
  }, Array.from(EXNREF_PROBE));
  const version = page.context().browser()?.version() ?? 'unknown';
  testInfo.annotations.push({ type: 'browser', description: `${browserName} ${version}` });
  console.log(`probe: ${browserName} ${version} ${JSON.stringify(features)}`);
  expect(features, `${browserName} ${version}`).toEqual({
    exnref: true,
    decompressionStream: true,
    subtleCrypto: true,
    cacheApi: true,
    secureContext: true,
    worker: true,
  });
});
