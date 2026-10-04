/**
 * ImportDialog — place and run one document import (document import P5; I11, I22).
 *
 * One dialog per importable file, in the place queue (Editor). It proposes a folder and a name
 * (`<stem>.qmd`, advanced to the first free `<stem> 2`, … when the qmd or its `<stem>_media/`
 * folder is taken), checks the source against Rust's size and type rules before reading it, runs
 * the import with progress and Cancel, then hands the result to `commit` to be stored. The
 * dialog stays open through the whole run, so a mixed drop never shows two dialogs at once; it is
 * dequeued only on cancel or from the report's Close.
 *
 * Phases: `form` → `importing` (cancellable) → `writing` (locked: the writes take a fraction of a
 * second, and stopping them halfway would only trigger cleanup) → `report`, or closed when the
 * import succeeded with nothing to report.
 */

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import ModalDialog from './ModalDialog';
import FolderPicker from './FolderPicker';
import { DiagnosticList } from './DownloadAsControl';
import { validateProjectPath } from './fileUpload';
import { download, importDoc } from '../strings';
import { getImportService, type ImportDiagnostic, type ImportOutcome, type ImportProgress } from '../pandoc/importService';
import type { LoadProgress } from '../pandoc/pandocLoader';
import type { UiState } from '../pandoc/pandocRunner';
import { loadText } from '../pandoc/downloadText';
import type { CommitResult } from '../services/importStorage';
import { joinPath } from '../utils/uniquePath';
import { isTaken, mediaDirFor, proposeImportName } from '../utils/importNames';
import { groupDiagnostics } from '../utils/importReport';
import './NewFileDialog.css';
import './DownloadAsControl.css';
import './ImportDialog.css';

export interface ImportRequest {
  kind: 'import';
  file: File;
  /** Folder to start with. */
  folder: string;
}

export interface ImportDialogProps {
  request: ImportRequest;
  /** Every folder in the project (explicit and file-derived). */
  folders: string[];
  existingPaths: string[];
  /** Dequeue this request: the import was cancelled, or the report was closed. */
  onClose: () => void;
  /** Store a successful import's files and open its qmd; resolves with what happened. */
  commit: (outcome: Extract<ImportOutcome, { ok: true }>, qmdPath: string) => Promise<CommitResult>;
}

/** The form's state is per request, and two queued requests can share a file name: key by the File itself. */
const requestIds = new WeakMap<File, number>();
let nextRequestId = 0;
const requestKey = (file: File) => {
  let id = requestIds.get(file);
  if (id === undefined) requestIds.set(file, (id = nextRequestId++));
  return `import:${id}`;
};

export default function ImportDialog(props: ImportDialogProps) {
  // Mount the form fresh per request so its proposal is computed against the index at the moment
  // the request reaches the head of the queue.
  return <ImportForm key={requestKey(props.request.file)} {...props} />;
}

type Phase =
  | { kind: 'form' }
  | { kind: 'importing'; stage: ImportProgress; load?: LoadProgress }
  | { kind: 'writing' }
  | { kind: 'report'; ok: boolean; diagnostics: ImportDiagnostic[]; uiState?: UiState };

function ImportForm({ request, folders, existingPaths, onClose, commit }: ImportDialogProps) {
  const { file } = request;
  const [folder, setFolder] = useState(request.folder);
  const [name, setName] = useState(() =>
    proposeImportName(file.name, request.folder, { paths: new Set(existingPaths), folders: new Set(folders) }),
  );
  const [phase, setPhase] = useState<Phase>({ kind: 'form' });
  /** Rust's refusal of the source (Q-24-1 / Q-24-2); null until the check has run. */
  const [validation, setValidation] = useState<ImportDiagnostic[] | null>(null);

  const abortRef = useRef<AbortController | null>(null);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  // Refuse by name and size before anything is read. The picker's `accept` filter is only advisory.
  useEffect(() => {
    let live = true;
    getImportService()
      .validateImportSource(file)
      .then((d) => live && setValidation(d))
      .catch((err: unknown) => {
        // The import itself will report the same failure; do not block the form on it.
        console.error('[import] could not validate the source:', err);
        if (live) setValidation([]);
      });
    return () => {
      live = false;
    };
  }, [file]);

  const occupied = useMemo(() => ({ paths: new Set(existingPaths), folders: new Set(folders) }), [existingPaths, folders]);
  const trimmedName = name.trim();
  const newPath = trimmedName ? joinPath(folder, trimmedName) : '';
  const mediaDir = trimmedName ? mediaDirFor(folder, trimmedName) : '';

  const nameError = useMemo(() => {
    if (!trimmedName) return importDoc.errorEmptyName;
    if (!/\.qmd$/i.test(trimmedName)) return importDoc.errorExtension;
    const pathError = validateProjectPath(newPath);
    if (pathError) return pathError;
    if (isTaken(newPath, occupied)) return importDoc.errorExists;
    if (isTaken(mediaDir, occupied)) return importDoc.errorMediaExists(mediaDir);
    return null;
  }, [trimmedName, newPath, mediaDir, occupied]);

  const refused = (validation?.length ?? 0) > 0;
  const canImport = phase.kind === 'form' && validation !== null && !refused && nameError === null;

  const startImport = useCallback(async () => {
    if (!canImport) return;
    const controller = new AbortController();
    abortRef.current = controller;
    setPhase({ kind: 'importing', stage: 'reading' });
    const set = (p: Phase) => {
      if (alive.current) setPhase(p);
    };
    const finish = (p: Phase) => {
      abortRef.current = null;
      set(p);
    };

    const outcome = await getImportService().importDocument(file, newPath, {
      signal: controller.signal,
      onProgress: (stage) => set({ kind: 'importing', stage }),
      onLoadProgress: (load) => set({ kind: 'importing', stage: 'loading-pandoc', load }),
    });
    // Cancelled, or cancelled just as the run finished: nothing is written and no report is shown.
    if ((!outcome.ok && outcome.cancelled) || controller.signal.aborted) {
      abortRef.current = null;
      if (alive.current) onClose();
      return;
    }
    if (!outcome.ok) {
      finish({ kind: 'report', ok: false, diagnostics: outcome.diagnostics, uiState: outcome.uiState });
      return;
    }

    // Past this point the writes run to completion: Cancel and Escape are disabled.
    abortRef.current = null;
    set({ kind: 'writing' });
    let result: CommitResult;
    try {
      result = await commit(outcome, newPath);
    } catch (err) {
      console.error('[import] storing the import failed:', err);
      result = {
        ok: false,
        diagnostics: [
          { origin: 'host', kind: 'error', code: 'import-write-failed', message: err instanceof Error ? err.message : String(err), path: newPath },
          ...outcome.diagnostics,
        ],
      };
    }
    if (!alive.current) return;
    if (result.ok && result.diagnostics.length === 0) {
      onClose();
      return;
    }
    set({ kind: 'report', ok: result.ok, diagnostics: result.diagnostics });
  }, [canImport, file, newPath, commit, onClose]);

  const cancel = useCallback(() => abortRef.current?.abort(), []);

  // Enter submits the form; Escape and Tab containment are owned by ModalDialog.
  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key !== 'Enter' || phase.kind !== 'form') return;
      if (e.target instanceof HTMLButtonElement) return;
      e.preventDefault();
      void startImport();
    },
    [phase.kind, startImport],
  );

  // ModalDialog sends Escape, the close button and a backdrop click here. While the writes run it does nothing.
  const handleClose = useCallback(() => {
    if (phase.kind === 'importing') cancel();
    else if (phase.kind !== 'writing') onClose();
  }, [phase.kind, cancel, onClose]);

  return (
    <ModalDialog title={importDoc.dialogTitle(file.name)} className="new-file-dialog import-dialog" onClose={handleClose} onKeyDown={handleKeyDown}>
      <div className="import-visually-hidden" role="status" aria-live="polite" aria-label={importDoc.progressRegionLabel}>
        {liveText(phase)}
      </div>

      {phase.kind === 'form' && (
        <>
          <div className="dialog-content">
            <div className="text-file-form">
              <div className="folder-input">
                <label htmlFor="import-folder">{importDoc.folderLabel}</label>
                <FolderPicker id="import-folder" folders={folders} value={folder} onChange={setFolder} />
              </div>
              <div className="filename-input">
                <label htmlFor="import-name">{importDoc.nameLabel}</label>
                <input
                  id="import-name"
                  type="text"
                  className="qh-input focus-accent"
                  value={name}
                  autoFocus
                  aria-invalid={nameError !== null || undefined}
                  aria-describedby="import-media-line import-errors"
                  onChange={(e) => setName(e.target.value)}
                />
              </div>
              <p id="import-media-line" className="import-media-line">
                {importDoc.mediaLine(mediaDir)}
              </p>
              <div id="import-errors">
                {refused && <DiagnosticList diagnostics={validation ?? []} />}
                {!refused && nameError && <div className="qh-error inline">{nameError}</div>}
              </div>
            </div>
          </div>
          <div className="dialog-actions">
            <button className="qh-btn outline" onClick={onClose}>
              {importDoc.cancel}
            </button>
            <button className="qh-btn primary" onClick={() => void startImport()} disabled={!canImport}>
              {importDoc.import}
            </button>
          </div>
        </>
      )}

      {phase.kind === 'importing' && (
        <>
          <div className="dialog-content">
            <div className="import-progress" data-testid="import-progress">
              <div>{progressText(phase)}</div>
              <progress
                aria-label={progressText(phase)}
                max={phase.load?.phase === 'download' && phase.load.total ? phase.load.total : undefined}
                value={phase.load?.phase === 'download' && phase.load.total ? phase.load.loaded : undefined}
              />
            </div>
          </div>
          <div className="dialog-actions">
            <button className="qh-btn outline" onClick={cancel}>
              {importDoc.cancel}
            </button>
          </div>
        </>
      )}

      {phase.kind === 'writing' && (
        <div className="dialog-content">
          <div className="import-progress" data-testid="import-progress">
            <div>{importDoc.writing}</div>
            <progress aria-label={importDoc.writing} />
          </div>
        </div>
      )}

      {phase.kind === 'report' && (
        <>
          <div className="dialog-content">
            <ImportReport ok={phase.ok} diagnostics={phase.diagnostics} uiState={phase.uiState} />
          </div>
          <div className="dialog-actions">
            <button className="qh-btn primary" onClick={onClose} autoFocus>
              {importDoc.close}
            </button>
          </div>
        </>
      )}
    </ModalDialog>
  );
}

/** Text for the visible panel: the first-use download shows its byte counts. */
function progressText(phase: Extract<Phase, { kind: 'importing' }>): string {
  if (phase.stage === 'loading-pandoc' && phase.load) return loadText(phase.load);
  return importDoc.progress[phase.stage];
}

/** Short text for the live region: no byte counts, so it is announced only on real changes. */
function liveText(phase: Phase): string {
  switch (phase.kind) {
    case 'importing':
      return phase.stage === 'loading-pandoc' && phase.load ? loadText(phase.load, false) : importDoc.progress[phase.stage];
    case 'writing':
      return importDoc.writing;
    case 'report':
      return phase.ok ? (phase.diagnostics.length ? importDoc.reportSucceededWithNotes : importDoc.reportSucceeded) : importDoc.reportFailed;
    default:
      return '';
  }
}

/** The import report: diagnostics grouped by kind, the failure (if any) first. */
export function ImportReport({ ok, diagnostics, uiState }: { ok: boolean; diagnostics: ImportDiagnostic[]; uiState?: UiState }) {
  const groups = groupDiagnostics(diagnostics);
  // A loader or worker failure (offline, blocked worker, …) reads the same as in Download as.
  const uiText = uiState ? download.failed[uiState] : '';
  return (
    <div className="import-report" data-testid="import-report">
      <p className="import-report-headline">{ok ? importDoc.reportSucceededWithNotes : importDoc.reportFailed}</p>
      {uiText && <p className="import-report-failure">{uiText}</p>}
      {groups.errors.length > 0 && (
        <section className="import-report-group">
          <h3>{importDoc.groupErrors(groups.errors.length)}</h3>
          <DiagnosticList diagnostics={groups.errors} />
        </section>
      )}
      {groups.warnings.length > 0 && (
        <section className="import-report-group">
          <h3>{importDoc.groupWarnings(groups.warnings.length)}</h3>
          <DiagnosticList diagnostics={groups.warnings} />
        </section>
      )}
      {groups.info.length > 0 && (
        <section className="import-report-group">
          <h3>{importDoc.groupInfo(groups.info.length)}</h3>
          <DiagnosticList diagnostics={groups.info} />
        </section>
      )}
    </div>
  );
}
