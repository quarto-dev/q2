/**
 * @vitest-environment jsdom
 *
 * ImportDialog (document import P5 T3): the proposal, the validation and the name errors, then the
 * run: progress, Cancel, the write lock and the report view. Uses P4's stub service.
 */
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup, waitFor, within } from '@testing-library/react';
import ImportDialog, { type ImportDialogProps } from './ImportDialog';
import { createStubImportService, setImportServiceForTests, type ImportOutcome, type ImportService } from '../pandoc/importService';
import type { CommitResult } from '../services/importStorage';
import { download, importDoc } from '../strings';

afterEach(() => {
  cleanup();
  setImportServiceForTests(undefined);
});

const file = (name: string, size = 10) => {
  const f = new File(['x'], name);
  Object.defineProperty(f, 'size', { value: size });
  return f;
};

const okCommit = (outcome: Extract<ImportOutcome, { ok: true }>, qmdPath: string): Promise<CommitResult> =>
  Promise.resolve({ ok: true, qmdPath, qmdDocId: 'd', qmd: outcome.qmd, diagnostics: outcome.diagnostics });

function setup(
  props: Partial<Omit<ImportDialogProps, 'request'>> & { file?: File; folder?: string; service?: ImportService } = {},
) {
  setImportServiceForTests(props.service ?? createStubImportService());
  const onClose = vi.fn();
  const commit = vi.fn(props.commit ?? okCommit);
  const view = render(
    <ImportDialog
      request={{ kind: 'import', file: props.file ?? file('report.docx'), folder: props.folder ?? '' }}
      folders={props.folders ?? []}
      existingPaths={props.existingPaths ?? []}
      onClose={props.onClose ?? onClose}
      commit={commit}
    />,
  );
  return { onClose, commit, ...view };
}

const nameInput = () => screen.getByLabelText(importDoc.nameLabel) as HTMLInputElement;
const importButton = () => screen.getByRole('button', { name: importDoc.import }) as HTMLButtonElement;
/** The report's Close button (the header's × has the same accessible name). */
const closeButton = () => screen.getAllByRole('button', { name: importDoc.close }).find((b) => b.classList.contains('primary'))!;
/** Validation resolves asynchronously; the Import button is the signal that the form is ready. */
const ready = () => waitFor(() => expect(importButton().disabled).toBe(false));

/** A service whose importDocument the test controls. */
function controlledService(base = createStubImportService()) {
  let resolve!: (o: ImportOutcome) => void;
  let seen: { signal?: AbortSignal; onProgress?: (p: never) => void; onLoadProgress?: (p: never) => void } = {};
  const pending = new Promise<ImportOutcome>((r) => (resolve = r));
  const service: ImportService = {
    ...base,
    importDocument: (_f, _p, opts) => {
      seen = opts ?? {};
      return pending;
    },
  };
  return { service, resolve, opts: () => seen };
}

const okOutcome = (diagnostics: Extract<ImportOutcome, { ok: true }>['diagnostics'] = []): Extract<ImportOutcome, { ok: true }> => ({
  ok: true,
  qmd: '# Hi\n',
  media: [],
  diagnostics,
});

describe('ImportDialog: the form', () => {
  it('proposes <stem>.qmd and shows where the images will go', async () => {
    setup({ folder: 'docs' });
    expect(nameInput().value).toBe('report.qmd');
    expect(screen.getByText(importDoc.mediaLine('docs/report_media'))).toBeTruthy();
    expect(screen.getByRole('dialog', { name: importDoc.dialogTitle('report.docx') })).toBeTruthy();
    await ready();
  });

  it('sanitizes the stem (spaces become hyphens, interior dots too)', async () => {
    setup({ file: file('My Report.v2.docx') });
    expect(nameInput().value).toBe('My-Report-v2.qmd');
    await ready();
  });

  it('advances past an existing qmd, for the qmd and the media folder together', async () => {
    setup({ folder: 'docs', existingPaths: ['docs/report.qmd'], folders: ['docs'] });
    expect(nameInput().value).toBe('report 2.qmd');
    expect(screen.getByText(importDoc.mediaLine('docs/report 2_media'))).toBeTruthy();
    await ready();
  });

  it('advances when only the media folder exists', async () => {
    setup({ existingPaths: ['report_media/a.png'], folders: ['report_media'] });
    expect(nameInput().value).toBe('report 2.qmd');
    await ready();
  });

  it('keeps Import disabled until the source has been validated, then enables it', async () => {
    setup();
    await ready();
  });

  it('a typed name that collides with a file shows an error and disables Import', async () => {
    setup({ existingPaths: ['taken.qmd'] });
    await ready();
    fireEvent.change(nameInput(), { target: { value: 'taken.qmd' } });
    expect(screen.getByText(importDoc.errorExists)).toBeTruthy();
    expect(importButton().disabled).toBe(true);
    fireEvent.change(nameInput(), { target: { value: 'free.qmd' } });
    expect(screen.queryByText(importDoc.errorExists)).toBeNull();
    expect(importButton().disabled).toBe(false);
  });

  it('a typed name whose media folder exists is an error', async () => {
    setup({ existingPaths: ['pics_media/a.png'], folders: ['pics_media'] });
    await ready();
    fireEvent.change(nameInput(), { target: { value: 'pics.qmd' } });
    expect(screen.getByText(importDoc.errorMediaExists('pics_media'))).toBeTruthy();
    expect(importButton().disabled).toBe(true);
  });

  it('accepts a typed name with spaces', async () => {
    setup();
    await ready();
    fireEvent.change(nameInput(), { target: { value: 'my report.qmd' } });
    expect(screen.getByText(importDoc.mediaLine('my report_media'))).toBeTruthy();
    expect(importButton().disabled).toBe(false);
  });

  it.each([
    ['', importDoc.errorEmptyName],
    ['notes', importDoc.errorExtension],
    ['notes.md', importDoc.errorExtension],
    ['a:b.qmd', 'Path contains invalid characters'],
    ['../x.qmd', 'Path must not contain "." or ".." segments'],
  ])('rejects the name %j', async (typed, message) => {
    setup();
    await ready();
    fireEvent.change(nameInput(), { target: { value: typed } });
    expect(screen.getByText(message)).toBeTruthy();
    expect(importButton().disabled).toBe(true);
  });

  it('shows Rust\'s text for an unsupported type and disables Import', async () => {
    setup({ file: file('picture.png') });
    await waitFor(() => expect(screen.getByText(/Unsupported file type/)).toBeTruthy());
    expect(screen.getByText(/\[Q-24-1\]/)).toBeTruthy();
    expect(importButton().disabled).toBe(true);
  });

  it('shows Rust\'s text for a file over the cap and disables Import', async () => {
    setup({ file: file('huge.docx', 30 * 1024 * 1024) });
    await waitFor(() => expect(screen.getByText(/\[Q-24-2\]/)).toBeTruthy());
    expect(importButton().disabled).toBe(true);
  });

  it('Cancel dequeues without importing', async () => {
    const importDocument = vi.fn();
    const base = createStubImportService();
    const { onClose } = setup({ service: { ...base, importDocument } });
    await ready();
    fireEvent.click(within(screen.getByRole('dialog')).getByRole('button', { name: importDoc.cancel }));
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(importDocument).not.toHaveBeenCalled();
  });
});

describe('ImportDialog: the run', () => {
  const start = async () => {
    await ready();
    fireEvent.click(importButton());
  };

  it('imports, commits to the typed path, and closes when there is nothing to report', async () => {
    const base = createStubImportService();
    const service: ImportService = { ...base, importDocument: async () => okOutcome() };
    const { onClose, commit } = setup({ service, folder: 'docs' });
    await start();
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(commit).toHaveBeenCalledTimes(1);
    expect(commit.mock.calls[0][1]).toBe('docs/report.qmd');
    expect(screen.queryByTestId('import-report')).toBeNull();
  });

  it('Enter in the name field starts the import', async () => {
    const base = createStubImportService();
    const importDocument = vi.fn(async () => okOutcome());
    const { onClose } = setup({ service: { ...base, importDocument } });
    await ready();
    fireEvent.keyDown(nameInput(), { key: 'Enter' });
    await waitFor(() => expect(onClose).toHaveBeenCalled());
    expect(importDocument).toHaveBeenCalledTimes(1);
  });

  it('shows each progress stage, with the first-use download counts, in the panel and the live region', async () => {
    const { service, resolve, opts } = controlledService();
    setup({ service });
    await start();
    expect(screen.getByTestId('import-progress').textContent).toContain(importDoc.progress.reading);
    const o = opts();
    const { act } = await import('@testing-library/react');
    act(() => o.onProgress?.('loading-pandoc' as never));
    expect(screen.getByTestId('import-progress').textContent).toContain(importDoc.progress['loading-pandoc']);
    act(() => o.onLoadProgress?.({ phase: 'download', loaded: 5 * 1024 * 1024, total: 16 * 1024 * 1024 } as never));
    expect(screen.getByTestId('import-progress').textContent).toContain('5.0 MB of 16.0 MB');
    // The live region omits byte counts so it is announced only on real changes.
    expect(screen.getByRole('status').textContent).toBe(download.downloadingConverterShort);
    act(() => o.onProgress?.('converting' as never));
    expect(screen.getByRole('status').textContent).toBe(importDoc.progress.converting);
    resolve(okOutcome());
  });

  it('Cancel aborts the run, dequeues, writes nothing and shows no report', async () => {
    const { service, resolve, opts } = controlledService();
    const { onClose, commit } = setup({ service });
    await start();
    fireEvent.click(screen.getByRole('button', { name: importDoc.cancel }));
    expect(opts().signal?.aborted).toBe(true);
    resolve({ ok: false, cancelled: true, diagnostics: [] });
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(commit).not.toHaveBeenCalled();
    expect(screen.queryByTestId('import-report')).toBeNull();
  });

  it('Escape while importing cancels like the Cancel button', async () => {
    const { service, resolve, opts } = controlledService();
    const { onClose } = setup({ service });
    await start();
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(opts().signal?.aborted).toBe(true);
    resolve({ ok: false, cancelled: true, diagnostics: [] });
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it('a run that finishes just after Cancel is not written', async () => {
    const { service, resolve } = controlledService();
    const { onClose, commit } = setup({ service });
    await start();
    fireEvent.click(screen.getByRole('button', { name: importDoc.cancel }));
    resolve(okOutcome());
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(commit).not.toHaveBeenCalled();
  });

  it('locks Cancel, Escape and the close button while the writes run', async () => {
    const base = createStubImportService();
    const service: ImportService = { ...base, importDocument: async () => okOutcome() };
    let finish!: (r: CommitResult) => void;
    const commit = () => new Promise<CommitResult>((r) => (finish = r));
    const { onClose } = setup({ service, commit });
    await start();
    await waitFor(() => expect(screen.getByTestId('import-progress').textContent).toContain(importDoc.writing));
    expect(screen.queryByRole('button', { name: importDoc.cancel })).toBeNull();
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(onClose).not.toHaveBeenCalled();
    finish({ ok: true, qmdPath: 'report.qmd', qmdDocId: 'd', qmd: '', diagnostics: [] });
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
  });

  it('shows the report for a successful import with notes, grouped by kind, and Close dequeues', async () => {
    // The stub's success carries a warning, an info and a host warning (no title or problem).
    const { onClose } = setup();
    await start();
    const report = await screen.findByTestId('import-report');
    expect(onClose).not.toHaveBeenCalled();
    expect(within(report).getByText(importDoc.reportSucceededWithNotes)).toBeTruthy();
    expect(within(report).getByText(importDoc.groupWarnings(2))).toBeTruthy();
    expect(within(report).getByText(importDoc.groupInfo(1))).toBeTruthy();
    expect(within(report).queryByText(importDoc.groupErrors(1))).toBeNull();
    // A Rust diagnostic shows title and problem; a host one only its message.
    expect(within(report).getByText(/Reader warning: pandoc warned about something/)).toBeTruthy();
    expect(within(report).getByText(/A large image was left out\./)).toBeTruthy();
    fireEvent.click(closeButton());
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('files an info-kind note under info', async () => {
    const base = createStubImportService();
    const service: ImportService = {
      ...base,
      importDocument: async () =>
        okOutcome([{ origin: 'rust', kind: 'note', code: 'Q-24-6', title: 'A note', problem: 'FYI' }]),
    };
    setup({ service });
    await start();
    const report = await screen.findByTestId('import-report');
    expect(within(report).getByText(importDoc.groupInfo(1))).toBeTruthy();
  });

  it('shows a failed import with Rust\'s diagnostic and stores nothing', async () => {
    const { onClose, commit } = setup({ file: file('corrupt.docx') });
    await start();
    const report = await screen.findByTestId('import-report');
    expect(within(report).getByText(importDoc.reportFailed)).toBeTruthy();
    expect(within(report).getByText(importDoc.groupErrors(1))).toBeTruthy();
    expect(within(report).getByText(/Q-24-3/)).toBeTruthy();
    expect(commit).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it('shows a loader failure (offline) with the same text Download as uses, then the diagnostics', async () => {
    const base = createStubImportService();
    const service: ImportService = {
      ...base,
      importDocument: async () => ({
        ok: false,
        uiState: 'offline',
        diagnostics: [{ origin: 'host', kind: 'error', code: 'offline', message: 'fetch failed' }],
      }),
    };
    setup({ service });
    await start();
    const report = await screen.findByTestId('import-report');
    expect(within(report).getByText(download.failed.offline)).toBeTruthy();
    expect(within(report).getByText(/fetch failed/)).toBeTruthy();
  });

  it('shows a storage failure first, then the import report', async () => {
    const warning = { origin: 'rust', kind: 'warning', code: 'Q-24-4', title: 'Reader warning', problem: 'p' } as const;
    const base = createStubImportService();
    const service: ImportService = { ...base, importDocument: async () => okOutcome([warning]) };
    const commit = async (): Promise<CommitResult> => ({
      ok: false,
      diagnostics: [
        { origin: 'host', kind: 'error', code: 'import-write-failed', message: 'Could not add the document (report.qmd): boom', path: 'report.qmd' },
        { origin: 'host', kind: 'error', code: 'import-cleanup-failed', message: 'Could not remove report_media/a.png after the failed import: no', path: 'report_media/a.png' },
        warning,
      ],
    });
    setup({ service, commit });
    await start();
    const report = await screen.findByTestId('import-report');
    expect(within(report).getByText(importDoc.reportFailed)).toBeTruthy();
    const items = within(report).getAllByRole('listitem').map((li) => li.textContent);
    expect(items[0]).toContain('[import-write-failed]');
    expect(items[0]).toContain('report.qmd');
    expect(items[1]).toContain('[import-cleanup-failed]');
    expect(items[2]).toContain('Reader warning');
  });

  it('a commit that throws becomes an import-write-failed report', async () => {
    const base = createStubImportService();
    const service: ImportService = { ...base, importDocument: async () => okOutcome() };
    const err = vi.spyOn(console, 'error').mockImplementation(() => {});
    setup({ service, commit: () => Promise.reject(new Error('index gone')) });
    await start();
    const report = await screen.findByTestId('import-report');
    expect(within(report).getByText(/index gone/)).toBeTruthy();
    expect(within(report).getByText(/import-write-failed/)).toBeTruthy();
    err.mockRestore();
  });

  it('two queued requests for the same file name each get a fresh form', async () => {
    const view = setup({ existingPaths: [] });
    await ready();
    fireEvent.change(nameInput(), { target: { value: 'edited.qmd' } });
    view.rerender(
      <ImportDialog request={{ kind: 'import', file: file('report.docx'), folder: '' }} folders={[]} existingPaths={['report.qmd']} onClose={vi.fn()} commit={okCommit} />,
    );
    expect(nameInput().value).toBe('report 2.qmd');
  });
});
