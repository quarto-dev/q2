/**
 * Tests for the "Export ZIP" wiring in ProjectTab.
 *
 * Scope: the component derives ONE project-folder slug and uses it for both
 * the in-archive top-level folder (passed to `onExportZip`) and the download
 * filename stem, so the two can never drift (GH #147). The slug is sanitized
 * via `projectFolderName`, so a hostile character in the project name must not
 * leak into either.
 *
 * The path normalization itself is exhaustively covered by node-env unit tests
 * (quarto-sync-client/export-zip.test.ts and project-folder-name.test.ts); here
 * we only assert the UI hands the same, sanitized slug to both consumers.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import ProjectTab from './ProjectTab';
import type { ProjectEntry } from '@quarto/preview-renderer/types/project';

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

describe('ProjectTab — Export ZIP wiring', () => {
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

  it('passes the sanitized slug as rootDir and reuses it for the filename', () => {
    const onExportZip = vi.fn(() => new Uint8Array([1, 2, 3]));
    render(
      <ProjectTab
        project={makeProject('Demo Playground')}
        onChooseNewProject={() => {}}
        onExportZip={onExportZip}
      />,
    );

    fireEvent.click(screen.getByText('Export ZIP'));

    expect(onExportZip).toHaveBeenCalledWith('Demo-Playground');
    expect(clickedDownloadNames).toEqual(['Demo-Playground.zip']);
  });

  it('sanitizes hostile characters in the project name for both outputs', () => {
    const onExportZip = vi.fn(() => new Uint8Array([1]));
    render(
      <ProjectTab
        project={makeProject('Demo: Playground?')}
        onChooseNewProject={() => {}}
        onExportZip={onExportZip}
      />,
    );

    fireEvent.click(screen.getByText('Export ZIP'));

    // ':' and '?' collapse to hyphens; folder and filename stay in lock-step.
    expect(onExportZip).toHaveBeenCalledWith('Demo-Playground');
    expect(clickedDownloadNames).toEqual(['Demo-Playground.zip']);
  });

  it('falls back to "project" when the name is empty', () => {
    const onExportZip = vi.fn(() => new Uint8Array([1]));
    render(
      <ProjectTab
        project={makeProject('')}
        onChooseNewProject={() => {}}
        onExportZip={onExportZip}
      />,
    );

    fireEvent.click(screen.getByText('Export ZIP'));

    expect(onExportZip).toHaveBeenCalledWith('project');
    expect(clickedDownloadNames).toEqual(['project.zip']);
  });
});
