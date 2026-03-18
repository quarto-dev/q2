import { Ast } from './ReactAstDebugRenderer';
import { SlideAst } from './ReactAstSlideRenderer';

// Simplified Pandoc AST type for setAst callback
interface PandocAST {
  'pandoc-api-version': [number, number, number];
  meta: Record<string, unknown>;
  blocks: unknown[];
}

interface ReactRendererProps {
  // Pandoc AST as JSON string
  astJson: string;
  // Current file path for resolving relative links
  currentFilePath: string;
  // Callback when user navigates to a different document (with optional anchor)
  onNavigateToDocument: (targetPath: string, anchor: string | null) => void;
  // Callback when AST is modified
  setAst: (newAst: PandocAST) => void;
  // Optional controlled current slide index
  currentSlideIndex?: number;
  // Callback when slide changes (for manual navigation via arrows/buttons)
  onSlideChange?: (slideIndex: number) => void;
  // Format type: 'q2-slides' or 'q2-debug'
  format: string;
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
  currentFilePath,
  onNavigateToDocument,
  setAst,
  currentSlideIndex,
  onSlideChange,
  format,
}: ReactRendererProps) {
  if (format === 'q2-debug') {
    return (
      <div style={{
        width: '100%',
        height: '100%',
        position: 'absolute',
        top: 0,
        left: 0,
        right: 0,
        bottom: 0,
        overflowY: 'scroll'
      }}>
        <Ast
          astJson={astJson}
          onNavigateToDocument={onNavigateToDocument}
          setAst={setAst}
        />
      </div>
    );
  }

  // q2-slides format
  return (
    <SlideAst
      astJson={astJson}
      currentFilePath={currentFilePath}
      onNavigateToDocument={onNavigateToDocument}
      currentSlide={currentSlideIndex}
      onSlideChange={onSlideChange}
    />
  );
}

export default ReactRenderer;
