// Minimal "reactji" Block override used by the smoke-all q2-debug fixture.
//
// Detects inline Spans with class "quarto-edit-comment" on Para/Plain blocks
// (the same convention elliot/comment.tsx uses) and renders one counter
// button per such span next to the block content. The button uses local React
// state so a click increments without round-tripping the AST through
// Automerge — that's a separate feature path, out of scope for this test.
//
// What this exercises:
//   - render-components dynamic import + load
//   - registry accumulator (Block override layered on top of defaults)
//   - JSX + React.useState inside dynamically loaded user code
//   - click handlers wiring up correctly

const React = (window as any).React;
const { Block: B } = (window as any).__REACT_AST_DEBUG_RENDERER__;

function isReaction(inline: any): boolean {
  return (
    inline?.t === 'Span' &&
    Array.isArray(inline?.c?.[0]?.[1]) &&
    inline.c[0][1].includes('quarto-edit-comment')
  );
}

function reactionLabel(inline: any): string {
  const inner = inline?.c?.[1] ?? [];
  return inner.map((o: any) => (o?.t === 'Str' ? o.c : '')).join('');
}

const Counter = ({ label, initial }: { label: string; initial: number }) => {
  const [count, setCount] = React.useState(initial);
  return (
    <button
      data-testid={`reaction-${label}`}
      onClick={() => setCount((c: number) => c + 1)}
      style={{
        margin: '0 4px',
        padding: '2px 8px',
        border: '1px solid #999',
        borderRadius: '12px',
        background: '#eee',
        cursor: 'pointer',
        font: 'inherit',
      }}
    >
      {label} {count}
    </button>
  );
};

export const Block = (args: any) => {
  const { node: block, onNavigateToDocument, setLocalAst } = args;

  if (block?.t !== 'Para' && block?.t !== 'Plain') {
    return (
      <B
        node={block}
        onNavigateToDocument={onNavigateToDocument}
        setLocalAst={setLocalAst}
      />
    );
  }

  const children = Array.isArray(block.c) ? block.c : [];
  const reactions = children.filter(isReaction);
  const counts = new Map<string, number>();
  for (const r of reactions) {
    const label = reactionLabel(r);
    if (label) counts.set(label, (counts.get(label) ?? 0) + 1);
  }

  const cleanBlock = { ...block, c: children.filter((n: any) => !isReaction(n)) };

  return (
    <div data-testid="reactji-wrapper">
      <B
        node={cleanBlock}
        onNavigateToDocument={onNavigateToDocument}
        setLocalAst={setLocalAst}
      />
      {[...counts.entries()].map(([label, count]) => (
        <Counter key={label} label={label} initial={count} />
      ))}
    </div>
  );
};
