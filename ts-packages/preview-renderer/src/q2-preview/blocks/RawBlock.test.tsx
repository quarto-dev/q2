/**
 * @vitest-environment jsdom
 *
 * Regression tests for the `RawBlock(html, …)` script-re-execution
 * shim added in bd-my0o5.
 *
 * `dangerouslySetInnerHTML` inserts script tags as inert DOM nodes —
 * the HTML spec only executes scripts the parser sees in the initial
 * document, or scripts created via `document.createElement`. That
 * broke engines emitting in-band `<script>` includes (the mermaid
 * engine's jsdelivr import in bd-gwfdo) in the preview iframe even
 * though they worked fine in static `q2 render`.
 *
 * The `RawHtmlBlock` component below `RawBlock` walks the rendered
 * container for `<script>` tags and replaces each with a freshly
 * created one. In a real browser the replacement executes; jsdom
 * (by default) does not run inline scripts even after createElement,
 * so we cannot assert execution here — only the structural
 * recreation. The end-to-end browser verification (Chrome DevTools
 * MCP run in bd-my0o5 Phase 3) is what proves execution.
 */
import { describe, it, expect } from 'vitest';
import { render } from '@testing-library/react';
import { RawBlock } from './RawBlock';
import type { RawBlock as RawBlockType } from '../../framework';

function rb(format: string, content: string): RawBlockType {
    return { t: 'RawBlock', c: [format, content] };
}

describe('preview-renderer q2-preview RawBlock (bd-my0o5)', () => {
    it('renders raw HTML via dangerouslySetInnerHTML', () => {
        const node = rb('html', '<p class="from-raw">hello</p>');
        const { container } = render(<RawBlock node={node} />);
        const p = container.querySelector('p.from-raw');
        expect(p).not.toBeNull();
        expect(p!.textContent).toBe('hello');
    });

    it('recreates inline <script> tags after mount (script-created elements are executable in browsers)', () => {
        const node = rb('html', '<script id="orig-marker" type="module">/* module body */</script>');
        const { container } = render(<RawBlock node={node} />);

        const scripts = container.querySelectorAll('script');
        expect(scripts.length).toBe(1);
        // Attributes and body were copied onto the replacement.
        expect(scripts[0].getAttribute('id')).toBe('orig-marker');
        expect(scripts[0].getAttribute('type')).toBe('module');
        expect(scripts[0].textContent).toContain('module body');
    });

    it('preserves the mermaid engine\'s emission shape end-to-end through the renderer', () => {
        // Verbatim of what MermaidEngine emits after a render cycle:
        // a <pre class="mermaid"> for each cell and one
        // <script type="module"> tag that imports mermaid.
        const html = [
            '<pre class="mermaid">',
            'graph TD',
            'A --&gt; B',
            '</pre>',
            '<script type="module">',
            "import mermaid from 'https://cdn.jsdelivr.net/npm/mermaid@11/dist/mermaid.esm.min.mjs';",
            'mermaid.initialize({ startOnLoad: true });',
            '</script>',
        ].join('\n');
        const node = rb('html', html);
        const { container } = render(<RawBlock node={node} />);

        expect(container.querySelectorAll('pre.mermaid').length).toBe(1);
        const script = container.querySelector('script[type="module"]');
        expect(script).not.toBeNull();
        // The recreated script must still carry the import; this is
        // what a real browser would execute and that would set
        // `window.mermaid` and run the diagram render.
        expect(script!.textContent).toContain('mermaid.esm.min.mjs');
        expect(script!.textContent).toContain('startOnLoad: true');
    });

    it('passes through non-html raw blocks as <pre>', () => {
        const node = rb('latex', '\\section{Foo}');
        const { container } = render(<RawBlock node={node} />);
        const pre = container.querySelector('pre');
        expect(pre).not.toBeNull();
        expect(pre!.textContent).toContain('\\section{Foo}');
    });
});
