/**
 * Unit Tests for NewFileDialog Component
 *
 * Tests the template selection functionality in the New File dialog.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import NewFileDialog from './NewFileDialog';

// Mock the template service
vi.mock('../services/templateService', () => ({
  discoverTemplates: vi.fn(),
}));

import { discoverTemplates } from '../services/templateService';

const mockDiscoverTemplates = vi.mocked(discoverTemplates);

describe('NewFileDialog', () => {
  const defaultProps = {
    isOpen: true,
    existingPaths: [],
    onClose: vi.fn(),
    onCreateTextFile: vi.fn(),
    onUploadBinaryFile: vi.fn(),
  };

  beforeEach(() => {
    vi.clearAllMocks();
    // Default to no templates
    mockDiscoverTemplates.mockResolvedValue([]);
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('basic rendering', () => {
    it('renders the dialog when open', () => {
      render(<NewFileDialog {...defaultProps} />);

      expect(screen.getByText('Add File')).toBeInTheDocument();
      expect(screen.getByText('New Text File')).toBeInTheDocument();
      expect(screen.getByText('Upload File')).toBeInTheDocument();
    });

    it('does not render when closed', () => {
      render(<NewFileDialog {...defaultProps} isOpen={false} />);

      expect(screen.queryByText('Add File')).not.toBeInTheDocument();
    });

    it('shows filename input in text mode', () => {
      render(<NewFileDialog {...defaultProps} />);

      expect(screen.getByLabelText('Filename:')).toBeInTheDocument();
    });
  });

  describe('template dropdown', () => {
    it('does not show template dropdown when no templates exist', async () => {
      mockDiscoverTemplates.mockResolvedValue([]);

      render(<NewFileDialog {...defaultProps} />);

      await waitFor(() => {
        expect(mockDiscoverTemplates).toHaveBeenCalled();
      });

      expect(screen.queryByLabelText('Template:')).not.toBeInTheDocument();
    });

    it('shows template dropdown when templates exist', async () => {
      mockDiscoverTemplates.mockResolvedValue([
        {
          path: '/project/_quarto-hub-templates/article.qmd',
          displayName: 'Article',
          strippedContent: '---\ntitle: Untitled\n---\n\n# Content',
        },
      ]);

      render(<NewFileDialog {...defaultProps} />);

      await waitFor(() => {
        expect(screen.getByLabelText('Template:')).toBeInTheDocument();
      });
    });

    it('shows all templates in dropdown', async () => {
      mockDiscoverTemplates.mockResolvedValue([
        {
          path: '/project/_quarto-hub-templates/article.qmd',
          displayName: 'Article',
          strippedContent: 'article content',
        },
        {
          path: '/project/_quarto-hub-templates/report.qmd',
          displayName: 'Report',
          strippedContent: 'report content',
        },
        {
          path: '/project/_quarto-hub-templates/presentation.qmd',
          displayName: 'Presentation',
          strippedContent: 'presentation content',
        },
      ]);

      render(<NewFileDialog {...defaultProps} />);

      await waitFor(() => {
        expect(screen.getByLabelText('Template:')).toBeInTheDocument();
      });

      const select = screen.getByLabelText('Template:');
      expect(select).toBeInTheDocument();

      // Check all options are present
      expect(screen.getByRole('option', { name: 'Blank file' })).toBeInTheDocument();
      expect(screen.getByRole('option', { name: 'Article' })).toBeInTheDocument();
      expect(screen.getByRole('option', { name: 'Report' })).toBeInTheDocument();
      expect(screen.getByRole('option', { name: 'Presentation' })).toBeInTheDocument();
    });

    it('defaults to Blank file option', async () => {
      mockDiscoverTemplates.mockResolvedValue([
        {
          path: '/project/_quarto-hub-templates/article.qmd',
          displayName: 'Article',
          strippedContent: 'content',
        },
      ]);

      render(<NewFileDialog {...defaultProps} />);

      await waitFor(() => {
        expect(screen.getByLabelText('Template:')).toBeInTheDocument();
      });

      const select = screen.getByLabelText('Template:') as HTMLSelectElement;
      expect(select.value).toBe('');
    });
  });

  describe('file creation with templates', () => {
    it('creates file with empty content when Blank file is selected', async () => {
      mockDiscoverTemplates.mockResolvedValue([
        {
          path: '/project/_quarto-hub-templates/article.qmd',
          displayName: 'Article',
          strippedContent: 'article content',
        },
      ]);

      render(<NewFileDialog {...defaultProps} />);

      await waitFor(() => {
        expect(screen.getByLabelText('Template:')).toBeInTheDocument();
      });

      // Enter filename
      const filenameInput = screen.getByLabelText('Filename:');
      fireEvent.change(filenameInput, { target: { value: 'test.qmd' } });

      // Click create
      const createButton = screen.getByRole('button', { name: 'Create' });
      fireEvent.click(createButton);

      expect(defaultProps.onCreateTextFile).toHaveBeenCalledWith('test.qmd', '');
    });

    it('creates file with template content when template is selected', async () => {
      const templateContent = '---\ntitle: Untitled Article\n---\n\n# Introduction\n';

      mockDiscoverTemplates.mockResolvedValue([
        {
          path: '/project/_quarto-hub-templates/article.qmd',
          displayName: 'Article',
          strippedContent: templateContent,
        },
      ]);

      render(<NewFileDialog {...defaultProps} />);

      await waitFor(() => {
        expect(screen.getByLabelText('Template:')).toBeInTheDocument();
      });

      // Select template
      const select = screen.getByLabelText('Template:');
      fireEvent.change(select, {
        target: { value: '/project/_quarto-hub-templates/article.qmd' },
      });

      // Enter filename
      const filenameInput = screen.getByLabelText('Filename:');
      fireEvent.change(filenameInput, { target: { value: 'my-article.qmd' } });

      // Click create
      const createButton = screen.getByRole('button', { name: 'Create' });
      fireEvent.click(createButton);

      expect(defaultProps.onCreateTextFile).toHaveBeenCalledWith(
        'my-article.qmd',
        templateContent
      );
    });

    it('can switch between templates', async () => {
      mockDiscoverTemplates.mockResolvedValue([
        {
          path: '/project/_quarto-hub-templates/article.qmd',
          displayName: 'Article',
          strippedContent: 'article content',
        },
        {
          path: '/project/_quarto-hub-templates/report.qmd',
          displayName: 'Report',
          strippedContent: 'report content',
        },
      ]);

      render(<NewFileDialog {...defaultProps} />);

      await waitFor(() => {
        expect(screen.getByLabelText('Template:')).toBeInTheDocument();
      });

      const select = screen.getByLabelText('Template:') as HTMLSelectElement;

      // Select Article
      fireEvent.change(select, {
        target: { value: '/project/_quarto-hub-templates/article.qmd' },
      });

      // Enter filename and create
      const filenameInput = screen.getByLabelText('Filename:');
      fireEvent.change(filenameInput, { target: { value: 'test.qmd' } });

      const createButton = screen.getByRole('button', { name: 'Create' });
      fireEvent.click(createButton);

      expect(defaultProps.onCreateTextFile).toHaveBeenCalledWith('test.qmd', 'article content');
    });

    it('resets to Blank file when switching back', async () => {
      mockDiscoverTemplates.mockResolvedValue([
        {
          path: '/project/_quarto-hub-templates/article.qmd',
          displayName: 'Article',
          strippedContent: 'article content',
        },
      ]);

      render(<NewFileDialog {...defaultProps} />);

      await waitFor(() => {
        expect(screen.getByLabelText('Template:')).toBeInTheDocument();
      });

      const select = screen.getByLabelText('Template:') as HTMLSelectElement;

      // Select Article
      fireEvent.change(select, {
        target: { value: '/project/_quarto-hub-templates/article.qmd' },
      });

      // Switch back to Blank
      fireEvent.change(select, { target: { value: '' } });

      // Enter filename and create
      const filenameInput = screen.getByLabelText('Filename:');
      fireEvent.change(filenameInput, { target: { value: 'test.qmd' } });

      const createButton = screen.getByRole('button', { name: 'Create' });
      fireEvent.click(createButton);

      expect(defaultProps.onCreateTextFile).toHaveBeenCalledWith('test.qmd', '');
    });
  });

  describe('state reset', () => {
    it('resets template selection when dialog closes', async () => {
      mockDiscoverTemplates.mockResolvedValue([
        {
          path: '/project/_quarto-hub-templates/article.qmd',
          displayName: 'Article',
          strippedContent: 'content',
        },
      ]);

      const { rerender } = render(<NewFileDialog {...defaultProps} />);

      await waitFor(() => {
        expect(screen.getByLabelText('Template:')).toBeInTheDocument();
      });

      // Select a template
      const select = screen.getByLabelText('Template:') as HTMLSelectElement;
      fireEvent.change(select, {
        target: { value: '/project/_quarto-hub-templates/article.qmd' },
      });

      // Close dialog
      rerender(<NewFileDialog {...defaultProps} isOpen={false} />);

      // Reopen dialog
      rerender(<NewFileDialog {...defaultProps} isOpen={true} />);

      await waitFor(() => {
        const newSelect = screen.getByLabelText('Template:') as HTMLSelectElement;
        expect(newSelect.value).toBe('');
      });
    });
  });
});
