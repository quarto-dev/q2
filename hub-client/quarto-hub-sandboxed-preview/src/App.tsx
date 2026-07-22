import { useEffect, useState } from 'react';
import katex from 'katex';
// @ts-ignore
import 'katex/dist/katex.min.css';
import { init } from './registerServiceWorker';
import { AstRenderer } from './basicRenderer';

interface UpdateAstPayload {
  astJson: string;
  currentFilePath: string;
}

export function App() {
  const [astJson, setAstJson] = useState<string>('');
  // const [dogImage, setDogImage] = useState<string>('');

  useEffect(() => {
    init().then(() => {
      window.parent.postMessage({ type: 'IFRAME_READY' }, '*');
    })
  }, [])

  useEffect(() => {
    // Listen for messages from parent
    const handleMessage = async (event: MessageEvent) => {
      console.log('iframe message', event.data)
      if (event.data.type === 'UPDATE_AST') {
        const payload = event.data.payload as UpdateAstPayload;
        setAstJson(payload.astJson);
      }
    };
    window.addEventListener('message', handleMessage);

    return () => window.removeEventListener('message', handleMessage);
  }, []);

  if (!astJson) {
    return <div style={{ padding: '20px' }}>Loading q2-sandboxed-preview renderer...</div>;
  }

  try {
    const ast = JSON.parse(astJson);

    return (
      <div style={{ padding: '20px' }}>
        <AstRenderer node={ast} />
      </div>
    );
  } catch (err) {
    return (
      <div style={{ padding: 20, color: 'red' }}>
        <strong>Parse Error:</strong>
        <pre>{err instanceof Error ? err.message : String(err)}</pre>
      </div>
    );
  }
}

// Example showing katex is available
console.log('KaTeX version:', katex.version);
