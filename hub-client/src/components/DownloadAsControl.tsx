/**
 * "Download as" control (pandoc-host H5): a top-bar button with a menu of formats, a
 * progress/status panel, and a polite live region.
 *
 * Presentational: the status comes from the `DownloadController` (see `useDownloadAs`), so
 * the dev harness can render every state. Accessibility (D8): the menu is the shared
 * `Menu` (APG menu-button pattern); a disabled button is `aria-disabled` and described by
 * text (not a tooltip only); progress and errors reach a live region.
 */
import { Fragment, useEffect, useId, useRef, useState } from 'react';
import { Menu, MenuItem, MenuLabel } from './Menu';
import { CheckIcon, CopyIcon, DownloadIcon } from './icons';
import Tooltip from './Tooltip';
import { download } from '../strings';
import './DownloadAsControl.css';
import type { Diagnostic, DownloadFormat, DownloadStatus } from '../pandoc/downloadController';
import { doneText, liveText, workingText } from '../pandoc/downloadText';
import { TYPST_PDF_KEY } from '../pandoc/formatKeys';
import type { ImportDiagnostic } from '../pandoc/importService';

export interface DownloadAsControlProps {
  formats: DownloadFormat[];
  status: DownloadStatus;
  /** When set, the button is disabled and this text explains why. */
  disabledReason?: string;
  /** A book chapter's entries pass the scope (`'auto'` the whole book, `'chapter'` this chapter); the others pass the format alone. */
  onSelect: (format: DownloadFormat, scope?: 'auto' | 'chapter') => void;
  /** The resolver's book field for the open document; `chapter: true` offers the book entries. */
  book?: { chapter: boolean } | null;
  /** Called when the menu opens, so the host can refresh `book` (the project's chapter list may have changed). */
  onOpen?: () => void;
  onCancel: () => void;
  onDismiss: () => void;
}

/** What the list renders: Download-as's diagnostics, and an import report's (which adds TS-side host codes). */
export type ListedDiagnostic = Diagnostic | ImportDiagnostic;

const diagText = (d: ListedDiagnostic): { code?: string; title: string; detail?: string } => {
  if (d.origin === 'host') return { code: d.code, title: d.message };
  // typst's own diagnostics: the message, and where in the generated .typ it was raised.
  if (d.origin === 'typst') return { title: d.message, detail: d.range ? `${d.path} ${d.range}` : d.path };
  return { code: d.code, title: d.title, detail: d.problem };
};

/** One diagnostic as plain text, the way the list shows it. */
const diagLine = (d: ListedDiagnostic): string => {
  const t = diagText(d);
  return `${t.code ? `[${t.code}] ` : ''}${t.title}${t.detail ? `: ${t.detail}` : ''}`;
};

/** The warnings as plain text, one per line (not the panel's headline). */
export function warningsCopyText(warnings: ListedDiagnostic[]): string {
  return warnings.map(diagLine).join('\n');
}

/** Small icon button, top-right of the panel, that copies `text`; the icon turns to a check for a moment. */
function CopyButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(t);
  }, [copied]);
  return (
    <button
      type="button"
      className="qh-icon-btn download-copy-btn"
      aria-label={copied ? download.copiedStatus : download.copyStatus}
      title={copied ? download.copiedStatus : download.copyStatus}
      onClick={() => {
        navigator.clipboard.writeText(text).then(
          () => setCopied(true),
          (err) => console.error('Clipboard write failed:', err),
        );
      }}
    >
      {copied ? <CheckIcon size={14} /> : <CopyIcon size={14} />}
    </button>
  );
}

export function DiagnosticList({ diagnostics }: { diagnostics: ListedDiagnostic[] }) {
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

/** Formats that are a whole book when the document is a chapter; docx and pptx always download the active chapter. */
const BOOK_FORMATS: readonly string[] = ['typst', TYPST_PDF_KEY, 'epub'];

export default function DownloadAsControl({ formats, status, disabledReason, onSelect, book, onOpen, onCancel, onDismiss }: DownloadAsControlProps) {
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
        if (disabled) return;
        if (!open) onOpen?.();
        setOpen((v) => !v);
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
          {formats.map((f) =>
            book?.chapter && BOOK_FORMATS.includes(f.key) ? (
              <Fragment key={f.key}>
                <MenuItem onSelect={() => onSelect(f, 'auto')}>{download.downloadBookAs(f.label)}</MenuItem>
                <MenuItem subtext={f.label} onSelect={() => onSelect(f, 'chapter')}>
                  {download.thisChapterOnly}
                </MenuItem>
              </Fragment>
            ) : (
              <MenuItem key={f.key} onSelect={() => onSelect(f)}>
                {f.label}
              </MenuItem>
            ),
          )}
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
              <div className="download-status-text">{doneText(status)}</div>
              {status.book && <div className="download-note">{download.done(status.fileName)}</div>}
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
                  <div className="download-warnings-body">
                    <CopyButton text={warningsCopyText(status.warnings)} />
                    <DiagnosticList diagnostics={status.warnings} />
                  </div>
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
