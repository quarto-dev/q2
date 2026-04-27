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

    it('shows the destination input', () => {
      render(<NewAssetDialog {...defaultProps} defaultDestination="images" />);
      const input = screen.getByLabelText(/destination/i) as HTMLInputElement;
      expect(input.value).toBe('images');
    });

    it('shows empty destination as project root', () => {
      render(<NewAssetDialog {...defaultProps} defaultDestination="" />);
      const input = screen.getByLabelText(/destination/i) as HTMLInputElement;
      expect(input.value).toBe('');
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

    it('marks empty initial files with an error', async () => {
      const files = [makeFile('empty.png', 0)];
      render(<NewAssetDialog {...defaultProps} initialFiles={files} />);
      await waitFor(() => {
        expect(screen.getByText(/empty/i)).toBeInTheDocument();
      });
    });
  });

  describe('destination validation', () => {
    it('reports an error for leading slash', () => {
      render(<NewAssetDialog {...defaultProps} defaultDestination="" />);
      const input = screen.getByLabelText(/destination/i);
      fireEvent.change(input, { target: { value: '/images' } });
      expect(screen.getByText(/leading slash/i)).toBeInTheDocument();
    });

    it('reports an error for ".." segments', () => {
      render(<NewAssetDialog {...defaultProps} defaultDestination="" />);
      const input = screen.getByLabelText(/destination/i);
      fireEvent.change(input, { target: { value: 'foo/..' } });
      expect(screen.getByText(/"\."|".."|segments/i)).toBeInTheDocument();
    });

    it('reports an error for forbidden chars', () => {
      render(<NewAssetDialog {...defaultProps} defaultDestination="" />);
      const input = screen.getByLabelText(/destination/i);
      fireEvent.change(input, { target: { value: 'foo<bar' } });
      expect(screen.getByText(/invalid char/i)).toBeInTheDocument();
    });

    it('reports an error for empty segment (trailing slash)', () => {
      render(<NewAssetDialog {...defaultProps} defaultDestination="" />);
      const input = screen.getByLabelText(/destination/i);
      fireEvent.change(input, { target: { value: 'foo/' } });
      expect(screen.getByText(/empty segment/i)).toBeInTheDocument();
    });

    it('accepts a valid nested destination', () => {
      render(<NewAssetDialog {...defaultProps} defaultDestination="" />);
      const input = screen.getByLabelText(/destination/i);
      fireEvent.change(input, { target: { value: '_quarto/grammars/toml' } });
      expect(screen.queryByText(/slash|invalid|segment/i)).not.toBeInTheDocument();
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
