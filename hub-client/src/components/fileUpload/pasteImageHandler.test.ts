/**
 * Tests for the paste-image ingest orchestration (bd-706b0ixu).
 *
 * The handler factory takes injected deps, so the full flow — size
 * validation, sequential ingest, filename generation, single-file
 * selection-as-alt, multi-file space join, file-switch guard — is
 * exercised without jsdom or Monaco. Editor.tsx supplies real deps.
 */

import { describe, it, expect, vi } from 'vitest';
import {
  createPasteImageHandler,
  type PasteImageEditor,
  type PasteRange,
} from './pasteImageHandler';

const HASHES: Record<string, string> = {
  'a.png': 'aaaa1111'.repeat(8),
  'b.png': 'bbbb2222'.repeat(8),
};

function makeFile(name: string, type = 'image/png', bytes = 16): File {
  return new File([new Uint8Array(bytes).fill(7)], name, { type });
}

const cursorAt = (line: number, col: number): PasteRange => ({
  startLineNumber: line,
  startColumn: col,
  endLineNumber: line,
  endColumn: col,
});

interface FakeEditorOpts {
  selection?: PasteRange | null;
  selectedText?: string;
}

function makeFakeEditor(opts: FakeEditorOpts = {}) {
  const replacements: Array<{ range: PasteRange; text: string }> = [];
  const editor: PasteImageEditor = {
    getSelection: () => opts.selection ?? cursorAt(3, 5),
    getTextInRange: () => opts.selectedText ?? '',
    replaceRange: (range, text) => {
      replacements.push({ range, text });
    },
  };
  return { editor, replacements };
}

interface HarnessOpts extends FakeEditorOpts {
  currentFilePath?: string | null | (() => string | null);
  maxFileSize?: number;
  createBinaryFile?: (
    path: string,
    content: Uint8Array,
    mimeType: string
  ) => Promise<{ path: string }>;
}

function makeHarness(opts: HarnessOpts = {}) {
  const { editor, replacements } = makeFakeEditor(opts);
  const created: Array<{ path: string; mimeType: string; size: number }> = [];
  const errors: string[] = [];

  const currentFilePath = opts.currentFilePath ?? 'posts/hello.qmd';
  const getCurrentFilePath =
    typeof currentFilePath === 'function'
      ? currentFilePath
      : () => currentFilePath;

  const handler = createPasteImageHandler({
    getCurrentFilePath,
    getEditor: () => editor,
    processFile: async (file: File) => ({
      content: new Uint8Array(await file.arrayBuffer()),
      mimeType: file.type,
      hash: HASHES[file.name] ?? 'cccc3333'.repeat(8),
    }),
    createBinaryFile:
      opts.createBinaryFile ??
      (async (path, content, mimeType) => {
        created.push({ path, mimeType, size: content.length });
        return { path };
      }),
    maxFileSize: opts.maxFileSize ?? 1024 * 1024,
    onError: (message) => {
      errors.push(message);
    },
  });

  return { handler, replacements, created, errors };
}

describe('createPasteImageHandler', () => {
  it('single file: creates the binary next to the current doc and inserts a reference', async () => {
    const { handler, replacements, created } = makeHarness();

    const inserted = await handler([makeFile('a.png')]);

    expect(inserted).toBe(true);
    expect(created).toEqual([
      { path: 'posts/pasted-aaaa1111.png', mimeType: 'image/png', size: 16 },
    ]);
    expect(replacements).toEqual([
      { range: cursorAt(3, 5), text: '![](pasted-aaaa1111.png)' },
    ]);
  });

  it('uses the selection as alt text for a single-file paste', async () => {
    const { handler, replacements } = makeHarness({
      selection: {
        startLineNumber: 3,
        startColumn: 5,
        endLineNumber: 3,
        endColumn: 12,
      },
      selectedText: 'my [old] caption',
    });

    await handler([makeFile('a.png')]);

    expect(replacements[0].text).toBe(
      '![my \\[old\\] caption](pasted-aaaa1111.png)'
    );
  });

  it('multi-file: inserts space-separated references without alt text', async () => {
    const { handler, replacements, created } = makeHarness({
      selectedText: 'a selection that must NOT become alt text',
      selection: {
        startLineNumber: 1,
        startColumn: 1,
        endLineNumber: 1,
        endColumn: 9,
      },
    });

    await handler([makeFile('a.png'), makeFile('b.png')]);

    expect(created.map((c) => c.path)).toEqual([
      'posts/pasted-aaaa1111.png',
      'posts/pasted-bbbb2222.png',
    ]);
    expect(replacements[0].text).toBe(
      '![](pasted-aaaa1111.png) ![](pasted-bbbb2222.png)'
    );
  });

  it('uses the path returned by createBinaryFile (rename-on-conflict)', async () => {
    const { handler, replacements } = makeHarness({
      createBinaryFile: async () => ({
        path: 'posts/pasted-aaaa1111-99887766.png',
      }),
    });

    await handler([makeFile('a.png')]);

    expect(replacements[0].text).toBe('![](pasted-aaaa1111-99887766.png)');
  });

  it('current file at project root: creates at root with a bare reference', async () => {
    const { handler, replacements, created } = makeHarness({
      currentFilePath: 'hello.qmd',
    });

    await handler([makeFile('a.png')]);

    expect(created[0].path).toBe('pasted-aaaa1111.png');
    expect(replacements[0].text).toBe('![](pasted-aaaa1111.png)');
  });

  it('oversize file: reports an error, creates and inserts nothing', async () => {
    const { handler, replacements, created, errors } = makeHarness({
      maxFileSize: 8,
    });

    const inserted = await handler([makeFile('a.png', 'image/png', 16)]);

    expect(inserted).toBe(false);
    expect(created).toEqual([]);
    expect(replacements).toEqual([]);
    expect(errors).toHaveLength(1);
  });

  it('mixed sizes: ingests the valid file, reports the oversize one', async () => {
    const { handler, replacements, created, errors } = makeHarness({
      maxFileSize: 20,
    });

    const inserted = await handler([
      makeFile('a.png', 'image/png', 16),
      makeFile('b.png', 'image/png', 64),
    ]);

    expect(inserted).toBe(true);
    expect(created.map((c) => c.path)).toEqual(['posts/pasted-aaaa1111.png']);
    expect(replacements[0].text).toBe('![](pasted-aaaa1111.png)');
    expect(errors).toHaveLength(1);
  });

  it('skips insertion when the current file changed mid-flight', async () => {
    const getCurrentFilePath = vi
      .fn<() => string | null>()
      .mockReturnValueOnce('posts/hello.qmd') // captured at paste time
      .mockReturnValue('other.qmd'); // by insertion time

    const { handler, replacements, created } = makeHarness({
      currentFilePath: getCurrentFilePath as unknown as () => string | null,
    });

    const inserted = await handler([makeFile('a.png')]);

    expect(inserted).toBe(false);
    expect(created).toHaveLength(1); // file creation is not rolled back
    expect(replacements).toEqual([]);
  });

  it('returns false without an editor', async () => {
    const errors: string[] = [];
    const handler = createPasteImageHandler({
      getCurrentFilePath: () => 'hello.qmd',
      getEditor: () => null,
      processFile: async () => {
        throw new Error('should not be called');
      },
      createBinaryFile: async () => {
        throw new Error('should not be called');
      },
      maxFileSize: 1024,
      onError: (m) => errors.push(m),
    });

    expect(await handler([makeFile('a.png')])).toBe(false);
    expect(errors).toEqual([]);
  });

  it('reports a create failure and continues with remaining files', async () => {
    let call = 0;
    const { handler, replacements, errors } = makeHarness({
      createBinaryFile: async (path) => {
        call += 1;
        if (call === 1) throw new Error('boom');
        return { path };
      },
    });

    const inserted = await handler([makeFile('a.png'), makeFile('b.png')]);

    expect(inserted).toBe(true);
    expect(errors).toHaveLength(1);
    expect(replacements[0].text).toBe('![](pasted-bbbb2222.png)');
  });
});
