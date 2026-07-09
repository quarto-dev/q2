// Simple AST renderer component
export function AstRenderer({ node }: { node: any }) {
  if (!node) return null;

  // Handle text content
  if (typeof node === 'string') {
    return <>{node}</>;
  }

  // Handle arrays of nodes
  if (Array.isArray(node)) {
    return <>{node.map((child, i) => <AstRenderer key={i} node={child} />)}</>;
  }

  // Handle Pandoc AST object structure
  if (node.t) {
    const type = node.t;
    const content = node.c;

    // Inline elements
    if (type === 'Str') {
      return <>{content}</>;
    }
    if (type === 'Space') {
      return <> </>;
    }
    if (type === 'SoftBreak') {
      return <> </>;
    }
    if (type === 'LineBreak') {
      return <br />;
    }
    if (type === 'Emph') {
      return (
        <span style={{ border: '1px solid black', padding: '2px' }}>
          <AstRenderer node={content} />
        </span>
      );
    }
    if (type === 'Strong') {
      return (
        <span style={{ border: '1px solid black', padding: '2px' }}>
          <AstRenderer node={content} />
        </span>
      );
    }
    if (type === 'Code') {
      return (
        <span style={{ border: '1px solid black', padding: '2px' }}>
          <AstRenderer node={content[1]} />
        </span>
      );
    }
    if (type === 'Link') {
      return (
        <span style={{ border: '1px solid black', padding: '2px' }}>
          <AstRenderer node={content[1]} />
        </span>
      );
    }
    if (type === 'Image') {
      // content: [attrs, alt_text, [url, title]]
      const url = content[2][0];
      return <img src={url} alt="" style={{ maxWidth: '100%' }} />;
    }

    // Block elements
    if (type === 'Para') {
      return (
        <div style={{ border: '1px solid black', padding: '4px', marginBottom: '8px' }}>
          <AstRenderer node={content} />
        </div>
      );
    }
    if (type === 'Plain') {
      return (
        <div style={{ border: '1px solid black', padding: '4px', marginBottom: '8px' }}>
          <AstRenderer node={content} />
        </div>
      );
    }
    if (type === 'Header') {
      // content: [level, attrs, inlines]
      return (
        <div style={{ border: '1px solid black', padding: '4px', marginBottom: '8px' }}>
          <AstRenderer node={content[2]} />
        </div>
      );
    }
    if (type === 'CodeBlock') {
      return (
        <div style={{ border: '1px solid black', padding: '4px', marginBottom: '8px' }}>
          <AstRenderer node={content[1]} />
        </div>
      );
    }
    if (type === 'BulletList' || type === 'OrderedList') {
      return (
        <div style={{ border: '1px solid black', padding: '4px', marginBottom: '8px' }}>
          <AstRenderer node={content} />
        </div>
      );
    }
    if (type === 'BlockQuote') {
      return (
        <div style={{ border: '1px solid black', padding: '4px', marginBottom: '8px' }}>
          <AstRenderer node={content} />
        </div>
      );
    }
    if (type === 'Div') {
      return (
        <div style={{ border: '1px solid black', padding: '4px', marginBottom: '8px' }}>
          <AstRenderer node={content[1]} />
        </div>
      );
    }

    // Default: render with border for any unknown block/inline
    return (
      <div style={{ border: '1px solid black', padding: '4px', marginBottom: '8px' }}>
        <AstRenderer node={content} />
      </div>
    );
  }

  // Handle document root with blocks array
  if (node.blocks) {
    return <AstRenderer node={node.blocks} />;
  }

  return null;
}
