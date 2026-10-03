import { useState, useEffect, useRef } from 'react';
import type * as Monaco from 'monaco-editor';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import { isSourceFile } from '@quarto/preview-renderer/types/project';
import type { Diagnostic, RenderComment } from '@quarto/preview-renderer/types/diagnostic';
import type { ActorIdentity, CaptureRef } from '@quarto/preview-runtime';
import { parseQmdToAst, isWasmReady, initWasm, resolvePandocFormats } from '@quarto/preview-runtime';
import Preview from './Preview';
import ReactPreview from './ReactPreview';
import { FallbackView, NonQmdPlaceholderView } from '@quarto/preview-renderer/overlays/PreviewStaticInfoViews';
import { classifyPreviewMode, type PreviewMode } from './getQ2Format';
import { DownloadOnlyView, NeitherView } from './DownloadOnlyViews';
import PdfPreviewPane from './PdfPreviewPane';
import { formatByKey, menuFormats, pdfPreviewAvailable } from '../../pandoc/downloadService';
import { useDownloadAs } from '../../pandoc/useDownloadAs';

interface PreviewRouterProps {
  content: string;
  /** The project's id: the PDF preview keys its in-flight renders by it (two projects can share a path). */
  projectKey?: string;
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
  /** The router's mode (react / dom / download / neither); drives the "Download as" control. */
  onPreviewModeChange?: (mode: PreviewMode) => void;
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
   * Reports the active page's outstanding editorial comments up to
   * `Editor.tsx` for the comments-toggle badge (bd-0rsk07il, GH
   * #445). Only fires from the ReactPreview branch — the non-React
   * `Preview` branch has no comment chrome.
   */
  onCommentsChange?: (comments: RenderComment[]) => void;
  /**
   * Reports `useAttribution`'s in-flight state up to `Editor.tsx` so
   * the Attribution pill can animate its border while attribution
   * data is being generated. Only fires from the ReactPreview branch;
   * the non-React `Preview` branch never computes attribution.
   */
  onAttributionGeneratingChange?: (generating: boolean) => void;
}

/**
 * Router component that selects between ReactPreview and Preview based on
 * the document's resolved format (see `getQ2Format` for the rule).
 *
 * - The default — no `format:` key, `format: html`, or a `format: html: {…}`
 *   map — and every `q2-*` pseudo-format except `q2-html-render`, plus
 *   `revealjs`, mount `ReactPreview` (the React AST renderer; `html`
 *   renders as `q2-preview`, matching `q2 preview`).
 * - `format: q2-html-render` (the explicit full-DOM opt-out) and non-html
 *   formats (`pdf`, `docx`, extension formats, …) mount `Preview`, the
 *   MorphIframe renderer that morphs a complete HTML render into an iframe.
 *
 * The resolved format is echoed up via `onFormatChange`; `Editor.tsx` gates
 * the Edit / Authors pills and the printable-document affordance on it.
 */
export default function PreviewRouter(props: PreviewRouterProps) {
  const [previewMode, setPreviewMode] = useState<PreviewMode>({ mode: 'dom' });
  const reactFormat = previewMode.mode === 'react' ? previewMode.format : null;
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
        const deps = {
          resolve: (path: string) => (isWasmReady() ? resolvePandocFormats(path) : null),
          canDownload: (key: string) => menuFormats().some((f) => f.key === key),
          canPreviewPdf: pdfPreviewAvailable,
        };
        // A format the parser's metadata merge does not know (`latex`) fails the parse. The
        // resolver still classifies it; a failed parse of a previewable document changes nothing.
        const mode = classifyPreviewMode(result.success ? result.ast : '{"meta":{}}', props.currentFile?.path, deps);
        if (result.success || mode.mode === 'download' || mode.mode === 'pdf' || mode.mode === 'neither') {
          const format = mode.mode === 'react' ? mode.format : null;
          // Keep the same object when nothing changed, so a keystroke does not re-render consumers.
          setPreviewMode((prev) => (JSON.stringify(prev) === JSON.stringify(mode) ? prev : mode));
          lastStableFormatRef.current = format;
          props.onFormatChange?.(format);
          props.onPreviewModeChange?.(mode);
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
    // Quiet placeholder: light gray, no text — the same gray as the
    // renderer page that follows, so the hand-off is seamless.
    return <div style={{ height: '100%', background: '#f4f4f4' }} aria-busy="true" />;
  }

  // Non-source files (not .qmd/.md): show placeholder
  if (!isSourceFile(props.currentFile?.path)) {
    return <NonQmdPlaceholderView filename={props.currentFile?.path ?? 'no currentFile path'} />;
  }

  // Render the appropriate preview component with shared WASM error banner.
  // `identities` and `attributionOn` are for ReactPreview only — Preview
  // doesn't know about either.
  const { onRegisterScrollToLine, onRegisterSetScrollRatio, onRegisterReplayScroll, onFormatChange, onPreviewModeChange: _onPreviewModeChange, onContentRewrite, fileContents, identities, captures, attributionOn, commentsMode, onCommentsChange, onAttributionGeneratingChange, ...commonProps } = props;

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      {wasmError && (
        // WASM loading fallback
        <FallbackView content={props.content} message="Loading WASM renderer..." />
      )}
      <div style={{ flex: 1, overflow: 'hidden' }}>
        {previewMode.mode === 'download' ? (
          <DownloadModePane formatKey={previewMode.formatKey} path={props.currentFile?.path ?? null} content={props.content} wasmReady={wasmStatus === 'ready'} />
        ) : previewMode.mode === 'pdf' ? (
          <PdfPreviewPane path={props.currentFile?.path ?? null} content={props.content} projectKey={props.projectKey} />
        ) : previewMode.mode === 'neither' ? (
          <NeitherView formatKey={previewMode.formatKey} />
        ) : reactFormat ? (
          <ReactPreview {...commonProps} onContentRewrite={onContentRewrite} fileContents={fileContents} format={reactFormat} identities={identities} captures={captures} attributionOn={attributionOn} commentsMode={commentsMode} onCommentsChange={onCommentsChange} onAttributionGeneratingChange={onAttributionGeneratingChange} onRegisterReplayScroll={onRegisterReplayScroll} />
        ) : (
          // Phase 9 Decision 6: pass `fileContents` so any sibling
          // edit (including `_quarto.yml`) triggers a re-render via
          // the Map identity changing on every Automerge update.
          // bd-uy4uygha: `captures` threads the capture sidecar so the full-DOM
          // `q2-html-render` preview can splice executed output too (previously
          // only ReactPreview / q2-preview consumed captures).
          <Preview {...commonProps} fileContents={fileContents} captures={captures} onRegisterScrollToLine={onRegisterScrollToLine} onRegisterSetScrollRatio={onRegisterSetScrollRatio} />
        )}
      </div>
    </div>
  );
}

/** The "download" mode pane: its own hook instance of the shared controller. */
function DownloadModePane({ formatKey, path, content, wasmReady }: { formatKey: string; path: string | null; content: string; wasmReady: boolean }) {
  const dl = useDownloadAs(path, content, wasmReady);
  const format = wasmReady ? formatByKey(formatKey) : undefined;
  if (!format) return <NeitherView formatKey={formatKey} />;
  return <DownloadOnlyView format={format} busy={dl.status.phase === 'working'} onDownload={() => dl.start(format)} />;
}
