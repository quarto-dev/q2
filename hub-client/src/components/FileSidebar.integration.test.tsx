/**
 * Tests for FileSidebar component focused on the asset-upload path.
 *
 * Covers the Upload button and the drop handler's destination derivation
 * (added in the generic-file-uploader plan, Phase C).
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import FileSidebar from './FileSidebar';
import type { FileEntry } from '@quarto/preview-renderer/types/project';

// The printable service touches the WASM VFS + window.open; stub it so
// the button-wiring tests stay in the component layer.
const openPrintableDocument = vi.fn(() => Promise.resolve());
vi.mock('../services/printableDocument', () => ({
  openPrintableDocument: (...args: unknown[]) => openPrintableDocument(...args),
}));

function file(path: string): FileEntry {
  return { path, docId: 'doc-' + path };
}

describe('FileSidebar asset-upload integration', () => {
  const baseFiles: FileEntry[] = [
    file('index.qmd'),
    file('notes/a.qmd'),
    file('notes/b.qmd'),
    file('images/cat.png'),
    file('_quarto/grammars/toml/toml.wasm'),
  ];

  const baseProps = {
    files: baseFiles,
    currentFile: null,
    onSelectFile: vi.fn(),
    onNewFile: vi.fn(),
    onUploadFiles: vi.fn(),
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  describe('Upload button', () => {
    it('calls onUploadFiles with empty files and root destination when nothing is selected', () => {
      const onUploadFiles = vi.fn();
      render(<FileSidebar {...baseProps} onUploadFiles={onUploadFiles} />);
      fireEvent.click(screen.getByRole('button', { name: /upload/i }));
      expect(onUploadFiles).toHaveBeenCalledWith([], '');
    });

    it('uses the parent folder of the currently selected file', () => {
      const onUploadFiles = vi.fn();
      render(
        <FileSidebar
          {...baseProps}
          currentFile={file('notes/a.qmd')}
          onUploadFiles={onUploadFiles}
        />
      );
      fireEvent.click(screen.getByRole('button', { name: /upload/i }));
      expect(onUploadFiles).toHaveBeenCalledWith([], 'notes');
    });

    it('uses root for a file at project root', () => {
      const onUploadFiles = vi.fn();
      render(
        <FileSidebar
          {...baseProps}
          currentFile={file('index.qmd')}
          onUploadFiles={onUploadFiles}
        />
      );
      fireEvent.click(screen.getByRole('button', { name: /upload/i }));
      expect(onUploadFiles).toHaveBeenCalledWith([], '');
    });

    it('uses a deeply nested folder path', () => {
      const onUploadFiles = vi.fn();
      render(
        <FileSidebar
          {...baseProps}
          currentFile={file('_quarto/grammars/toml/toml.wasm')}
          onUploadFiles={onUploadFiles}
        />
      );
      fireEvent.click(screen.getByRole('button', { name: /upload/i }));
      expect(onUploadFiles).toHaveBeenCalledWith([], '_quarto/grammars/toml');
    });
  });

  describe('Open printable version button (issue #315)', () => {
    const printableName = /printable version/i;

    it('is hidden when there is no printable format', () => {
      render(<FileSidebar {...baseProps} currentFile={file('index.qmd')} />);
      expect(screen.queryByRole('button', { name: printableName })).toBeNull();
    });

    it('is hidden for non-printable formats', () => {
      render(
        <FileSidebar
          {...baseProps}
          currentFile={file('index.qmd')}
          currentFormat="q2-debug"
        />,
      );
      expect(screen.queryByRole('button', { name: printableName })).toBeNull();
    });

    it.each(['q2-preview', 'q2-slides', 'revealjs'])(
      'is shown for the printable format %s',
      (fmt) => {
        render(
          <FileSidebar
            {...baseProps}
            currentFile={file('index.qmd')}
            currentFormat={fmt}
          />,
        );
        expect(
          screen.getByRole('button', { name: printableName }),
        ).toBeTruthy();
      },
    );

    it('opens the printable document for the current file on click', () => {
      render(
        <FileSidebar
          {...baseProps}
          currentFile={file('notes/a.qmd')}
          currentFormat="revealjs"
        />,
      );
      fireEvent.click(screen.getByRole('button', { name: printableName }));
      expect(openPrintableDocument).toHaveBeenCalledWith('notes/a.qmd', 'revealjs');
    });
  });
});
