/**
 * Tests for NewAssetDialog component.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react';
import NewAssetDialog from './NewAssetDialog';

function makeFile(name: string, size = 1024, type = 'application/octet-stream'): File {
  const blob = new Blob([new Uint8Array(Math.min(size, 16))], { type });
  const file = new File([blob], name, { type });
  Object.defineProperty(file, 'size', { value: size });
  return file;
}

describe('NewAssetDialog', () => {
  const defaultProps = {
    isOpen: true,
    existingPaths: [] as string[],
    defaultDestination: '',
    onClose: vi.fn(),
    onUploadAsset: vi.fn(),
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  describe('basic rendering', () => {
    it('renders when open', () => {
      render(<NewAssetDialog {...defaultProps} />);
      expect(screen.getByText('Add asset to project')).toBeInTheDocument();
    });

    it('does not render when closed', () => {
      render(<NewAssetDialog {...defaultProps} isOpen={false} />);
      expect(screen.queryByText('Add asset to project')).not.toBeInTheDocument();
    });

    it('shows the destination folder in the folder picker', () => {
      render(<NewAssetDialog {...defaultProps} defaultDestination="images" folders={['images']} />);
      expect(screen.getByRole('button', { name: 'Choose folder' })).toHaveTextContent('images');
    });

    it('shows empty destination as project root', () => {
      render(<NewAssetDialog {...defaultProps} defaultDestination="" />);
      expect(screen.getByRole('button', { name: 'Choose folder' })).toHaveTextContent('(project root)');
    });
  });

  describe('initial files', () => {
    it('pre-populates the preview list', () => {
      const files = [makeFile('foo.png'), makeFile('bar.wasm')];
      render(<NewAssetDialog {...defaultProps} initialFiles={files} />);
      expect(screen.getByDisplayValue('foo.png')).toBeInTheDocument();
      expect(screen.getByDisplayValue('bar.wasm')).toBeInTheDocument();
    });

    it('marks oversized initial files with an error', async () => {
      const oversize = 20 * 1024 * 1024;
      const files = [makeFile('big.bin', oversize)];
      render(<NewAssetDialog {...defaultProps} initialFiles={files} />);
      await waitFor(() => {
        expect(screen.getByText(/exceeds maximum/i)).toBeInTheDocument();
      });
    });

    it('accepts empty initial files (a blank new file is a normal upload)', async () => {
      const files = [makeFile('empty.qmd', 0)];
      render(<NewAssetDialog {...defaultProps} initialFiles={files} />);
      await waitFor(() => {
        expect(screen.getByDisplayValue('empty.qmd')).toBeInTheDocument();
      });
      expect(screen.queryByText(/empty/i)).not.toBeInTheDocument();
    });
  });

  describe('destination folder picker', () => {
    it('lists the project folders and picking one changes the destination', () => {
      render(
        <NewAssetDialog {...defaultProps} defaultDestination="" folders={['images', 'images/icons']} />
      );
      fireEvent.click(screen.getByRole('button', { name: 'Choose folder' }));
      const menu = screen.getByRole('menu', { name: 'Choose folder' });
      expect(menu).toHaveTextContent('(project root)');
      expect(menu).toHaveTextContent('images');
      expect(menu).toHaveTextContent('icons');

      fireEvent.click(screen.getAllByRole('menuitem').find((el) => el.textContent === 'icons')!);
      expect(screen.getByRole('button', { name: 'Choose folder' })).toHaveTextContent('images/icons');
    });
  });

  describe('upload flow', () => {
    it('calls onUploadAsset with composed path for each valid file', async () => {
      const onUpload = vi.fn();
      const files = [makeFile('toml.wasm'), makeFile('highlights.scm')];
      render(
        <NewAssetDialog
          {...defaultProps}
          defaultDestination="_quarto/grammars/toml"
          initialFiles={files}
          onUploadAsset={onUpload}
        />
      );

      fireEvent.click(screen.getByRole('button', { name: /upload/i }));

      await waitFor(() => {
        expect(onUpload).toHaveBeenCalledTimes(2);
      });
      const calls = onUpload.mock.calls.map(([f, path]) => ({ name: f.name, path }));
      expect(calls).toContainEqual({
        name: 'toml.wasm',
        path: '_quarto/grammars/toml/toml.wasm',
      });
      expect(calls).toContainEqual({
        name: 'highlights.scm',
        path: '_quarto/grammars/toml/highlights.scm',
      });
    });

    it('composes path correctly for project root (no destination)', async () => {
      const onUpload = vi.fn();
      const files = [makeFile('foo.png')];
      render(
        <NewAssetDialog
          {...defaultProps}
          defaultDestination=""
          initialFiles={files}
          onUploadAsset={onUpload}
        />
      );

      fireEvent.click(screen.getByRole('button', { name: /upload/i }));

      await waitFor(() => {
        expect(onUpload).toHaveBeenCalledWith(expect.any(File), 'foo.png');
      });
    });

    it('blocks upload when destination is invalid', () => {
      const onUpload = vi.fn();
      const files = [makeFile('foo.png')];
      render(
        <NewAssetDialog
          {...defaultProps}
          defaultDestination="/bad"
          initialFiles={files}
          onUploadAsset={onUpload}
        />
      );

      const uploadBtn = screen.getByRole('button', { name: /upload/i }) as HTMLButtonElement;
      expect(uploadBtn.disabled).toBe(true);
    });

    it('rejects collision with an existing path', async () => {
      const onUpload = vi.fn();
      const files = [makeFile('foo.png')];
      render(
        <NewAssetDialog
          {...defaultProps}
          existingPaths={['images/foo.png']}
          defaultDestination="images"
          initialFiles={files}
          onUploadAsset={onUpload}
        />
      );

      await waitFor(() => {
        expect(screen.getByText(/already exists/i)).toBeInTheDocument();
      });
    });

    it('calls onClose after successful upload', async () => {
      const onUpload = vi.fn();
      const onClose = vi.fn();
      const files = [makeFile('foo.png')];
      render(
        <NewAssetDialog
          {...defaultProps}
          defaultDestination=""
          initialFiles={files}
          onUploadAsset={onUpload}
          onClose={onClose}
        />
      );

      fireEvent.click(screen.getByRole('button', { name: /upload/i }));

      await waitFor(() => {
        expect(onClose).toHaveBeenCalled();
      });
    });
  });

  describe('file removal', () => {
    it('removes a file from the preview list', () => {
      const files = [makeFile('foo.png'), makeFile('bar.png')];
      render(<NewAssetDialog {...defaultProps} initialFiles={files} />);

      expect(screen.getByDisplayValue('foo.png')).toBeInTheDocument();
      expect(screen.getByDisplayValue('bar.png')).toBeInTheDocument();

      const removeBtns = screen.getAllByLabelText(/remove/i);
      fireEvent.click(removeBtns[0]);

      expect(screen.queryByDisplayValue('foo.png')).not.toBeInTheDocument();
      expect(screen.getByDisplayValue('bar.png')).toBeInTheDocument();
    });
  });
});
