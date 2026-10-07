/**
 * Browser-versus-native parity (host phase H9): the typst.ts compiler wasm (typst 0.14.2) and a
 * native typst 0.14.2 binary compile the same recorded `.typ` fixtures with the same fonts; the
 * criterion is equal page count and equal text content, not layout. Compiling the recorded
 * pandoc-3.11 typst writer output at all is also the writer-versus-compiler skew check (T2).
 *
 * The native binary is `$TYPST_PARITY_BIN`, else `typst` on PATH; it must report exactly the version
 * in `resources/typst-wasm.json`. The test is skipped without one, unless `TYPST_PARITY_REQUIRED=1`
 * (CI), which turns "not available" into a failure. Text is read with the pdf.js build that
 * `scripts/fetch-pandoc-wasm.mjs` places in `hub-client/public/pdfjs/`.
 *
 * Known differences (not asserted): layout and glyph positions; PDF object ids and dates; native
 * typst reads system fonts unless told not to, so both runs use only the default and vendored fonts.
 */
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { TypstSession } from './session.ts';
import { defaultFonts, recordedTypst, repo, typstModule, vendoredPackages } from './fixtures.test-util.ts';

const pinned = JSON.parse(readFileSync(path.join(repo, 'resources/typst-wasm.json'), 'utf8')).typst_version as string;
const bin = process.env.TYPST_PARITY_BIN ?? 'typst';
const required = process.env.TYPST_PARITY_REQUIRED === '1';
const pdfjsMain = path.join(repo, 'hub-client/public/pdfjs/build/pdf.mjs');
const pdfjsWorker = path.join(repo, 'hub-client/public/pdfjs/build/pdf.worker.mjs');

function nativeVersion(): string | null {
  try {
    return /typst (\S+)/.exec(execFileSync(bin, ['--version'], { encoding: 'utf8' }))?.[1] ?? null;
  } catch {
    return null;
  }
}

const version = nativeVersion();
const problem = version === null ? `no native typst (${bin})` : version !== pinned ? `native typst is ${version}, the browser compiler is ${pinned}` : !existsSync(pdfjsMain) ? 'pdf.js is not fetched (node scripts/fetch-pandoc-wasm.mjs)' : null;
if (problem && required) throw new Error(`TYPST_PARITY_REQUIRED=1 but ${problem}`);

const FIXTURES = ['callouts', 'citations', 'crossrefs', 'images', 'shortcodes', 'tables'];

async function pdfFacts(pdf: Uint8Array): Promise<{ pages: number; text: string }> {
  const pdfjs = await import(/* @vite-ignore */ pathToFileURL(pdfjsMain).href);
  pdfjs.GlobalWorkerOptions.workerSrc = pathToFileURL(pdfjsWorker).href;
  const task = pdfjs.getDocument({ data: pdf.slice(), useSystemFonts: false, verbosity: 0 });
  const doc = await task.promise;
  const parts: string[] = [];
  for (let i = 1; i <= doc.numPages; i++) {
    const content = await (await doc.getPage(i)).getTextContent();
    parts.push(content.items.map((it: { str?: string }) => it.str ?? '').join(' '));
  }
  const pages = doc.numPages as number;
  await task.destroy();
  // Content, not layout: collapse whitespace (line breaks and spacing differ with positions).
  return { pages, text: parts.join('\n').replace(/\s+/g, ' ').trim() };
}

describe.skipIf(problem !== null)('browser typst versus native typst', () => {
  let session: TypstSession;
  let work: string;
  beforeAll(async () => {
    session = await TypstSession.create({ module: await typstModule(), fonts: defaultFonts(), vendoredPackages: vendoredPackages() });
    work = mkdtempSync(path.join(tmpdir(), 'typst-parity-'));
    mkdirSync(path.join(work, 'fonts'));
    defaultFonts().forEach((f, i) => writeFileSync(path.join(work, 'fonts', `font-${String(i).padStart(3, '0')}.otf`), f));
  }, 60_000);
  afterAll(() => rmSync(work, { recursive: true, force: true }));

  for (const name of FIXTURES) {
    it(`${name}: same page count and text`, async () => {
      const fixture = recordedTypst(name);
      const browser = await session.compile(fixture);
      if (!browser.ok) throw new Error(`browser compile failed: ${JSON.stringify(browser.diagnostics)}`);

      const root = path.join(work, name);
      for (const f of fixture.files) {
        const dest = path.join(root, f.path);
        mkdirSync(path.dirname(dest), { recursive: true });
        writeFileSync(dest, f.bytes);
      }
      const out = path.join(root, 'native.pdf');
      execFileSync(bin, ['compile', '--root', root, '--ignore-system-fonts', '--font-path', path.join(work, 'fonts'), path.join(root, fixture.main), out], { stdio: 'pipe' });

      const a = await pdfFacts(browser.pdf);
      const b = await pdfFacts(new Uint8Array(readFileSync(out)));
      expect(a.pages).toBe(b.pages);
      expect(a.text).toBe(b.text);
      expect(a.text.length).toBeGreaterThan(0);
    }, 60_000);
  }
});

it('lists the fixtures it compares (every recorded typst run)', () => {
  const dir = path.join(repo, 'crates/quarto-core/tests/fixtures/pandoc-recordings/recordings');
  expect(readdirSync(dir).filter((d) => d.endsWith('-typst')).map((d) => d.replace(/-typst$/, '')).sort()).toEqual([...FIXTURES].sort());
});
