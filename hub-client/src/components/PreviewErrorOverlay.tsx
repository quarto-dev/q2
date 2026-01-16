import type { Diagnostic } from '../types/diagnostic';
import { stripAnsi } from '../utils/stripAnsi';
import { usePreference } from '../hooks/usePreference';

interface PreviewErrorOverlayProps {
  error: { message: string; diagnostics?: Diagnostic[] } | null;
  visible: boolean;
}

export function PreviewErrorOverlay({ error, visible }: PreviewErrorOverlayProps) {
  // Collapsed state persisted in localStorage (defaults to collapsed)
  const [collapsed, setCollapsed] = usePreference('errorOverlayCollapsed');

  if (!visible || !error) return null;

  const cleanMessage = stripAnsi(error.message);

  if (collapsed) {
    // Collapsed state: minimal indicator
    return (
      <div className="preview-error-overlay preview-error-overlay--collapsed">
        <button
          className="preview-error-expand-btn"
          onClick={() => setCollapsed(false)}
          title="Show error details"
        >
          <span className="preview-error-icon">&#9888;</span> Error
        </button>
      </div>
    );
  }

  // Expanded state: full error toast
  return (
    <div className="preview-error-overlay preview-error-overlay--expanded">
      <div className="preview-error-header">
        <span className="preview-error-title">
          <span className="preview-error-icon">&#9888;</span> Render Error
        </span>
        <button
          className="preview-error-collapse-btn"
          onClick={() => setCollapsed(true)}
          title="Collapse"
        >
          &minus;
        </button>
      </div>
      <div className="preview-error-content">
        <pre className="preview-error-message">{cleanMessage}</pre>
        {error.diagnostics && error.diagnostics.length > 0 && (
          <ul className="preview-error-diagnostics">
            {error.diagnostics.map((d, i) => (
              <li key={i}>
                {d.start_line != null && <span className="diagnostic-line">Line {d.start_line}: </span>}
                <span className="diagnostic-title">{d.title}</span>
                {d.problem && <span className="diagnostic-problem"> - {d.problem}</span>}
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
