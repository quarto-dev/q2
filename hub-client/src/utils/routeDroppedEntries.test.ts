import { describe, expect, it } from 'vitest';
import { createStubImportService } from '../pandoc/importService';
import { routeDroppedEntries } from './routeDroppedEntries';
import type { DroppedEntries } from './droppedEntries';

const formats = await createStubImportService().getImportFormats();
const file = (name: string, size = 10) => {
  const f = new File(['x'], name);
  Object.defineProperty(f, 'size', { value: size });
  return f;
};
const drop = (rels: Array<[File, string?]>, folders: string[] = []): DroppedEntries => ({
  files: rels.map(([f, rel]) => ({ file: f, relativePath: rel ?? f.name })),
  folders,
});
const route = (entries: DroppedEntries, destination = 'docs') => routeDroppedEntries(entries, { formats, destination });

describe('routeDroppedEntries', () => {
  it('imports an upper-case extension', () => {
    const doc = file('REPORT.DOCX');
    const r = route(drop([[doc]]));
    expect(r.imports).toEqual([{ file: doc, folder: 'docs' }]);
    expect(r.uploads.files).toEqual([]);
  });

  it('splits a docx and a png: the docx imports, the png uploads', () => {
    const doc = file('a.docx');
    const png = file('b.png');
    const r = route(drop([[doc], [png]]));
    expect(r.imports.map((i) => i.file)).toEqual([doc]);
    expect(r.uploads.files.map((f) => f.file)).toEqual([png]);
  });

  it('routes a 15 MB docx to import, not to the upload path (which would reject it)', () => {
    const big = file('big.docx', 15 * 1024 * 1024);
    const r = route(drop([[big]]));
    expect(r.imports).toHaveLength(1);
    expect(r.uploads.files).toHaveLength(0);
  });

  it('stores a docx inside a dropped folder as-is, and keeps the folder', () => {
    const inner = file('deep.docx');
    const r = route(drop([[inner, 'folder/deep.docx']], ['folder']));
    expect(r.imports).toEqual([]);
    expect(r.uploads.files.map((f) => f.relativePath)).toEqual(['folder/deep.docx']);
    expect(r.uploads.folders).toEqual(['folder']);
  });

  it('a top-level docx beside a dropped folder still imports; the folder is kept', () => {
    const top = file('top.docx');
    const inner = file('x.png');
    const r = route(drop([[top], [inner, 'pics/x.png']], ['pics']));
    expect(r.imports.map((i) => i.file)).toEqual([top]);
    expect(r.uploads.files.map((f) => f.relativePath)).toEqual(['pics/x.png']);
    expect(r.uploads.folders).toEqual(['pics']);
  });

  it('uploads an unsupported extension, as today', () => {
    const r = route(drop([[file('notes.txt')], [file('data.csv')]]));
    expect(r.imports).toEqual([]);
    expect(r.uploads.files).toHaveLength(2);
  });

  it('a drop of only importable files leaves no uploads and no folders', () => {
    const r = route(drop([[file('a.docx')], [file('b.odt')], [file('c.rtf')], [file('d.epub')], [file('e.pptx')]]));
    expect(r.imports).toHaveLength(5);
    expect(r.uploads).toEqual({ files: [], folders: [] });
  });

  it('with no format table everything is an upload', () => {
    const entries = drop([[file('a.docx')]]);
    const r = routeDroppedEntries(entries, { formats: null, destination: '' });
    expect(r.imports).toEqual([]);
    expect(r.uploads).toBe(entries);
  });

  it('every import gets the destination folder', () => {
    const r = route(drop([[file('a.docx')], [file('b.docx')]]), 'x/y');
    expect(r.imports.map((i) => i.folder)).toEqual(['x/y', 'x/y']);
  });
});
