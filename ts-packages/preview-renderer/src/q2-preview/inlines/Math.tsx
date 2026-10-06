import { memo, useMemo } from 'react';
import katex from 'katex';
import type { MathInline, NodeArgs } from '../../framework';

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
    // Memoized per (latex, mode): `memo` above only skips when `node` is
    // referentially equal, and every new AST rebuilds nodes. React 19 re-sets
    // innerHTML whenever the object identity changes, so without this the
    // KaTeX DOM would be torn down and rebuilt on every preview re-render.
    const innerHtml = useMemo(() => {
        try {
            return {
                __html: katex.renderToString(latex, {
                    displayMode: isDisplayMath,
                    throwOnError: false,
                    output: 'html',
                }),
            };
        } catch {
            return null;
        }
    }, [latex, isDisplayMath]);
    if (innerHtml === null) {
        return <span>{latex}</span>;
    }
    return <span dangerouslySetInnerHTML={innerHtml} />;
});
