/**
 * The pandoc.wasm parity net in a browser (pandoc-host H3): each P7 docx golden fixture is
 * seeded into the page's VFS, run through the dev harness (`__quartoTest.pandoc.download`:
 * Rust `render_pandoc_request` -> worker -> pandoc.wasm -> a real download), saved through
 * Playwright's `download` event, and compared with the Q1 golden by R0's extractor CLI.
 * The Node twin is src/pandoc/goldenParity.wasm.test.ts.
 *
 * Needs a VITE_E2E=1 build with hub-client/public/pandoc/pandoc.wasm.gz (`npm run test:harness`)
 * and `cargo build -p quarto-output-extract`.
 */
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import type {} from './helpers/testHooks';
import { findExtractor, fixtureFiles, goldenFixtures, referenceFor, snapshotName } from '../src/test-utils/goldenFixtures';

const SDE = 1_700_000_000;
const extractor = findExtractor();
const outDir = process.env.Q2_PARITY_OUT ?? mkdtempSync(path.join(os.tmpdir(), 'q2-parity-browser-'));
mkdirSync(outDir, { recursive: true });

test('has the extractor (required in CI)', () => {
  if (!extractor && process.env.CI) throw new Error('quarto-output-extract missing: cargo build -p quarto-output-extract');
  test.skip(!extractor, 'quarto-output-extract missing: cargo build -p quarto-output-extract');
});

for (const fixture of goldenFixtures()) {
  test(`docx parity: ${fixture.qmd}`, async ({ page }) => {
    test.skip(!extractor, 'quarto-output-extract missing');
    await page.goto('/');
    await page.waitForFunction(() => !!window.__quartoTestReady);
    await page.evaluate(async () => {
      await window.__quartoTestReady;
    });

    const files = fixtureFiles(fixture).map((f) => ({ path: f.path, base64: Buffer.from(f.bytes).toString('base64') }));
    const downloaded = page.waitForEvent('download');
    const summary = await page.evaluate(
      async ({ files, qmd, sde }) => {
        const { wasmRenderer, pandoc } = window.__quartoTest!;
        await wasmRenderer.initWasm();
        wasmRenderer.vfsClear();
        for (const f of files) {
          const bin = atob(f.base64);
          wasmRenderer.vfsAddBinaryFile(`/project/${f.path}`, Uint8Array.from(bin, (c) => c.charCodeAt(0)));
        }
        const r = await pandoc.download(`/project/${qmd}`, { format: 'docx', sourceDateEpoch: sde });
        return { ok: r.ok, failure: r.failure, requestError: r.requestError, diagnostics: r.diagnostics, fileName: r.fileName, timings: r.timings, bytes: r.output?.byteLength };
      },
      { files, qmd: fixture.qmd, sde: SDE },
    );
    expect(summary.ok, JSON.stringify(summary)).toBe(true);

    const download = await downloaded;
    const stem = path.basename(fixture.qmd, '.qmd');
    expect(download.suggestedFilename()).toBe(`${stem}.docx`);
    const out = path.join(outDir, `${snapshotName(fixture.qmd, 'docx')}.docx`);
    await download.saveAs(out);

    const actual = execFileSync(extractor!, ['extract', out], { maxBuffer: 1 << 28 }).toString('utf8').trimEnd();
    const reference = referenceFor(fixture);
    expect(actual, `differs from ${reference.source} (output kept at ${out})`).toBe(reference.text);
    test.info().annotations.push({ type: 'timings', description: JSON.stringify(summary.timings) });
  });
}
