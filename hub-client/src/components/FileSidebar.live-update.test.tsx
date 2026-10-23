/**
 * @vitest-environment jsdom
 *
 * Live-update contract for the files sidebar. User report: a collaborator's
 * newly created document didn't show in the sidebar until a manual reload.
 * The sync layer (quarto-sync-client's onFilesChange) delivers the new
 * FileEntry live; what the sidebar must then do with a growing `files`
 * prop, per the user's stated rules:
 *
 *  1. A new file at the project root is always visible immediately —
 *     the root is always "being viewed".
 *  2. A new file inside a folder the user has EXPANDED appears, and the
 *     folder's expansion state survives the update.
 *  3. A new file inside a COLLAPSED folder must not force the folder
 *     open — and a folder that is itself brand new renders collapsed.
 */
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import FileSidebar from './FileSidebar';
import type { FileEntry } from '@quarto/preview-renderer/types/project';

afterEach(cleanup);

function entry(path: string): FileEntry {
  return { path, docId: `doc-${path}` } as FileEntry;
}

const baseProps = {
  currentFile: null,
  onSelectFile: vi.fn(),
  onNewFile: vi.fn(),
  onUploadFiles: vi.fn(),
};

describe('FileSidebar live updates', () => {
  it('shows a new top-level file as soon as it arrives in `files`', () => {
    const { rerender } = render(
      <FileSidebar {...baseProps} files={[entry('index.qmd')]} />,
    );

    rerender(
      <FileSidebar {...baseProps} files={[entry('index.qmd'), entry('new-doc.qmd')]} />,
    );

    expect(screen.getByRole('treeitem', { name: 'new-doc.qmd' })).toBeTruthy();
  });

  it('shows a new file inside an already-expanded folder, keeping the folder expanded', () => {
    const initial = [entry('index.qmd'), entry('notes/a.qmd')];
    const { rerender } = render(<FileSidebar {...baseProps} files={initial} />);

    // Folder rows' accessible name includes the chevron glyph ("▶ notes").
    fireEvent.click(screen.getByRole('treeitem', { name: /notes/ }));
    expect(
      screen.getByRole('treeitem', { name: /notes/ }).getAttribute('aria-expanded'),
    ).toBe('true');

    rerender(
      <FileSidebar
        {...baseProps}
        files={[...initial, entry('notes/new-doc.qmd')]}
      />,
    );

    expect(screen.getByRole('treeitem', { name: 'new-doc.qmd' })).toBeTruthy();
    expect(
      screen.getByRole('treeitem', { name: /notes/ }).getAttribute('aria-expanded'),
    ).toBe('true');
  });

  it('does not auto-expand a collapsed folder when a file arrives inside it', () => {
    const initial = [entry('index.qmd'), entry('notes/a.qmd')];
    const { rerender } = render(<FileSidebar {...baseProps} files={initial} />);

    rerender(
      <FileSidebar
        {...baseProps}
        files={[...initial, entry('notes/new-doc.qmd')]}
      />,
    );

    expect(screen.queryByRole('treeitem', { name: 'new-doc.qmd' })).toBeNull();
    expect(
      screen.getByRole('treeitem', { name: /notes/ }).getAttribute('aria-expanded'),
    ).toBe('false');
  });

  it('renders a brand-new folder (arriving with its first file) collapsed', () => {
    const { rerender } = render(
      <FileSidebar {...baseProps} files={[entry('index.qmd')]} />,
    );

    rerender(
      <FileSidebar
        {...baseProps}
        files={[entry('index.qmd'), entry('brand-new-dir/new-doc.qmd')]}
      />,
    );

    expect(
      screen.getByRole('treeitem', { name: /brand-new-dir/ }).getAttribute('aria-expanded'),
    ).toBe('false');
    expect(screen.queryByRole('treeitem', { name: 'new-doc.qmd' })).toBeNull();
  });
});
