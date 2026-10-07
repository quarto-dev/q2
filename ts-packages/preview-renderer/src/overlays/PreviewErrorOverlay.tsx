import { useEffect, useState } from 'react';
import type { Diagnostic, Pass1Failure } from '../types/diagnostic';
import { stripAnsi } from '../utils/stripAnsi';

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
  /**
   * Whether the overlay is rendered in its collapsed (minimal-indicator)
   * state. When omitted, the overlay falls back to internal state. The
   * controlled form lets the hosting surface persist collapsedness in
   * its own preference store: hub-client wraps with `usePreference`
   * (localStorage); the q2-preview SPA can wire it to session state or
   * leave it uncontrolled.
   */
  collapsed?: boolean;
  /** Toggle the collapsed state. Required only when `collapsed` is supplied. */
  onToggleCollapsed?: (next: boolean) => void;
}

type OverlayError = NonNullable<PreviewErrorOverlayProps['error']>;

const diagnosticLine = (d: Diagnostic): string =>
  `${d.start_line != null ? `Line ${d.start_line}: ` : ''}${d.title}${d.problem ? ` - ${d.problem}` : ''}`;

/** The overlay's content as plain text (message, diagnostics, sibling failures), without its title. */
function errorCopyText(error: OverlayError): string {
  const parts = [stripAnsi(error.message)];
  if (error.diagnostics?.length) parts.push(error.diagnostics.map(diagnosticLine).join('\n'));
  for (const f of error.pass1Failures ?? []) {
    const body = f.diagnostics.length > 0 ? f.diagnostics.map(diagnosticLine).join('\n') : stripAnsi(f.error);
    parts.push(`${f.source_file} failed to parse\n${body}`);
  }
  return parts.join('\n\n');
}

/** Small copy-to-clipboard icon button; the icon turns to a check for a moment after copying. */
function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(t);
  }, [copied]);
  const label = copied ? 'Copied' : 'Copy error details';
  return (
    <button
      type="button"
      className="preview-error-copy-btn"
      aria-label={label}
      title={label}
      onClick={() => {
        navigator.clipboard.writeText(text).then(
          () => setCopied(true),
          (err) => console.error('Clipboard write failed:', err),
        );
      }}
    >
      <svg
        width="14"
        height="14"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        {copied ? (
          <polyline points="20 6 9 17 4 12" />
        ) : (
          <>
            <rect x="9" y="9" width="13" height="13" rx="2" ry="2" />
            <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
          </>
        )}
      </svg>
    </button>
  );
}

export function PreviewErrorOverlay({
  error,
  visible,
  collapsed: controlledCollapsed,
  onToggleCollapsed,
}: PreviewErrorOverlayProps) {
  const [uncontrolledCollapsed, setUncontrolledCollapsed] = useState(true);
  const collapsed = controlledCollapsed ?? uncontrolledCollapsed;
  const setCollapsed = (next: boolean) => {
    if (onToggleCollapsed) onToggleCollapsed(next);
    if (controlledCollapsed === undefined) setUncontrolledCollapsed(next);
  };

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
        <CopyButton text={errorCopyText(error)} />
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
