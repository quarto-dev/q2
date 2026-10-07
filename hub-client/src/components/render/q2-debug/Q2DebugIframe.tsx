import { useEffect, useRef, useState } from 'react';
import type { RefObject } from 'react';
import type * as Monaco from 'monaco-editor';
import type { CursorPayload } from '../iframeMessageDispatch';
import { utf8Length, utf16IndexForByteOffset } from './sourceOffset';

interface Q2DebugIframeProps {
  astJson: string;
  currentFilePath: string;
  onNavigateToDocument?: (path: string, anchor: string | null) => void;
  setAst: (newAst: any) => void;
  customComponentsCode?: Record<string, string>; // Component name -> transpiled JS code
  /**
   * The QMD source `astJson` was parsed from. The iframe slices it by
   * the nodes' byte ranges to align escaped/entity source against
   * rendered `Str` text when placing the virtual caret.
   */
  renderedContent?: string;
  /**
   * Monaco editor handle. When present (and `editorReady`), the
   * wrapper mirrors the editor cursor into the iframe as a `CURSOR`
   * message carrying the UTF-8 byte offset, which the iframe turns
   * into a virtual caret over the matching AST node.
   */
  editorRef?: RefObject<Monaco.editor.IStandaloneCodeEditor | null>;
  editorReady?: boolean;
}

/**
 * The editor caret as a UTF-8 byte offset into the model text — the
 * unit the AST's `l.b.o` / `l.e.o` fields use. Monaco's `getOffsetAt`
 * is UTF-16, so the prefix is re-measured in bytes (`utf8Length` has
 * an ASCII fast path, so this is cheap for typical documents).
 */
function cursorPayload(
  editor: Monaco.editor.IStandaloneCodeEditor,
  focused: boolean,
): CursorPayload | null {
  const model = editor.getModel();
  const position = editor.getPosition();
  if (!model || !position) return null;
  const text = model.getValue();
  const byteAt = (p: Monaco.IPosition) => utf8Length(text.slice(0, model.getOffsetAt(p)));
  const payload: CursorPayload = { offset: byteAt(position), focused };
  const sel = editor.getSelection();
  if (sel && !sel.isEmpty()) {
    payload.selection = {
      start: byteAt(sel.getStartPosition()),
      end: byteAt(sel.getEndPosition()),
    };
  }
  return payload;
}

/** Monaco position for a UTF-8 byte offset into the model text. */
function positionForByteOffset(model: Monaco.editor.ITextModel, offset: number): Monaco.Position {
  return model.getPositionAt(utf16IndexForByteOffset(model.getValue(), offset));
}

/**
 * Wrapper component that renders the q2-debug Ast component in a
 * sandboxed iframe. Verbatim port of the original AstIframe with the
 * iframe `src` pointing at the renamed `/q2-debug.html` route.
 */
export function Q2DebugIframe({
  astJson,
  currentFilePath,
  onNavigateToDocument,
  setAst,
  customComponentsCode,
  renderedContent,
  editorRef,
  editorReady,
}: Q2DebugIframeProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [iframeReady, setIframeReady] = useState(false);

  // Handle messages from the iframe
  useEffect(() => {
    const handleMessage = (event: MessageEvent) => {
      // In production, verify event.origin for security
      if (event.data.type === 'IFRAME_READY') {
        setIframeReady(true);
      } else if (event.data.type === 'NAVIGATE_TO_DOCUMENT') {
        onNavigateToDocument?.(event.data.path, event.data.anchor);
      } else if (event.data.type === 'SET_AST') {
        setAst(event.data.ast);
      } else if (event.data.type === 'CURSOR_CLICK') {
        // Reverse of `cursorPayload`: a UTF-8 byte offset from the
        // iframe's click hit-test becomes a Monaco position. With
        // `extend` (shift-click) the current selection anchor is kept
        // and only the head moves.
        const editor = editorRef?.current;
        const model = editor?.getModel();
        if (!editor || !model || typeof event.data.offset !== 'number') return;
        const head = positionForByteOffset(model, event.data.offset);
        const current = editor.getSelection();
        if (event.data.extend && current) {
          editor.setSelection({
            selectionStartLineNumber: current.selectionStartLineNumber,
            selectionStartColumn: current.selectionStartColumn,
            positionLineNumber: head.lineNumber,
            positionColumn: head.column,
          });
        } else {
          editor.setPosition(head);
        }
        editor.revealPositionInCenterIfOutsideViewport(head);
        editor.focus();
      } else if (event.data.type === 'CURSOR_SELECT') {
        // Drag in the preview: anchor stays where the pointer went
        // down, head follows the pointer.
        const editor = editorRef?.current;
        const model = editor?.getModel();
        if (!editor || !model) return;
        const { anchor, head } = event.data;
        if (typeof anchor !== 'number' || typeof head !== 'number') return;
        const a = positionForByteOffset(model, anchor);
        const h = positionForByteOffset(model, head);
        editor.setSelection({
          selectionStartLineNumber: a.lineNumber,
          selectionStartColumn: a.column,
          positionLineNumber: h.lineNumber,
          positionColumn: h.column,
        });
        editor.revealPositionInCenterIfOutsideViewport(h);
      }
    };

    window.addEventListener('message', handleMessage);
    return () => window.removeEventListener('message', handleMessage);
  }, [onNavigateToDocument, setAst, editorRef]);

  // Send custom components code when iframe is ready (only once or when it changes)
  useEffect(() => {
    if (!iframeReady || !iframeRef.current?.contentWindow) return;

    if (customComponentsCode) {
      iframeRef.current.contentWindow.postMessage(
        {
          type: 'LOAD_CUSTOM_COMPONENTS',
          componentsCode: customComponentsCode,
        },
        '*'
      );
    }
  }, [iframeReady, customComponentsCode]);

  // Send AST updates when iframe is ready
  useEffect(() => {
    if (!iframeReady || !iframeRef.current?.contentWindow) return;

    iframeRef.current.contentWindow.postMessage(
      {
        type: 'UPDATE_AST',
        payload: {
          astJson,
          currentFilePath,
          renderedContent,
        },
      },
      '*'
    );
  }, [iframeReady, astJson, currentFilePath, renderedContent]);

  // Mirror the Monaco cursor into the iframe. Re-sent on every AST
  // update too (the iframe re-places the caret after it re-renders,
  // but a fresh payload keeps the two in lockstep when the content
  // snapshot the AST was parsed from changes).
  useEffect(() => {
    if (!iframeReady || !editorReady) return;
    const editor = editorRef?.current;
    const win = iframeRef.current?.contentWindow;
    if (!editor || !win) return;

    let focused = editor.hasTextFocus();
    const send = () => {
      win.postMessage({ type: 'CURSOR', payload: cursorPayload(editor, focused) }, '*');
    };
    const disposables = [
      // Fires for caret moves and selection changes alike.
      editor.onDidChangeCursorSelection(send),
      editor.onDidFocusEditorText(() => { focused = true; send(); }),
      editor.onDidBlurEditorText(() => { focused = false; send(); }),
    ];
    send();
    return () => {
      disposables.forEach(d => d.dispose());
      win.postMessage({ type: 'CURSOR', payload: null }, '*');
    };
  }, [iframeReady, editorReady, editorRef, astJson]);

  return (
    <iframe
      ref={iframeRef}
      src="q2-debug.html"
      title="q2-debug Renderer"
      sandbox="allow-scripts allow-same-origin"
      style={{
        width: '100%',
        height: '100%',
        border: 'none',
        display: 'block',
      }}
    />
  );
}
