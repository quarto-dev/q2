# Collapsible Sections for OutlinePanel

**Issue**: kyoto-ub5
**Created**: 2026-01-29
**Status**: Complete

## Overview

Add expand/collapse functionality to the OutlinePanel. Symbols with children (e.g., headers with subsections) can be collapsed by clicking a chevron. All symbols are expanded by default, preserving current behavior while adding the ability to collapse.

This reuses the same pattern implemented in FileSidebar (kyoto-cvr) but is simpler since the `Symbol` data is already hierarchical.

## Current Implementation

**File**: `hub-client/src/components/OutlinePanel.tsx`

The `SymbolTree` component recursively renders symbols:
- Each symbol with `children.length > 0` renders a nested `<ul>`
- Currently all children are always visible (no collapse)
- Symbols are identified by `${symbol.name}-${symbol.range.start.line}-${index}`

**Data structure** (`src/types/intelligence.ts`):
```typescript
interface Symbol {
  name: string;
  detail?: string;
  kind: SymbolKind;
  range: Range;
  selectionRange: Range;
  children: Symbol[];
}
```

## Design

### Symbol Identifiers

We need a stable identifier for each symbol to track expansion state. Use the same key pattern already used for React keys:

```typescript
function getSymbolId(symbol: Symbol, index: number): string {
  return `${symbol.range.start.line}-${symbol.name}-${index}`;
}
```

### State Management

Add state to `OutlinePanel`:

```typescript
// Track collapsed symbols (inverted logic: store collapsed, not expanded)
// This means new symbols are expanded by default
const [collapsedSymbols, setCollapsedSymbols] = useState<Set<string>>(new Set());

const toggleSymbol = useCallback((symbolId: string) => {
  setCollapsedSymbols(prev => {
    const next = new Set(prev);
    if (next.has(symbolId)) {
      next.delete(symbolId);
    } else {
      next.add(symbolId);
    }
    return next;
  });
}, []);
```

**Note**: We use `collapsedSymbols` (inverted) rather than `expandedSymbols` so that all symbols are expanded by default without needing to compute an initial set.

### Updated SymbolTree

```typescript
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
  return (
    <ul className="outline-list">
      {symbols.map((symbol, index) => {
        const symbolId = getSymbolId(symbol, index);
        const hasChildren = symbol.children && symbol.children.length > 0;
        const isCollapsed = collapsedSymbols.has(symbolId);
        const { icon, className } = getSymbolIcon(symbol.kind);

        return (
          <li key={symbolId} className="outline-item">
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
```

### CSS Changes

```css
.outline-row {
  display: flex;
  align-items: center;
}

.outline-chevron {
  width: 16px;
  height: 16px;
  padding: 0;
  margin-right: 2px;
  background: none;
  border: none;
  color: #666;
  font-size: 8px;
  cursor: pointer;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}

.outline-chevron:hover {
  color: #999;
}

/* Indent items without chevrons to align with those that have them */
.outline-item:not(:has(> .outline-row > .outline-chevron)) > .outline-row {
  padding-left: 18px; /* 16px chevron width + 2px margin */
}
```

**Alternative for browser compatibility** (`:has()` may not be supported everywhere):
Add a `has-children` class to items and use that for alignment.

## Work Items

- [x] Add `collapsedSymbols` state to OutlinePanel
- [x] Add `toggleSymbol` callback
- [x] Create `getSymbolId()` helper function
- [x] Update `SymbolTree` to accept collapse props
- [x] Add chevron toggle button for symbols with children
- [x] Conditionally render children based on collapsed state
- [x] Update CSS for chevron and row alignment
- [x] Test with documents having nested headers
- [x] Update changelog

## Testing Plan

### Manual Testing

1. **No children**: Symbols without children show no chevron
2. **With children**: Symbols with children show chevron (▼ when expanded)
3. **Click chevron**: Collapses children, chevron changes to ▶
4. **Click again**: Expands children, chevron changes to ▼
5. **Click symbol name**: Still navigates to symbol (not affected by collapse)
6. **Nested collapse**: Collapsing a parent hides all descendants
7. **Deep nesting**: Works correctly with multiple levels of headers

### Edge Cases

1. **Empty outline**: Still shows "No outline available"
2. **Loading state**: Still shows loading indicator
3. **Symbol without children array**: Treat as no children (defensive)

## Future Enhancements (Out of Scope)

- Expand/collapse all button
- Remember collapsed state across file switches
- Highlight current section based on cursor position
