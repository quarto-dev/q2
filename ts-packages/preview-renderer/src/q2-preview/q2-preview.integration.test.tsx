/**
 * q2-preview registry contract — Plan 2B Phase 5.1, extended in 2C.
 *
 * 2B fills the empty Plan-2A registry with real-HTML leaves for every
 * Pandoc base type. 2C adds the CustomBlock / CustomInline dispatchers
 * and the type-keyed CustomNode components (Callout, Theorem, ...) +
 * the `__fallback__` entry. After 2C, an unknown CustomNode type_name
 * routes through `__fallback__` (a styled box that displays the
 * type_name) rather than the muted-gray "(not yet implemented)"
 * placeholder; the placeholder still fires for unregistered Pandoc tags.
 *
 * Per plan §"Test replacement note", four Plan-2A tests are replaced
 * here:
 *  - "renders a top-level block as a muted-gray placeholder" → real
 *    Para → `<p>` assertion.
 *  - "recurses into children so nested inlines also surface
 *    placeholders" → real Str → text assertion.
 *  - "uses the muted-gray aesthetic on the placeholder DOM" → narrowed
 *    to an unknown-type_name CustomNode falling through __fallback__.
 *  - "renders registry containing only {Ast, Block, Inline}" →
 *    asserts the post-2C registry shape (Pandoc base tags present,
 *    CustomBlock/CustomInline present, CustomNode type_name keys
 *    present).
 *
 * Pandoc-base gap-fill component tests, Image edge cases, atomic
 * CustomNode read-only, recursion-contract bypass, reference-
 * preservation, and class-compatibility tests are appended below.
 */

import { describe, it, expect, vi } from 'vitest';
import { render, fireEvent } from '@testing-library/react';
import { Ast } from '../framework';
import type {
    BlockNode,
    InlineNode,
    NodeArgs,
    ParaBlock,
    PandocAST,
    FormatRegistry,
} from '../framework';
import { previewRegistry } from './registry';
import { AssetManifestContext } from './AssetManifestContext';
import {
    SECTION,
    SECTION_LEVEL_PREFIX,
    FOOTNOTE_REF,
} from './quartoClasses';

function astJson(blocks: any[]): string {
    const ast: PandocAST = {
        'pandoc-api-version': [1, 23, 0],
        meta: {},
        blocks: blocks as any,
    };
    return JSON.stringify(ast);
}

const noopNav = () => {};
const noopSet = () => {};

function mount(blocks: any[]) {
    return render(
        <Ast
            astJson={astJson(blocks)}
            currentFilePath="/project/test.qmd"
            onNavigateToDocument={noopNav}
            setAst={noopSet}
            registry={previewRegistry}
        />,
    );
}

const STR = (c: string) => ({ t: 'Str', c });
const PARA = (...inlines: any[]) => ({ t: 'Para', c: inlines });

describe('q2-preview registry — Pandoc base types render as real HTML', () => {
    it('renders Para → <p>', () => {
        const { container } = mount([PARA(STR('hello'))]);
        const p = container.querySelector('p');
        expect(p).not.toBeNull();
        expect(p!.textContent).toBe('hello');
    });

    it('renders Str inlines as text content (no placeholder)', () => {
        const { container } = mount([PARA(STR('hello'))]);
        expect(container.textContent).toBe('hello');
        expect(container.textContent).not.toContain('not yet implemented');
    });

    it('renders unknown CustomNode type_name through the Fallback styled box (not the placeholder)', () => {
        // After 2C, CustomBlock/CustomInline dispatchers route to the
        // `__fallback__` entry on miss. The fallback emits a styled
        // box displaying the unknown type_name; it is NOT the
        // muted-gray "(not yet implemented)" placeholder used for
        // unregistered Pandoc tags.
        const customBlockAst = JSON.stringify({
            'pandoc-api-version': [1, 23, 0],
            meta: {},
            blocks: [
                {
                    t: 'CustomBlock',
                    type_name: 'UnknownExtension',
                    slots: {},
                    plain_data: null,
                    attr: ['', [], []],
                },
            ],
        });
        const { container } = render(
            <Ast
                astJson={customBlockAst}
                currentFilePath="/project/test.qmd"
                onNavigateToDocument={noopNav}
                setAst={noopSet}
                registry={previewRegistry}
            />,
        );
        // No muted-gray placeholder fires — the fallback handles the
        // unknown type_name.
        expect(container.querySelector('.q2-preview-placeholder')).toBeNull();
        // The fallback emits a div displaying the type_name text.
        expect(container.textContent).toContain('UnknownExtension');
    });

    it('registry contains Pandoc base tags + CustomBlock/CustomInline + type-keyed CustomNode components', () => {
        const keys = new Set(Object.keys(previewRegistry));
        // Required by the framework
        expect(keys.has('Ast')).toBe(true);
        expect(keys.has('Block')).toBe(true);
        expect(keys.has('Inline')).toBe(true);
        // 2B leaves: blocks
        for (const tag of ['Para', 'Plain', 'Header', 'CodeBlock', 'BulletList', 'OrderedList', 'BlockQuote', 'Div', 'HorizontalRule', 'RawBlock', 'Figure', 'LineBlock', 'DefinitionList', 'Table']) {
            expect(keys.has(tag)).toBe(true);
        }
        // 2B leaves: inlines
        for (const tag of ['Str', 'Space', 'SoftBreak', 'LineBreak', 'Emph', 'Strong', 'Code', 'Link', 'Image', 'Span', 'Quoted', 'Math', 'Underline', 'Strikeout', 'Superscript', 'Subscript', 'SmallCaps', 'RawInline', 'Cite', 'Note']) {
            expect(keys.has(tag)).toBe(true);
        }
        // 2C dispatchers
        expect(keys.has('CustomBlock')).toBe(true);
        expect(keys.has('CustomInline')).toBe(true);
        // 2C type-keyed CustomNode components
        for (const tag of ['Callout', 'Theorem', 'Proof', 'FloatRefTarget', 'Equation', 'CrossrefResolvedRef']) {
            expect(keys.has(tag)).toBe(true);
        }
        // 2C fallback
        expect(keys.has('__fallback__')).toBe(true);
    });
});

describe('q2-preview Pandoc base-type gap-fill components', () => {
    it('LineBlock → <div class="line-block"> with each line a <div>', () => {
        const ast = [{
            t: 'LineBlock',
            c: [
                [STR('line one')],
                [STR('line two')],
            ],
        }];
        const { container } = mount(ast);
        const lb = container.querySelector('.line-block');
        expect(lb).not.toBeNull();
        const lines = lb!.querySelectorAll(':scope > div');
        expect(lines).toHaveLength(2);
        expect(lines[0].textContent).toBe('line one');
        expect(lines[1].textContent).toBe('line two');
    });

    it('DefinitionList → <dl> with siblings <dt>/<dd>', () => {
        const ast = [{
            t: 'DefinitionList',
            c: [
                [[STR('term1')], [[PARA(STR('def1a'))], [PARA(STR('def1b'))]]],
                [[STR('term2')], [[PARA(STR('def2'))]]],
            ],
        }];
        const { container } = mount(ast);
        const dl = container.querySelector('dl');
        expect(dl).not.toBeNull();
        const dts = dl!.querySelectorAll('dt');
        const dds = dl!.querySelectorAll('dd');
        expect(dts).toHaveLength(2);
        expect(dds).toHaveLength(3); // term1 has two defs, term2 has one
        expect(dts[0].textContent).toBe('term1');
        expect(dts[1].textContent).toBe('term2');
    });

    it('Underline / Strikeout / Superscript / Subscript / SmallCaps render canonical tags', () => {
        const ast = [PARA(
            { t: 'Underline', c: [STR('u')] },
            { t: 'Strikeout', c: [STR('s')] },
            { t: 'Superscript', c: [STR('sup')] },
            { t: 'Subscript', c: [STR('sub')] },
            { t: 'SmallCaps', c: [STR('sc')] },
        )];
        const { container } = mount(ast);
        expect(container.querySelector('u')!.textContent).toBe('u');
        expect(container.querySelector('s')!.textContent).toBe('s');
        expect(container.querySelector('sup')!.textContent).toBe('sup');
        expect(container.querySelector('sub')!.textContent).toBe('sub');
        const sc = container.querySelector('span[style*="small-caps"]');
        expect(sc).not.toBeNull();
        expect(sc!.textContent).toBe('sc');
    });

    it('RawInline (format=html) injects raw HTML; non-html falls back to <code>', () => {
        const html = [PARA({ t: 'RawInline', c: ['html', '<b>bold</b>'] })];
        const tex = [PARA({ t: 'RawInline', c: ['tex', '\\textbf{bold}'] })];
        const r1 = mount(html).container;
        const r2 = mount(tex).container;
        expect(r1.querySelector('b')).not.toBeNull();
        expect(r2.querySelector('code')!.textContent).toBe('\\textbf{bold}');
    });

    it('Cite renders c[1] inlines (visible link text); ignores citations metadata', () => {
        const ast = [PARA({
            t: 'Cite',
            c: [
                [{ id: 'citekey', mode: { t: 'NormalCitation' } }],
                [STR('@citekey')],
            ],
        })];
        const { container } = mount(ast);
        expect(container.textContent).toBe('@citekey');
    });

    it('Quoted (DoubleQuote / SingleQuote) wraps inlines in curly quotes', () => {
        const ast = [PARA(
            { t: 'Quoted', c: [{ t: 'DoubleQuote' }, [STR('hello')]] },
            STR(' '),
            { t: 'Quoted', c: [{ t: 'SingleQuote' }, [STR('world')]] },
        )];
        const { container } = mount(ast);
        expect(container.textContent).toBe('“hello” ‘world’');
    });

    it('Header renders the correct hN tag based on level', () => {
        const ast = [
            { t: 'Header', c: [1, ['h1-id', [], []], [STR('one')]] },
            { t: 'Header', c: [3, ['', ['cls'], []], [STR('three')]] },
            { t: 'Header', c: [6, ['', [], []], [STR('six')]] },
        ];
        const { container } = mount(ast);
        expect(container.querySelector('h1')!.id).toBe('h1-id');
        expect(container.querySelector('h3')!.className).toBe('cls');
        expect(container.querySelector('h6')!.textContent).toBe('six');
    });

    it('OrderedList → <ol> with start attribute when not 1', () => {
        const ast = [{
            t: 'OrderedList',
            c: [
                [3, { t: 'Decimal' }, { t: 'Period' }],
                [[PARA(STR('three'))], [PARA(STR('four'))]],
            ],
        }];
        const { container } = mount(ast);
        const ol = container.querySelector('ol');
        expect(ol).not.toBeNull();
        expect(ol!.getAttribute('start')).toBe('3');
    });

    it('CodeBlock puts the language class on <pre>, leaving <code> bare (matches q2 render)', () => {
        // bd-y1fs3: q2 render's HTML writer (see
        // `crates/pampa/src/writers/html.rs::Block::CodeBlock`)
        // writes `<pre class="…"><code>…</code></pre>` — classes on
        // the outer container, bare <code>. The React renderer must
        // match so Quarto theme rules (e.g. `pre.sourceCode > code`)
        // resolve identically across the two pipelines.
        const ast = [{
            t: 'CodeBlock',
            c: [['', ['python'], []], 'print("hi")'],
        }];
        const { container } = mount(ast);
        const pre = container.querySelector('pre');
        const code = pre?.querySelector('code');
        expect(pre).not.toBeNull();
        expect(code).not.toBeNull();
        expect(pre!.className).toBe('python');
        expect(code!.className).toBe('');
        expect(code!.textContent).toBe('print("hi")');
    });

    // ─── bd-nxslt: code-cell syntax highlighting in q2 preview ──────────
    //
    // Mirrors the HTML writer's `write_highlighted_body` in
    // `crates/pampa/src/writers/html.rs`. CodeHighlightStage (Rust
    // side) annotates `CodeBlock.attr.kvs` with `data-hl-spans`,
    // whose value is the JSON triple-array format defined in
    // `crates/quarto-highlight-encoding`: `[[start_byte, end_byte,
    // capture_name], ...]`. The React component must read that
    // attribute and render nested `<span class="hl-CAP">…</span>`
    // with `.` replaced by `-` in the capture (`function.builtin`
    // → `hl-function-builtin`). Plain text falls through unchanged
    // when the attribute is absent or empty.

    it('CodeBlock renders highlight spans when data-hl-spans is present', () => {
        // `cat("hi")` — 9 bytes. Spans: cat=function (0,3),
        // "(" and ")" = punctuation.bracket (3,4) (8,9), "hi" inside
        // the string = string (4,8). Mirrors what the R grammar
        // would emit.
        const text = 'cat("hi")';
        const spans = [
            [0, 3, 'function'],
            [3, 4, 'punctuation.bracket'],
            [4, 8, 'string'],
            [8, 9, 'punctuation.bracket'],
        ];
        const ast = [{
            t: 'CodeBlock',
            c: [
                ['', ['r'], [['data-hl-spans', JSON.stringify(spans)]]],
                text,
            ],
        }];
        const { container } = mount(ast);
        const pre = container.querySelector('pre');
        const code = pre?.querySelector('code');
        expect(pre).not.toBeNull();
        expect(code).not.toBeNull();

        // bd-s3z1g: highlighted code blocks emit Pandoc's nested
        // structure — `<div class="sourceCode"><pre class="sourceCode lang">
        // <code class="sourceCode lang">…</code></pre></div>` — matching
        // the native writer (`write_highlighted_codeblock` in
        // `crates/pampa/src/writers/html.rs`). The div wrapper is what
        // Quarto's theme CSS keys off for the rounded background; the
        // `sourceCode` class on `<code>` is what `pre.sourceCode > code`
        // rules need.
        const divWrapper = pre!.parentElement;
        expect(divWrapper).not.toBeNull();
        expect(divWrapper!.tagName).toBe('DIV');
        expect(divWrapper!.className.split(/\s+/)).toContain('sourceCode');
        expect(pre!.className.split(/\s+/)).toContain('sourceCode');
        expect(pre!.className.split(/\s+/)).toContain('r');
        expect(code!.className.split(/\s+/)).toContain('sourceCode');
        expect(code!.className.split(/\s+/)).toContain('r');

        // Span structure: cat in hl-function, parens in
        // hl-punctuation-bracket, inner literal in hl-string.
        const highlightSpans = code!.querySelectorAll('span[class^="hl-"]');
        expect(highlightSpans.length).toBe(4);
        expect(highlightSpans[0].className).toBe('hl-function');
        expect(highlightSpans[0].textContent).toBe('cat');
        expect(highlightSpans[1].className).toBe('hl-punctuation-bracket');
        expect(highlightSpans[1].textContent).toBe('(');
        expect(highlightSpans[2].className).toBe('hl-string');
        expect(highlightSpans[2].textContent).toBe('"hi"');
        expect(highlightSpans[3].className).toBe('hl-punctuation-bracket');
        expect(highlightSpans[3].textContent).toBe(')');

        // Whole-text round-trip: every character is preserved.
        expect(code!.textContent).toBe('cat("hi")');

        // Raw `data-hl-spans` attribute must NOT leak through as a
        // DOM attribute. Matches the Rust HTML writer's behavior
        // (`!output.html().contains("data-hl-spans=")` in
        // crates/quarto-core/tests/integration/render_to_html_user_grammars.rs).
        expect(pre!.hasAttribute('data-hl-spans')).toBe(false);
        expect(code!.hasAttribute('data-hl-spans')).toBe(false);
    });

    it('CodeBlock places id and non-language classes on the div wrapper (highlighted)', () => {
        // bd-s3z1g: mirrors the native parity test
        // `highlighted_codeblock_non_language_classes_move_to_div` in
        // `crates/pampa/src/writers/html.rs`. First class in the AST
        // attr is the language; remaining classes (e.g. `cell-code`)
        // move to the outer div, alongside `sourceCode`. Id also goes
        // on the div, not on the pre.
        const spans = [[0, 3, 'function']];
        const ast = [{
            t: 'CodeBlock',
            c: [
                ['cb1', ['r', 'cell-code'], [['data-hl-spans', JSON.stringify(spans)]]],
                'cat("hi")',
            ],
        }];
        const { container } = mount(ast);
        const pre = container.querySelector('pre')!;
        const divWrapper = pre.parentElement!;

        expect(divWrapper.tagName).toBe('DIV');
        const divClasses = divWrapper.className.split(/\s+/);
        expect(divClasses).toContain('sourceCode');
        expect(divClasses).toContain('cell-code');
        expect(divWrapper.getAttribute('id')).toBe('cb1');

        // <pre> carries only sourceCode + language, no id.
        const preClasses = pre.className.split(/\s+/);
        expect(preClasses).toContain('sourceCode');
        expect(preClasses).toContain('r');
        expect(preClasses).not.toContain('cell-code');
        expect(pre.hasAttribute('id')).toBe(false);
    });

    it('CodeBlock falls back to plain text when data-hl-spans is absent', () => {
        // Behaviorally identical to the existing "renders <pre><code
        // class=lang>" test, but explicit about the no-highlight
        // path so a regression that incorrectly fires the highlighter
        // (e.g. on an empty array) gets caught here.
        const ast = [{
            t: 'CodeBlock',
            c: [['', ['r'], []], 'cat("hi")'],
        }];
        const { container } = mount(ast);
        const code = container.querySelector('pre > code')!;
        expect(code.querySelectorAll('span[class^="hl-"]').length).toBe(0);
        expect(code.textContent).toBe('cat("hi")');
    });

    it('CodeBlock falls back to plain text when data-hl-spans is empty array', () => {
        // Defensive: the encoder may emit `[]` for a code cell whose
        // grammar lookup succeeded but produced no captures (e.g.
        // a single-character cell with no matchable tokens). Treat
        // an empty array the same as missing attribute — no spans
        // emitted, plain text rendered.
        const ast = [{
            t: 'CodeBlock',
            c: [
                ['', ['r'], [['data-hl-spans', '[]']]],
                'x',
            ],
        }];
        const { container } = mount(ast);
        const code = container.querySelector('pre > code')!;
        expect(code.querySelectorAll('span[class^="hl-"]').length).toBe(0);
        expect(code.textContent).toBe('x');
    });

    it('CodeBlock highlight survives non-ASCII source (utf-8 byte offsets)', () => {
        // `data-hl-spans` byte offsets index into the utf-8
        // representation, not utf-16 / char counts. A grammar
        // matching `α` (a 2-byte char) at offset 0 should still
        // produce a span containing exactly that character.
        // Mirrors how the Rust writer slices `&text[cursor..end]`
        // by byte index — `&str` slicing must hit utf-8 boundaries
        // and we expect the same here.
        const text = 'α'; // 2 bytes in utf-8, 1 utf-16 unit in JS string
        const spans = [[0, 2, 'identifier']];
        const ast = [{
            t: 'CodeBlock',
            c: [
                ['', ['r'], [['data-hl-spans', JSON.stringify(spans)]]],
                text,
            ],
        }];
        const { container } = mount(ast);
        const code = container.querySelector('pre > code')!;
        const span = code.querySelector('span.hl-identifier')!;
        expect(span).not.toBeNull();
        expect(span.textContent).toBe('α');
        expect(code.textContent).toBe('α');
    });

    // ─── bd-coffj: Div with class="section" → <section> tag ──────────────
    //
    // The native HTML writer
    // (`crates/pampa/src/writers/html.rs::Block::Div`, lines 1129-1142)
    // emits `<section>...</section>` for a Pandoc `Div` whose class
    // list contains `"section"` — this is the sectionize transform's
    // output. The React `Div` component must match: Quarto theme CSS
    // keys off the `<section>` tag (e.g.
    // `main.content > p:has(+ section) { margin-bottom: 2rem }`),
    // and `<div class="section">` doesn't trigger those rules. The
    // visible symptom is a paragraph-before-section bottom-margin of
    // 17px in preview vs 34px in render against the fixture website.

    it('Div with class="section" renders as <section> (sectionize transform)', () => {
        const ast = [{
            t: 'Div',
            c: [
                ['a-section', ['section', 'level3'], []],
                [{ t: 'Header', c: [3, ['', [], []], [STR('A section')]] }],
            ],
        }];
        const { container } = mount(ast);
        // The container itself must be a <section>, not a <div>.
        const section = container.querySelector('section.section.level3');
        expect(section).not.toBeNull();
        expect(section!.id).toBe('a-section');
        expect(section!.className).toBe('section level3');
        // Negative check: there must NOT be a <div> with the section
        // classes for this Pandoc Div. (Other unrelated <div>s in
        // the rendered tree are fine.)
        const divWithSection = container.querySelector('div.section.level3');
        expect(divWithSection).toBeNull();
    });

    it('Div without "section" class still renders as <div>', () => {
        const ast = [{
            t: 'Div',
            c: [
                ['my-callout', ['callout-note'], []],
                [PARA(STR('callout body'))],
            ],
        }];
        const { container } = mount(ast);
        // Regression guard: only `section` triggers the elevation;
        // other Quarto-extension classes (callouts, columns, etc.)
        // keep <div>.
        const div = container.querySelector('div.callout-note');
        expect(div).not.toBeNull();
        expect(container.querySelector('section.callout-note')).toBeNull();
    });

    it('Image — manifest hit returns the blob URL', () => {
        const ast = [PARA({
            t: 'Image',
            c: [['', [], []], [STR('hero alt')], ['hero.png', '']],
        })];
        const { container } = render(
            <AssetManifestContext.Provider value={{ 'hero.png': 'blob:abc' }}>
                <Ast
                    astJson={astJson(ast)}
                    currentFilePath="/project/test.qmd"
                    onNavigateToDocument={noopNav}
                    setAst={noopSet}
                    registry={previewRegistry}
                />
            </AssetManifestContext.Provider>,
        );
        const img = container.querySelector('img')!;
        expect(img.getAttribute('src')).toBe('blob:abc');
        expect(img.alt).toBe('hero alt');
    });

    it('Image — external https URL passes through unchanged', () => {
        const ast = [PARA({
            t: 'Image',
            c: [['', [], []], [STR('alt')], ['https://cdn.example.com/hero.png', '']],
        })];
        const { container } = mount(ast);
        const img = container.querySelector('img')!;
        expect(img.getAttribute('src')).toBe('https://cdn.example.com/hero.png');
    });

    it('Image — data: URI passes through unchanged', () => {
        const ast = [PARA({
            t: 'Image',
            c: [['', [], []], [STR('alt')], ['data:image/png;base64,iVBOR', '']],
        })];
        const { container } = mount(ast);
        const img = container.querySelector('img')!;
        expect(img.getAttribute('src')).toBe('data:image/png;base64,iVBOR');
    });

    it('Image — manifest miss falls back to original URL (broken-image affordance)', () => {
        const ast = [PARA({
            t: 'Image',
            c: [['', [], []], [STR('alt')], ['hero.png', '']],
        })];
        // Default empty manifest from the registry's default Provider.
        const { container } = mount(ast);
        const img = container.querySelector('img')!;
        expect(img.getAttribute('src')).toBe('hero.png');
    });

    it('Image — width/height kvs become <img> attrs', () => {
        const ast = [PARA({
            t: 'Image',
            c: [
                ['', [], [['width', '400'], ['height', '300']]],
                [STR('alt')],
                ['hero.png', ''],
            ],
        })];
        const { container } = mount(ast);
        const img = container.querySelector('img')!;
        expect(img.getAttribute('width')).toBe('400');
        expect(img.getAttribute('height')).toBe('300');
    });

    it('Image — id, classes, title attributes propagate', () => {
        const ast = [PARA({
            t: 'Image',
            c: [['my-id', ['cls-a', 'cls-b'], []], [STR('alt')], ['hero.png', 'a tooltip']],
        })];
        const { container } = mount(ast);
        const img = container.querySelector('img')!;
        expect(img.id).toBe('my-id');
        expect(img.className).toBe('cls-a cls-b');
        expect(img.title).toBe('a tooltip');
    });

    it('Image — alt text uses Stringify (handles Emph / Code / SoftBreak)', () => {
        const ast = [PARA({
            t: 'Image',
            c: [
                ['', [], []],
                [
                    STR('hello'),
                    { t: 'Space' },
                    { t: 'Emph', c: [STR('world')] },
                    { t: 'SoftBreak' },
                    { t: 'Code', c: [['', [], []], 'snippet'] },
                ],
                ['hero.png', ''],
            ],
        })];
        const { container } = mount(ast);
        const img = container.querySelector('img')!;
        expect(img.alt).toBe('hello world snippet');
    });

    it('Figure renders <figure> + body + <figcaption> with body recursion', () => {
        const ast = [{
            t: 'Figure',
            c: [
                ['fig-1', [], []],
                [null, [PARA(STR('A caption'))]],
                [PARA(STR('body content'))],
            ],
        }];
        const { container } = mount(ast);
        const fig = container.querySelector('figure');
        expect(fig).not.toBeNull();
        expect(fig!.id).toBe('fig-1');
        const cap = container.querySelector('figcaption');
        expect(cap).not.toBeNull();
        expect(cap!.textContent).toBe('A caption');
        // Body content (Para 'body content') sits as a sibling outside <figcaption>.
        expect(fig!.textContent).toContain('body content');
    });

    // Helpers for the Table tests below. Pandoc Table shape:
    //   Table = [Attr, Caption, ColSpec[], TableHead, TableBody[], TableFoot]
    //   Caption = [shortCaption|null, blocks]
    //   TableHead = [Attr, Row[]]
    //   TableBody = [Attr, RowHeadColumns, headRows[], bodyRows[]]
    //   TableFoot = [Attr, Row[]]
    //   Row = [Attr, Cell[]]
    //   Cell = [Attr, Alignment, RowSpan, ColSpan, BlockNode[]]
    const EMPTY_ATTR: [string, string[], unknown[]] = ['', [], []];
    const CELL = (text: string) => [
        EMPTY_ATTR,
        { t: 'AlignDefault' },
        1,
        1,
        [PARA(STR(text))],
    ];
    const ROW = (...cells: any[]) => [EMPTY_ATTR, cells];
    const tableAst = (headRows: any[], bodyRows: any[]) => ({
        t: 'Table',
        c: [
            EMPTY_ATTR,
            [null, []],
            [],
            [EMPTY_ATTR, headRows],
            [[EMPTY_ATTR, 0, [], bodyRows]],
            [EMPTY_ATTR, []],
        ],
    });

    // bd-elgxx (D4/D5 react): the preview Table component must emit row
    // classes matching the pampa HTML writer (bd-12fpz). Head rows get
    // class="header"; body rows alternate "odd" / "even" starting at
    // "odd". Mirrors `pampa::writers::html::tests::head_row_emits_header_class`
    // and `..body_rows_alternate_odd_even`.
    it('Table head row emits <tr class="header"> (bd-elgxx)', () => {
        const ast = [tableAst([ROW(CELL('h0'))], [])];
        const { container } = mount(ast);
        const headTr = container.querySelector('thead tr');
        expect(headTr).not.toBeNull();
        expect(headTr!.className).toBe('header');
    });

    it('Table body rows alternate <tr class="odd"> / <tr class="even"> starting at odd (bd-elgxx)', () => {
        const ast = [tableAst(
            [],
            [ROW(CELL('b0')), ROW(CELL('b1')), ROW(CELL('b2')), ROW(CELL('b3'))],
        )];
        const { container } = mount(ast);
        const bodyTrs = container.querySelectorAll('tbody tr');
        expect(bodyTrs).toHaveLength(4);
        expect(bodyTrs[0].className).toBe('odd');
        expect(bodyTrs[1].className).toBe('even');
        expect(bodyTrs[2].className).toBe('odd');
        expect(bodyTrs[3].className).toBe('even');
    });

    it('Table with both head and body emits header on thead rows and odd/even on tbody rows (bd-elgxx)', () => {
        const ast = [tableAst(
            [ROW(CELL('h0'))],
            [ROW(CELL('b0')), ROW(CELL('b1'))],
        )];
        const { container } = mount(ast);
        expect(container.querySelector('thead tr')!.className).toBe('header');
        const bodyTrs = container.querySelectorAll('tbody tr');
        expect(bodyTrs[0].className).toBe('odd');
        expect(bodyTrs[1].className).toBe('even');
    });
});

describe('Atomic-aware gate (framework Node)', () => {
    it('atomic CustomInline (CrossrefResolvedRef) inside a Para receives a no-op setLocalAst via the framework gate', () => {
        // Hijack Inline registry entry to capture the setLocalAst it
        // receives. The atomic gate runs in framework's `Node` (in
        // dispatch.tsx) before either format's Inline dispatcher,
        // replacing setLocalAst with a NOOP for atomic content.
        const captures: Array<{ t: string; setLocalAst: (n: unknown) => void }> = [];
        const CapturingInline = (args: NodeArgs<InlineNode>) => {
            captures.push({ t: args.node.t, setLocalAst: args.setLocalAst as (n: unknown) => void });
            return <span>captured-{args.node.t}</span>;
        };

        const ast = [PARA({
            t: 'CustomInline',
            type_name: 'CrossrefResolvedRef',
            slots: { suffix: { kind: 'inlines', value: [] } },
            plain_data: {
                identifier: 'fig-1', kind: 'Figure', ref_type: 'fig',
                resolved: true, kind_source: 'builtin',
                order: { section: [], order: 1 },
            },
            attr: ['', [], []],
        })];
        const merged: FormatRegistry = {
            ...previewRegistry,
            Inline: CapturingInline,
        } as FormatRegistry;
        render(
            <Ast
                astJson={astJson(ast)}
                currentFilePath="/project/test.qmd"
                onNavigateToDocument={noopNav}
                setAst={() => {}}
                registry={merged}
            />,
        );

        const xrefCapture = captures.find((c) => c.t === 'CustomInline');
        expect(xrefCapture).toBeDefined();
        // The atomic gate's NOOP_SET_LOCAL_AST is a singleton inside the
        // framework. Direct identity check isn't possible from outside
        // the module, but invoking it must NOT call the parent's setAst
        // — assert by behavioral observation: invoking it produces no
        // side effect we can detect via setAstSpy.
        const setAstSpy = vi.fn();
        const ast2 = [PARA({
            t: 'CustomInline',
            type_name: 'CrossrefResolvedRef',
            slots: {},
            plain_data: null,
            attr: ['', [], []],
        })];
        captures.length = 0;
        render(
            <Ast
                astJson={astJson(ast2)}
                currentFilePath="/project/test.qmd"
                onNavigateToDocument={noopNav}
                setAst={setAstSpy}
                registry={merged}
            />,
        );
        const fresh = captures.find((c) => c.t === 'CustomInline')!;
        // Calling the captured setLocalAst — should be no-op.
        fresh.setLocalAst({ t: 'Str', c: 'EDITED' });
        expect(setAstSpy).not.toHaveBeenCalled();
    });

    it('user override that iterates node.c directly disables the atomic gate (negative regression guard)', () => {
        // Locks the documented "Recursion contract for the atomic gate"
        // behavior: user-TSX components that walk node.c and emit
        // hand-rolled JSX bypass the framework's per-<Node> gate. This
        // is the failure mode v1 accepts; the test makes the contract
        // observable so a future hardening pass will fail it loudly.
        const setAstSpy = vi.fn();

        const BypassingPara = ({ node, setLocalAst }: NodeArgs<ParaBlock>) => (
            <p data-testid="bypassing-para">
                {node.c.map((child, i) => (
                    <button
                        key={i}
                        data-testid={`child-${i}`}
                        onClick={() =>
                            setLocalAst({
                                t: 'Para',
                                c: [
                                    ...node.c.slice(0, i),
                                    { t: 'Str', c: 'EDITED' },
                                    ...node.c.slice(i + 1),
                                ],
                            })
                        }
                    >
                        click
                    </button>
                ))}
            </p>
        );

        const ast: PandocAST = {
            'pandoc-api-version': [1, 23, 0],
            meta: {},
            blocks: [{
                t: 'Para',
                c: [
                    STR('see '),
                    {
                        t: 'CustomInline',
                        type_name: 'CrossrefResolvedRef',
                        slots: { suffix: { kind: 'inlines', value: [] } },
                        plain_data: {
                            identifier: 'fig-1', kind: 'Figure', ref_type: 'fig',
                            resolved: true, kind_source: 'builtin',
                            order: { section: [], order: 1 },
                        },
                        attr: ['', [], []],
                    } as InlineNode,
                ],
            }] as BlockNode[],
        };

        const merged: FormatRegistry = {
            ...previewRegistry,
            Para: BypassingPara as FormatRegistry['Block'],
        } as FormatRegistry;

        const { getByTestId } = render(
            <Ast
                astJson={JSON.stringify(ast)}
                currentFilePath="/project/test.qmd"
                onNavigateToDocument={noopNav}
                setAst={setAstSpy}
                registry={merged}
            />,
        );

        // Click child-1 — the atomic CrossrefResolvedRef. Because
        // BypassingPara constructs setLocalAst outside <Node>, the
        // gate never fires for descendants. The edit should reach
        // setAst, demonstrating the contract's failure mode.
        fireEvent.click(getByTestId('child-1'));
        expect(setAstSpy).toHaveBeenCalledTimes(1);
    });
});

describe('Class-compatibility (stub constants)', () => {
    it('quartoClasses constants match expected strings', () => {
        // Stub-scope sanity: catches accidental edits to the constants
        // that the smoke-all fixtures and component renderers depend on.
        expect(SECTION).toBe('section');
        expect(SECTION_LEVEL_PREFIX).toBe('level');
        expect(FOOTNOTE_REF).toBe('footnote-ref');
    });

    it('Note.tsx emits class="footnote-ref" on the inline <sup>', () => {
        const noteAst = [PARA({
            t: 'Note',
            c: [PARA(STR('the note body'))],
        })];
        // Without NoteNumberingContext.Provider wrapping, Note falls back
        // to '?' but still emits the canonical class.
        const { container } = mount(noteAst);
        const sup = container.querySelector('sup');
        expect(sup).not.toBeNull();
        expect(sup!.className).toBe('footnote-ref');
    });
});
