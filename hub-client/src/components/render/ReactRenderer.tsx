import { useMemo, Component } from 'react';
import type { ReactNode } from 'react';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import { Q2DebugIframe } from './q2-debug/Q2DebugIframe';
import { Q2PreviewIframe } from '@quarto/preview-renderer/iframe/Q2PreviewIframe';
import { Q2RawIframe } from './q2-raw/Q2RawIframe';
import { SlideAst } from './ReactAstSlideRenderer';
import { RevealjsSlideAst } from './RevealjsReactAstSlideRenderer';
import { transpileTSX } from '../../services/tsxTranspiler';
import { resolveComponentPath } from '@quarto/preview-renderer/utils/componentPath';
import type { PandocAST } from '@quarto/preview-renderer/framework';
import { extractMetaString } from '@quarto/preview-renderer/framework';

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
  // Format type: 'q2-slides', 'q2-debug', or 'q2-preview'
  format: string;
  /**
   * Compiled theme CSS fingerprint, three-way (Plan 2A item 11):
   *   - `string`: render produced a theme; iframe should display it.
   *   - `null`: render succeeded with no theme intended.
   *   - `undefined`: render failed or pre-first-render; iframe keeps
   *     last-good styling.
   * Forwarded to `Q2PreviewIframe` only; q2-debug ignores it.
   */
  themeFingerprint?: string | null;
  /**
   * The QMD source text that was used to produce the current render
   * generation. The byte offsets in `astJson` belong to this content
   * snapshot, not to the live editor text (which may have diverged).
   * Forwarded to `Q2PreviewIframe` as `renderedContent` so the iframe
   * can slice source bytes without skew.
   */
  renderedContent?: string;
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
  themeFingerprint,
  renderedContent,
}: ReactRendererProps) {
  // Extract component paths - only recompute when the list of paths
  // changes. The gate covers both q2-debug and q2-preview because both
  // load user TSX overrides via the iframe's
  // `LOAD_CUSTOM_COMPONENTS` postMessage handler. Plan 2A item 13
  // extended the q2-debug-only gate to also include q2-preview.
  const componentPathsKey = useMemo(() => {
    if (format !== 'q2-debug' && format !== 'q2-preview') {
      return '';
    }

    const ast = JSON.parse(astJson);
    // Walk the MetaList → MetaInlines → Str(c) chain. Entries that
    // don't resolve to a non-empty string are dropped: this includes
    // (a) `render-components:\n  -` mid-typing, where the bullet has
    // no value and parses to `null`, and (b) an empty MetaInlines
    // (the user typed the path-string-open delimiter but no content
    // yet). Without this filter, `resolveComponentPath(undefined …)`
    // throws inside this useMemo and the iframe-host page goes blank
    // with no upstream ErrorBoundary to catch it.
    const rawPaths: unknown[] =
      ast?.meta?.['render-components']?.c?.map?.((o: any) => o?.c?.[0]?.c) ??
      [];
    const componentPaths = rawPaths.filter(
      (p): p is string => typeof p === 'string' && p.length > 0,
    );

    return JSON.stringify(componentPaths);
  }, [format, astJson]);

  // Transpile components - only when component paths list changes (not when their contents change)
  const customComponentsCode = useMemo(() => {
    if (!componentPathsKey) {
      return {};
    }

    const componentPaths = JSON.parse(componentPathsKey) as string[];

    const componentsCode: Record<string, string> = {};
    for (const path of componentPaths) {
      // render-components entries can be either project-root-absolute
      // (leading slash) or relative to the current document's directory.
      // fileContents is keyed by project-root-relative paths without the
      // leading slash, so resolve before lookup.
      const lookupPath = resolveComponentPath(path, currentFilePath);
      const tsxCode = fileContents.get(lookupPath);
      if (!tsxCode) {
        console.warn(`[ReactRenderer] Component file not found: ${path}`);
        continue;
      }

      try {
        const jsCode = transpileTSX(tsxCode);
        componentsCode[path] = jsCode;
      } catch (err) {
        console.error(`[ReactRenderer] Failed to transpile component ${path}:`, err);
      }
    }

    return componentsCode;
  }, [componentPathsKey, currentFilePath]);

  // Plan 2A item 12: format-dispatch split. q2-debug (raw AST view)
  // and q2-preview (post-pipeline AST for the live preview) now run
  // through distinct iframe wrappers. q2-preview adds a parent-side
  // theme-CSS effect that q2-debug doesn't need, so the two surfaces
  // diverge at the wrapper level. They still share the shared
  // `framework/` plumbing (registry, dispatchers, Ast).
  if (format === 'q2-raw') {
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
        }}>
          <Q2RawIframe astJson={astJson} />
        </div>
      </ErrorBoundary>
    );
  }
  if (format === 'q2-debug') {
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
        }}>
          <Q2DebugIframe
            astJson={astJson}
            currentFilePath={currentFilePath}
            onNavigateToDocument={onNavigateToDocument}
            setAst={setAst}
            customComponentsCode={customComponentsCode}
          />
        </div>
      </ErrorBoundary>
    );
  }
  if (format === 'q2-preview') {
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
        }}>
          <Q2PreviewIframe
            astJson={astJson}
            currentFilePath={currentFilePath}
            onNavigateToDocument={onNavigateToDocument}
            setAst={setAst}
            customComponentsCode={customComponentsCode}
            themeFingerprint={themeFingerprint}
            renderedContent={renderedContent}
          />
        </div>
      </ErrorBoundary>
    );
  }

  // q2-slides or revealjs format - check if it's revealjs
  const ast = JSON.parse(astJson);
  const isRevealjs =
    format === 'revealjs' || extractMetaString(ast?.meta?.format) === 'revealjs';

  return (
    <ErrorBoundary>
      {isRevealjs ? (
        <RevealjsSlideAst
          astJson={astJson}
          currentFilePath={currentFilePath}
          onNavigateToDocument={onNavigateToDocument}
          currentSlide={currentSlideIndex}
          onSlideChange={onSlideChange}
        />
      ) : (
        <SlideAst
          astJson={astJson}
          currentFilePath={currentFilePath}
          onNavigateToDocument={onNavigateToDocument}
          currentSlide={currentSlideIndex}
          onSlideChange={onSlideChange}
        />
      )}
    </ErrorBoundary>
  );
}

export default ReactRenderer;
