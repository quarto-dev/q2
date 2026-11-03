import { useEffect, useRef, useState } from 'react';
import { getUserIdentity } from '../../../services/userSettings';

interface Q2DebugIframeProps {
  astJson: string;
  currentFilePath: string;
  onNavigateToDocument?: (path: string, anchor: string | null) => void;
  /** Writes the AST back; returns the QMD it was written as (see the edit acknowledgement below). */
  setAst: (newAst: any) => string | void;
  customComponentsCode?: Record<string, string>; // Component name -> transpiled JS code
  /** The QMD that produced `astJson` (same render generation). */
  renderedContent?: string;
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
}: Q2DebugIframeProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [iframeReady, setIframeReady] = useState(false);
  // The last caret edit the iframe sent (`editSeq`) and the QMD it was
  // written as. A render whose content equals that QMD reflects the edit, and
  // its UPDATE_AST carries `editAck: editSeq`; the iframe holds other updates
  // while an edit is in flight so a stale render can't undo what was typed.
  const lastEditRef = useRef<{ seq: number; qmd: string } | null>(null);
  // The user's name and colour, for the AST caret's flag in the iframe.
  const [user, setUser] = useState<{ name: string; color: string } | null>(null);

  useEffect(() => {
    let cancelled = false;
    getUserIdentity()
      .then((s) => { if (!cancelled) setUser({ name: s.userName, color: s.userColor }); })
      .catch(() => { /* no identity: the caret shows without a flag */ });
    return () => { cancelled = true; };
  }, []);

  // Handle messages from the iframe
  useEffect(() => {
    const handleMessage = (event: MessageEvent) => {
      // In production, verify event.origin for security
      if (event.data.type === 'IFRAME_READY') {
        setIframeReady(true);
      } else if (event.data.type === 'NAVIGATE_TO_DOCUMENT') {
        onNavigateToDocument?.(event.data.path, event.data.anchor);
      } else if (event.data.type === 'SET_AST') {
        const qmd = setAst(event.data.ast);
        if (typeof event.data.editSeq === 'number' && typeof qmd === 'string') {
          lastEditRef.current = { seq: event.data.editSeq, qmd };
        }
      }
    };

    window.addEventListener('message', handleMessage);
    return () => window.removeEventListener('message', handleMessage);
  }, [onNavigateToDocument, setAst]);

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

    const lastEdit = lastEditRef.current;
    const editAck = lastEdit !== null && renderedContent === lastEdit.qmd ? lastEdit.seq : null;
    iframeRef.current.contentWindow.postMessage(
      {
        type: 'UPDATE_AST',
        payload: {
          astJson,
          currentFilePath,
          user,
          editAck,
        },
      },
      '*'
    );
  }, [iframeReady, astJson, currentFilePath, user, renderedContent]);

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
