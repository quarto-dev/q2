import React from 'react';
import { RegistryContext } from './RegistryContext';
import type { PandocAST } from './types';

interface AstProps {
    astJson: string;
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
 * Framework root: parses the AST JSON, installs the registry on the
 * <RegistryContext> Provider, and hands control to the format's
 * registered 'Ast' component.
 */
export function Ast({
    astJson,
    currentFilePath: _currentFilePath,
    onNavigateToDocument,
    setAst,
    currentSlide: _currentSlide,
    onSlideChange: _onSlideChange,
    registry,
}: AstProps) {
    let ast: PandocAST;

    try {
        ast = JSON.parse(astJson);
    } catch (err) {
        return (
            <div className="error" style={{ padding: '20px', color: 'red' }}>
                Failed to parse AST: {err instanceof Error ? err.message : String(err)}
            </div>
        );
    }

    const AstComponent = registry['Ast'];

    return (
        <RegistryContext.Provider value={{ registry }}>
            <AstComponent ast={ast} onNavigateToDocument={onNavigateToDocument} setAst={setAst} />
        </RegistryContext.Provider>
    );
}
