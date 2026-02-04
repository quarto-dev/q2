import { SlideAst } from './ReactAstSlideRenderer';

interface ReactRendererProps {
  // Pandoc AST as JSON string
  astJson: string;
  // Current file path for resolving relative links
  currentFilePath: string;
  // Callback when user navigates to a different document (with optional anchor)
  onNavigateToDocument: (targetPath: string, anchor: string | null) => void;
}

/**
 * React-based renderer that displays Pandoc AST as React components.
 *
 * Unlike the HTML/iframe-based preview, this renders the AST directly
 * as React elements, providing better integration with React's state
 * management and event handling.
 */
function ReactRenderer({
  astJson,
  onNavigateToDocument,
}: ReactRendererProps) {
  return (
    <div style={{
      width: '100%',
      height: '100%',
      position: 'absolute',
      top: 0,
      left: 0,
      right: 0,
      bottom: 0
    }}>
      <SlideAst
        astJson={astJson}
        onNavigateToDocument={onNavigateToDocument}
      />
    </div>
  );
}

export default ReactRenderer;
