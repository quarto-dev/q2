import { beforeAll, describe, expect, it } from 'vitest';
import { TypstSession } from './session.ts';
import { defaultFonts, recordedTypst, text, typstModule, vendoredPackages } from './fixtures.test-util.ts';

let session: TypstSession;
beforeAll(async () => {
  session = await TypstSession.create({ module: await typstModule(), fonts: defaultFonts(), vendoredPackages: vendoredPackages() });
}, 60_000);

const one = (src: string) => ({ main: '/main.typ', files: [{ path: '/main.typ', bytes: text(src) }] });

describe('PDF from the recorded native typst runs', () => {
  // Page counts from native typst 0.14.2 on the same fonts (typst-assets defaults + vendored Font
  // Awesome, `--ignore-system-fonts`), compiling the same recorded `.typ`.
  const cases: [string, number][] = [
    ['callouts', 2],
    ['citations', 1],
    ['crossrefs', 2],
    ['images', 1],
    ['shortcodes', 2],
    ['tables', 1],
  ];
  for (const [name, pages] of cases) {
    it(`${name}: a valid PDF of ${pages} page(s)`, async () => {
      const r = await session.compile(recordedTypst(name));
      if (!r.ok) throw new Error(JSON.stringify(r.diagnostics));
      expect(new TextDecoder('latin1').decode(r.pdf.subarray(0, 8))).toMatch(/^%PDF-1\.\d/);
      expect(new TextDecoder('latin1').decode(r.pdf.subarray(-6))).toContain('%%EOF');
      expect(r.pages).toBe(pages);
    });
  }
});

describe('fonts', () => {
  it('loads the default fonts and reports their families', () => {
    expect(session.fontFamilies).toEqual(expect.arrayContaining(['Libertinus Serif', 'New Computer Modern', 'DejaVu Sans Mono']));
    expect(session.fontFamilies.some((f) => f.startsWith('Font Awesome 6'))).toBe(true);
  });

  it('refuses to compile with no fonts (typst would write a text-less PDF without a diagnostic)', async () => {
    const bare = await TypstSession.create({ module: await typstModule(), fonts: [] });
    const r = await bare.compile(one('Hello'));
    expect(r).toMatchObject({ ok: false, kind: 'invalid-input' });
    expect(r.diagnostics[0]).toMatchObject({ code: 'no-fonts' });
  });

  it('puts text in the PDF (page text uses the loaded fonts)', async () => {
    const r = await session.compile(one('#set text(font: "Libertinus Serif")\nHello'));
    expect(r.ok).toBe(true);
    // Embedded font programs are in the file only when a loaded font was used.
    if (r.ok) expect(new TextDecoder('latin1').decode(r.pdf)).toMatch(/\/FontFile3|\/FontFile2/);
  });
});

describe('the pinned compiler', () => {
  it('is typst 0.14.2 (writer-versus-compiler skew guard, T2)', async () => {
    const ok = await session.compile(one('#assert(sys.version == version(0, 14, 2))\nok'));
    if (!ok.ok) throw new Error(`the wasm is not typst 0.14.2: ${JSON.stringify(ok.diagnostics)}`);
    // Negative control: the assertion does fail on a different version.
    expect(await session.compile(one('#assert(sys.version == version(0, 14, 3))'))).toMatchObject({ ok: false, kind: 'typst-error' });
  });
});

describe('diagnostics', () => {
  it('reports a typst error with its position', async () => {
    const r = await session.compile(one('= Title\n#let x = (1 + "a")\n#x'));
    expect(r).toMatchObject({ ok: false, kind: 'typst-error' });
    expect(r.diagnostics[0]).toMatchObject({ origin: 'typst', kind: 'error', stage: 'typst', path: '/main.typ' });
  });

  it('rejects input that breaks a mount rule, before any compile', async () => {
    for (const files of [[{ path: 'main.typ', bytes: text('x') }], [{ path: '/a/../main.typ', bytes: text('x') }], [{ path: '/@memory/x.typ', bytes: text('x') }]]) {
      const r = await session.compile({ main: files[0].path, files });
      expect(r).toMatchObject({ ok: false, kind: 'invalid-input' });
    }
    expect(await session.compile({ main: '/missing.typ', files: [{ path: '/main.typ', bytes: text('x') }] })).toMatchObject({ ok: false, kind: 'invalid-input' });
  });

  it('enforces the total-bytes limit', async () => {
    const small = await TypstSession.create({ module: await typstModule(), fonts: defaultFonts(), limits: { total_bytes: 8 } });
    expect(await small.compile(one('Hello, a document longer than eight bytes'))).toMatchObject({ ok: false, kind: 'invalid-input' });
  });
});
