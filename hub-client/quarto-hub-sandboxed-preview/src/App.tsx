import { useEffect, useState } from 'react';
import katex from 'katex';
import 'katex/dist/katex.min.css';

interface UpdateAstPayload {
  astJson: string;
  currentFilePath: string;
}

const requestVFS = (path: string): Promise<any> => {
  const ret = new Promise((resolve, reject) => {
    const handleMessage = (event: MessageEvent) => {
      if (event.data.type === 'url_response') {
        window.removeEventListener('message', handleMessage);

        if (event.data.success === true) resolve(event.data.content)
        else reject(event.data.error)
      }
    };
    window.addEventListener('message', handleMessage);
  })
  window.parent.postMessage({ type: 'url', path }, '*');
  return ret
}

export function App() {
  const [astJson, setAstJson] = useState<string>('');
  const [dogImage, setDogImage] = useState<string>('');

  useEffect(() => {
    // Listen for messages from parent
    const handleMessage = async (event: MessageEvent) => {
      console.log('iframe message', event.data)
      if (event.data.type === 'UPDATE_AST') {
        const payload = event.data.payload as UpdateAstPayload;
        setAstJson(payload.astJson);
        const dog = await requestVFS('dog_room.png')
        console.log('yo yo', { dog })
        setDogImage(dog);
      }
    };
    window.addEventListener('message', handleMessage);

    window.parent.postMessage({ type: 'IFRAME_READY' }, '*');

    return () => window.removeEventListener('message', handleMessage);
  }, []);

  if (!astJson) {
    return <div style={{ padding: '20px' }}>Loading q2-raw renderer...</div>;
  }

  try {
    const ast = JSON.parse(astJson);
    const prettyJson = JSON.stringify(ast, null, 2);

    return (
      <div>
        {dogImage && (
          <img src={`data:image/png;base64,${dogImage}`} alt="Dog" style={{ maxWidth: '100%' }} />
        )}
        <pre
          style={{
            margin: 0,
            padding: 16,
            fontFamily: "'Courier New', monospace",
            fontSize: 12,
            whiteSpace: 'pre-wrap',
            wordWrap: 'break-word',
          }}
        >
          {prettyJson}
        </pre>
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
