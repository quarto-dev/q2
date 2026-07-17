/**
 * Tests for the "Import from ZIP" flow in ProjectSelector.
 *
 * Scope: the component's UI wiring — the button reveals the form,
 * choosing a ZIP prefills the title, submitting reads the file bytes and
 * routes them through importProjectFromZip, and the parsed files reach
 * onProjectCreated (with parse errors surfaced instead).
 *
 * importProjectFromZip is mocked here for two reasons: (1) it lives in
 * the WASM-bearing @quarto/preview-runtime, which we don't want to load
 * in a jsdom unit test, and (2) the actual ZIP parsing is exhaustively
 * covered by node-env unit tests (quarto-sync-client/import-zip.test.ts
 * and preview-runtime/automergeSync.test.ts).
 *
 * Do NOT call the real fflate zipSync/unzipSync from a jsdom test: under
 * vitest's jsdom environment, jsdom's TextEncoder.encode() returns a
 * Uint8Array from a different JS realm, so `result instanceof Uint8Array`
 * is false (jsdom/jsdom#2524). fflate's strToU8 uses TextEncoder, and its
 * zipSync flatten step decides "file vs directory" with `val instanceof
 * u8` — so a strToU8-produced array is misclassified as a directory and
 * the archive comes out corrupt. Real browsers are single-realm, so this
 * is a test-environment artifact only (the e2e in e2e/import-zip.spec.ts
 * exercises the real path).
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react';

const { importMock } = vi.hoisted(() => ({ importMock: vi.fn() }));

vi.mock('@quarto/preview-runtime', () => ({
  getProjectChoices: vi.fn().mockResolvedValue([]),
  createProject: vi.fn(),
  importProjectFromZip: importMock,
}));

vi.mock('../services/projectStorage', () => ({
  listProjects: vi.fn().mockResolvedValue([]),
}));

vi.mock('../services/userSettings', () => ({
  getUserIdentity: vi.fn().mockResolvedValue(null),
  updateUserName: vi.fn(),
  updateUserColor: vi.fn(),
  resetUserIdentity: vi.fn(),
}));

vi.mock('./ThemeContext', () => ({
  useTheme: () => ({ colorScheme: 'auto', cycleColorScheme: vi.fn() }),
}));

import ProjectSelector from './ProjectSelector';

const ZIP_BYTES = new Uint8Array([0x50, 0x4b, 0x03, 0x04, 1, 2, 3, 4]);

/** Build a File whose arrayBuffer() resolves to the given bytes. */
function zipFile(name: string, bytes: Uint8Array = ZIP_BYTES): File {
  const file = new File([bytes], name, { type: 'application/zip' });
  // jsdom's Blob.arrayBuffer can be flaky across versions; pin it.
  Object.defineProperty(file, 'arrayBuffer', {
    value: async () => bytes.slice().buffer,
    configurable: true,
  });
  return file;
}

/** Attach a FileList to a file input (the `files` prop is read-only). */
function setInputFiles(input: HTMLElement, files: File[]) {
  Object.defineProperty(input, 'files', { value: files, configurable: true });
  fireEvent.change(input);
}

function renderSelector(onProjectCreated = vi.fn()) {
  render(
    <ProjectSelector
      onSelectProject={vi.fn()}
      onProjectCreated={onProjectCreated}
      projectSetEntries={[]}
    />,
  );
  return { onProjectCreated };
}

afterEach(cleanup);

describe('ProjectSelector — Import from ZIP', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('reveals the import form when the button is clicked', async () => {
    renderSelector();

    fireEvent.click(await screen.findByRole('button', { name: /Import from ZIP/i }));

    expect(
      screen.getByText('Create a new project from the contents of a .zip archive'),
    ).toBeTruthy();
    expect(screen.getByLabelText('ZIP File')).toBeTruthy();
  });

  it('prefills the title from the ZIP filename', async () => {
    renderSelector();
    fireEvent.click(await screen.findByRole('button', { name: /Import from ZIP/i }));

    setInputFiles(screen.getByLabelText('ZIP File'), [zipFile('My Project.zip')]);

    const titleInput = screen.getByLabelText('Project Title') as HTMLInputElement;
    expect(titleInput.value).toBe('My Project');
  });

  it('reads the file bytes, parses, and calls onProjectCreated', async () => {
    const parsed = [
      { path: 'index.qmd', content_type: 'text', content: '# Hi' },
      { path: 'img/logo.png', content_type: 'binary', content: 'iVBORw==', mime_type: 'image/png' },
    ];
    importMock.mockReturnValue(parsed);

    const { onProjectCreated } = renderSelector();
    fireEvent.click(await screen.findByRole('button', { name: /Import from ZIP/i }));

    setInputFiles(screen.getByLabelText('ZIP File'), [zipFile('My Project.zip')]);
    fireEvent.click(screen.getByRole('button', { name: /Import Project/i }));

    await waitFor(() => expect(onProjectCreated).toHaveBeenCalledTimes(1));

    // The file's bytes were read and handed to the parser.
    expect(importMock).toHaveBeenCalledTimes(1);
    const passedBytes = importMock.mock.calls[0][0];
    expect(passedBytes).toBeInstanceOf(Uint8Array);
    expect(Array.from(passedBytes)).toEqual(Array.from(ZIP_BYTES));

    // The parsed files + form values flow to the create callback. Not
    // connected to a hub, so the (editable) Sync Server URL field defaults
    // to empty — a local-only import.
    const [files, title, projectType, syncServer] = onProjectCreated.mock.calls[0];
    expect(files).toEqual(parsed);
    expect(title).toBe('My Project');
    expect(projectType).toBe('imported');
    expect(syncServer).toBe('');
  });

  it('lets the sync server field be edited to target a hub', async () => {
    const parsed = [{ path: 'index.qmd', content_type: 'text', content: '# Hi' }];
    importMock.mockReturnValue(parsed);

    const { onProjectCreated } = renderSelector();
    fireEvent.click(await screen.findByRole('button', { name: /Import from ZIP/i }));

    const syncServerInput = screen.getByLabelText(/sync server url/i) as HTMLInputElement;
    expect(syncServerInput.value).toBe('');
    fireEvent.change(syncServerInput, { target: { value: 'wss://my-hub.example.com/ws' } });

    setInputFiles(screen.getByLabelText('ZIP File'), [zipFile('My Project.zip')]);
    fireEvent.click(screen.getByRole('button', { name: /Import Project/i }));

    await waitFor(() => expect(onProjectCreated).toHaveBeenCalledTimes(1));
    const [, , , syncServer] = onProjectCreated.mock.calls[0];
    expect(syncServer).toBe('wss://my-hub.example.com/ws');
  });

  it('surfaces a parse error and does not create a project', async () => {
    importMock.mockImplementation(() => {
      throw new Error('No files found in the archive.');
    });

    const { onProjectCreated } = renderSelector();
    fireEvent.click(await screen.findByRole('button', { name: /Import from ZIP/i }));

    setInputFiles(screen.getByLabelText('ZIP File'), [zipFile('Empty.zip')]);
    fireEvent.click(screen.getByRole('button', { name: /Import Project/i }));

    await waitFor(() => expect(screen.getByText(/no files found/i)).toBeTruthy());
    expect(onProjectCreated).not.toHaveBeenCalled();
  });

  it('requires a file before the import can be submitted', async () => {
    renderSelector();
    fireEvent.click(await screen.findByRole('button', { name: /Import from ZIP/i }));

    // With no file chosen, the submit button is disabled.
    const submit = screen.getByRole('button', { name: /Import Project/i }) as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
  });
});
