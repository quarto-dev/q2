import { useState, useEffect, useRef } from 'react';
import type * as Monaco from 'monaco-editor';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import { isSourceFile } from '@quarto/preview-renderer/types/project';
import type { Diagnostic } from '@quarto/preview-renderer/types/diagnostic';
import type { ActorIdentity, CaptureRef } from '@quarto/preview-runtime';
import { parseQmdToAst, isWasmReady, initWasm } from '@quarto/preview-runtime';
import Preview from './Preview';
import ReactPreview from './ReactPreview';
import { FallbackView, NonQmdPlaceholderView } from '@quarto/preview-renderer/overlays/PreviewStaticInfoViews';
import { getQ2Format } from './getQ2Format';

interface PreviewRouterProps {
  content: string;
  currentFile: FileEntry | null;
  files: FileEntry[];
  fileContents: Map<string, string>;
  scrollSyncEnabled: boolean;
  editorRef: React.RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
  editorReady: boolean;
  editorHasFocusRef: React.RefObject<boolean>;
  onFileChange: (file: FileEntry, anchor?: string) => void;
  onOpenNewFileDialog: (initialFilename: string) => void;
  onDiagnosticsChange: (diagnostics: Diagnostic[]) => void;
  onWasmStatusChange?: (status: 'loading' | 'ready' | 'error', error: string | null) => void;
  onRegisterScrollToLine?: (fn: (line: number) => void) => void;
  onRegisterSetScrollRatio?: (fn: (ratio: number) => void) => void;
  /** q2-preview only: register a deferred scroll-to-line for replay scrubbing. */
  onRegisterReplayScroll?: (fn: (line: number) => void) => void;
  onAstChange?: (astJson: string | null) => void;
  currentSlideIndex?: number;
  onSlideChange?: (slideIndex: number) => void;
  onFormatChange?: (format: string | null) => void;
  onContentRewrite: (content: string) => void;
  /**
   * Automerge actor → display identity (name + colour). Threaded
   * through to ReactPreview's `useAttribution` so the Attribution
   * overlay uses profile-metadata names where available, instead
   * of the `actor.slice(0, 8)` fallback hash.
   */
  identities?: Record<string, ActorIdentity>;
  /**
   * Path → recorded engine capture sidecar entry (bd-sfet3264).
   * Threaded into ReactPreview so the active document's capture can be
   * fetched and spliced into the rendered AST.
   */
  captures?: Record<string, CaptureRef>;
  /**
   * Attribution overlay on/off. Session-only — owned by `Editor.tsx`
   * as `useState`, threaded down here and into `ReactPreview` to
   * drive `useAttribution`.
   */
  attributionOn: boolean;
  /**
   * Comment-bubble display mode (expand / show / hide). Session-only —
   * owned by `Editor.tsx`, threaded into `ReactPreview` (the non-React
   * `Preview` branch has no comment chrome).
   */
  commentsMode?: 'expand' | 'show' | 'hide';
  /**
   * Reports `useAttribution`'s in-flight state up to `Editor.tsx` so
   * the Attribution pill can animate its border while attribution
   * data is being generated. Only fires from the ReactPreview branch;
   * the non-React `Preview` branch never computes attribution.
   */
  onAttributionGeneratingChange?: (generating: boolean) => void;
}

/**
 * Router component that selects between Preview and ReactPreview based on document format.
 *
 * - If format: q2-slides or format: q2-debug is present in the YAML frontmatter, use ReactPreview
 * - Otherwise, use the normal Preview component (for regular HTML rendering)
 */
export default function PreviewRouter(props: PreviewRouterProps) {
  const [reactFormat, setReactFormat] = useState<string | null>(null);
  const [checkedPath, setCheckedPath] = useState<string | undefined>(undefined);
  const initialChecking = checkedPath !== props.currentFile?.path;

  // Track the last stable format to avoid unmounting during re-checks
  const lastStableFormatRef = useRef<string | null>(null);

  // WASM initialization state - shared by both Preview and ReactPreview
  const [wasmStatus, setWasmStatus] = useState<'loading' | 'ready' | 'error'>('loading');
  const [wasmError, setWasmError] = useState<string | null>(null);

  // Initialize WASM on mount
  useEffect(() => {
    async function init() {
      try {
        setWasmStatus('loading');
        await initWasm();
        setWasmStatus('ready');
      } catch (err) {
        setWasmStatus('error');
        setWasmError(err instanceof Error ? err.message : String(err));
      }
    }

    init();
  }, []);

  // Notify parent when WASM status changes
  useEffect(() => {
    props.onWasmStatusChange?.(wasmStatus, wasmError);
  }, [wasmStatus, wasmError, props.onWasmStatusChange]);

  // Check the format whenever content changes
  useEffect(() => {
    async function checkFormat() {
      try {
        // Skip format check if WASM isn't ready yet (will retry when it is)
        if (!isWasmReady()) {
          return;
        }

        // Parse the QMD to AST to check metadata
        const result = await parseQmdToAst(props.content);
        if (result.success) {
          const format = getQ2Format(result.ast);
          setReactFormat(format);
          lastStableFormatRef.current = format;
          props.onFormatChange?.(format);
        }
      } catch (err) {
        console.error('[PreviewRouter] Error checking format:', err);
      } finally {
        setCheckedPath(props.currentFile?.path);
      }
    }

    checkFormat();
  }, [props.content, props.currentFile?.path, wasmStatus]);

  // Show loading state only during the very first format check.
  // Subsequent re-checks keep the current Preview mounted to avoid
  // a destructive unmount/remount cycle on every keystroke.
  if (initialChecking) {
    return (
      <div style={{ padding: '20px', color: '#666' }}>
        Loading preview...
      </div>
    );
  }

  // Non-source files (not .qmd/.md): show placeholder
  if (!isSourceFile(props.currentFile?.path)) {
    return <NonQmdPlaceholderView filename={props.currentFile?.path ?? 'no currentFile path'} />;
  }

  // Render the appropriate preview component with shared WASM error banner.
  // `identities` and `attributionOn` are for ReactPreview only — Preview
  // doesn't know about either.
  const { onRegisterScrollToLine, onRegisterSetScrollRatio, onRegisterReplayScroll, onFormatChange, onContentRewrite, fileContents, identities, captures, attributionOn, commentsMode, onAttributionGeneratingChange, ...commonProps } = props;

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      {wasmError && (
        // WASM loading fallback
        <FallbackView content={props.content} message="Loading WASM renderer..." />
      )}
      <div style={{ flex: 1, overflow: 'hidden' }}>
        {reactFormat ? (
          <ReactPreview {...commonProps} onContentRewrite={onContentRewrite} fileContents={fileContents} format={reactFormat} identities={identities} captures={captures} attributionOn={attributionOn} commentsMode={commentsMode} onAttributionGeneratingChange={onAttributionGeneratingChange} onRegisterReplayScroll={onRegisterReplayScroll} />
        ) : (
          // Phase 9 Decision 6: pass `fileContents` so any sibling
          // edit (including `_quarto.yml`) triggers a re-render via
          // the Map identity changing on every Automerge update.
          // bd-uy4uygha: `captures` threads the capture sidecar so the default
          // `format: html` preview can splice executed output (previously only
          // ReactPreview / q2-preview consumed captures).
          <Preview {...commonProps} fileContents={fileContents} captures={captures} onRegisterScrollToLine={onRegisterScrollToLine} onRegisterSetScrollRatio={onRegisterSetScrollRatio} />
        )}
      </div>
    </div>
  );
}
