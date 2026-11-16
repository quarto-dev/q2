import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import {
  COLD_BUILD_ACTOR,
  COLD_BUILD_TEXT_LENGTH,
  createColdBuildAttributionFixture,
  installColdBuildIdleGate,
} from '../test-utils/attributionColdBuild';

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

afterEach(() => {
  vi.unstubAllGlobals();
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

  it('includes multiple edits made during a cold build in the downloaded payload', async () => {
    const actual = await vi.importActual<typeof import('../services/attribution-runs')>(
      '../services/attribution-runs',
    );
    buildRunListAttribution.mockImplementation(actual.buildRunListAttribution);
    const fixture = createColdBuildAttributionFixture();
    const gate = installColdBuildIdleGate();
    getFileHandle.mockReturnValue(fixture.handle);

    const pendingJson = attributionJsonFor('doc.qmd', 'docx');
    expect(gate.pending).toBe(1);
    fixture.append('X', 2);
    fixture.append('Y', 3);
    gate.runNext();

    const json = JSON.parse((await pendingJson)!);
    expect(json.runs).toEqual([
      { start: 0, end: COLD_BUILD_TEXT_LENGTH, actor: COLD_BUILD_ACTOR, time: 1 },
      { start: COLD_BUILD_TEXT_LENGTH, end: COLD_BUILD_TEXT_LENGTH + 1, actor: COLD_BUILD_ACTOR, time: 2 },
      { start: COLD_BUILD_TEXT_LENGTH + 1, end: COLD_BUILD_TEXT_LENGTH + 2, actor: COLD_BUILD_ACTOR, time: 3 },
    ]);
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
