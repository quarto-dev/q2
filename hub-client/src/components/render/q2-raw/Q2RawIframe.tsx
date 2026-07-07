import { useEffect, useRef, useState } from 'react';

interface Q2RawIframeProps {
  astJson: string;
}

// In production, q2-raw.html is served from a separate domain for sandboxing.
// In dev/local-prod, it uses a different port.
const Q2_RAW_URL = import.meta.env.VITE_Q2_RAW_URL || 'q2-raw.html';

/**
 * Simplest possible iframe wrapper: displays JSON.stringified AST
 * in a pre element. No interactivity, no custom components, no navigation.
 */
export function Q2RawIframe({ astJson }: Q2RawIframeProps) {
  const iframeRef = useRef<HTMLIFrameElement>(null);
  const [iframeReady, setIframeReady] = useState(false);

  // Handle messages from the iframe
  useEffect(() => {
    const handleMessage = async (event: MessageEvent) => {
      console.log('message received!!', event.data)
      if (event.data.type === 'IFRAME_READY') {
        setIframeReady(true)
      } else if (event.data.type === 'url' && event.data.path) {
        // Read from VFS and respond
        const wasm = await import('wasm-quarto-hub-client');

        // Determine if this is a binary file based on extension
        const isBinary = /\.(png|jpg|jpeg|gif|pdf|ico|webp|ttf|woff|woff2|zip|wasm)$/i.test(event.data.path);

        const resultJson = isBinary
          ? wasm.vfs_read_binary_file(event.data.path)
          : wasm.vfs_read_file(event.data.path);

        const result = JSON.parse(resultJson) as {
          success: boolean;
          content?: string;
          error?: string;
        };

        if (iframeRef.current?.contentWindow) {
          iframeRef.current.contentWindow.postMessage(
            {
              type: 'url_response',
              path: event.data.path,
              success: result.success,
              content: result.content,
              error: result.error,
              isBinary,
            },
            '*'
          );
        }
      }
    };

    window.addEventListener('message', handleMessage);
    return () => window.removeEventListener('message', handleMessage);
  }, []);

  // Send AST updates when iframe is ready
  useEffect(() => {
    if (!iframeReady || !iframeRef.current?.contentWindow) return;

    iframeRef.current.contentWindow.postMessage(
      {
        type: 'UPDATE_AST',
        payload: { astJson },
      },
      '*'
    );

  }, [iframeReady, astJson]);

  return (
    <iframe
      ref={iframeRef}
      src={Q2_RAW_URL}
      title="q2-raw Renderer"
      sandbox="allow-scripts allow-same-origin"
      style={{
        width: '99%',
        height: '100%',
        border: 'none',
        display: 'block',
      }}
    />
  );
}
