/**
 * "Download as" control (pandoc-host H5): a top-bar button with a menu of formats, a
 * progress/status panel, and a polite live region.
 *
 * Presentational: the status comes from the `DownloadController` (see `useDownloadAs`), so
 * the dev harness can render every state. Accessibility (D8): the menu is the shared
 * `Menu` (APG menu-button pattern); a disabled button is `aria-disabled` and described by
 * text (not a tooltip only); progress and errors reach a live region.
 */
import { useId, useRef, useState } from 'react';
import { Menu, MenuItem, MenuLabel } from './Menu';
import { DownloadIcon } from './icons';
import Tooltip from './Tooltip';
import { download } from '../strings';
import './DownloadAsControl.css';
import type { Diagnostic, DownloadFormat, DownloadStatus } from '../pandoc/downloadController';
import { liveText, workingText } from '../pandoc/downloadText';

export interface DownloadAsControlProps {
  formats: DownloadFormat[];
  status: DownloadStatus;
  /** When set, the button is disabled and this text explains why. */
  disabledReason?: string;
  onSelect: (format: DownloadFormat) => void;
  onCancel: () => void;
  onDismiss: () => void;
}

const diagText = (d: Diagnostic): { code?: string; title: string; detail?: string } => {
  if (d.origin === 'host') return { code: d.code, title: d.message };
  // typst's own diagnostics: the message, and where in the generated .typ it was raised.
  if (d.origin === 'typst') return { title: d.message, detail: d.range ? `${d.path} ${d.range}` : d.path };
  return { code: d.code, title: d.title, detail: d.problem };
};

function DiagnosticList({ diagnostics }: { diagnostics: Diagnostic[] }) {
  if (diagnostics.length === 0) return null;
  return (
    <ul className="download-diagnostics">
      {diagnostics.map((d, i) => {
        const t = diagText(d);
        return (
          <li key={i} className={`download-diagnostic download-diagnostic-${d.kind}`}>
            {t.code && <span className="download-diagnostic-code">[{t.code}] </span>}
            {t.title}
            {t.detail ? `: ${t.detail}` : ''}
          </li>
        );
      })}
    </ul>
  );
}

export default function DownloadAsControl({ formats, status, disabledReason, onSelect, onCancel, onDismiss }: DownloadAsControlProps) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const descId = useId();
  const panelId = useId();
  const disabled = disabledReason !== undefined || formats.length === 0;
  const busy = status.phase === 'working';

  const button = (
    <button
      ref={triggerRef}
      type="button"
      className={`qh-icon-btn boxed download-btn${disabled ? ' is-disabled' : ''}`}
      aria-label={download.buttonLabel}
      aria-haspopup={disabled ? undefined : 'menu'}
      aria-expanded={disabled ? undefined : open}
      aria-disabled={disabled || undefined}
      aria-describedby={disabled && disabledReason ? descId : status.phase !== 'idle' ? panelId : undefined}
      onClick={() => {
        if (!disabled) setOpen((v) => !v);
      }}
    >
      {busy ? <span aria-hidden="true">…</span> : <DownloadIcon />}
    </button>
  );

  return (
    <div className="download-btn-box qh-menu-anchor">
      {disabled ? button : <Tooltip content={download.buttonTooltip}>{button}</Tooltip>}
      {disabled && disabledReason && (
        <span id={descId} className="download-visually-hidden">
          {disabledReason}
        </span>
      )}
      {open && !disabled && (
        <Menu onClose={() => setOpen(false)} triggerRef={triggerRef} aria-label={download.menuLabel} className="qh-menu-right">
          <MenuLabel>{download.menuLabel.toUpperCase()}</MenuLabel>
          {formats.map((f) => (
            <MenuItem key={f.key} onSelect={() => onSelect(f)}>
              {f.label}
            </MenuItem>
          ))}
          <MenuLabel>{formats.some((f) => f.key === 'pdf') ? download.sizeHintPdf : download.sizeHint}</MenuLabel>
        </Menu>
      )}
      <div className="download-visually-hidden" role="status" aria-live="polite" aria-label={download.statusRegionLabel}>
        {liveText(status)}
      </div>
      {status.phase !== 'idle' && (
        <div id={panelId} className={`download-status download-status-${status.phase}`} data-testid="download-status">
          {status.phase === 'working' && (
            <>
              <div className="download-status-text">{workingText(status)}</div>
              <progress
                className="download-progress"
                aria-label={workingText(status)}
                max={status.load?.phase === 'download' && status.load.total ? status.load.total : undefined}
                value={status.load?.phase === 'download' && status.load.total ? status.load.loaded : undefined}
              />
              <div className="download-status-actions">
                <button type="button" className="qh-btn small outline" onClick={onCancel}>
                  {download.cancel}
                </button>
              </div>
            </>
          )}
          {status.phase === 'done' && (
            <>
              <div className="download-status-text">{download.done(status.fileName)}</div>
              {status.unexecutedCells > 0 && <div className="download-note">{download.unexecutedCells(status.unexecutedCells)}</div>}
              {status.format.key === 'typst' && <div className="download-note">{download.typstDangling}</div>}
              {status.notices.map((n, i) => (
                <div key={i} className="download-note">
                  {n}
                </div>
              ))}
              {status.warnings.length > 0 && (
                <details className="download-warnings">
                  <summary>{download.warnings(status.warnings.length)}</summary>
                  <DiagnosticList diagnostics={status.warnings} />
                </details>
              )}
              <div className="download-status-actions">
                <button type="button" className="qh-btn small outline" onClick={onDismiss}>
                  {download.dismiss}
                </button>
              </div>
            </>
          )}
          {status.phase === 'failed' && (
            <>
              <div className="download-status-text download-status-error">{download.failed[status.state] || status.message}</div>
              {status.message && download.failed[status.state] && <div className="download-note">{status.message}</div>}
              <DiagnosticList diagnostics={status.diagnostics} />
              {status.notices.map((n, i) => (
                <div key={i} className="download-note">
                  {n}
                </div>
              ))}
              <div className="download-status-actions">
                <button type="button" className="qh-btn small outline" onClick={onDismiss}>
                  {download.dismiss}
                </button>
              </div>
            </>
          )}
          {status.phase === 'cancelled' && (
            <>
              <div className="download-status-text">{download.cancelled}</div>
              <div className="download-status-actions">
                <button type="button" className="qh-btn small outline" onClick={onDismiss}>
                  {download.dismiss}
                </button>
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
