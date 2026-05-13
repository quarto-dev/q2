import { memo } from 'react';
import katex from 'katex';
import type { MathInline, NodeArgs } from '@quarto/preview-renderer/framework';

/**
 * Math (DisplayMath / InlineMath) → KaTeX-rendered HTML wrapped in a
 * `<span>`. Two divergences from Elliot's original demo pattern:
 *
 * 1. Direct ESM `import katex` instead of reading `window.katex`.
 *    `window.katex` is set inside `loadCustomComponents` (only on
 *    `LOAD_CUSTOM_COMPONENTS`); a doc with no user-TSX overrides
 *    would crash on `window.katex.renderToString`. The static import
 *    works in every render path. (See plan §"Math.tsx — KaTeX leaf"
 *    for the iframe-sandbox rationale that makes the same-origin
 *    bundled import safe.)
 *
 * 2. Explicit `<span>{latex}</span>` fallback on KaTeX error so a
 *    failed parse surfaces the raw LaTeX instead of vanishing.
 */
export const Math = memo(({ node }: NodeArgs<MathInline>) => {
    const [{ t: mathType }, latex] = node.c;
    const isDisplayMath = mathType === 'DisplayMath';
    try {
        const html = katex.renderToString(latex, {
            displayMode: isDisplayMath,
            throwOnError: false,
            output: 'html',
        });
        return <span dangerouslySetInnerHTML={{ __html: html }} />;
    } catch {
        return <span>{latex}</span>;
    }
});
