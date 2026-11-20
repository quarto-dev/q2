/**
 * Unit Tests for NewFileDialog Component
 *
 * Tests the template selection functionality in the New File dialog.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { useState } from 'react';
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

      expect(screen.getByText('New file')).toBeInTheDocument();
    });

    it('does not render when closed', () => {
      render(<NewFileDialog {...defaultProps} isOpen={false} />);

      expect(screen.queryByText('New file')).not.toBeInTheDocument();
    });

    it('does not render any upload-tab UI', () => {
      render(<NewFileDialog {...defaultProps} />);

      expect(screen.queryByText(/upload file/i)).not.toBeInTheDocument();
      expect(screen.queryByText(/drag.*drop/i)).not.toBeInTheDocument();
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

  describe('file type', () => {
    function createWith(type: string, name: string) {
      fireEvent.click(screen.getByRole('radio', { name: type }));
      fireEvent.change(screen.getByLabelText('Filename:'), { target: { value: name } });
      fireEvent.click(screen.getByRole('button', { name: 'Create' }));
    }

    it('appends the extension of a specific file type', () => {
      render(<NewFileDialog {...defaultProps} />);

      createWith('.md', 'intro');

      expect(defaultProps.onCreateTextFile).toHaveBeenCalledWith('intro.md', '');
    });

    it('creates a file without an extension as typed when Other is chosen', () => {
      render(<NewFileDialog {...defaultProps} />);

      createWith('Other', '.python-version');

      expect(defaultProps.onCreateTextFile).toHaveBeenCalledWith('.python-version', '');
    });

    it('keeps the name as typed when Other is chosen', () => {
      render(<NewFileDialog {...defaultProps} />);

      fireEvent.click(screen.getByRole('radio', { name: 'Other' }));
      expect(screen.queryByLabelText('Extension:')).not.toBeInTheDocument();
      expect(screen.getByLabelText('Filename:')).toHaveAttribute(
        'placeholder',
        'e.g., styles.css'
      );

      createWith('Other', 'styles.css');

      expect(defaultProps.onCreateTextFile).toHaveBeenCalledWith('styles.css', '');
    });

    it('seeds Other with the full name when the initial file has an unknown extension', () => {
      render(<NewFileDialog {...defaultProps} initialFilename="notes/styles.css" />);

      expect(screen.getByRole('radio', { name: 'Other' })).toHaveAttribute('aria-checked', 'true');
      expect((screen.getByLabelText('Filename:') as HTMLInputElement).value).toBe('styles.css');

      fireEvent.click(screen.getByRole('button', { name: 'Create' }));

      expect(defaultProps.onCreateTextFile).toHaveBeenCalledWith('notes/styles.css', '');
    });
  });

  describe('state reset (remount-per-open contract, bd-cp9wzu6o)', () => {
    // Real parents mount the dialog only while it is open, so every open is
    // a fresh mount with pristine form state; there is no in-place reset to
    // test. These tests pin the contract through the same open/close cycle a
    // user drives.
    function StatefulHarness({ initialFilename }: { initialFilename?: string }) {
      const [open, setOpen] = useState(false);
      return (
        <>
          <button onClick={() => setOpen(true)}>Open dialog</button>
          {open && (
            <NewFileDialog
              {...defaultProps}
              isOpen
              initialFilename={initialFilename}
              onClose={() => setOpen(false)}
            />
          )}
        </>
      );
    }

    it('reopens with pristine form state', async () => {
      mockDiscoverTemplates.mockResolvedValue([
        {
          path: '/project/_quarto-hub-templates/article.qmd',
          displayName: 'Article',
          strippedContent: 'content',
        },
      ]);

      render(<StatefulHarness />);
      fireEvent.click(screen.getByRole('button', { name: 'Open dialog' }));

      await waitFor(() => {
        expect(screen.getByLabelText('Template:')).toBeInTheDocument();
      });

      // Dirty the form: pick a template, type a filename.
      fireEvent.change(screen.getByLabelText('Template:'), {
        target: { value: '/project/_quarto-hub-templates/article.qmd' },
      });
      fireEvent.change(screen.getByLabelText('Filename:'), {
        target: { value: 'dirty.qmd' },
      });

      // Close and reopen.
      fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
      fireEvent.click(screen.getByRole('button', { name: 'Open dialog' }));

      await waitFor(() => {
        expect(screen.getByLabelText('Template:')).toBeInTheDocument();
      });
      expect((screen.getByLabelText('Template:') as HTMLSelectElement).value).toBe('');
      expect((screen.getByLabelText('Filename:') as HTMLInputElement).value).toBe('');
    });

    it('seeds folder and filename from initialFilename on open', async () => {
      render(<StatefulHarness initialFilename="notes/draft.qmd" />);
      fireEvent.click(screen.getByRole('button', { name: 'Open dialog' }));

      expect((screen.getByLabelText('Filename:') as HTMLInputElement).value).toBe('draft');
      expect(screen.getByRole('button', { name: 'Choose folder' })).toHaveTextContent('notes');
    });
  });

  describe('Enter key handling (GH #635, bd-zcv0iea4)', () => {
    // The browser's default action for an un-prevented Enter keydown is a
    // synthesized click on whatever is focused when the default action is
    // processed. Because closing the dialog restores focus to the trigger
    // button (ModalDialog's WCAG contract), an un-prevented Enter-submit
    // reopens the dialog. jsdom performs no keyboard activation, so the
    // reopen itself can't reproduce here; instead these tests pin the
    // mechanism: fireEvent returns `!event.defaultPrevented`.
    it('submits on Enter in the filename input, closes, and prevents the default action', () => {
      render(<NewFileDialog {...defaultProps} />);

      const filenameInput = screen.getByLabelText('Filename:');
      fireEvent.change(filenameInput, { target: { value: 'notes.qmd' } });
      const notPrevented = fireEvent.keyDown(filenameInput, { key: 'Enter' });

      expect(defaultProps.onCreateTextFile).toHaveBeenCalledWith('notes.qmd', '');
      expect(defaultProps.onClose).toHaveBeenCalledTimes(1);
      expect(notPrevented).toBe(false);
    });

    it('does not submit when Enter is pressed on the Cancel button', () => {
      render(<NewFileDialog {...defaultProps} />);

      const filenameInput = screen.getByLabelText('Filename:');
      fireEvent.change(filenameInput, { target: { value: 'notes.qmd' } });

      // The user tabs to Cancel and presses Enter: the keydown bubbles to
      // the dialog container, whose Enter handler must not treat it as a
      // submit — the button's own (native) activation is the intent.
      const cancelButton = screen.getByRole('button', { name: 'Cancel' });
      cancelButton.focus();
      fireEvent.keyDown(cancelButton, { key: 'Enter' });

      expect(defaultProps.onCreateTextFile).not.toHaveBeenCalled();
    });

    it('does not double-submit when Enter is pressed on the Create button', () => {
      render(<NewFileDialog {...defaultProps} />);

      const filenameInput = screen.getByLabelText('Filename:');
      fireEvent.change(filenameInput, { target: { value: 'notes.qmd' } });

      // Enter on the focused Create button activates it natively (a click,
      // which jsdom doesn't synthesize); the bubbled keydown must not ALSO
      // run the submit handler, or real browsers create the file twice.
      const createButton = screen.getByRole('button', { name: 'Create' });
      createButton.focus();
      fireEvent.keyDown(createButton, { key: 'Enter' });

      expect(defaultProps.onCreateTextFile).not.toHaveBeenCalled();
    });

    it('shows the validation error and stays open on Enter with an empty filename', () => {
      render(<NewFileDialog {...defaultProps} />);

      const filenameInput = screen.getByLabelText('Filename:');
      fireEvent.keyDown(filenameInput, { key: 'Enter' });

      expect(defaultProps.onCreateTextFile).not.toHaveBeenCalled();
      expect(defaultProps.onClose).not.toHaveBeenCalled();
      expect(screen.getByText('Filename is required')).toBeInTheDocument();
    });
  });

  describe('initial focus', () => {
    // The filename input used to be focused on a 100ms timer. A keyboard
    // user (or Playwright) who moved focus within that window had it
    // yanked back to the input, so Enter on Cancel submitted the form
    // (flaky new-file-dialog.harness.spec.ts on Firefox CI).
    it('focuses the filename input on open', () => {
      render(<NewFileDialog {...defaultProps} />);

      expect(screen.getByLabelText('Filename:')).toHaveFocus();
    });

    it('does not pull focus back to the filename input later', () => {
      vi.useFakeTimers();
      try {
        render(<NewFileDialog {...defaultProps} />);
        const cancelButton = screen.getByRole('button', { name: 'Cancel' });
        cancelButton.focus();
        vi.advanceTimersByTime(500);

        expect(cancelButton).toHaveFocus();
      } finally {
        vi.useRealTimers();
      }
    });
  });

  describe('accessibility semantics', () => {
    it('exposes role="dialog" with aria-modal and the title as accessible name', () => {
      render(<NewFileDialog {...defaultProps} />);

      const dialog = screen.getByRole('dialog');
      expect(dialog).toHaveAttribute('aria-modal', 'true');
      const labelId = dialog.getAttribute('aria-labelledby');
      expect(labelId).toBeTruthy();
      expect(document.getElementById(labelId!)).toHaveTextContent('New file');
    });

    it('gives the close button an accessible name', () => {
      render(<NewFileDialog {...defaultProps} />);

      expect(screen.getByRole('button', { name: 'Close' })).toBeInTheDocument();
    });
  });
});
