/**
 * Tests for ProjectTab.
 *
 * Export ZIP wiring: the component derives ONE project-folder slug and uses
 * it for both the in-archive top-level folder (passed to `onExportZip`) and
 * the download filename stem, so the two can never drift (GH #147). The slug
 * is sanitized via `projectFolderName`, so a hostile character in the project
 * name must not leak into either.
 *
 * Screenshot Preview: the button captures the `.preview-pane` element via
 * html2canvas and downloads a PNG. html2canvas is mocked here; we assert the
 * right element is captured and a PNG download is triggered.
 *
 * The path normalization itself is exhaustively covered by node-env unit tests
 * (quarto-sync-client/export-zip.test.ts and project-folder-name.test.ts); here
 * we only assert the UI hands the same, sanitized slug to both consumers.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react';
import ProjectTab from './ProjectTab';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';

vi.mock('html2canvas', () => ({ default: vi.fn() }));
import html2canvas from 'html2canvas';

const mockHtml2canvas = vi.mocked(html2canvas);

function makeProject(description: string): ProjectEntry {
  return {
    id: 'local-1',
    indexDocId: 'automerge:abc123',
    syncServer: 'wss://example.test/sync',
    description,
    createdAt: '2026-07-01T00:00:00.000Z',
    lastAccessed: '2026-07-01T00:00:00.000Z',
  };
}

describe('ProjectTab', () => {
  let clickedDownloadNames: string[];
  let createElementSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    clickedDownloadNames = [];
    // jsdom does not implement object URLs; stub them.
    vi.stubGlobal('URL', {
      ...URL,
      createObjectURL: vi.fn(() => 'blob:mock'),
      revokeObjectURL: vi.fn(),
    });
    // Capture the download filename of any anchor the handler clicks.
    const realCreateElement = document.createElement.bind(document);
    createElementSpy = vi
      .spyOn(document, 'createElement')
      .mockImplementation((tag: string, opts?: ElementCreationOptions) => {
        const el = realCreateElement(tag, opts);
        if (tag === 'a') {
          vi.spyOn(el as HTMLAnchorElement, 'click').mockImplementation(() => {
            clickedDownloadNames.push((el as HTMLAnchorElement).download);
          });
        }
        return el;
      });
  });

  afterEach(() => {
    createElementSpy.mockRestore();
    vi.unstubAllGlobals();
    cleanup();
  });

  describe('Export ZIP wiring', () => {
    it('passes the sanitized slug as rootDir and reuses it for the filename', () => {
      const onExportZip = vi.fn(() => new Uint8Array([1, 2, 3]));
      render(<ProjectTab project={makeProject('Demo Playground')} onExportZip={onExportZip} />);

      fireEvent.click(screen.getByText('Export ZIP'));

      expect(onExportZip).toHaveBeenCalledWith('Demo-Playground');
      expect(clickedDownloadNames).toEqual(['Demo-Playground.zip']);
    });

    it('sanitizes hostile characters in the project name for both outputs', () => {
      const onExportZip = vi.fn(() => new Uint8Array([1]));
      render(<ProjectTab project={makeProject('Demo: Playground?')} onExportZip={onExportZip} />);

      fireEvent.click(screen.getByText('Export ZIP'));

      // ':' and '?' collapse to hyphens; folder and filename stay in lock-step.
      expect(onExportZip).toHaveBeenCalledWith('Demo-Playground');
      expect(clickedDownloadNames).toEqual(['Demo-Playground.zip']);
    });

    it('falls back to "project" when the name is empty', () => {
      const onExportZip = vi.fn(() => new Uint8Array([1]));
      render(<ProjectTab project={makeProject('')} onExportZip={onExportZip} />);

      fireEvent.click(screen.getByText('Export ZIP'));

      expect(onExportZip).toHaveBeenCalledWith('project');
      expect(clickedDownloadNames).toEqual(['project.zip']);
    });
  });

  describe('Screenshot Preview', () => {
    let previewPane: HTMLElement;

    beforeEach(() => {
      previewPane = document.createElement('div');
      previewPane.className = 'preview-pane';
      document.body.appendChild(previewPane);
      mockHtml2canvas.mockResolvedValue({
        toBlob: (cb: (blob: Blob | null) => void) =>
          cb(new Blob(['png'], { type: 'image/png' })),
      } as unknown as HTMLCanvasElement);
    });

    afterEach(() => {
      previewPane.remove();
      mockHtml2canvas.mockReset();
    });

    it('captures the preview pane and downloads a PNG', async () => {
      render(<ProjectTab project={makeProject('Demo')} onExportZip={vi.fn()} />);

      fireEvent.click(screen.getByText('📸 Screenshot Preview'));

      await waitFor(() =>
        expect(
          clickedDownloadNames.some((n) => /^preview-screenshot-.*\.png$/.test(n)),
        ).toBe(true),
      );
      expect(mockHtml2canvas).toHaveBeenCalledWith(
        previewPane,
        expect.objectContaining({ useCORS: true }),
      );
    });
  });

  it('does not offer project switching — the header switcher owns that', () => {
    render(<ProjectTab project={makeProject('Demo')} onExportZip={vi.fn()} />);

    expect(screen.queryByText('Choose New Project')).toBeNull();
  });
});
