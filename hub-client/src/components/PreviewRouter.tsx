import { useState, useEffect } from 'react';
import type * as Monaco from 'monaco-editor';
import type { FileEntry } from '../types/project';
import type { Diagnostic } from '../types/diagnostic';
import { parseQmdToAst, isWasmReady, initWasm } from '../services/wasmRenderer';
import Preview from './Preview';
import ReactPreview from './ReactPreview';

interface PreviewRouterProps {
  content: string;
  currentFile: FileEntry | null;
  files: FileEntry[];
  scrollSyncEnabled: boolean;
  editorRef: React.RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
  editorReady: boolean;
  editorHasFocusRef: React.RefObject<boolean>;
  onFileChange: (file: FileEntry, anchor?: string) => void;
  onOpenNewFileDialog: (initialFilename: string) => void;
  onDiagnosticsChange: (diagnostics: Diagnostic[]) => void;
  onWasmStatusChange?: (status: 'loading' | 'ready' | 'error', error: string | null) => void;
  onRegisterScrollToLine?: (fn: (line: number) => void) => void;
  onAstChange?: (astJson: string | null) => void;
  currentSlideIndex?: number;
  onSlideChange?: (slideIndex: number) => void;
  onFormatChange?: (isSlides: boolean) => void;
}

/**
 * Check if the parsed AST metadata contains format: q2-slides
 */
function hasQ2SlidesFormat(astJson: string): boolean {
  try {
    const ast = JSON.parse(astJson);
    const fmt = ast?.meta?.format;
    if (!fmt) return false;
    // MetaString: { t: "MetaString", c: "q2-slides" }
    if (fmt.t === 'MetaString') return fmt.c === 'q2-slides';
    // MetaInlines: { t: "MetaInlines", c: [{ t: "Str", c: "q2-slides" }] }
    if (fmt.t === 'MetaInlines') return fmt.c?.[0]?.c === 'q2-slides';
    return false;
  } catch (err) {
    console.error('[PreviewRouter] Failed to parse AST:', err);
    return false;
  }
}

/**
 * Router component that selects between Preview and ReactPreview based on document format.
 *
 * - If format: q2-slides is present in the YAML frontmatter, use ReactPreview (for slides)
 * - Otherwise, use the normal Preview component (for regular HTML rendering)
 */
export default function PreviewRouter(props: PreviewRouterProps) {
  const [useReactPreview, setUseReactPreview] = useState(false);
  const [isChecking, setIsChecking] = useState(true);

  // Check the format whenever content changes
  useEffect(() => {
    let cancelled = false;

    async function checkFormat() {
      setIsChecking(true);

      try {
        // Ensure WASM is ready
        if (!isWasmReady()) {
          await initWasm();
        }

        // Parse the QMD to AST to check metadata
        const result = await parseQmdToAst(props.content);

        if (cancelled) return;

        if (result.success) {
          const hasSlides = hasQ2SlidesFormat(result.ast);
          setUseReactPreview(hasSlides);
          props.onFormatChange?.(hasSlides);
        } else {
          // On parse error, default to normal Preview
          setUseReactPreview(false);
          props.onFormatChange?.(false);
        }
      } catch (err) {
        console.error('[PreviewRouter] Error checking format:', err);
        if (!cancelled) {
          setUseReactPreview(false);
          props.onFormatChange?.(false);
        }
      } finally {
        if (!cancelled) {
          setIsChecking(false);
        }
      }
    }

    checkFormat();

    return () => {
      cancelled = true;
    };
  }, [props.content, props.currentFile?.path]);

  // Show loading state while checking format
  if (isChecking) {
    return (
      <div style={{ padding: '20px', color: '#666' }}>
        Loading preview...
      </div>
    );
  }

  // Render the appropriate preview component
  if (useReactPreview) {
    // ReactPreview doesn't use onRegisterScrollToLine or onFormatChange, so we omit them
    const { onRegisterScrollToLine, onFormatChange, ...reactPreviewProps } = props;
    return <ReactPreview {...reactPreviewProps} />;
  } else {
    const { onFormatChange, ...previewProps } = props;
    return <Preview {...previewProps} />;
  }
}
