import { useEffect, useRef, useState } from 'react';

interface AstIframeProps {
  astJson: string;
  currentFilePath: string;
  onNavigateToDocument?: (path: string, anchor: string | null) => void;
  setAst: (newAst: any) => void;
  customComponentsCode?: Record<string, string>; // Component name -> transpiled JS code
}

/**
 * Wrapper component that renders the Ast component in a sandboxed iframe
 */
export function AstIframe({
  astJson,
  currentFilePath,
  onNavigateToDocument,
  setAst,
  customComponentsCode,
}: AstIframeProps) {
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

    iframeRef.current.contentWindow.postMessage(
      {
        type: 'UPDATE_AST',
        payload: {
          astJson,
          currentFilePath,
        },
      },
      '*'
    );
  }, [iframeReady, astJson, currentFilePath]);

  return (
    <iframe
      ref={iframeRef}
      src="/ast-renderer.html"
      title="AST Renderer"
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
