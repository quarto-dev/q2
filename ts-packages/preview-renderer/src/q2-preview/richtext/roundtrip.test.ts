// Phase 1 (bd-sjb4pzx8) — production round-trip fidelity guard for the rich-text
// bridge. Mirrors the Phase-0 spike but exercises the REAL production modules
// (astToDoc + docToMarkdown against the tiptap-named schema):
//
//   qmd -> pampa AST -> astToDoc -> docToMarkdown -> pampa AST -> compare
//
// DISABLED BY DEFAULT (bd-d8nol0xn): shells out to the native `pampa` binary via
// the test-utils oracle (slow, and intermittently flaky under the parallel load of
// a full `vitest run` / `cargo xtask verify`). This is a fidelity oracle, not a
// routine gate. Opt in (and only runs when native pampa is buildable):
//   QUARTO_RUN_PAMPA_ROUNDTRIP=1 npx vitest run src/q2-preview/richtext/roundtrip.test.ts
// (from ts-packages/preview-renderer).

import { describe, it, expect } from 'vitest';
import { astToDoc } from './astToProseMirror';
import { docToMarkdown } from './serializer';
import { parseUntransformed, astEqual, pampaAvailable } from '../../test-utils/pampaOracle';

const FIXTURES: { name: string; qmd: string }[] = [
  { name: 'plain-para', qmd: 'A simple paragraph of prose.\n' },
  { name: 'inline-formatting', qmd: 'A para with **bold**, *italic*, `code`, and a [link](https://example.com).\n' },
  { name: 'nested-emphasis', qmd: 'Text with **_bold italic_** and a [**bold link**](https://x.com).\n' },
  { name: 'sub-sup-strike', qmd: 'Water H~2~O, exponent 2^10^, and ~~struck~~ text.\n' },
  { name: 'atx-heading', qmd: '## A second-level heading\n' },
  { name: 'heading-with-marks', qmd: '### A **bold** heading with `code` and a [link](https://x.com)\n' },
  { name: 'bullet-list', qmd: '- first item\n- second item\n- third item\n' },
  { name: 'ordered-list', qmd: '3. three\n4. four\n5. five\n' },
  { name: 'blockquote', qmd: '> a quoted line\n> still quoted\n' },
  // bd-iwv3708i — Quoted inlines now seed as editable plaintext straight quotes
  // (not chips); they must still round-trip back to a `Quoted` node via pampa.
  { name: 'quoted-double', qmd: 'He said "smart quotes" and it works.\n' },
  { name: 'quoted-single', qmd: 'A phrase with \'single quotes\' too.\n' },
  { name: 'quoted-nested', qmd: 'Nested: "outer \'inner\' done" here.\n' },
  { name: 'quoted-with-marks', qmd: 'A quote with **bold** inside: "very *important* text".\n' },
  { name: 'code-block-quarto', qmd: '```{python}\nprint("hi")\n```\n' },
  { name: 'shortcode', qmd: 'Watch {{< video https://youtu.be/abc >}} now.\n' },
  { name: 'inline-math', qmd: 'Identity $e^{i\\pi}+1=0$ is neat.\n' },
  { name: 'crossref', qmd: 'See @fig-plot for details.\n' },
  { name: 'citation', qmd: 'Established [@knuth1984].\n' },
  { name: 'raw-html-inline', qmd: 'Text with <span class="x">raw</span> inside.\n' },
];

// The hard gate is SEMANTIC equivalence — the round-trip must not drop or change
// nodes. Byte-exact round-tripping is a nice-to-have, not required (cosmetic
// reformatting like `***x***` -> `**_x_**`, or `-` -> `*` bullets, is acceptable);
// we log it for visibility but do not fail on it.
describe.skipIf(!process.env.QUARTO_RUN_PAMPA_ROUNDTRIP || !pampaAvailable())('rich-text bridge round-trip', () => {
  for (const fx of FIXTURES) {
    it(`preserves semantics: ${fx.name}`, () => {
      const astIn = parseUntransformed(fx.qmd);
      const { doc, unknown } = astToDoc(astIn.blocks, astIn.astContext.p, fx.qmd);
      const mdOut = docToMarkdown(doc);
      const astOut = parseUntransformed(mdOut);

      const equivalent = astEqual(astIn.blocks, astOut.blocks, { normalize: true });
      const exact = astEqual(astIn.blocks, astOut.blocks, { normalize: false });

      if (!equivalent) {
        // eslint-disable-next-line no-console
        console.error(`\n${fx.name} SEMANTIC BREAK\n--- qmd ---\n${fx.qmd}\n--- md_out ---\n${mdOut}`);
      } else if (!exact) {
        // eslint-disable-next-line no-console
        console.log(`${fx.name}: equivalent (cosmetic reformat)`);
      }
      expect(unknown, `${fx.name}: unmapped node types`).toEqual([]);
      expect(equivalent, `${fx.name}: round-trip changed/dropped a node`).toBe(true);
    });
  }
});
