const React = window.React;
const { memo } = React;

const {
  renderChildren,
  renderNode
} = window.__REACT_AST_DEBUG_RENDERER__;

// ============================================================================
// BLOCK ELEMENTS
// ============================================================================

export const Para = (args) => (
  <p>{renderChildren(args)}</p>
);

export const Plain = (args) => (
  <>{renderChildren(args)}</>
);

export const Header = (args) => {
  const level = args.node.c[0];
  const [id, classes, attrs] = args.node.c[1];
  const Tag = `h${level}`;

  const props = {
    id: id || undefined,
    className: classes.length > 0 ? classes.join(' ') : undefined,
  };

  // Convert attrs array to style/data attributes
  attrs.forEach(([key, value]) => {
    if (key === 'style') {
      props.style = parseStyleString(value);
    } else {
      props[`data-${key}`] = value;
    }
  });

  return <Tag {...props}>{renderChildren(args)}</Tag>;
};

export const CodeBlock = (args) => {
  const [[id, classes, attrs], code] = args.node.c;

  const props = {
    id,
    className: classes.join(' '),
  };

  attrs.forEach(([key, value]) => {
    props[`data-${key}`] = value;
  });

  return (
    <pre {...props}><code>{code}</code></pre>
  );
};

export const BulletList = (args) => (
  <ul>{renderChildren(args)}</ul>
);

export const OrderedList = (args) => {
  const [[start]] = args.node.c;
  return <ol start={start}>{renderChildren(args)}</ol>;
};

export const BlockQuote = (args) => (
  <blockquote>{renderChildren(args)}</blockquote>
);

export const Div = (args) => {
  const [[id, classes, attrs]] = args.node.c;

  const props = {
    id: id || undefined,
    className: classes.length > 0 ? classes.join(' ') : undefined,
  };

  attrs.forEach(([key, value]) => {
    if (key === 'style') {
      props.style = parseStyleString(value);
    } else {
      props[`data-${key}`] = value;
    }
  });

  return <div {...props}>{renderChildren(args)}</div>;
};

export const HorizontalRule = () => <hr />;

export const RawBlock = memo((args) => {
  const [format, content] = args.node.c;

  // Only render HTML raw blocks
  if (format === 'html') {
    return <div dangerouslySetInnerHTML={{ __html: content }} />;
  }

  // For other formats, render as preformatted text
  return <pre className={`raw-${format}`}>{content}</pre>;
}, (prevArgs, nextArgs) => {
  // Custom comparison: only re-render if the content actually changed
  const [prevFormat, prevContent] = prevArgs.node.c;
  const [nextFormat, nextContent] = nextArgs.node.c;
  return prevFormat === nextFormat && prevContent === nextContent;
});

export const Figure = (args) => {
  const [[id, classes, attrs], [, captionBlocks]] = args.node.c;

  const props = {
    id: id || undefined,
    className: classes.length > 0 ? classes.join(' ') : undefined,
  };

  attrs.forEach(([key, value]) => {
    if (key === 'style') {
      props.style = parseStyleString(value);
    } else {
      props[`data-${key}`] = value;
    }
  });

  return (
    <figure {...props}>
      {renderChildren(args)}
      {captionBlocks && captionBlocks.length > 0 && (
        <figcaption>
          {captionBlocks.map((block, i) =>
            renderNode({
              key: i,
              node: block,
              setLocalAst: () => { },
              onNavigateToDocument: args.onNavigateToDocument
            }, block.t)
          )}
        </figcaption>
      )}
    </figure>
  );
};

// ============================================================================
// INLINE ELEMENTS
// ============================================================================

export const Str = ({ node }) => <>{node.c}</>;

export const Space = () => ' ';

export const SoftBreak = () => '\n';

export const LineBreak = () => <br />;

export const Emph = (args) => (
  <em>{renderChildren(args)}</em>
);

export const Strong = (args) => (
  <strong>{renderChildren(args)}</strong>
);

export const Code = ({ node }) => {
  const [[id, classes, attrs], code] = node.c;

  const props = {
    id,
    style: { background: 'rgb(245, 245, 245)' },
    className: classes.join(' '),
  };

  attrs.forEach(([key, value]) => {
    props[`data-${key}`] = value;
  });

  return <code {...props}>{code}</code>;
};

export const Link = (args) => {
  const [[id, classes, attrs], , [url, title]] = args.node.c;

  const props = {
    href: url,
    title: title || undefined,
    id: id || undefined,
    className: classes.length > 0 ? classes.join(' ') : undefined,
  };

  attrs.forEach(([key, value]) => {
    props[`data-${key}`] = value;
  });

  // Handle navigation if callback provided
  const handleClick = (e) => {
    if (args.onNavigateToDocument && url.startsWith('/')) {
      e.preventDefault();
      const [path, anchor] = url.split('#');
      args.onNavigateToDocument(path, anchor || null);
    }
  };

  return (
    <a {...props} onClick={handleClick}>
      {renderChildren(args)}
    </a>
  );
};

export const Image = ({ node }) => {
  const [[id, classes, attrs], inlines, [src, title]] = node.c;

  const props = {
    src,
    alt: inlines.map(inline =>
      inline.t === 'Str' ? inline.c : ''
    ).join(''),
    title: title || undefined,
    id: id || undefined,
    className: classes.length > 0 ? classes.join(' ') : undefined,
  };

  attrs.forEach(([key, value]) => {
    if (key === 'width') {
      props.width = value;
    } else if (key === 'height') {
      props.height = value;
    } else {
      props[`data-${key}`] = value;
    }
  });

  return <img {...props} />;
};

export const Span = (args) => {
  const [[id, classes, attrs]] = args.node.c;

  const props = {
    id: id || undefined,
    className: classes.length > 0 ? classes.join(' ') : undefined,
  };

  attrs.forEach(([key, value]) => {
    if (key === 'style') {
      props.style = parseStyleString(value);
    } else {
      props[`data-${key}`] = value;
    }
  });

  return <span {...props}>{renderChildren(args)}</span>;
};

// ============================================================================
// UTILITY FUNCTIONS
// ============================================================================

function parseStyleString(styleStr) {
  const style = {};
  styleStr.split(';').forEach(rule => {
    const [prop, value] = rule.split(':').map(s => s.trim());
    if (prop && value) {
      // Convert CSS property names to camelCase (e.g., 'background-color' -> 'backgroundColor')
      const camelProp = prop.replace(/-([a-z])/g, (g) => g[1].toUpperCase());
      style[camelProp] = value;
    }
  });
  return style;
}
