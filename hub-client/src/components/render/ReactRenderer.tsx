import { useMemo, useRef, useCallback, Component } from 'react';
import type { ReactNode, Ref } from 'react';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import { Q2DebugIframe } from './q2-debug/Q2DebugIframe';
import { Q2PreviewIframe, type Q2PreviewIframeHandle } from '@quarto/preview-renderer/iframe/Q2PreviewIframe';
import { Q2SandboxedPreviewIframe } from './q2-sandboxed-preview/Q2SandboxedPreviewIframe';
import { SlideAst } from './ReactAstSlideRenderer';
import { transpileTSX } from '../../services/tsxTranspiler';
import { resolveComponentPath } from '@quarto/preview-renderer/utils/componentPath';
import type { PandocAST } from '@quarto/preview-renderer/framework';

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
  /**
   * Pre-pipeline (untransformed) AST JSON shipped in lockstep with
   * `astJson` + `renderedContent` (same compound-state generation).
   * Forwarded to `Q2PreviewIframe` for the structural editability
   * gate (Plan 2a).
   */
  untransformedAstJson?: string | null;
  /**
   * Reactji-authorship demo (2026-05-25 plan): current viewer's
   * Automerge actor id, forwarded only to `Q2PreviewIframe` so user
   * TSX can do `actor === me` checks. `null` is a valid "unknown"
   * value. Sourced from `getActorId()` in `ReactPreview`.
   */
  currentActor?: string | null;
  /**
   * P3.2: nesting-cursor mode for nested blocks. Forwarded to
   * `Q2PreviewIframe` only (q2-debug/slides don't support it).
   */
  unlockNestingCursor?: boolean;
  /**
   * bd-j1nto6eq: rich-text (tiptap) block editor. Forwarded to
   * `Q2PreviewIframe` only (q2-debug/slides don't support it), exactly like
   * `unlockNestingCursor`.
   */
  richText?: boolean;
  /**
   * P3.2: per-siKey clean QMD buffers for nested blocks, produced by
   * `regenerateNestedBuffers` in `ReactPreview` (gated on
   * `unlockNestingCursor`). Forwarded to `Q2PreviewIframe` only.
   */
  nestedEditBuffers?: Record<string, string>;
  /**
   * Scroll-sync wiring, forwarded to `Q2PreviewIframe` only (the
   * q2-preview format). The other formats (q2-debug, q2-slides,
   * revealjs) don't participate in editor↔preview scroll sync.
   */
  scrollHandleRef?: Ref<Q2PreviewIframeHandle>;
  onPreviewScroll?: () => void;
  onPreviewClick?: () => void;
  onAstRendered?: () => void;
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
  untransformedAstJson,
  currentActor,
  unlockNestingCursor,
  richText,
  nestedEditBuffers,
  scrollHandleRef,
  onPreviewScroll,
  onPreviewClick,
  onAstRendered,
}: ReactRendererProps) {
  // Stable wrappers for Q2PreviewIframe props that are useEffect dependencies.
  //
  // Q2PreviewIframe's message listener re-registers when either setAst or
  // onNavigateToDocument changes identity.  setAst changes on every content
  // update (handleSetAst closes over `content`); onNavigateToDocument changes
  // when the files list syncs from Automerge.  A listener swap that races with
  // the iframe's SET_AST postMessage silently drops the message.
  //
  // Using refs keeps the listener registered exactly once from mount to unmount,
  // while still dispatching to the latest callback on each invocation.
  const setAstRef = useRef(setAst);
  setAstRef.current = setAst;
  const stableSetAst = useCallback((ast: unknown) => setAstRef.current(ast as any), []);

  const onNavigateRef = useRef(onNavigateToDocument);
  onNavigateRef.current = onNavigateToDocument;
  const stableNavigate = useCallback(
    (targetPath: string, anchor: string | null) => onNavigateRef.current(targetPath, anchor),
    [],
  );

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
  if (format === 'q2-sandboxed-preview') {
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
          <Q2SandboxedPreviewIframe astJson={astJson} />
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
            onNavigateToDocument={stableNavigate}
            setAst={stableSetAst}
            customComponentsCode={customComponentsCode}
          />
        </div>
      </ErrorBoundary>
    );
  }
  // Convergence (bd-vwp4y5ku): `format: revealjs` renders through the
  // SAME shared q2-preview iframe as `q2 preview`. The iframe's
  // `PreviewRoot` auto-detects slides (`isSlides` for revealjs/q2-slides)
  // and mounts `RevealDeck`, which applies the document's compiled reveal
  // theme via the `<style data-q2-theme>` transport — instead of the
  // legacy hand-rolled `RevealjsSlideAst` deck that hardcoded reveal's
  // stock `white.css`. `ReactPreview.doRender` feeds this branch the
  // themed preview AST (from `renderPageForPreview`) + `themeFingerprint`.
  if (format === 'q2-preview' || format === 'revealjs') {
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
            onNavigateToDocument={stableNavigate}
            setAst={stableSetAst}
            customComponentsCode={customComponentsCode}
            themeFingerprint={themeFingerprint}
            renderedContent={renderedContent}
            untransformedAstJson={untransformedAstJson}
            currentActor={currentActor}
            unlockNestingCursor={unlockNestingCursor}
            richText={richText}
            nestedEditBuffers={nestedEditBuffers}
            currentSlideIndex={currentSlideIndex}
            onSlideChange={onSlideChange}
            scrollHandleRef={scrollHandleRef}
            onScroll={onPreviewScroll}
            onClick={onPreviewClick}
            onAstRendered={onAstRendered}
          />
        </div>
      </ErrorBoundary>
    );
  }

  // q2-slides: the generic (non-reveal) slide preview. `format: revealjs`
  // is handled above through the shared q2-preview iframe (bd-vwp4y5ku);
  // the hand-rolled `RevealjsSlideAst` deck was retired with that
  // convergence, so there is no longer a reveal-specific branch here.
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
