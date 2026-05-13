import React from 'react';
import { RegistryContext } from './RegistryContext';
import { unwrapCustomNodes } from './customNode';
import type { PandocAST } from './types';
import type { SourceInfoPool } from '../types/sourceInfo';

interface AstPropsCommon {
    /** Current file path for resolving relative image paths */
    currentFilePath: string;
    onNavigateToDocument?: (path: string, anchor: string | null) => void;
    setAst: (newAst: PandocAST) => void;
    /** Optional controlled current slide index. If provided, component uses this instead of internal state. */
    currentSlide?: number;
    /** Callback when current slide changes (for controlled mode). */
    onSlideChange?: (slideIndex: number) => void;
    registry: Record<string, (props: any) => React.ReactNode>;
}

/**
 * Discriminated input: callers may pass either the unparsed JSON string
 * (`astJson`) or an already-parsed AST object (`ast`). The parsed-input
 * path skips an internal `JSON.parse` — q2-preview's PreviewRoot uses
 * this to avoid double-parsing when it walks the AST for Note numbering
 * before handing the parsed object to <Ast>.
 */
type AstProps =
    | (AstPropsCommon & { astJson: string; ast?: undefined })
    | (AstPropsCommon & { ast: PandocAST; astJson?: undefined });

/**
 * Framework root: parses the AST JSON, installs the registry on the
 * <RegistryContext> Provider, and hands control to the format's
 * registered 'Ast' component.
 *
 * Plan 2B co-edits:
 *  1. Accept a discriminated input (`astJson` or `ast`); skip JSON.parse
 *     on the parsed-input path.
 *  2. Call `unwrapCustomNodes(parsed)` on both branches so the registry
 *     dispatchers only ever see real Divs/Spans (no `__quarto_custom_node`
 *     wrappers leak through).
 *  3. Extract `astContext.sourceInfoPool` from the parsed AST onto the
 *     RegistryContext.Provider value so the atomic-aware gate inside
 *     `Node` (in dispatch.tsx) can read it.
 */
export function Ast(props: AstProps) {
    const {
        currentFilePath: _currentFilePath,
        onNavigateToDocument,
        setAst,
        currentSlide: _currentSlide,
        onSlideChange: _onSlideChange,
        registry,
    } = props;

    let parsed: PandocAST;
    if ('ast' in props && props.ast !== undefined) {
        parsed = props.ast;
    } else {
        try {
            parsed = JSON.parse(props.astJson as string);
        } catch (err) {
            return (
                <div className="error" style={{ padding: '20px', color: 'red' }}>
                    Failed to parse AST: {err instanceof Error ? err.message : String(err)}
                </div>
            );
        }
    }

    // Read the source-info pool *before* unwrap. Unwrap only rewrites
    // descendants of `c` fields; the wrapper's `astContext` field on the
    // root is untouched, but reading pre-unwrap keeps the cost off the
    // descent.
    const pool = (parsed as unknown as { astContext?: { sourceInfoPool?: SourceInfoPool } })
        .astContext?.sourceInfoPool;

    // Unconditional unwrap: replaces every wire-format custom-node Div/Span
    // with the JS-native CustomBlockNode/CustomInlineNode shape. Safe for
    // q2-debug too — q2-debug's pre-pipeline AST never contains
    // `__quarto_custom_node` wrappers, so unwrap is a no-op there (subtree
    // returned by reference per the walker purity contract).
    const ast = unwrapCustomNodes(parsed);

    const AstComponent = registry['Ast'];

    return (
        <RegistryContext.Provider value={{ registry, sourceInfoPool: pool }}>
            <AstComponent ast={ast} onNavigateToDocument={onNavigateToDocument} setAst={setAst} />
        </RegistryContext.Provider>
    );
}
