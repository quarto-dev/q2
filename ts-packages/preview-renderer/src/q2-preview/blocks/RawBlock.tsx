import { useEffect, useRef } from 'react';
import type { NodeArgs, RawBlock as RawBlockType } from '../../framework';

/**
 * RawBlock semantics:
 *  - format === 'html' (or 'html5'): inject raw HTML via
 *    `dangerouslySetInnerHTML` so users can embed exact markup. After
 *    mount we re-execute any `<script>` tags inside the injected
 *    content (see [`RawHtmlBlock`](#RawHtmlBlock)).
 *  - any other format: render as a `<pre>` block so the source is
 *    visible (a Pandoc Markdown writer's text isn't meaningful HTML).
 *
 * Sanitization is the user's responsibility — RawBlock means "trust
 * the author." The iframe sandbox limits the blast radius.
 */
export const RawBlock = ({ node }: NodeArgs<RawBlockType>) => {
    const [format, content] = node.c;
    if (format === 'html' || format === 'html5') {
        return <RawHtmlBlock content={content} />;
    }
    return <pre>{content}</pre>;
};

/**
 * Render a `RawBlock(html, …)` so embedded `<script>` tags actually
 * execute, matching the static-render behaviour.
 *
 * `dangerouslySetInnerHTML` inserts script tags as inert DOM nodes —
 * the browser parses them but never runs them, because the HTML
 * specification only executes scripts that the parser sees in the
 * page's *initial* HTML or that are created via `document.createElement`.
 * In static `q2 render`, raw-HTML script blocks ride along with the
 * initial document and execute as expected; in the preview's React
 * iframe, they would otherwise be silently dropped.
 *
 * The fix: after mounting the raw HTML, walk the container for
 * `<script>` tags and replace each with a freshly created one that
 * carries the same attributes and content. The replacement is what
 * the spec calls a "script-created" element, and the browser does
 * execute it.
 *
 * **Security implications.** Scripts inside `RawBlock(html)` will
 * now run inside the preview's React iframe. This is the same posture
 * `q2 render`'s static HTML already has — both load page content the
 * user themselves authored — but it is a behaviour change for the
 * preview pipeline, which previously silently ignored such scripts.
 * Discussed and accepted with the user on 2026-05-29 in the
 * `bd-my0o5` mermaid-preview verification work; ratified for the
 * `feature/mermaid-engine` branch with the caveat that the branch is
 * NOT to be merged to main until the hub-client JS security model
 * design lands.
 *
 * **Follow-up.** The proper structural fix is `bd-mqk49` (the
 * engine → stage extension API): once engines can declare per-format
 * AST passes on their output, diagram engines like `MermaidEngine`
 * (`crates/quarto-core/src/engine/mermaid.rs`) emit marker nodes that
 * the preview renderer can handle with dedicated, script-free React
 * components, removing the need for this generic re-execution.
 *
 * See `claude-notes/plans/2026-05-28-mermaidjs-engine-design.md`,
 * Q-B and Phase 3.
 */
function RawHtmlBlock({ content }: { content: string }) {
    const ref = useRef<HTMLDivElement>(null);

    useEffect(() => {
        const container = ref.current;
        if (!container) return;
        // Walking via querySelectorAll on a snapshot keeps the iteration
        // stable while we mutate the tree (replaceWith removes from the
        // live NodeList).
        const inert = Array.from(container.querySelectorAll('script'));
        for (const old of inert) {
            const fresh = document.createElement('script');
            for (const attr of Array.from(old.attributes)) {
                fresh.setAttribute(attr.name, attr.value);
            }
            // Inline scripts: copy the body verbatim. External scripts
            // (`src=...`) need no body — the src attribute is enough.
            if (old.textContent) {
                fresh.textContent = old.textContent;
            }
            old.replaceWith(fresh);
        }
    }, [content]);

    return <div ref={ref} dangerouslySetInnerHTML={{ __html: content }} />;
}
