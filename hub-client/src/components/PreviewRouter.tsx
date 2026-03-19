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
  onRegisterSetScrollRatio?: (fn: (ratio: number) => void) => void;
  onAstChange?: (astJson: string | null) => void;
  currentSlideIndex?: number;
  onSlideChange?: (slideIndex: number) => void;
  onFormatChange?: (format: string | null) => void;
  setContent: (content: string) => void;
}

/**
 * Extract format string from the parsed AST metadata
 * Returns null if no format is found, otherwise returns the format string (e.g., 'q2-slides', 'q2-debug')
 */
function getQ2Format(astJson: string): string | null {
  try {
    const ast = JSON.parse(astJson);
    const fmt = ast?.meta?.format;
    if (!fmt) return null;
    let formatStr: string | null = null;
    // MetaString: { t: "MetaString", c: "q2-slides" }
    if (fmt.t === 'MetaString') formatStr = fmt.c;
    // MetaInlines: { t: "MetaInlines", c: [{ t: "Str", c: "q2-slides" }] }
    if (fmt.t === 'MetaInlines') formatStr = fmt.c?.[0]?.c;
    // Only return formats handled by ReactPreview
    if (formatStr?.startsWith('q2-')) return formatStr;
    return null;
  } catch (err) {
    console.error('[PreviewRouter] Failed to parse AST:', err);
    return null;
  }
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

  // Check the format whenever content changes
  useEffect(() => {
    let cancelled = false;

    async function checkFormat() {
      try {
        // Ensure WASM is ready
        if (!isWasmReady()) {
          await initWasm();
        }

        // Parse the QMD to AST to check metadata
        const result = await parseQmdToAst(props.content);

        if (cancelled) return;

        if (result.success) {
          const format = getQ2Format(result.ast);
          setReactFormat(format);
          props.onFormatChange?.(format);
        } else {
          // On parse error, default to normal Preview
          setReactFormat(null);
          props.onFormatChange?.(null);
        }
      } catch (err) {
        console.error('[PreviewRouter] Error checking format:', err);
        if (!cancelled) {
          setReactFormat(null);
          props.onFormatChange?.(null);
        }
      } finally {
        if (!cancelled) {
          setCheckedPath(props.currentFile?.path);
        }
      }
    }

    checkFormat();

    return () => {
      cancelled = true;
    };
  }, [props.content, props.currentFile?.path]);

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

  // Render the appropriate preview component
  if (reactFormat) {
    // ReactPreview doesn't use onRegisterScrollToLine/onRegisterSetScrollRatio/onFormatChange, so we omit them
    const { onRegisterScrollToLine, onRegisterSetScrollRatio, onFormatChange, ...reactPreviewProps } = props;
    return <ReactPreview {...reactPreviewProps} format={reactFormat} />;
  } else {
    const { onFormatChange, setContent, ...previewProps } = props;
    return <Preview {...previewProps} />;
  }
}
