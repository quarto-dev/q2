/**
 * @vitest-environment jsdom
 *
 * Full-text search moved from an inline sidebar box to SearchFilesDialog,
 * opened by the sidebar's magnifying-glass button.
 */

import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react';
import type { FileEntry } from '@quarto/preview-renderer/types/project';
import FileSidebar from './FileSidebar';
import SearchFilesDialog from './SearchFilesDialog';
import type { SearchResult } from '../services/search';

afterEach(cleanup);

function entry(path: string): FileEntry {
  return { path, docId: `doc-${path}` } as FileEntry;
}

describe('FileSidebar search button', () => {
  it('does not render a search button when onOpenSearch is not provided', () => {
    render(
      <FileSidebar
        files={[entry('a.qmd')]}
        currentFile={null}
        onSelectFile={() => {}}
        onNewFile={() => {}}
        onUploadFiles={() => {}}
      />
    );
    expect(screen.queryByRole('button', { name: 'Search files' })).toBeNull();
  });

  it('opens search via the header button', () => {
    const onOpenSearch = vi.fn();
    render(
      <FileSidebar
        files={[entry('a.qmd')]}
        currentFile={null}
        onSelectFile={() => {}}
        onNewFile={() => {}}
        onUploadFiles={() => {}}
        onOpenSearch={onOpenSearch}
      />
    );
    fireEvent.click(screen.getByRole('button', { name: 'Search files' }));
    expect(onOpenSearch).toHaveBeenCalledTimes(1);
  });
});

describe('SearchFilesDialog', () => {
  const base = { isOpen: true, onClose: () => {} };

  it('runs a query and renders ranked results with snippets', async () => {
    const searchFiles = vi.fn(
      async (): Promise<SearchResult[]> => [{ path: 'intro.qmd', score: 2, terms: ['search'] }]
    );
    const fileContents = new Map([['intro.qmd', 'a document about search engines']]);
    render(
      <SearchFilesDialog
        {...base}
        files={[entry('intro.qmd'), entry('other.qmd')]}
        searchFiles={searchFiles}
        fileContents={fileContents}
        onSelectFile={() => {}}
      />
    );

    fireEvent.change(screen.getByRole('searchbox', { name: 'Search files' }), { target: { value: 'search' } });

    await waitFor(() => expect(searchFiles).toHaveBeenCalledWith('search', expect.anything()));
    expect(await screen.findByText('intro.qmd')).toBeTruthy();
    // Snippet highlights the matched term in a <mark>.
    const mark = document.querySelector('.search-result-snippet mark');
    expect(mark?.textContent).toBe('search');
  });

  it('selects the file and its first match when a result is clicked, then closes', async () => {
    const onSelectFile = vi.fn();
    const onClose = vi.fn();
    const searchFiles = vi.fn(
      async (): Promise<SearchResult[]> => [{ path: 'intro.qmd', score: 1, terms: ['intro'] }]
    );
    render(
      <SearchFilesDialog
        {...base}
        onClose={onClose}
        files={[entry('intro.qmd')]}
        searchFiles={searchFiles}
        fileContents={new Map([['intro.qmd', 'An intro to things']])}
        onSelectFile={onSelectFile}
      />
    );

    fireEvent.change(screen.getByRole('searchbox', { name: 'Search files' }), { target: { value: 'intro' } });
    fireEvent.click(await screen.findByText('intro.qmd'));

    expect(onSelectFile).toHaveBeenCalledWith(
      expect.objectContaining({ path: 'intro.qmd' }),
      { index: 3, length: 5 }
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('passes a null match when the file text is not loaded', async () => {
    const onSelectFile = vi.fn();
    const searchFiles = vi.fn(
      async (): Promise<SearchResult[]> => [{ path: 'intro.qmd', score: 1, terms: ['intro'] }]
    );
    render(
      <SearchFilesDialog
        {...base}
        files={[entry('intro.qmd')]}
        searchFiles={searchFiles}
        onSelectFile={onSelectFile}
      />
    );
    fireEvent.change(screen.getByRole('searchbox', { name: 'Search files' }), { target: { value: 'intro' } });
    fireEvent.click(await screen.findByText('intro.qmd'));
    expect(onSelectFile).toHaveBeenCalledWith(expect.objectContaining({ path: 'intro.qmd' }), null);
  });

  it('shows a no-matches state when the query returns nothing', async () => {
    const searchFiles = vi.fn(async (): Promise<SearchResult[]> => []);
    render(
      <SearchFilesDialog {...base} files={[entry('a.qmd')]} searchFiles={searchFiles} onSelectFile={() => {}} />
    );
    fireEvent.change(screen.getByRole('searchbox', { name: 'Search files' }), { target: { value: 'zzz' } });
    expect(await screen.findByText('No matches')).toBeTruthy();
  });

  it('Enter opens the highlighted result; ArrowDown moves the highlight', async () => {
    const onSelectFile = vi.fn();
    const searchFiles = vi.fn(
      async (): Promise<SearchResult[]> => [
        { path: 'a.qmd', score: 2, terms: ['x'] },
        { path: 'b.qmd', score: 1, terms: ['x'] },
      ]
    );
    render(
      <SearchFilesDialog
        {...base}
        files={[entry('a.qmd'), entry('b.qmd')]}
        searchFiles={searchFiles}
        onSelectFile={onSelectFile}
      />
    );
    const input = screen.getByRole('searchbox', { name: 'Search files' });
    fireEvent.change(input, { target: { value: 'x' } });
    await screen.findByText('b.qmd');

    fireEvent.keyDown(input, { key: 'ArrowDown' });
    fireEvent.keyDown(input, { key: 'Enter' });
    expect(onSelectFile).toHaveBeenCalledWith(expect.objectContaining({ path: 'b.qmd' }), null);
  });
});
