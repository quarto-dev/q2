import { beforeEach, describe, expect, it, vi } from 'vitest';

const getFileHandle = vi.fn();
vi.mock('@quarto/preview-runtime', () => ({ getFileHandle: (p: string) => getFileHandle(p) }));
const buildRunListAttribution = vi.fn();
vi.mock('../services/attribution-runs', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../services/attribution-runs')>()),
  buildRunListAttribution: (...args: unknown[]) => buildRunListAttribution(...args),
}));

import { attributionJsonFor, setDownloadIdentities } from './downloadAttribution';

const TEXT = 'été [>> why?]';

beforeEach(() => {
  getFileHandle.mockReset();
  buildRunListAttribution.mockReset();
  setDownloadIdentities({ bear: { name: 'Kind Bear', color: '#112233' } });
  getFileHandle.mockReturnValue({ doc: () => ({ text: TEXT }) });
  buildRunListAttribution.mockResolvedValue({ runs: [{ start: 0, end: TEXT.length, actor: 'bear', time: 1_700_000_000 }] });
});

describe('attributionJsonFor', () => {
  it('names the author from the identity table, with offsets in bytes', async () => {
    const json = JSON.parse((await attributionJsonFor('doc.qmd', 'docx'))!);
    expect(json.identities).toEqual({ bear: { name: 'Kind Bear', color: '#112233' } });
    // "été " is 6 bytes in UTF-8 but 4 characters; the run covers the whole text.
    expect(json.runs).toEqual([{ start: 0, end: new TextEncoder().encode(TEXT).length, actor: 'bear', time: 1_700_000_000 }]);
    expect(getFileHandle).toHaveBeenCalledWith('doc.qmd');
  });

  it('is built for pptx too', async () => {
    expect(await attributionJsonFor('doc.qmd', 'pptx')).toBeDefined();
  });

  it('does no work for a format that has no comments or changes to stamp', async () => {
    for (const format of ['typst', 'typst-pdf', 'epub']) expect(await attributionJsonFor('doc.qmd', format)).toBeUndefined();
    expect(buildRunListAttribution).not.toHaveBeenCalled();
  });

  it('is undefined when the file has no handle or no history', async () => {
    getFileHandle.mockReturnValue(undefined);
    expect(await attributionJsonFor('doc.qmd', 'docx')).toBeUndefined();
    getFileHandle.mockReturnValue({ doc: () => ({ text: TEXT }) });
    buildRunListAttribution.mockResolvedValue(null);
    expect(await attributionJsonFor('doc.qmd', 'docx')).toBeUndefined();
  });

  it('never fails the download: a history error leaves the marks unstamped', async () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {});
    buildRunListAttribution.mockRejectedValue(new Error('boom'));
    expect(await attributionJsonFor('doc.qmd', 'docx')).toBeUndefined();
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });
});
