/**
 * Outline Panel Component
 *
 * Document outline view for the sidebar accordion.
 * Displays a hierarchical tree of document symbols (headers, code cells).
 * Clicking a symbol navigates the editor to that location.
 * Symbols with children can be collapsed/expanded.
 */

import { useState, useCallback } from 'react';
import type { Symbol, SymbolKind } from '../types/intelligence';
import './OutlinePanel.css';

/**
 * Generate a stable identifier for a symbol.
 * Used for tracking collapsed state.
 */
function getSymbolId(symbol: Symbol, index: number): string {
  return `${symbol.range.start.line}-${symbol.name}-${index}`;
}

export interface OutlinePanelProps {
  /** Document symbols to display. */
  symbols: Symbol[];
  /** Called when a symbol is clicked. */
  onSymbolClick: (symbol: Symbol) => void;
  /** Whether symbols are loading. */
  loading?: boolean;
  /** Error message to display. */
  error?: string | null;
}

/**
 * Get an icon for a symbol kind.
 */
function getSymbolIcon(kind: SymbolKind): { icon: string; className: string } {
  switch (kind) {
    case 'string':
      // Headers use SymbolKind::String in our LSP
      return { icon: '§', className: 'header' };
    case 'function':
      // Code cells use SymbolKind::Function
      return { icon: 'ƒ', className: 'function' };
    case 'module':
      return { icon: '◫', className: 'code' };
    case 'class':
      return { icon: '◇', className: 'code' };
    case 'method':
      return { icon: '○', className: 'function' };
    case 'variable':
      return { icon: '◦', className: 'code' };
    case 'constant':
      return { icon: '●', className: 'code' };
    default:
      return { icon: '•', className: '' };
  }
}

/**
 * Render a tree of symbols recursively with collapse support.
 */
function SymbolTree({
  symbols,
  onSymbolClick,
  collapsedSymbols,
  onToggleSymbol,
}: {
  symbols: Symbol[];
  onSymbolClick: (symbol: Symbol) => void;
  collapsedSymbols: Set<string>;
  onToggleSymbol: (symbolId: string) => void;
}) {
  if (symbols.length === 0) {
    return null;
  }

  return (
    <ul className="outline-list">
      {symbols.map((symbol, index) => {
        const symbolId = getSymbolId(symbol, index);
        const hasChildren = symbol.children && symbol.children.length > 0;
        const isCollapsed = collapsedSymbols.has(symbolId);
        const { icon, className } = getSymbolIcon(symbol.kind);

        return (
          <li
            key={symbolId}
            className={`outline-item ${hasChildren ? 'has-children' : ''}`}
          >
            <div className="outline-row">
              {hasChildren && (
                <button
                  className="outline-chevron"
                  onClick={(e) => {
                    e.stopPropagation();
                    onToggleSymbol(symbolId);
                  }}
                  aria-label={isCollapsed ? 'Expand' : 'Collapse'}
                >
                  {isCollapsed ? '▶' : '▼'}
                </button>
              )}
              <button
                className="outline-button"
                onClick={() => onSymbolClick(symbol)}
                title={`Go to ${symbol.name}`}
              >
                <span className={`outline-icon ${className}`}>{icon}</span>
                <span className="outline-name">{symbol.name}</span>
                {symbol.detail && (
                  <span className="outline-detail">{symbol.detail}</span>
                )}
              </button>
            </div>
            {hasChildren && !isCollapsed && (
              <SymbolTree
                symbols={symbol.children}
                onSymbolClick={onSymbolClick}
                collapsedSymbols={collapsedSymbols}
                onToggleSymbol={onToggleSymbol}
              />
            )}
          </li>
        );
      })}
    </ul>
  );
}

/**
 * Document outline panel for the sidebar accordion.
 *
 * Displays a hierarchical tree of document symbols (headers, code cells).
 * Clicking a symbol navigates the editor to that location.
 * Symbols with children can be collapsed/expanded by clicking the chevron.
 */
export default function OutlinePanel({
  symbols,
  onSymbolClick,
  loading = false,
  error = null,
}: OutlinePanelProps) {
  // Track collapsed symbols (inverted logic: store collapsed, not expanded)
  // This means new symbols are expanded by default
  const [collapsedSymbols, setCollapsedSymbols] = useState<Set<string>>(
    new Set()
  );

  // Toggle a symbol's collapsed state
  const toggleSymbol = useCallback((symbolId: string) => {
    setCollapsedSymbols((prev) => {
      const next = new Set(prev);
      if (next.has(symbolId)) {
        next.delete(symbolId);
      } else {
        next.add(symbolId);
      }
      return next;
    });
  }, []);

  // Show loading only when we have no symbols yet.
  // If we already have symbols, keep showing them during refresh to avoid flash.
  if (loading && symbols.length === 0) {
    return (
      <div className="outline-panel">
        <div className="outline-loading">Loading outline</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="outline-panel">
        <div className="outline-error">{error}</div>
      </div>
    );
  }

  if (symbols.length === 0) {
    return (
      <div className="outline-panel">
        <div className="outline-empty">No outline available</div>
      </div>
    );
  }

  return (
    <div className="outline-panel">
      <SymbolTree
        symbols={symbols}
        onSymbolClick={onSymbolClick}
        collapsedSymbols={collapsedSymbols}
        onToggleSymbol={toggleSymbol}
      />
    </div>
  );
}
