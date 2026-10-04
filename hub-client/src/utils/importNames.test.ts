import { describe, expect, it } from 'vitest';
import { importStem, mediaDirFor, proposeImportName, type Occupied } from './importNames';

const occ = (paths: string[] = [], folders: string[] = []): Occupied => ({ paths: new Set(paths), folders: new Set(folders) });

describe('importStem', () => {
  it('drops the extension and sanitizes spaces', () => {
    expect(importStem('report.docx')).toBe('report');
    expect(importStem('My Report.docx')).toBe('My-Report');
    expect(importStem('REPORT.DOCX')).toBe('REPORT');
  });

  it('keeps interior dots as hyphens rather than treating the last as an extension', () => {
    expect(importStem('my.report.v2.docx')).toBe('my-report-v2');
  });

  it('falls back when nothing is left of the name', () => {
    expect(importStem('.docx')).toBe('document');
    expect(importStem('---.docx')).toBe('document');
  });

  it('handles a name with no extension', () => {
    expect(importStem('notes')).toBe('notes');
  });
});

describe('mediaDirFor', () => {
  it('is <folder>/<stem>_media, and follows a typed name', () => {
    expect(mediaDirFor('', 'a.qmd')).toBe('a_media');
    expect(mediaDirFor('docs/x', 'My Report 2.qmd')).toBe('docs/x/My Report 2_media');
  });
});

describe('proposeImportName', () => {
  it('proposes <stem>.qmd when free', () => {
    expect(proposeImportName('report.docx', 'docs', occ())).toBe('report.qmd');
  });

  it('advances when the qmd exists', () => {
    expect(proposeImportName('report.docx', 'docs', occ(['docs/report.qmd']))).toBe('report 2.qmd');
  });

  it('advances when only the media folder exists', () => {
    expect(proposeImportName('report.docx', 'docs', occ([], ['docs/report_media']))).toBe('report 2.qmd');
    expect(proposeImportName('report.docx', '', occ(['report_media/a.png'], ['report_media']))).toBe('report 2.qmd');
  });

  it('moves both together past a mix of taken names', () => {
    const o = occ(['report.qmd', 'report 3.qmd'], ['report 2_media']);
    expect(proposeImportName('report.docx', '', o)).toBe('report 4.qmd');
  });

  it('only counts the destination folder', () => {
    expect(proposeImportName('report.docx', 'a', occ(['b/report.qmd']))).toBe('report.qmd');
  });

  it('two sources with one stem get distinct names once the first is stored', () => {
    const paths = new Set<string>();
    const first = proposeImportName('report.docx', '', { paths, folders: new Set() });
    paths.add(first);
    const second = proposeImportName('report.odt', '', { paths, folders: new Set() });
    expect([first, second]).toEqual(['report.qmd', 'report 2.qmd']);
  });
});
