// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import {
  canonicalize,
  OPAQUE_MARKER,
  ParityRuleViolation,
  extractParityRoot,
  compareParity,
} from './domParity';

function el(html: string): Element {
  const host = document.createElement('div');
  host.innerHTML = html;
  return host.firstElementChild!;
}

describe('canonicalize — shape', () => {
  it('emits one line per node, indented by depth, no closing lines', () => {
    const out = canonicalize(el('<main id="x"><p>hi <em>there</em></p></main>'));
    expect(out).toBe(
      ['<main id="x">', '  <p>', '    "hi "', '    <em>', '      "there"'].join('\n'),
    );
  });

  it('sorts attributes by name and lower-cases tag names', () => {
    const out = canonicalize(el('<DIV title="t" class="c" id="i"></DIV>'));
    expect(out).toBe('<div class="c" id="i" title="t">');
  });

  it('drops pretty-printing whitespace between block elements and drops comments', () => {
    const out = canonicalize(el('<div>\n  <!-- c -->\n  <p>  a \n b  </p>\n</div>'));
    expect(out).toBe(['<div>', '  <p>', '    "a b"'].join('\n'));
  });

  it('collapses whitespace inside class but preserves token order', () => {
    const out = canonicalize(el('<pre class="  sourceCode   python ">x</pre>'));
    expect(out).toBe(['<pre class="sourceCode python">', '  "x"'].join('\n'));
  });

  it('escapes quotes in attribute values so lines stay single-line', () => {
    const out = canonicalize(el('<a title=\'say "hi"\'></a>'));
    expect(out).toBe('<a title="say &quot;hi&quot;">');
  });
});

describe('canonicalize — inline whitespace is significant', () => {
  it('distinguishes <em>a</em> <em>b</em> from <em>a</em><em>b</em>', () => {
    const spaced = canonicalize(el('<p><em>a</em> <em>b</em></p>'));
    const tight = canonicalize(el('<p><em>a</em><em>b</em></p>'));
    expect(spaced).not.toBe(tight);
    expect(spaced).toBe(['<p>', '  <em>', '    "a"', '  " "', '  <em>', '    "b"'].join('\n'));
  });

  it('keeps a trailing space before an inline sibling but trims before a block sibling', () => {
    expect(canonicalize(el('<p>hi <em>x</em></p>'))).toContain('"hi "');
    expect(canonicalize(el('<div>hi <p>x</p></div>'))).toContain('"hi"');
  });

  it('keeps text verbatim inside <pre>', () => {
    const out = canonicalize(el('<pre><code>x\n  y\n</code></pre>'));
    expect(out).toBe(['<pre>', '  <code>', '    "x\\n  y\\n"'].join('\n'));
  });

  it('merges adjacent text nodes (React emits one per Str/Space)', () => {
    const p = document.createElement('p');
    p.appendChild(document.createTextNode('hi'));
    p.appendChild(document.createTextNode(' '));
    p.appendChild(document.createTextNode('there'));
    expect(canonicalize(p)).toBe(canonicalize(el('<p>hi there</p>')));
    expect(p.childNodes.length).toBe(3); // input not mutated
  });
});

describe('PARITY_RULES', () => {
  it('strips preview-only source-tracking attributes', () => {
    const out = canonicalize(el('<p data-loc="f:1:1-1:5" data-sid="7" class="k">x</p>'));
    expect(out).toBe(['<p class="k">', '  "x"'].join('\n'));
  });

  it('keeps id and every other data-* attribute', () => {
    const out = canonicalize(el('<section id="intro" data-qf-ref-type="fig"></section>'));
    expect(out).toBe('<section data-qf-ref-type="fig" id="intro">');
  });

  it('makes span.math contents opaque but keeps the span and its classes', () => {
    const a = canonicalize(el('<span class="math inline">\\(x^2\\)</span>'));
    const b = canonicalize(el('<span class="math inline"><span class="katex">…</span></span>'));
    expect(a).toBe(b);
    expect(a).toBe(['<span class="math inline">', `  ${OPAQUE_MARKER}`].join('\n'));
  });

  it('throws ParityRuleViolation when data-hl-spans leaks', () => {
    expect(() => canonicalize(el('<pre data-hl-spans="[]"><code>x</code></pre>'))).toThrow(
      ParityRuleViolation,
    );
  });

  it("unwraps React's attribute-less RawBlock <div> wrapper (data-loc does not count)", () => {
    // preview: RawBlock.tsx host element carrying only data-loc; render: the raw HTML inline.
    const preview = canonicalize(
      el('<main><div data-loc="f:1:1-2:1"><button class="code-copy-button">c</button></div> <p>x</p></main>'),
    );
    const render = canonicalize(el('<main><button class="code-copy-button">c</button>\n<p>x</p></main>'));
    expect(preview).toBe(render);
    expect(preview).not.toContain('<div');
  });

  it('keeps a <div> that has any surviving attribute, and unwraps nested bare divs', () => {
    expect(canonicalize(el('<div class="k"><p>x</p></div>'))).toBe(['<div class="k">', '  <p>', '    "x"'].join('\n'));
    expect(canonicalize(el('<main><div><div><p>x</p></div></div></main>'))).toBe(
      canonicalize(el('<main><p>x</p></main>')),
    );
  });
});

describe('extractParityRoot / compareParity', () => {
  it('finds main#quarto-document-content and names the side on failure', () => {
    const host = document.createElement('div');
    host.innerHTML =
      '<div id="quarto-content"><main class="content" id="quarto-document-content"><p>x</p></main></div>';
    expect(extractParityRoot(host, 'render').tagName).toBe('MAIN');
    const empty = document.createElement('div');
    expect(() => extractParityRoot(empty, 'preview')).toThrow(/preview.*main#quarto-document-content/);
  });

  it('reports equal for identical subtrees modulo rules', () => {
    const r = compareParity(
      el('<main id="quarto-document-content" class="content"><p>a</p></main>'),
      el('<main class="content" id="quarto-document-content"><p data-loc="x">a</p></main>'),
    );
    expect(r.equal).toBe(true);
    expect(r.render).toBe(r.preview);
  });

  it('reports unequal and exposes both canonical texts for a class divergence', () => {
    const r = compareParity(
      el('<main id="quarto-document-content"><pre class="sourceCode python"><code>x</code></pre></main>'),
      el('<main id="quarto-document-content"><pre class="python"><code>x</code></pre></main>'),
    );
    expect(r.equal).toBe(false);
    expect(r.render).toContain('class="sourceCode python"');
    expect(r.preview).toContain('class="python"');
  });
});
