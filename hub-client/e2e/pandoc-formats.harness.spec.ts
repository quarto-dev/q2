/**
 * The pandoc.wasm parity net for typst, pptx and epub in a browser (pandoc-host H3, formats
 * half): R0's recorded documents are seeded into the page's VFS at `/__q2_doc__`, run through
 * the dev harness (`__quartoTest.pandoc.download`: Rust request -> worker -> pandoc.wasm -> a
 * real download), saved through Playwright's `download` event and compared with the
 * recording's native reference by R0's extractor CLI. Node twin:
 * src/pandoc/recordingParity.wasm.test.ts.
 *
 * Needs a VITE_E2E=1 build with the pandoc asset (`npm run test:harness`) and
 * `cargo build -p quarto-output-extract`.
 */
import { mkdirSync, mkdtempSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { expect, test } from '@playwright/test';
import type {} from './helpers/testHooks';
import { extractText, findExtractor } from '../src/test-utils/goldenFixtures';
import { DOC_DIR, recordedDocs, referenceText } from '../src/test-utils/recordingFixtures';

const SDE = 1_700_000_000;
const extractor = findExtractor();
const outDir = process.env.Q2_PARITY_OUT ?? mkdtempSync(path.join(os.tmpdir(), 'q2-formats-browser-'));
mkdirSync(outDir, { recursive: true });

test('has the extractor (required in CI)', () => {
  if (!extractor && process.env.CI) throw new Error('quarto-output-extract missing: cargo build -p quarto-output-extract');
  test.skip(!extractor, 'quarto-output-extract missing: cargo build -p quarto-output-extract');
});

for (const doc of recordedDocs()) {
  test(`format parity: ${doc.name}`, async ({ page }) => {
    test.skip(!extractor, 'quarto-output-extract missing');
    await page.goto('/');
    await page.waitForFunction(() => !!window.__quartoTestReady);
    await page.evaluate(async () => {
      await window.__quartoTestReady;
    });

    const files = doc.files.map((f) => ({ path: f.path, base64: Buffer.from(f.bytes).toString('base64') }));
    const downloaded = page.waitForEvent('download');
    const summary = await page.evaluate(
      async ({ files, qmd, format, dir, sde }) => {
        const { wasmRenderer, pandoc } = window.__quartoTest!;
        await wasmRenderer.initWasm();
        wasmRenderer.vfsClear();
        for (const f of files) wasmRenderer.vfsAddBinaryFile(`${dir}/${f.path}`, Uint8Array.from(atob(f.base64), (c) => c.charCodeAt(0)));
        const r = await pandoc.download(`${dir}/${qmd}`, { format, sourceDateEpoch: sde });
        return { ok: r.ok, failure: r.failure, requestError: r.requestError, diagnostics: r.diagnostics, fileName: r.fileName, bytes: r.output?.byteLength };
      },
      { files, qmd: doc.qmd, format: doc.format, dir: DOC_DIR, sde: SDE },
    );
    expect(summary.ok, JSON.stringify(summary)).toBe(true);

    const download = await downloaded;
    expect(download.suggestedFilename()).toBe(`${path.basename(doc.qmd, '.qmd')}.${doc.outputExt}`);
    const out = path.join(outDir, `${doc.name}.${doc.outputExt}`);
    await download.saveAs(out);

    const actual = extractText(extractor!, out);
    const expected = referenceText(doc, (f) => extractText(extractor!, f));
    expect(actual, `differs from ${doc.referencePath} (output kept at ${out})`).toBe(expected);
  });
}
