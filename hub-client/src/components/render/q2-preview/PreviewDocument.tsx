import { renderChildren } from '../framework';
import type { PandocAST } from '../framework';

/**
 * q2-preview's document-root wrapper. Registered into `registry.ts`
 * under the `'Ast'` key. Calls `renderChildren({ node: ast, setLocalAst: setAst, ... })`
 * with no debug styling — q2-preview's eventual goal is
 * Quarto-Bootstrap parity, so the document root produces no chrome
 * of its own.
 */
export const PreviewDocument = ({
    ast,
    onNavigateToDocument,
    setAst,
}: {
    ast: PandocAST;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
}) => (
    <>
        {renderChildren({
            node: ast as any,
            setLocalAst: setAst as any,
            onNavigateToDocument,
        })}
    </>
);
