/**
 * Outline Panel Component
 *
 * Document outline view for the sidebar accordion.
 * Displays a hierarchical tree of document symbols (headers, code cells).
 * Clicking a symbol navigates the editor to that location.
 */

import type { Symbol, SymbolKind } from '../types/intelligence';
import './OutlinePanel.css';

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
 * Render a tree of symbols recursively.
 */
function SymbolTree({
  symbols,
  onSymbolClick,
}: {
  symbols: Symbol[];
  onSymbolClick: (symbol: Symbol) => void;
}) {
  if (symbols.length === 0) {
    return null;
  }

  return (
    <ul className="outline-list">
      {symbols.map((symbol, index) => {
        const { icon, className } = getSymbolIcon(symbol.kind);
        return (
          <li key={`${symbol.name}-${symbol.range.start.line}-${index}`} className="outline-item">
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
            {symbol.children.length > 0 && (
              <SymbolTree symbols={symbol.children} onSymbolClick={onSymbolClick} />
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
 */
export default function OutlinePanel({
  symbols,
  onSymbolClick,
  loading = false,
  error = null,
}: OutlinePanelProps) {
  if (loading) {
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
      <SymbolTree symbols={symbols} onSymbolClick={onSymbolClick} />
    </div>
  );
}
