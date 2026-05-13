import type { Diagnostic } from '@quarto/preview-renderer/types/diagnostic';
import { stripAnsi } from '@quarto/preview-renderer/utils/stripAnsi';

// Fallback component for when WASM isn't ready yet
export function FallbackView({ content, message }: { content: string; message: string }) {
  return (
    <div style={{ padding: '16px' }}>
      <div style={{ marginBottom: '16px', padding: '8px', background: '#e3f2fd' }}>
        {message}
      </div>
      <pre><code>{content}</code></pre>
    </div>
  );
}

// Error display component
export function ErrorView({ content, error, diagnostics }: { content: string; error: string; diagnostics?: Diagnostic[] }) {
  const cleanError = stripAnsi(error);

  return (
    <div style={{ padding: '16px' }}>
      <div style={{ marginBottom: '16px', padding: '8px', background: '#ffebee', color: '#c62828' }}>
        <strong>Render Error</strong>
        <pre style={{ marginTop: '8px', whiteSpace: 'pre-wrap' }}>{cleanError}</pre>
        {diagnostics && diagnostics.length > 0 && (
          <ul>
            {diagnostics.map((d, i) => (
              <li key={i}>{stripAnsi(d.title)}</li>
            ))}
          </ul>
        )}
      </div>
      <pre><code>{content}</code></pre>
    </div>
  );
}

// Placeholder component for non-QMD files
export function NonQmdPlaceholderView({ filename }: { filename: string }) {
  const extension = filename.split('.').pop() || 'file';

  return (
    <div style={{
      height: '100%',
      display: 'flex',
      alignItems: 'center',
      justifyContent: 'center',
      background: '#f5f5f5',
      color: '#666'
    }}>
      <div style={{ textAlign: 'center', padding: '24px' }}>
        <div style={{ fontSize: '48px', marginBottom: '16px', opacity: 0.5 }}>
          &#128196;
        </div>
        <div style={{ fontSize: '14px', lineHeight: '1.6' }}>
          Preview is available for <span style={{ fontFamily: "'SF Mono', Monaco, monospace" }}>.qmd</span> files
        </div>
        <div style={{ fontSize: '14px', lineHeight: '1.6', marginTop: '8px' }}>
          This <span style={{ fontFamily: "'SF Mono', Monaco, monospace" }}>.{extension}</span> file can be edited in the editor
        </div>
      </div>
    </div>
  );
}
