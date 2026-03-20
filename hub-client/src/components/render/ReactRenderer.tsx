import { useState, useEffect, useMemo, Component } from 'react';
import type { ReactNode } from 'react';
import type { FileEntry } from '../../types/project';
import { Ast, componentRegistry } from '../render/ReactAstDebugRenderer';
import { SlideAst } from './ReactAstSlideRenderer';
import { transpileAndImportTSX } from '../../services/tsxTranspiler';

// Simple error boundary to catch errors in custom components
class ErrorBoundary extends Component<
  { children: ReactNode },
  { hasError: boolean; error: Error | null }
> {
  constructor(props: { children: ReactNode }) {
    super(props);
    this.state = { hasError: false, error: null };
  }

  static getDerivedStateFromError(error: Error) {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: React.ErrorInfo) {
    console.error('[ErrorBoundary] Caught error:', error, errorInfo);
  }

  render() {
    if (this.state.hasError) {
      return (
        <div style={{
          padding: '20px',
          backgroundColor: '#fee',
          border: '1px solid #fcc',
          borderRadius: '4px',
          fontFamily: 'monospace',
          fontSize: '14px'
        }}>
          <h3 style={{ margin: '0 0 10px 0', color: '#c00' }}>Error in Component</h3>
          <p style={{ margin: '0 0 10px 0' }}>
            <strong>Message:</strong> {this.state.error?.message}
          </p>
          <details>
            <summary style={{ cursor: 'pointer' }}>Stack trace</summary>
            <pre style={{ fontSize: '12px', overflow: 'auto' }}>
              {this.state.error?.stack}
            </pre>
          </details>
        </div>
      );
    }

    return this.props.children;
  }
}

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
  // All files in the project (for loading custom components)
  files: FileEntry[];
  // File contents map
  fileContents: Map<string, string>;
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
 * Load and transpile custom TSX components from the VFS
 */
async function loadCustomComponents(
  componentPaths: string[],
  fileContents: Map<string, string>
): Promise<Record<string, React.ComponentType<any>>> {
  let allComponents: Record<string, React.ComponentType<any>> = {};

  for (const path of componentPaths) {
    try {
      // Get file content from the map
      const tsxCode = fileContents.get(path);

      if (!tsxCode) {
        console.warn(`[ReactRenderer] Component file not found: ${path}`);
        continue;
      }

      // Transpile and import the components
      const exports = await transpileAndImportTSX(tsxCode);

      allComponents = { ...allComponents, ...(exports as Record<string, React.ComponentType<any>>) };

    } catch (err) {
      console.error(`[ReactRenderer] Failed to load component ${path}:`, err);
    }
  }

  return allComponents;
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
  fileContents,
  onNavigateToDocument,
  setAst,
  currentSlideIndex,
  onSlideChange,
  format,
}: ReactRendererProps) {
  const [customComponents, setCustomComponents] = useState<Record<string, React.ComponentType<any>>>({});

  // Extract component paths from AST as a stable string for comparison
  const componentPathsKey = useMemo(() => {
    if (format !== 'q2-debug') {
      return '';
    }
    const ast = JSON.parse(astJson);
    const paths = ast?.meta?.['render-components']?.c?.map?.((o: any) => o?.c?.[0]?.c) ?? [];
    return JSON.stringify(paths);
  }, [astJson, format]);

  // Load custom components only when the component paths change
  useEffect(() => {
    if (!componentPathsKey) {
      setCustomComponents({});
      return;
    }

    const componentPaths = JSON.parse(componentPathsKey) as string[];
    loadCustomComponents(componentPaths, fileContents).then(setCustomComponents);
  }, [componentPathsKey]);

  if (format === 'q2-debug') {
    // Merge custom components with defaults (custom overrides defaults)
    const mergedRegistry = { ...componentRegistry, ...customComponents } as Record<string, (props: any) => React.ReactNode>;

    return (
      <ErrorBoundary>
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
            registry={mergedRegistry}
          />
        </div>
      </ErrorBoundary>
    );
  }

  // q2-slides format
  return (
    <ErrorBoundary>
      <SlideAst
        astJson={astJson}
        currentFilePath={currentFilePath}
        onNavigateToDocument={onNavigateToDocument}
        currentSlide={currentSlideIndex}
        onSlideChange={onSlideChange}
      />
    </ErrorBoundary>
  );
}

export default ReactRenderer;
