/**
 * @vitest-environment jsdom
 *
 * External file drops on the real FileSidebar (document import P5 T4): the sidebar derives the
 * destination folder and hands the raw entries to `onDropFiles`; the Editor then routes importable
 * top-level files to the import queue (`routeDroppedEntries`). These are the first drop tests in
 * the codebase. The editor, preview-pane, no-file-open and dialog-open cases need the mounted app
 * and are Playwright cases (`e2e/import-document.spec.ts`).
 */
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react';
import FileSidebar from './FileSidebar';
import NewAssetDialog from './NewAssetDialog';
import { createStubImportService } from '../pandoc/importService';
import { routeDroppedEntries, type RoutedDrop } from '../utils/routeDroppedEntries';
import type { DroppedEntries } from '../utils/droppedEntries';
import type { FileEntry } from '@quarto/preview-renderer/types/project';

afterEach(cleanup);

const entry = (path: string): FileEntry => ({ path, docId: `doc-${path}` });
const formats = await createStubImportService().getImportFormats();

const file = (name: string, size = 10) => {
  const f = new File(['x'], name);
  Object.defineProperty(f, 'size', { value: size });
  return f;
};

/** A DataTransfer-like object: jsdom has none, and the sidebar reads types, files, items and getData. */
const dataTransfer = (files: File[]) => ({ types: ['Files'], files, items: [], getData: () => '', dropEffect: 'none' });

/** What the Editor does with the sidebar's call: route it. */
function setup(props: { files?: FileEntry[]; currentFile?: FileEntry | null } = {}) {
  const routed: Array<{ destination: string; route: RoutedDrop; raw: DroppedEntries }> = [];
  const onDropFiles = vi.fn((raw: DroppedEntries, destination: string) => {
    routed.push({ destination, raw, route: routeDroppedEntries(raw, { formats, destination }) });
  });
  const view = render(
    <FileSidebar
      files={props.files ?? [entry('index.qmd'), entry('notes/a.qmd'), entry('notes/b.qmd')]}
      currentFile={props.currentFile ?? null}
      onSelectFile={vi.fn()}
      onNewFile={vi.fn()}
      onUploadFiles={vi.fn()}
      onDropFiles={onDropFiles}
    />,
  );
  return { onDropFiles, routed, ...view };
}

const dropOn = (target: Element, files: File[]) => fireEvent.drop(target, { dataTransfer: dataTransfer(files) });

describe('FileSidebar external drops', () => {
  it('a docx on the tree background lands in the project root, routed to import', async () => {
    const { routed } = setup();
    const doc = file('report.docx');
    dropOn(screen.getByRole('tree'), [doc]);
    await waitFor(() => expect(routed).toHaveLength(1));
    expect(routed[0].destination).toBe('');
    expect(routed[0].route.imports).toEqual([{ file: doc, folder: '' }]);
    expect(routed[0].route.uploads.files).toEqual([]);
  });

  it('a docx on a folder row lands in that folder', async () => {
    const { routed } = setup();
    dropOn(screen.getByText('notes'), [file('report.docx')]);
    await waitFor(() => expect(routed).toHaveLength(1));
    expect(routed[0].destination).toBe('notes');
    expect(routed[0].route.imports.map((i) => i.folder)).toEqual(['notes']);
  });

  it('a docx on a file row lands in that file\'s folder', async () => {
    const { routed } = setup();
    // Folders start collapsed: open `notes` to reach its files.
    fireEvent.click(screen.getByText('notes'));
    dropOn(screen.getByText('a.qmd'), [file('report.docx')]);
    await waitFor(() => expect(routed).toHaveLength(1));
    expect(routed[0].destination).toBe('notes');
    expect(routed[0].route.imports).toHaveLength(1);
  });

  it('a docx on a root-level file row lands in the project root', async () => {
    const { routed } = setup();
    dropOn(screen.getByText('index.qmd'), [file('report.docx')]);
    await waitFor(() => expect(routed).toHaveLength(1));
    expect(routed[0].destination).toBe('');
  });

  it('in an empty project the destination is the root, or the open file\'s folder', async () => {
    const root = setup({ files: [] });
    dropOn(root.container.querySelector('.file-sidebar')!, [file('report.docx')]);
    await waitFor(() => expect(root.routed).toHaveLength(1));
    expect(root.routed[0].destination).toBe('');
    expect(root.routed[0].route.imports).toHaveLength(1);
    cleanup();

    const withFile = setup({ files: [], currentFile: entry('notes/a.qmd') });
    dropOn(withFile.container.querySelector('.file-sidebar')!, [file('report.docx')]);
    await waitFor(() => expect(withFile.routed).toHaveLength(1));
    expect(withFile.routed[0].destination).toBe('notes');
  });

  it('a docx plus a png: the docx imports, the png is left for upload', async () => {
    const { routed } = setup();
    const doc = file('report.docx');
    const png = file('figure.png');
    dropOn(screen.getByText('notes'), [doc, png]);
    await waitFor(() => expect(routed).toHaveLength(1));
    expect(routed[0].route.imports.map((i) => i.file)).toEqual([doc]);
    expect(routed[0].route.uploads.files.map((f) => f.file)).toEqual([png]);
  });

  it('does not import a file that is not importable', async () => {
    const { routed } = setup();
    dropOn(screen.getByRole('tree'), [file('data.csv')]);
    await waitFor(() => expect(routed).toHaveLength(1));
    expect(routed[0].route.imports).toEqual([]);
    expect(routed[0].route.uploads.files).toHaveLength(1);
  });
});

describe('Add asset dialog drop zone', () => {
  it('stores a docx as-is, with no import routing (I5, I21)', async () => {
    const onUploadAsset = vi.fn();
    render(<NewAssetDialog isOpen existingPaths={[]} defaultDestination="" onClose={vi.fn()} onUploadAsset={onUploadAsset} />);
    const doc = file('report.docx', 2048);
    fireEvent.drop(screen.getByRole('dialog'), { dataTransfer: { files: [doc], types: ['Files'], items: [] } });
    const upload = await screen.findByRole('button', { name: /^upload/i });
    fireEvent.click(upload);
    await waitFor(() => expect(onUploadAsset).toHaveBeenCalledTimes(1));
    expect(onUploadAsset).toHaveBeenCalledWith(doc, 'report.docx');
  });
});
