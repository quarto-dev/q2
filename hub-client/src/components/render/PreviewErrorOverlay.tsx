import type { Diagnostic } from '@quarto/preview-renderer/types/diagnostic';
import type { Pass1Failure } from '@quarto/preview-runtime';
import { stripAnsi } from '@quarto/preview-renderer/utils/stripAnsi';
import { usePreference } from '../../hooks/usePreference';

interface PreviewErrorOverlayProps {
  error: {
    message: string;
    diagnostics?: Diagnostic[];
    /**
     * Sibling-page Pass-1 failures (bd-rqba). Rendered as a
     * separate section under the main error so each failing
     * file's parse diagnostic is attributed to its source. May
     * be present even when `diagnostics` is empty (the active
     * page rendered fine but a sibling didn't).
     */
    pass1Failures?: Pass1Failure[];
  } | null;
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
        {error.pass1Failures && error.pass1Failures.length > 0 && (
          <div className="preview-error-pass1-failures">
            {error.pass1Failures.map((f, i) => (
              <div className="preview-error-pass1-failure" key={i}>
                <div className="diagnostic-source-file">
                  <span className="diagnostic-icon">&#9888;</span>{' '}
                  <code>{f.source_file}</code> failed to parse
                </div>
                {f.diagnostics.length > 0 ? (
                  <ul className="preview-error-diagnostics">
                    {f.diagnostics.map((d, j) => (
                      <li key={j}>
                        {d.start_line != null && (
                          <span className="diagnostic-line">Line {d.start_line}: </span>
                        )}
                        <span className="diagnostic-title">{d.title}</span>
                        {d.problem && (
                          <span className="diagnostic-problem"> - {d.problem}</span>
                        )}
                      </li>
                    ))}
                  </ul>
                ) : (
                  <pre className="preview-error-message">
                    {stripAnsi(f.error)}
                  </pre>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
